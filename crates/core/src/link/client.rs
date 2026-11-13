//! The Ally's side of the companion API.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::enrol::{valid_code, Exchange, Side};
use super::{random_hex, sign, EnrolDone, EnrolFinish, EnrolOffer, EnrolStart, Hello, Pair, Prepare, Problem, RemoteGame, SaveFile, SavesFind, SavesFound, Status, Updating};
use crate::library::sync::{self, Manifest};
use crate::API_VERSION;

/// Enough for a status question on Wi-Fi; a desktop that is slower is off.
pub const QUICK: Duration = Duration::from_secs(3);
const SLOW: Duration = Duration::from_secs(40);
/// A measurement that takes longer than this has its answer: too slow.
const PROBE: Duration = Duration::from_secs(4);
/// One save file is the largest thing a desktop sends.
const LARGEST_REPLY: u64 = sync::LARGEST_FILE + 1024 * 1024;

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
        if status == 200 {
            Ok(bytes)
        } else {
            Err(problem(&bytes))
        }
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

    /// The cover picture of one game, as the file's bytes.
    pub fn cover(&self, game_id: &str) -> Result<Vec<u8>> {
        self.ask("GET", &format!("/v1/cover/{game_id}"), b"", SLOW)
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
        self.post("/v1/stream/prepare", &Prepare { game_id: game_id.to_string() }, QUICK * 2)
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
        let found: Option<SavesFound> = self.post("/v1/saves/find", &SavesFind { game_id: game_id.to_string(), title: title.to_string() }, SLOW)?;
        Ok(found.map(|found| RemoteSaves { link: self, key: found.key, manifest: found.manifest }))
    }
}

/// One game's save folder on a linked desktop.
pub struct RemoteSaves<'a> {
    link: &'a Link,
    key: String,
    manifest: Manifest,
}

impl RemoteSaves<'_> {
    fn ask(&self, what: &str, body: &[u8]) -> std::result::Result<Vec<u8>, String> {
        self.link.ask("POST", &format!("/v1/saves/{}/{what}", self.key), body, SLOW).map_err(|e| e.to_string())
    }

    fn file(path: &str, modified: i64) -> Vec<u8> {
        serde_json::to_vec(&SaveFile { path: path.to_string(), modified }).expect("serialisable")
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
