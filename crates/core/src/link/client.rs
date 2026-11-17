//! The Ally's side of the companion API.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::enrol::{self, valid_code, Exchange, Shared, Side};
use super::health::Pulse;
use super::probe::{self, Burst, BurstAsk, BurstPlan, Echo};
use super::quality::Line;
use super::{EnrolAsk, EnrolReveal, FitAsk, ScreenFit};
use crate::library::saveplace::SavePlace;
use super::{random_hex, sign, EnrolDone, EnrolFinish, EnrolOffer, EnrolStart, Hello, Install, Installing, Pair, Prepare, Problem, RemoteGame, RemoteLibrary, Retired, SaveFile, SavesFind, SavesFound, Status, Updating};
use crate::library::downloads::Download;
use crate::library::sync::{self, Manifest};
use crate::API_VERSION;

/// Enough for a status question on Wi-Fi; a desktop that is slower is off.
pub const QUICK: Duration = Duration::from_secs(3);
const SLOW: Duration = Duration::from_secs(40);
/// Telling a desktop that this Ally is gone is not worth more waiting than this.
const LEAVE: Duration = Duration::from_secs(2);
/// A measurement that takes longer than this has its answer: too slow.
const PROBE: Duration = Duration::from_secs(4);
/// One save file is the largest thing a desktop sends.
const LARGEST_REPLY: u64 = sync::LARGEST_FILE + 1024 * 1024;
/// A cover picture is small; one that is slower than this is not worth the
/// wait, and a desktop that is gone must not hold the rest up.
const COVER: Duration = Duration::from_secs(6);

/// What the observer is told about one request to a linked desktop. Never the
/// address, the body or the headers: only what is needed to see that the
/// connection is slow or broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Call<'a> {
    pub method: &'a str,
    /// The path without a query, and without the handle of a save folder.
    pub path: &'a str,
    /// The desktop's answer, when there was one.
    pub status: Option<u16>,
    /// What went wrong when there was no answer: "unreachable", "refused",
    /// "untrusted" or "host".
    pub error: Option<&'static str>,
    pub elapsed_ms: u64,
    /// "local" on the home network, "remote" through Tailscale.
    pub route: &'static str,
}

static OBSERVER: std::sync::RwLock<Option<fn(&Call)>> = std::sync::RwLock::new(None);

/// Have `observer` told about every request made to a linked desktop. For
/// writing a log: it runs on the thread that asked, so it must be quick.
pub fn set_observer(observer: fn(&Call)) {
    *OBSERVER.write().unwrap_or_else(|e| e.into_inner()) = Some(observer);
}

/// A path as it may be written down: no query, and the key that names a save
/// folder on the desktop (`/v1/saves/<key>/read`) left out.
fn loggable(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let mut parts: Vec<&str> = path.split('/').collect();
    // ["", "v1", "saves", key, what]
    if parts.len() >= 4 && parts[1] == "v1" && parts[2] == "saves" {
        parts[3] = "*";
    }
    parts.join("/")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Nothing answered: off, asleep, or not on this network.
    Unreachable,
    /// The desktop answered but no longer accepts this Ally.
    Refused,
    /// The desktop answered with a problem of its own, in its words.
    Host(String),
    /// The answer did not come from the desktop we linked.
    Untrusted,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unreachable => f.write_str("The desktop did not answer. It may be off or asleep."),
            Error::Refused => f.write_str("The desktop no longer accepts this Ally. Link it again."),
            Error::Host(text) => f.write_str(text),
            Error::Untrusted => f.write_str("The answer did not come from the linked desktop."),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).http_status_as_error(false).build().into()
}

fn url(address: &str, port: u16, path: &str) -> String {
    // An IPv6 address needs brackets in a URL.
    if address.contains(':') && !address.starts_with('[') {
        format!("http://[{address}]:{port}{path}")
    } else {
        format!("http://{address}:{port}{path}")
    }
}

fn problem(body: &[u8]) -> Error {
    match serde_json::from_slice::<Problem>(body) {
        Ok(p) => Error::Host(p.error),
        Err(_) => Error::Host("The desktop gave an answer CarlaOS did not understand.".to_string()),
    }
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|_| Error::Host("The desktop gave an answer CarlaOS did not understand.".to_string()))
}

/// Questions anyone may ask, used before a desktop is linked.
fn open<T: DeserializeOwned>(address: &str, port: u16, path: &str, body: Option<&[u8]>) -> Result<T> {
    let agent = agent(QUICK * 2);
    let target = url(address, port, path);
    let reply = match body {
        Some(body) => agent.post(&target).header("content-type", "application/json").send(body),
        None => agent.get(&target).call(),
    };
    let mut reply = reply.map_err(|_| Error::Unreachable)?;
    let status = reply.status().as_u16();
    let bytes = reply.body_mut().with_config().limit(LARGEST_REPLY).read_to_vec().map_err(|_| Error::Unreachable)?;
    if status == 200 {
        parse(&bytes)
    } else {
        Err(problem(&bytes))
    }
}

pub fn hello(address: &str, port: u16) -> Result<Hello> {
    let hello: Hello = open(address, port, "/v1/hello", None)?;
    if hello.api_version != API_VERSION {
        return Err(Error::Host(format!("{} runs a different version of CarlaOS. Update both.", hello.name)));
    }
    Ok(hello)
}

/// Link to a desktop with the code it shows. Returns the link to keep.
pub fn enrol(address: &str, port: u16, code: &str, device_name: &str) -> Result<(Hello, Link)> {
    if !valid_code(code) {
        return Err(Error::Host("The code is six digits.".to_string()));
    }
    let hello = hello(address, port)?;
    let exchange = Exchange::start(Side::Ally, code);
    let start = EnrolStart { device_name: device_name.to_string(), message: exchange.message() };
    let offer: EnrolOffer = open(address, port, "/v1/enrol/start", Some(&serde_json::to_vec(&start).expect("serialisable")))?;
    let wrong = || Error::Host("That is not the code on the desktop.".to_string());
    let shared = exchange.finish(&offer.message).ok_or_else(wrong)?;
    let finish = EnrolFinish { enrol_id: offer.enrol_id, proof: shared.proof(Side::Ally) };
    let done: EnrolDone = open(address, port, "/v1/enrol/finish", Some(&serde_json::to_vec(&finish).expect("serialisable")))?;
    // The desktop must prove it knew the code too, or it is not our desktop.
    if !shared.proof_ok(Side::Desktop, &done.proof) {
        return Err(wrong());
    }
    Ok((hello, Link { address: address.to_string(), port, device_id: done.device_id, key: shared.device_key() }))
}

/// A link that waits for someone at the desktop to allow it.
pub struct Asking {
    address: String,
    port: u16,
    enrol_id: String,
    shared: Shared,
    hello: Hello,
    /// The number the desktop shows too. The administrator compares them.
    pub check: String,
}

/// Ask a desktop to link without a code: someone there allows it. Works
/// while linking is open on the desktop (`Hello::approve`).
pub fn enrol_ask(address: &str, port: u16, device_name: &str) -> Result<Asking> {
    let hello = hello(address, port)?;
    let exchange = Exchange::start(Side::Ally, enrol::APPROVE);
    let message = exchange.message();
    // Commit first, show the message only once the desktop has sent its
    // own: neither side can then pick a message that steers the number.
    let ask = EnrolAsk { device_name: device_name.to_string(), commit: enrol::commit(&message) };
    let offer: EnrolOffer = open(address, port, "/v1/enrol/ask", Some(&serde_json::to_vec(&ask).expect("serialisable")))?;
    let shared = exchange.finish(&offer.message).ok_or_else(|| Error::Host("The desktop gave an answer CarlaOS did not understand.".to_string()))?;
    let reveal = EnrolReveal { enrol_id: offer.enrol_id.clone(), message };
    let _: serde_json::Value = open(address, port, "/v1/enrol/reveal", Some(&serde_json::to_vec(&reveal).expect("serialisable")))?;
    Ok(Asking { address: address.to_string(), port, enrol_id: offer.enrol_id, check: shared.check(), shared, hello })
}

impl Asking {
    /// Ask whether it was allowed yet. `None` while nobody has answered at
    /// the desktop; ask again in a moment.
    pub fn finish(&self) -> Result<Option<(Hello, Link)>> {
        let finish = EnrolFinish { enrol_id: self.enrol_id.clone(), proof: self.shared.proof(Side::Ally) };
        let done: EnrolDone = match open(&self.address, self.port, "/v1/enrol/finish", Some(&serde_json::to_vec(&finish).expect("serialisable"))) {
            Ok(done) => done,
            Err(Error::Host(text)) if text == enrol::WAITING => return Ok(None),
            Err(e) => return Err(e),
        };
        if !self.shared.proof_ok(Side::Desktop, &done.proof) {
            return Err(Error::Untrusted);
        }
        Ok(Some((self.hello.clone(), Link { address: self.address.clone(), port: self.port, device_id: done.device_id, key: self.shared.device_key() })))
    }
}

/// How long opening a connection to a port takes; `None` when it does not
/// open. For telling whether Sunshine can be reached through the firewall.
pub fn port_open(address: &str, port: u16, limit: Duration) -> Option<Duration> {
    use std::net::ToSocketAddrs;
    let target = (address, port).to_socket_addrs().ok()?.next()?;
    let asked = std::time::Instant::now();
    std::net::TcpStream::connect_timeout(&target, limit).ok().map(|_| asked.elapsed())
}

/// The ports Moonlight first speaks to Sunshine on.
pub const SUNSHINE_PORTS: [u16; 2] = [47984, 47989];

/// A linked desktop: where it is and the key shared with it.
#[derive(Clone, PartialEq, Eq)]
pub struct Link {
    pub address: String,
    pub port: u16,
    pub device_id: String,
    pub key: Vec<u8>,
}

impl Link {
    fn ask(&self, method: &str, path: &str, body: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        match self.ask_status(method, path, body, timeout)? {
            (200, bytes) => Ok(bytes),
            (_, bytes) => Err(problem(&bytes)),
        }
    }

    /// As `ask`, but a genuine (signed) refusal comes back with its status.
    /// Reported to the observer, if there is one, once it is over.
    fn ask_status(&self, method: &str, path: &str, body: &[u8], timeout: Duration) -> Result<(u16, Vec<u8>)> {
        let asked = std::time::Instant::now();
        let result = self.ask_once(method, path, body, timeout);
        let observer = *OBSERVER.read().unwrap_or_else(|e| e.into_inner());
        if let Some(observer) = observer {
            let (status, error) = match &result {
                Ok((status, _)) => (Some(*status), None),
                Err(Error::Unreachable) => (None, Some("unreachable")),
                Err(Error::Refused) => (None, Some("refused")),
                Err(Error::Untrusted) => (None, Some("untrusted")),
                Err(Error::Host(_)) => (None, Some("host")),
            };
            let path = loggable(path);
            let route = if super::wake::is_tailscale(&self.address) { "remote" } else { "local" };
            observer(&Call { method, path: &path, status, error, elapsed_ms: asked.elapsed().as_millis() as u64, route });
        }
        result
    }

    fn ask_once(&self, method: &str, path: &str, body: &[u8], timeout: Duration) -> Result<(u16, Vec<u8>)> {
        let (time, nonce) = (now(), random_hex(16));
        let agent = agent(timeout);
        let target = url(&self.address, self.port, path);
        let signature = sign::request(&self.key, method, path, time, &nonce, body);
        let reply = if method == "GET" {
            agent
                .get(&target)
                .header(sign::DEVICE, &self.device_id)
                .header(sign::TIME, time.to_string())
                .header(sign::NONCE, &nonce)
                .header(sign::SIGN, &signature)
                .call()
        } else {
            agent
                .post(&target)
                .header("content-type", "application/json")
                .header(sign::DEVICE, &self.device_id)
                .header(sign::TIME, time.to_string())
                .header(sign::NONCE, &nonce)
                .header(sign::SIGN, &signature)
                .send(body)
        };
        let mut reply = reply.map_err(|_| Error::Unreachable)?;
        let status = reply.status().as_u16();
        let signed = reply.headers().get(sign::SIGN).and_then(|v| v.to_str().ok()).map(str::to_string);
        let bytes = reply.body_mut().with_config().limit(LARGEST_REPLY).read_to_vec().map_err(|_| Error::Unreachable)?;
        // A desktop that does not know this device cannot sign its refusal.
        if status == 401 {
            return Err(Error::Refused);
        }
        if !signed.is_some_and(|s| sign::reply_ok(&self.key, &nonce, status, &bytes, &s)) {
            return Err(Error::Untrusted);
        }
        Ok((status, bytes))
    }

    fn post<T: DeserializeOwned>(&self, path: &str, body: &impl Serialize, timeout: Duration) -> Result<T> {
        parse(&self.ask("POST", path, &serde_json::to_vec(body).expect("serialisable"), timeout)?)
    }

    pub fn status(&self) -> Result<Status> {
        parse(&self.ask("GET", "/v1/status", b"", QUICK)?)
    }

    pub fn games(&self) -> Result<Vec<RemoteGame>> {
        parse(&self.ask("GET", "/v1/games", b"", SLOW)?)
    }

    /// The games of the desktop's library that are not installed there.
    pub fn library(&self) -> Result<RemoteLibrary> {
        parse(&self.ask("GET", "/v1/library", b"", SLOW)?)
    }

    /// Ask the desktop to install a game of its library. Answering can take
    /// a while: the store there may have to start again first.
    pub fn install(&self, game_id: &str) -> Result<Installing> {
        self.post("/v1/install", &Install { game_id: game_id.to_string() }, SLOW * 2)
    }

    /// What the desktop's stores are fetching now.
    pub fn downloads(&self) -> Result<Vec<Download>> {
        parse(&self.ask("GET", "/v1/downloads", b"", QUICK * 2)?)
    }

    /// The cover picture of one game, as the file's bytes.
    pub fn cover(&self, game_id: &str) -> Result<Vec<u8>> {
        self.ask("GET", &format!("/v1/cover/{game_id}"), b"", COVER)
    }

    /// What the player has earned in a game on that desktop. Empty when the
    /// game has no achievements or has never been run there.
    pub fn achievements(&self, game_id: &str) -> Result<Vec<crate::library::achievements::Achievement>> {
        parse(&self.ask("GET", &format!("/v1/achievements/{game_id}"), b"", QUICK)?)
    }

    /// Have the desktop send `bytes` of nothing in particular and say how
    /// long the whole exchange took. For measuring the connection.
    pub fn probe(&self, bytes: usize) -> Result<Duration> {
        let asked = std::time::Instant::now();
        let got = self.ask("GET", &format!("/v1/probe/{bytes}"), b"", PROBE)?;
        if got.len() != bytes {
            return Err(Error::Host("The desktop sent a different amount than was asked for.".to_string()));
        }
        Ok(asked.elapsed())
    }

    fn udp(&self) -> Option<std::net::SocketAddr> {
        use std::net::ToSocketAddrs;
        (self.address.as_str(), self.port).to_socket_addrs().ok()?.next()
    }

    /// Time packets of a video packet's size to the desktop and back.
    pub fn echo(&self, count: u32) -> Option<Echo> {
        probe::echo(self.udp()?, count, Duration::from_millis(10))
    }

    /// Have the desktop send packets at `kbps` for `ms` and count what
    /// arrives. `latency` is the delay there and back.
    pub fn burst(&self, kbps: u32, ms: u32, latency: Duration) -> Option<Burst> {
        let target = self.udp()?;
        let socket = std::net::UdpSocket::bind(if target.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).ok()?;
        // A packet out first: this Ally's firewall then lets the answers in.
        probe::echo_on(&socket, target, 3, Duration::from_millis(5))?;
        let ask = BurstAsk { port: socket.local_addr().ok()?.port(), kbps, ms };
        let plan: BurstPlan = self.post("/v1/probe/burst", &ask, PROBE).ok()?;
        Some(probe::receive_burst(&socket, target, plan, kbps, latency))
    }

    /// Tell the desktop how this Ally and the connection are.
    pub fn pulse(&self, pulse: &Pulse) -> Result<()> {
        self.post::<serde_json::Value>("/v1/link/pulse", pulse, QUICK).map(|_| ())
    }

    /// Tell the desktop this Ally was removed here, so it forgets it too.
    /// `Ok(false)` when the desktop is older and does not know the request.
    /// Short: a desktop that is off must not hold removal up.
    pub fn leave(&self) -> Result<bool> {
        match self.ask_status("POST", "/v1/link/leave", b"{}", LEAVE)? {
            (200, _) => Ok(true),
            (404, _) => Ok(false),
            (_, bytes) => Err(problem(&bytes)),
        }
    }

    /// What screen the desktop would give a stream of this size.
    pub fn fit(&self, screen: crate::display::Mode) -> Result<ScreenFit> {
        self.post("/v1/stream/fit", &FitAsk { screen }, QUICK)
    }

    /// Ask the desktop to go to sleep. It refuses while a game runs there.
    pub fn sleep(&self) -> Result<()> {
        self.post::<serde_json::Value>("/v1/power/sleep", &serde_json::json!({}), QUICK).map(|_| ())
    }

    /// Ask the desktop whether its administrator wants something from this
    /// Ally. The desktop holds the question for a while when there is
    /// nothing, so this can be asked again straight away.
    pub fn admin_next(&self) -> Result<Option<crate::remote::Command>> {
        parse(&self.ask("GET", "/v1/admin/next", b"", SLOW)?)
    }

    /// Hand back how a request went.
    pub fn admin_done(&self, done: &crate::remote::Done) -> Result<()> {
        self.post::<serde_json::Value>("/v1/admin/done", done, QUICK * 2).map(|_| ())
    }

    /// Close the game that is running on the desktop. Says whether one was.
    pub fn end_game(&self) -> Result<bool> {
        let done: serde_json::Value = self.post("/v1/stream/end", &serde_json::json!({}), QUICK)?;
        Ok(done["ended"].as_bool().unwrap_or(false))
    }

    /// Tell the desktop which game the stream that is about to start is for.
    pub fn prepare(&self, game_id: &str) -> Result<Status> {
        self.prepare_for(game_id, None)
    }

    /// The same, saying what picture will be asked for, so the desktop can
    /// switch its screen to match.
    /// Slow, because the desktop may first make the stream a screen of its own.
    pub fn prepare_for(&self, game_id: &str, screen: Option<crate::display::Mode>) -> Result<Status> {
        self.post("/v1/stream/prepare", &Prepare { game_id: game_id.to_string(), screen }, SLOW)
    }

    /// Whether the store on the desktop has an update to finish for a game,
    /// read there just now. A desktop from before this existed does not know
    /// the question, which counts as "nothing known".
    pub fn pending_update(&self, game_id: &str) -> Result<Option<crate::library::Update>> {
        parse(&self.ask("GET", &format!("/v1/games/update/{game_id}"), b"", QUICK)?)
    }

    /// Take away the loading screen that covers the desktop during a
    /// stream, so the player can see and answer what is behind it.
    pub fn reveal(&self) -> Result<()> {
        self.post::<serde_json::Value>("/v1/stream/reveal", &serde_json::json!({}), QUICK).map(|_| ())
    }

    /// Hand the desktop the PIN Moonlight is pairing with. Waits while the
    /// two finish, which can take a while.
    pub fn pair(&self, pin: &str, name: &str) -> Result<()> {
        self.post::<serde_json::Value>("/v1/stream/pair", &Pair { pin: pin.to_string(), name: name.to_string() }, SLOW).map(|_| ())
    }

    /// Ask the desktop to update its companion to the newest release. It
    /// answers once it knows whether there is one, then installs on its own
    /// and is away for a moment while it restarts.
    pub fn update(&self) -> Result<Updating> {
        self.post("/v1/update", &serde_json::json!({}), SLOW)
    }

    /// The desktop's copy of a game's saves, when its administrator said
    /// where that game keeps them there.
    pub fn saves(&self, game_id: &str, title: &str) -> Result<Option<RemoteSaves<'_>>> {
        self.saves_placed(game_id, title, None)
    }

    /// As `saves`, also telling the desktop where the game's saves are known
    /// to be, so that a desktop with no folder yet can suggest one.
    pub fn saves_placed(&self, game_id: &str, title: &str, place: Option<&SavePlace>) -> Result<Option<RemoteSaves<'_>>> {
        let wanted = SavesFind { game_id: game_id.to_string(), title: title.to_string(), place: place.cloned() };
        let found: Option<SavesFound> = self.post("/v1/saves/find", &wanted, SLOW)?;
        Ok(found.map(|found| RemoteSaves { link: self, key: found.key, manifest: found.manifest, place: found.place, keep_clean: found.keep_clean }))
    }
}

impl Line for Link {
    fn download(&self, bytes: usize) -> Option<Duration> {
        self.probe(bytes).ok()
    }

    fn echo(&self, count: u32) -> Option<Echo> {
        Link::echo(self, count)
    }

    fn burst(&self, kbps: u32, ms: u32, latency: Duration) -> Option<Burst> {
        Link::burst(self, kbps, ms, latency)
    }
}

/// One game's save folder on a linked desktop.
pub struct RemoteSaves<'a> {
    link: &'a Link,
    key: String,
    manifest: Manifest,
    /// Where the desktop's folder is, in words that hold on any machine.
    pub place: Option<SavePlace>,
    /// The desktop clears its live saves of this game after a stream.
    pub keep_clean: bool,
}

impl RemoteSaves<'_> {
    fn ask(&self, what: &str, body: &[u8]) -> std::result::Result<Vec<u8>, String> {
        self.link.ask("POST", &format!("/v1/saves/{}/{what}", self.key), body, SLOW).map_err(|e| e.to_string())
    }

    fn file(path: &str, modified: i64) -> Vec<u8> {
        serde_json::to_vec(&SaveFile { path: path.to_string(), modified }).expect("serialisable")
    }
}

/// How a desktop answered an offer to clear its copy of a game's saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetireAnswer {
    /// It took a safe copy and removed its live saves.
    Done(Retired),
    /// Its administrator did not ask for this game to be kept clean.
    NotFlagged,
    /// It checked and kept everything, for the reason given in its words.
    Kept(String),
    /// An older desktop that does not know the request.
    Unsupported,
}

impl RemoteSaves<'_> {
    /// Offer the desktop proof that this Ally holds an identical copy of the
    /// game's saves: `mine` is this Ally's description of its own folder. The
    /// desktop removes its live saves only when it is told to keep that game
    /// clean and its own files match `mine` exactly. Never retried by callers.
    pub fn retire(&self, mine: &Manifest) -> Result<RetireAnswer> {
        let path = format!("/v1/saves/{}/retire", self.key);
        let (status, bytes) = self.link.ask_status("POST", &path, &serde_json::to_vec(mine).expect("serialisable"), SLOW)?;
        match status {
            200 => Ok(RetireAnswer::Done(parse(&bytes)?)),
            403 => Ok(RetireAnswer::NotFlagged),
            404 => Ok(RetireAnswer::Unsupported),
            409 => match problem(&bytes) {
                Error::Host(text) => Ok(RetireAnswer::Kept(text)),
                other => Err(other),
            },
            _ => Err(problem(&bytes)),
        }
    }
}

impl sync::Side for RemoteSaves<'_> {
    /// As the desktop described it when it was asked.
    fn manifest(&self) -> std::result::Result<Manifest, String> {
        Ok(self.manifest.clone())
    }

    fn read(&self, path: &str) -> std::result::Result<Vec<u8>, String> {
        self.ask("read", &Self::file(path, 0))
    }

    fn write(&self, path: &str, contents: &[u8], modified: i64) -> std::result::Result<(), String> {
        let mut body = Self::file(path, modified);
        body.push(b'\n');
        body.extend_from_slice(contents);
        self.ask("write", &body).map(|_| ())
    }

    fn remove(&self, path: &str) -> std::result::Result<(), String> {
        self.ask("remove", &Self::file(path, 0)).map(|_| ())
    }

    fn protect(&self) -> std::result::Result<(), String> {
        self.ask("protect", b"{}").map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    type Seen = (String, String, Option<u16>, Option<&'static str>, u64, &'static str);
    static CALLS: Mutex<Vec<Seen>> = Mutex::new(Vec::new());

    fn remember(call: &Call) {
        CALLS.lock().unwrap_or_else(|e| e.into_inner()).push((call.method.to_string(), call.path.to_string(), call.status, call.error, call.elapsed_ms, call.route));
    }

    /// What the observer was told about requests to `path`. Tests share the
    /// observer, so each uses a path of its own.
    fn told(path: &str) -> Vec<Seen> {
        CALLS.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|c| c.1 == path).cloned().collect()
    }

    fn link(port: u16) -> Link {
        Link { address: "127.0.0.1".to_string(), port, device_id: "dev".to_string(), key: b"key of the test".to_vec() }
    }

    /// A desktop that answers one request: after `wait`, with `status`, signed
    /// with the key unless `signed` is false.
    fn desktop(wait: Duration, status: u16, signed: bool) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut byte = [0u8; 1];
            while !got.ends_with(b"\r\n\r\n") {
                if stream.read(&mut byte).unwrap_or(0) == 0 {
                    return;
                }
                got.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&got).to_lowercase();
            let header = |name: &str| head.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim().to_string()).unwrap_or_default();
            let mut body = vec![0u8; header("content-length:").parse().unwrap_or(0)];
            let _ = stream.read_exact(&mut body);
            std::thread::sleep(wait);
            let reply = b"{}";
            let sign = if signed { format!("x-carla-sign: {}\r\n", sign::reply(b"key of the test", &header("x-carla-nonce:"), status, reply)) } else { String::new() };
            let _ = write!(stream, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{sign}connection: close\r\n\r\n", reply.len());
            let _ = stream.write_all(reply);
        });
        port
    }

    #[test]
    fn a_connection_that_is_refused_is_reported_as_unreachable_without_an_answer() {
        set_observer(remember);
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let asked = std::time::Instant::now();
        assert!(matches!(link(port).achievements("observer-refused-connection"), Err(Error::Unreachable)));
        let calls = told("/v1/achievements/observer-refused-connection");
        assert_eq!(calls.len(), 1, "{calls:?}");
        let (method, _, status, error, ms, route) = calls[0].clone();
        assert_eq!((method.as_str(), status, error, route), ("GET", None, Some("unreachable"), "local"));
        assert!(ms <= asked.elapsed().as_millis() as u64);
    }

    #[test]
    fn a_signed_answer_is_reported_with_its_status_and_how_long_it_took() {
        set_observer(remember);
        let port = desktop(Duration::from_millis(120), 200, true);
        link(port).reveal().expect("the desktop answered");
        let calls = told("/v1/stream/reveal");
        assert_eq!(calls.len(), 1, "{calls:?}");
        let (method, _, status, error, ms, route) = calls[0].clone();
        assert_eq!((method.as_str(), status, error, route), ("POST", Some(200), None, "local"));
        assert!((100..3000).contains(&ms), "{ms} ms");
    }

    #[test]
    fn a_desktop_that_does_not_know_this_ally_is_reported_as_refused() {
        set_observer(remember);
        let port = desktop(Duration::ZERO, 401, false);
        assert!(matches!(link(port).pending_update("observer-refused-ally"), Err(Error::Refused)));
        let calls = told("/v1/games/update/observer-refused-ally");
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!((calls[0].2, calls[0].3), (None, Some("refused")));
    }

    #[test]
    fn an_answer_that_is_not_signed_is_reported_as_untrusted() {
        set_observer(remember);
        let port = desktop(Duration::ZERO, 200, false);
        assert!(matches!(link(port).downloads(), Err(Error::Untrusted)));
        let calls = told("/v1/downloads");
        assert!(calls.iter().any(|c| c.3 == Some("untrusted")), "{calls:?}");
    }

    #[test]
    fn the_path_is_written_without_a_query_or_the_key_of_a_save_folder() {
        assert_eq!(loggable("/v1/status"), "/v1/status");
        assert_eq!(loggable("/v1/cover/steam:10?token=secret"), "/v1/cover/steam:10");
        assert_eq!(loggable("/v1/saves/0123abcd/read"), "/v1/saves/*/read");
        assert_eq!(loggable("/v1/games/update/epic:abc"), "/v1/games/update/epic:abc");
    }

    #[test]
    fn what_the_observer_is_given_has_no_address_or_body() {
        let call = Call { method: "POST", path: "/v1/stream/reveal", status: Some(200), error: None, elapsed_ms: 3, route: "remote" };
        let text = format!("{call:?}");
        assert!(!text.contains("100.") && !text.contains("127."), "{text}");
    }
}
