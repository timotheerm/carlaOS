//! The companion's API. See `carla_core::link` for how requests are trusted.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use carla_core::library::achievements::Achievement;
use carla_core::library::Scanned;
use carla_core::link::enrol::{Exchange, Shared, Side};
use carla_core::link::sign::{self, Replay};
use carla_core::link::{self, EnrolDone, EnrolFinish, EnrolOffer, EnrolStart, Hello, Pair, Prepare, Problem, RemoteGame, State, Status, Updating};
use carla_core::{API_VERSION, VERSION};
use serde::Serialize;

use crate::files::{Config, Extra, Files, Pending};
use crate::sunshine::Streamer;

/// How long a scan of the installed games is reused.
const SCAN_AGE: Duration = Duration::from_secs(60);
/// How long the answer to "is Sunshine all right?" is reused.
const HEALTH_AGE: Duration = Duration::from_secs(15);
/// A link that was started but not finished is dropped after this.
const EXCHANGE_AGE: i64 = 60;

pub struct Request<'a> {
    pub method: &'a str,
    /// Path without the query.
    pub path: &'a str,
    pub device: Option<&'a str>,
    pub time: Option<&'a str>,
    pub nonce: Option<&'a str>,
    pub sign: Option<&'a str>,
    pub body: &'a [u8],
    /// Address the request came from.
    pub from: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub kind: &'static str,
    pub body: Vec<u8>,
    pub sign: Option<String>,
}

struct Waiting {
    shared: Shared,
    device_name: String,
    started: i64,
}

pub struct Server {
    files: Files,
    config: Config,
    streamer: Option<Box<dyn Streamer>>,
    scan: Box<dyn Fn() -> Vec<Found> + Send + Sync>,
    /// What Sunshine runs when a stream starts.
    launcher: String,
    hardware: String,
    /// The desktop's addresses and adapter hardware addresses, read afresh
    /// each time because Tailscale and Wi-Fi come and go.
    network: fn() -> (Vec<String>, Vec<String>),
    /// A Steam game's achievements on this desktop, by app id.
    achievements: fn(&str) -> Vec<Achievement>,
    /// Puts this desktop to sleep, a moment after it is called.
    sleep: fn() -> Result<(), String>,
    /// Starts an update of this companion and says which version is coming,
    /// or `None` when this is the newest.
    updater: fn() -> Result<Option<String>, String>,
    replay: Mutex<Replay>,
    exchanges: Mutex<HashMap<String, Waiting>>,
    games: Mutex<Option<(Instant, Vec<Found>)>>,
    health: Mutex<Option<(Instant, Result<(), String>)>>,
}

/// An installed game and the store it is from ("steam", "epic", "gog").
pub type Found = (&'static str, Scanned);

type Answer = Result<(Vec<u8>, &'static str), (u16, String)>;

fn json(value: &impl Serialize) -> Answer {
    Ok((serde_json::to_vec(value).expect("serialisable"), "application/json"))
}

fn refuse<T>(status: u16, text: &str) -> Result<T, (u16, String)> {
    Err((status, text.to_string()))
}

fn body<'a, T: serde::Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, (u16, String)> {
    serde_json::from_slice(bytes).map_err(|_| (400, "The request was not understood.".to_string()))
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn picture_kind(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

impl Server {
    pub fn new(
        files: Files,
        config: Config,
        streamer: Option<Box<dyn Streamer>>,
        scan: Box<dyn Fn() -> Vec<Found> + Send + Sync>,
        launcher: String,
        hardware: String,
        network: fn() -> (Vec<String>, Vec<String>),
        achievements: fn(&str) -> Vec<Achievement>,
        sleep: fn() -> Result<(), String>,
    ) -> Self {
        Self {
            files,
            config,
            streamer,
            scan,
            launcher,
            hardware,
            network,
            achievements,
            sleep,
            updater: || Err("This companion cannot update itself.".to_string()),
            replay: Mutex::default(),
            exchanges: Mutex::default(),
            games: Mutex::default(),
            health: Mutex::default(),
        }
    }

    /// Let linked devices ask this companion to update itself.
    pub fn with_updater(mut self, updater: fn() -> Result<Option<String>, String>) -> Self {
        self.updater = updater;
        self
    }

    fn update(&self, now: i64) -> Answer {
        // The update restarts the companion, which ends a stream.
        if let Some(title) = self.status(now).playing {
            return refuse(409, &format!("{title} is running on {}. Update it when the game is closed.", self.config.name));
        }
        match (self.updater)() {
            Ok(Some(version)) => json(&Updating { updating: true, version }),
            Ok(None) => json(&Updating { updating: false, version: VERSION.to_string() }),
            Err(e) => Err((503, e)),
        }
    }

    pub fn hello(&self) -> Hello {
        Hello { name: self.config.name.clone(), id: self.config.id.clone(), version: VERSION.to_string(), api_version: API_VERSION }
    }

    fn games(&self, fresh: bool) -> Vec<Found> {
        let mut games = locked(&self.games);
        match games.as_ref() {
            Some((at, list)) if !fresh && at.elapsed() < SCAN_AGE => list.clone(),
            _ => {
                let list = (self.scan)();
                *games = Some((Instant::now(), list.clone()));
                list
            }
        }
    }

    fn find(&self, game_id: &str) -> Option<Found> {
        let wanted = |list: Vec<Found>| list.into_iter().find(|(source, g)| format!("{source}:{}", g.source_id) == game_id);
        // A game installed a moment ago is not in an older scan.
        wanted(self.games(false)).or_else(|| wanted(self.games(true)))
    }

    /// A game the administrator added by hand, whose program is still there.
    fn extra(&self, game_id: &str) -> Option<Extra> {
        let key = game_id.strip_prefix("manual:")?;
        self.files.extras().into_iter().find(|e| e.key == key && e.exe.is_file())
    }

    fn listed(&self) -> Vec<RemoteGame> {
        let steam = self.games(true).into_iter().map(|(source, g)| RemoteGame {
            id: format!("{source}:{}", g.source_id),
            source: source.to_string(),
            source_id: g.source_id,
            title: g.title,
            has_cover: g.cover.is_some(),
        });
        let extras = self.files.extras().into_iter().filter(|e| e.exe.is_file()).map(|e| RemoteGame {
            id: format!("manual:{}", e.key),
            source: "manual".to_string(),
            source_id: e.key,
            title: e.title,
            has_cover: e.cover.is_some_and(|c| c.is_file()),
        });
        steam.chain(extras).collect()
    }

    fn sleep(&self, now: i64) -> Answer {
        if let Some(playing) = self.files.playing(now) {
            return refuse(409, &format!("{} is still running on {}.", playing.title, self.config.name));
        }
        (self.sleep)().map_err(|e| (503, e))?;
        json(&serde_json::json!({ "sleeping": true }))
    }

    fn streamer(&self) -> Result<&dyn Streamer, String> {
        self.streamer.as_deref().ok_or_else(|| "The companion has not been given Sunshine's user name and password. Run its set-up again.".to_string())
    }

    fn health(&self) -> Result<(), String> {
        let mut health = locked(&self.health);
        match health.as_ref() {
            Some((at, result)) if at.elapsed() < HEALTH_AGE => result.clone(),
            _ => {
                let result = self.streamer().and_then(Streamer::check);
                *health = Some((Instant::now(), result.clone()));
                result
            }
        }
    }

    fn status(&self, now: i64) -> Status {
        let playing = self.files.playing(now);
        let problem = self.health().err();
        let state = match (&problem, &playing) {
            (Some(_), _) => State::NotReady,
            (None, Some(_)) => State::Busy,
            (None, None) => State::Ready,
        };
        let (addresses, macs) = (self.network)();
        Status {
            addresses,
            macs,
            name: self.config.name.clone(),
            id: self.config.id.clone(),
            version: VERSION.to_string(),
            state,
            playing: playing.map(|p| p.title),
            problem,
            hardware: self.hardware.clone(),
        }
    }

    fn enrol_start(&self, bytes: &[u8], now: i64) -> Answer {
        let start: EnrolStart = body(bytes)?;
        let name = start.device_name.trim();
        if name.is_empty() || name.len() > 64 {
            return refuse(400, "The device needs a name.");
        }
        // Each attempt uses up one of the code's few tries, right or wrong,
        // so the code cannot be guessed by trying them all.
        let Some(code) = self.files.use_code(now) else {
            return refuse(403, "There is no code to enter right now. Ask the desktop for a new one.");
        };
        let exchange = Exchange::start(Side::Desktop, &code);
        let message = exchange.message();
        let Some(shared) = exchange.finish(&start.message) else {
            return refuse(400, "The request was not understood.");
        };
        let enrol_id = link::random_hex(16);
        let mut exchanges = locked(&self.exchanges);
        exchanges.retain(|_, w| now - w.started <= EXCHANGE_AGE);
        exchanges.insert(enrol_id.clone(), Waiting { shared, device_name: name.to_string(), started: now });
        json(&EnrolOffer { enrol_id, message })
    }

    fn enrol_finish(&self, bytes: &[u8], now: i64) -> Answer {
        let finish: EnrolFinish = body(bytes)?;
        let waiting = locked(&self.exchanges).remove(&finish.enrol_id);
        let Some(waiting) = waiting.filter(|w| now - w.started <= EXCHANGE_AGE) else {
            return refuse(403, "That took too long. Start again.");
        };
        if !waiting.shared.proof_ok(Side::Ally, &finish.proof) {
            return refuse(403, "That is not the code on the desktop.");
        }
        let device = self.files.add_device(&waiting.device_name, &waiting.shared.device_key()).map_err(|e| (500, e))?;
        self.files.clear_code();
        json(&EnrolDone { device_id: device.id, proof: waiting.shared.proof(Side::Desktop) })
    }

    fn prepare(&self, bytes: &[u8], now: i64) -> Answer {
        let prepare: Prepare = body(bytes)?;
        let pending = match (self.extra(&prepare.game_id), self.find(&prepare.game_id)) {
            (Some(extra), _) => Pending {
                game_id: prepare.game_id.clone(),
                title: extra.title,
                source: "manual".to_string(),
                source_id: extra.key,
                install_path: extra.exe.parent().map(PathBuf::from).unwrap_or_default(),
                exe: Some(extra.exe),
                args: extra.args,
                at: now,
            },
            (None, Some((source, game))) => Pending {
                game_id: prepare.game_id.clone(),
                title: game.title,
                source: source.to_string(),
                source_id: game.source_id,
                install_path: game.install_path,
                exe: game.exe,
                args: game.args,
                at: now,
            },
            (None, None) => return refuse(404, &format!("That game is no longer installed on {}.", self.config.name)),
        };
        if let Some(playing) = self.files.playing(now) {
            if playing.game_id != prepare.game_id {
                return refuse(409, &format!("{} is still running on {}. Close it there first.", playing.title, self.config.name));
            }
            // The same game is still running: the stream picks it up again.
            return json(&self.status(now));
        }
        self.streamer().and_then(|s| s.ensure_app(&self.launcher)).map_err(|e| (503, e))?;
        self.files.set_pending(&pending).map_err(|e| (500, e))?;
        json(&self.status(now))
    }

    fn pair(&self, bytes: &[u8], from: &str) -> Answer {
        let pair: Pair = body(bytes)?;
        if pair.pin.len() != 4 || !pair.pin.bytes().all(|b| b.is_ascii_digit()) {
            return refuse(400, "The pairing PIN is four digits.");
        }
        let streamer = self.streamer().map_err(|e| (503, e))?;
        streamer.ensure_app(&self.launcher).map_err(|e| (503, e))?;
        streamer.pair(&pair.pin, &pair.name, from).map_err(|e| (503, e))?;
        json(&serde_json::json!({ "paired": true }))
    }

    fn cover(&self, game_id: &str) -> Answer {
        let cover: Option<PathBuf> = match self.extra(game_id) {
            Some(extra) => extra.cover,
            None => self.find(game_id).and_then(|(_, g)| g.cover),
        };
        match cover.and_then(|path| std::fs::read(&path).ok().map(|bytes| (bytes, picture_kind(&path)))) {
            Some(found) => Ok(found),
            None => refuse(404, "There is no picture for that game."),
        }
    }

    fn signed(&self, req: &Request, now: i64) -> Answer {
        match (req.method, req.path) {
            ("GET", "/v1/status") => json(&self.status(now)),
            ("GET", "/v1/games") => json(&self.listed()),
            ("GET", path) if path.starts_with("/v1/cover/") => self.cover(&path["/v1/cover/".len()..]),
            ("GET", path) if path.starts_with("/v1/achievements/") => {
                // Only for games that are really installed here, by their own id.
                match self.find(&path["/v1/achievements/".len()..]) {
                    Some(("steam", game)) => json(&(self.achievements)(&game.source_id)),
                    // Other stores keep achievements on their servers only.
                    Some(_) => json(&Vec::<Achievement>::new()),
                    None => refuse(404, &format!("That game is no longer installed on {}.", self.config.name)),
                }
            }
            ("POST", "/v1/stream/prepare") => self.prepare(req.body, now),
            ("POST", "/v1/stream/pair") => self.pair(req.body, req.from),
            ("POST", "/v1/power/sleep") => self.sleep(now),
            ("POST", "/v1/update") => self.update(now),
            _ => refuse(404, "The companion does not know that request."),
        }
    }

    /// The key of the device that signed this request, if it is one we know
    /// and the request is genuine. `Err` carries the key when the request is
    /// genuine but old or repeated, so the refusal can still be signed.
    fn trusted(&self, req: &Request, now: i64) -> Result<Vec<u8>, Option<Vec<u8>>> {
        let known = || {
            let (device, nonce, sign) = (req.device?, req.nonce?, req.sign?);
            let time: i64 = req.time?.parse().ok()?;
            let key = self.files.devices().into_iter().find(|d| d.id == device)?.key()?;
            sign::request_ok(&key, req.method, req.path, time, nonce, req.body, sign).then_some((key, nonce, time))
        };
        let (key, nonce, time) = known().ok_or(None)?;
        if locked(&self.replay).fresh(nonce, time, now) {
            Ok(key)
        } else {
            Err(Some(key))
        }
    }

    pub fn handle(&self, req: &Request, now: i64) -> Reply {
        let open = match (req.method, req.path) {
            ("GET", "/v1/hello") => Some(json(&self.hello())),
            ("POST", "/v1/enrol/start") => Some(self.enrol_start(req.body, now)),
            ("POST", "/v1/enrol/finish") => Some(self.enrol_finish(req.body, now)),
            _ => None,
        };
        let (answer, key) = match open {
            Some(answer) => (answer, None),
            None => match self.trusted(req, now) {
                Ok(key) => (self.signed(req, now), Some(key)),
                Err(Some(key)) => (refuse(409, "The Ally and the desktop disagree about the time. Check the clock on both."), Some(key)),
                Err(None) => (refuse(401, "This device is not linked to the desktop."), None),
            },
        };
        let (status, kind, body) = match answer {
            Ok((body, kind)) => (200, kind, body),
            Err((status, error)) => (status, "application/json", serde_json::to_vec(&Problem { error }).expect("serialisable")),
        };
        let sign = key.map(|key| sign::reply(&key, req.nonce.unwrap_or_default(), status, &body));
        Reply { status, kind, body, sign }
    }
}

/// Serve the API on an open listener until the process ends.
pub fn serve(server: std::sync::Arc<Server>, http: tiny_http::Server) {
    let http = std::sync::Arc::new(http);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let (server, http) = (server.clone(), http.clone());
            std::thread::spawn(move || {
                while let Ok(mut request) = http.recv() {
                    // Nothing the Ally sends is large.
                    let mut bytes = Vec::new();
                    let read = std::io::Read::read_to_end(&mut std::io::Read::take(request.as_reader(), 64 * 1024), &mut bytes);
                    let header = |name: &str| {
                        request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
                    };
                    let (device, time, nonce, signature) = (header(sign::DEVICE), header(sign::TIME), header(sign::NONCE), header(sign::SIGN));
                    let method = request.method().as_str().to_ascii_uppercase();
                    let path = request.url().split('?').next().unwrap_or_default().to_string();
                    let from = request.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                    let reply = if read.is_err() {
                        Reply { status: 400, kind: "application/json", body: b"{\"error\":\"The request was not understood.\"}".to_vec(), sign: None }
                    } else {
                        server.handle(
                            &Request {
                                method: &method,
                                path: &path,
                                device: device.as_deref(),
                                time: time.as_deref(),
                                nonce: nonce.as_deref(),
                                sign: signature.as_deref(),
                                body: &bytes,
                                from: &from,
                            },
                            crate::files::now(),
                        )
                    };
                    let mut response = tiny_http::Response::from_data(reply.body).with_status_code(reply.status);
                    let mut add = |name: &str, value: &str| {
                        if let Ok(h) = tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()) {
                            response.add_header(h);
                        }
                    };
                    add("content-type", reply.kind);
                    if let Some(s) = &reply.sign {
                        add(sign::SIGN, s);
                    }
                    let _ = request.respond(response);
                }
            })
        })
        .collect();
    for worker in workers {
        let _ = worker.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use carla_core::link::client::{self, Error, Link};
    use std::sync::Arc;

    #[derive(Default)]
    struct FakeSunshine {
        down: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl Streamer for FakeSunshine {
        fn check(&self) -> Result<(), String> {
            if self.down {
                Err("Sunshine is not running on this desktop.".to_string())
            } else {
                Ok(())
            }
        }
        fn ensure_app(&self, command: &str) -> Result<(), String> {
            self.check()?;
            locked(&self.calls).push(format!("app {command}"));
            Ok(())
        }
        fn pair(&self, pin: &str, name: &str, from: &str) -> Result<(), String> {
            locked(&self.calls).push(format!("pair {pin} {name} {from}"));
            Ok(())
        }
    }

    /// How often any test desktop was put to sleep.
    static SLEEPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    struct Desk {
        files: Files,
        calls: Arc<Mutex<Vec<String>>>,
        port: u16,
    }

    /// A companion on this machine with two games, listening on a free port.
    fn desk(name: &str, down: bool) -> Desk {
        let dir = std::env::temp_dir().join(format!("carla-companion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cover.jpg"), b"picture bytes").unwrap();
        let files = Files::new(dir.clone());
        let config = Config::new("HOME PC");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sunshine = FakeSunshine { down, calls: calls.clone() };
        let scan = Box::new(move || {
            vec![
                ("steam", Scanned { source_id: "10".into(), title: "First Game".into(), install_path: dir.join("first"), cover: Some(dir.join("cover.jpg")), ..Scanned::default() }),
                ("steam", Scanned { source_id: "20".into(), title: "Second Game".into(), install_path: dir.join("second"), ..Scanned::default() }),
                ("gog", Scanned { source_id: "30".into(), title: "Third Game".into(), install_path: dir.join("third"), exe: Some(dir.join("third").join("third.exe")), args: "-windowed".into(), ..Scanned::default() }),
            ]
        });
        let server = Arc::new(Server::new(
            files.clone(),
            config,
            Some(Box::new(sunshine)),
            scan,
            "companion launch".into(),
            "Test CPU · 64 GB".into(),
            || (vec!["192.168.1.20".into()], vec!["aa:bb:cc:dd:ee:ff".into()]),
            |app| vec![Achievement { id: format!("ACH_{app}"), name: "Winner".into(), description: String::new(), hidden: false, unlocked: Some(7) }],
            || {
                SLEEPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            },
        ).with_updater(|| Ok(Some("9.9.9".into()))));
        let http = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = http.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || serve(server, http));
        Desk { files, calls, port }
    }

    fn linked(desk: &Desk) -> Link {
        let code = desk.files.new_code().unwrap().code;
        client::enrol("127.0.0.1", desk.port, &code, "Ally").unwrap().1
    }

    #[test]
    fn an_ally_links_with_the_code_and_then_streams_a_game() {
        let desk = desk("link", false);
        assert_eq!(client::hello("127.0.0.1", desk.port).unwrap().name, "HOME PC");

        let code = desk.files.new_code().unwrap().code;
        let (hello, link) = client::enrol("127.0.0.1", desk.port, &code, "Ally").unwrap();
        assert_eq!(hello.name, "HOME PC");
        assert_eq!(desk.files.devices().len(), 1);
        assert_eq!(desk.files.devices()[0].name, "Ally");
        assert!(desk.files.use_code(now_for_tests()).is_none(), "a code links one device");

        let status = link.status().unwrap();
        assert_eq!((status.state, status.hardware.as_str()), (State::Ready, "Test CPU · 64 GB"));
        assert_eq!((status.addresses, status.macs), (vec!["192.168.1.20".to_string()], vec!["aa:bb:cc:dd:ee:ff".to_string()]));

        let games = link.games().unwrap();
        assert_eq!(games.iter().map(|g| (g.id.as_str(), g.has_cover)).collect::<Vec<_>>(), [("steam:10", true), ("steam:20", false), ("gog:30", false)]);
        assert_eq!(link.cover("steam:10").unwrap(), b"picture bytes");
        assert!(matches!(link.cover("steam:20"), Err(Error::Host(_))));
        assert!(matches!(link.cover("../config.json"), Err(Error::Host(_))));
        let earned = link.achievements("steam:20").unwrap();
        assert_eq!((earned[0].id.as_str(), earned[0].unlocked), ("ACH_20", Some(7)));
        assert!(matches!(link.achievements("steam:99"), Err(Error::Host(_))));
        assert!(link.achievements("gog:30").unwrap().is_empty(), "only Steam keeps achievements on the disk");

        link.pair("1234", "Ally").unwrap();
        link.prepare("steam:20").unwrap();
        assert_eq!(*locked(&desk.calls), ["app companion launch", "pair 1234 Ally 127.0.0.1", "app companion launch"]);
        locked(&desk.calls).clear();
        let pending = desk.files.take_pending(now_for_tests()).unwrap();
        assert_eq!((pending.title.as_str(), pending.source_id.as_str()), ("Second Game", "20"));
        assert!(desk.files.take_pending(now_for_tests()).is_none(), "the launcher takes it once");
        // A game from another store goes to the launcher with its own program.
        link.prepare("gog:30").unwrap();
        let pending = desk.files.take_pending(now_for_tests()).unwrap();
        assert_eq!((pending.source.as_str(), pending.source_id.as_str(), pending.args.as_str()), ("gog", "30", "-windowed"));
        assert!(pending.exe.is_some_and(|exe| exe.ends_with("third.exe")));
        assert!(matches!(link.prepare("steam:99"), Err(Error::Host(_))));
    }

    #[test]
    fn a_linked_ally_can_ask_the_desktop_to_update_and_a_stranger_cannot() {
        let desk = desk("update", false);
        let link = linked(&desk);
        let answer = link.update().unwrap();
        assert_eq!((answer.updating, answer.version.as_str()), (true, "9.9.9"));
        let stranger = Link { device_id: "someone".into(), key: vec![7; 32], ..link };
        assert_eq!(stranger.update().err(), Some(Error::Refused));
    }

    fn now_for_tests() -> i64 {
        crate::files::now()
    }

    #[test]
    fn a_wrong_code_links_nothing_and_five_tries_use_the_code_up() {
        let desk = desk("wrong", false);
        let code = desk.files.new_code().unwrap().code;
        let wrong = if code == "000000" { "000001" } else { "000000" };
        for _ in 0..enrol_tries() {
            let refused = client::enrol("127.0.0.1", desk.port, wrong, "Stranger");
            assert_eq!(refused.err(), Some(Error::Host("That is not the code on the desktop.".into())));
        }
        assert!(desk.files.devices().is_empty());
        // The right code no longer works either: it has been used up.
        let late = client::enrol("127.0.0.1", desk.port, &code, "Ally");
        assert!(matches!(late, Err(Error::Host(text)) if text.contains("no code")));
    }

    fn enrol_tries() -> u32 {
        carla_core::link::enrol::CODE_TRIES
    }

    #[test]
    fn strangers_and_forgotten_devices_are_refused() {
        let desk = desk("refuse", false);
        let link = linked(&desk);
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        assert_eq!(stranger.status().err(), Some(Error::Refused));
        let unknown = Link { device_id: "nobody".into(), ..link.clone() };
        assert_eq!(unknown.games().err(), Some(Error::Refused));
        assert!(link.status().is_ok());
        let id = desk.files.devices()[0].id.clone();
        assert!(desk.files.forget_device(&id).unwrap());
        assert_eq!(link.status().err(), Some(Error::Refused));
    }

    #[test]
    fn a_captured_request_cannot_be_replayed_or_altered() {
        let desk = desk("replay", false);
        let link = linked(&desk);
        let files = desk.files.clone();
        let server = Server::new(files, Config::new("HOME PC"), None, Box::new(Vec::new), String::new(), String::new(), || (Vec::new(), Vec::new()), |_| Vec::new(), || Ok(()));
        let (now, nonce) = (1_000_000, "0123456789abcdef0123456789abcdef");
        let body = br#"{"game_id":"steam:10"}"#;
        let signature = sign::request(&link.key, "POST", "/v1/stream/prepare", now, nonce, body);
        let time = now.to_string();
        let request = |path: &'static str, body: &'static [u8]| Request {
            method: "POST",
            path,
            device: Some(&link.device_id),
            time: Some(&time),
            nonce: Some(nonce),
            sign: Some(&signature),
            body,
            from: "10.0.0.9",
        };
        assert_eq!(server.handle(&request("/v1/stream/pair", body), now).status, 401, "signed for another path");
        assert_eq!(server.handle(&request("/v1/stream/prepare", br#"{"game_id":"steam:20"}"#), now).status, 401, "body changed");
        // Genuine: it gets as far as the game lookup (this server has no games).
        let first = server.handle(&request("/v1/stream/prepare", body), now);
        assert_eq!(first.status, 404);
        assert!(sign::reply_ok(&link.key, nonce, 404, &first.body, first.sign.as_deref().unwrap()));
        assert_eq!(server.handle(&request("/v1/stream/prepare", body), now + 1).status, 409, "sent twice");
    }

    #[test]
    fn the_desktop_says_when_it_is_busy_or_not_ready() {
        let desk = desk("busy", false);
        let link = linked(&desk);
        desk.files.set_playing(Some(&crate::files::Playing { game_id: "steam:10".into(), title: "First Game".into(), at: now_for_tests() }));
        let status = link.status().unwrap();
        assert_eq!((status.state, status.playing.as_deref()), (State::Busy, Some("First Game")));
        // Another game is refused in words the player can act on; the same one resumes.
        assert_eq!(link.prepare("steam:20").err(), Some(Error::Host("First Game is still running on HOME PC. Close it there first.".into())));
        assert!(link.prepare("steam:10").is_ok());
        assert!(desk.files.take_pending(now_for_tests()).is_none(), "a resumed game is not started twice");

        let down = super::tests::desk("down", true);
        let link = linked(&down);
        let status = link.status().unwrap();
        assert_eq!((status.state, status.problem.as_deref()), (State::NotReady, Some("Sunshine is not running on this desktop.")));
        assert_eq!(link.prepare("steam:10").err(), Some(Error::Host("Sunshine is not running on this desktop.".into())));
    }

    #[test]
    fn a_game_added_by_hand_is_listed_and_streamed_like_any_other() {
        let desk = desk("extra", false);
        let link = linked(&desk);
        let exe = desk.files.dir().join("Lanterns.exe");
        std::fs::write(&exe, b"program").unwrap();
        let cover = desk.files.dir().join("cover.jpg");
        assert!(desk.files.add_extra("Paper Lanterns", std::path::Path::new("relative.exe"), "", None).is_err());
        assert!(desk.files.add_extra("  ", &exe, "", None).is_err());
        let added = desk.files.add_extra("Paper Lanterns", &exe, "-fullscreen", Some(&cover)).unwrap();
        // Adding the same program again renames it instead of listing it twice.
        desk.files.add_extra("Paper Lanterns Deluxe", &exe, "-fullscreen", Some(&cover)).unwrap();
        let id = format!("manual:{}", added.key);

        let games = link.games().unwrap();
        let listed: Vec<_> = games.iter().map(|g| (g.id.as_str(), g.source.as_str(), g.title.as_str(), g.has_cover)).collect();
        assert_eq!(listed, [("steam:10", "steam", "First Game", true), ("steam:20", "steam", "Second Game", false), ("gog:30", "gog", "Third Game", false), (id.as_str(), "manual", "Paper Lanterns Deluxe", true)]);
        assert_eq!(link.cover(&id).unwrap(), b"picture bytes");

        link.prepare(&id).unwrap();
        let pending = desk.files.take_pending(now_for_tests()).unwrap();
        assert_eq!((pending.source.as_str(), pending.exe.as_deref(), pending.args.as_str()), ("manual", Some(exe.as_path()), "-fullscreen"));
        assert_eq!(pending.install_path, desk.files.dir());

        // A program that has gone is no longer offered.
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(link.games().unwrap().len(), 3);
        assert!(matches!(link.prepare(&id), Err(Error::Host(_))));
        assert_eq!(desk.files.drop_extra(&added.key).unwrap().as_deref(), Some("Paper Lanterns Deluxe"));
        assert_eq!(desk.files.drop_extra(&added.key).unwrap(), None);
    }

    #[test]
    fn a_linked_ally_can_send_the_desktop_to_sleep_unless_a_game_runs() {
        let desk = desk("sleep", false);
        let link = linked(&desk);
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        let before = SLEEPS.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(stranger.sleep().err(), Some(Error::Refused));
        desk.files.set_playing(Some(&crate::files::Playing { game_id: "steam:10".into(), title: "First Game".into(), at: now_for_tests() }));
        assert_eq!(link.sleep().err(), Some(Error::Host("First Game is still running on HOME PC.".into())));
        assert_eq!(SLEEPS.load(std::sync::atomic::Ordering::SeqCst), before);
        desk.files.set_playing(None);
        link.sleep().unwrap();
        assert_eq!(SLEEPS.load(std::sync::atomic::Ordering::SeqCst), before + 1);
    }
}
