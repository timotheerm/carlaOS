//! The companion's API. See `carla_core::link` for how requests are trusted.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use carla_core::library::achievements::Achievement;
use carla_core::library::emulation;
use carla_core::library::find::{self, Candidate};
use carla_core::library::Scanned;
use carla_core::link::enrol::{self, Exchange, Shared, Side};
use carla_core::link::health::{Asking, Pulse};
use carla_core::link::probe::{self, BurstAsk};
use carla_core::link::{EnrolAsk, EnrolReveal, FitAsk, ScreenFit};
use carla_core::link::quality;
use carla_core::link::sign::{self, Replay};
use carla_core::library::saves::Backups;
use carla_core::library::sync::{self, Side as _};
use carla_core::library::downloads::{Download, Started};
use carla_core::library::owned::App;
use carla_core::link::{self, EnrolDone, EnrolFinish, EnrolOffer, EnrolStart, Hello, Install, Installing, Pair, Prepare, Problem, RemoteGame, RemoteLibrary, SaveFile, SavesFind, SavesFound, State, Status, Updating};
use carla_core::{API_VERSION, VERSION};
use serde::Serialize;

use crate::files::{Config, Extra, Files, Pending, SaveFolder, Seen};
use crate::sunshine::Streamer;
use crate::virtual_screen::{self, Driver};

#[path = "dashboard.rs"]
mod dashboard;
pub use dashboard::Vitals;

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

/// The header a local program puts its token in (see `Files::local_token`).

#[derive(Debug, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub kind: &'static str,
    pub body: Vec<u8>,
    pub sign: Option<String>,
}

/// What Steam on this desktop could install, and what it is fetching.
#[derive(Clone, Copy)]
pub struct Installs {
    /// Games of the library that are not installed here.
    pub offered: fn() -> Result<Vec<App>, String>,
    pub start: fn(&App) -> Result<Started, String>,
    pub downloads: fn() -> Vec<Download>,
    /// Room on the disk a new game would go to.
    pub free: fn() -> Option<u64>,
}

struct Waiting {
    /// Missing while a link that is allowed at the desktop has only
    /// committed to its message.
    shared: Option<Shared>,
    device_name: String,
    started: i64,
    /// Set for a link that someone at the desktop allows: this side's half
    /// of the exchange until the Ally shows its message, what the Ally
    /// committed to, and where it asks from.
    approve: Option<Approve>,
}

struct Approve {
    exchange: Option<Exchange>,
    commit: String,
    from: String,
}

impl Waiting {
    fn fresh(&self, now: i64) -> bool {
        now - self.started <= if self.approve.is_some() { enrol::APPROVE_LIFETIME } else { EXCHANGE_AGE }
    }
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
    installs: Installs,
    /// How long an Ally asking for requests is kept waiting when there are none.
    admin_hold: Duration,
    /// Makes a stream a virtual screen of its own, when there is a driver.
    screens: Option<Box<dyn Driver + Send + Sync>>,
    replay: Mutex<Replay>,
    exchanges: Mutex<HashMap<String, Waiting>>,
    games: Mutex<Option<(Instant, Vec<Found>)>>,
    health: Mutex<Option<(Instant, Result<(), String>)>>,
    /// Names of this desktop's graphics chips.
    graphics: Vec<String>,
    /// The socket measuring packets are sent from (see `probe`).
    probe: Option<std::net::UdpSocket>,
    bursting: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// When each Ally's last request was written down, to not write on every one.
    seen: Mutex<HashMap<String, (Instant, String)>>,
    /// How busy this PC is, for the dashboard.
    vitals: fn() -> Vitals,
    /// Looks for a newer release, for the dashboard.
    newer: fn() -> Result<Option<String>, String>,
    newest: Mutex<Option<(Instant, Result<Option<String>, String>)>>,
    /// Lets the dashboard's page, and nothing else, ask for more than the page.
    token: String,
    /// What each linked Ally was last heard saying.
    pings: Mutex<HashMap<String, dashboard::Pings>>,
    asked: Mutex<Option<dashboard::Asked>>,
    clients: Mutex<Option<(Instant, Option<Vec<String>>)>>,
    /// Looks for programs that may be games, for the dashboard's "Add a game".
    finder: Box<dyn Fn() -> Vec<Candidate> + Send + Sync>,
    /// What the last such look found. The page adds one of these by its
    /// id and nothing else: it never names a path or options itself.
    candidates: Mutex<Option<(Instant, Vec<dashboard::Offered>)>>,
    /// A look for games is running. At most one at a time, so that clicking
    /// the button again cannot occupy the workers that answer the Ally.
    finding: std::sync::atomic::AtomicBool,
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

/// Bytes that mean nothing and do not shrink if something on the way
/// compresses, for measuring the connection.
fn filler(bytes: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..bytes)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
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
            installs: Installs {
                offered: || Ok(Vec::new()),
                start: |_| Err("This companion cannot install games.".to_string()),
                downloads: Vec::new,
                free: || None,
            },
            admin_hold: Duration::from_secs(20),
            screens: None,
            replay: Mutex::default(),
            exchanges: Mutex::default(),
            games: Mutex::default(),
            health: Mutex::default(),
            graphics: Vec::new(),
            probe: None,
            bursting: Default::default(),
            seen: Mutex::default(),
            vitals: Vitals::default,
            newer: || Ok(None),
            newest: Mutex::default(),
            token: link::random_hex(16),
            pings: Mutex::default(),
            asked: Mutex::default(),
            clients: Mutex::default(),
            finder: Box::new(|| find::find(&find::default_roots(), &[])),
            candidates: Mutex::default(),
            finding: Default::default(),
        }
    }

    /// Look for games to add somewhere else than the usual folders.
    #[cfg(test)]
    pub fn with_finder(mut self, finder: Box<dyn Fn() -> Vec<Candidate> + Send + Sync>) -> Self {
        self.finder = finder;
        self
    }

    /// Say which graphics chips this desktop has.
    pub fn with_graphics(mut self, graphics: Vec<String>) -> Self {
        self.graphics = graphics;
        self
    }

    /// Let linked devices measure the connection with packets sent from
    /// this socket, which must be the one that answers echo packets.
    pub fn with_probe(mut self, socket: std::net::UdpSocket) -> Self {
        self.probe = Some(socket);
        self
    }

    /// Send a linked Ally a burst of packets, to the address its signed
    /// request came from. Answers at once with what is coming.
    fn burst(&self, bytes: &[u8], from: &str, now: i64) -> Answer {
        use std::sync::atomic::Ordering;
        let ask: BurstAsk = body(bytes)?;
        let Some(plan) = probe::plan(ask) else {
            return refuse(400, "That is more than a measurement needs.");
        };
        let socket = self.probe.as_ref().and_then(|s| s.try_clone().ok());
        let (Some(socket), Ok(ip)) = (socket, from.parse::<std::net::IpAddr>()) else {
            return refuse(503, "This desktop cannot send measuring packets.");
        };
        if self.files.streaming(now) {
            return refuse(409, "A stream is running. The connection is measured when it is over.");
        }
        if self.bursting.swap(true, Ordering::SeqCst) {
            return refuse(409, "Another measurement is running. Try again in a moment.");
        }
        let busy = self.bursting.clone();
        std::thread::spawn(move || {
            // Let the answer reach the Ally first, so it is listening.
            std::thread::sleep(Duration::from_millis(200));
            probe::send_burst(&socket, std::net::SocketAddr::new(ip, ask.port), plan);
            busy.store(false, Ordering::SeqCst);
        });
        json(&plan)
    }

    /// What screen a stream of this size would be shown on here.
    fn fit(&self, bytes: &[u8]) -> Answer {
        let ask: FitAsk = body(bytes)?;
        let virtual_screen = self.config.virtual_screen && self.screens.as_ref().is_some_and(|d| d.present());
        let mode = if virtual_screen { Some(ask.screen) } else { carla_core::display::choose(&carla_core::display::modes(), ask.screen) };
        json(&ScreenFit { virtual_screen, mode })
    }

    fn pulse(&self, device: &str, bytes: &[u8], now: i64) -> Answer {
        let pulse: Pulse = body(bytes)?;
        self.pulsed(device, &pulse);
        self.files.set_ally_pulse(device, pulse, now).map_err(|e| (500, e))?;
        json(&serde_json::json!({ "received": true }))
    }

    /// Write down that an Ally was heard from, at most every few seconds.
    fn note_seen(&self, device: &str, from: &str, now: i64, prepared: bool) {
        let mut seen = locked(&self.seen);
        let due = seen.get(device).is_none_or(|(at, address)| at.elapsed() >= Duration::from_secs(10) || address != from);
        if due || prepared {
            seen.insert(device.to_string(), (Instant::now(), from.to_string()));
            let before = self.files.ally_seen(device).and_then(|s| s.prepared);
            self.files.set_ally_seen(device, &Seen { at: now, address: from.to_string(), prepared: if prepared { Some(now) } else { before } });
        }
    }

    /// Tell the dashboard how busy this PC is and whether there is a newer release.
    pub fn with_dashboard(mut self, vitals: fn() -> Vitals, newer: fn() -> Result<Option<String>, String>) -> Self {
        self.vitals = vitals;
        self.newer = newer;
        self
    }

    /// Let linked devices ask this companion to update itself.
    pub fn with_updater(mut self, updater: fn() -> Result<Option<String>, String>) -> Self {
        self.updater = updater;
        self
    }

    /// Let linked devices see this desktop's library and install from it.
    pub fn with_installs(mut self, installs: Installs) -> Self {
        self.installs = installs;
        self
    }

    fn library(&self) -> Answer {
        let (games, problem) = match (self.installs.offered)() {
            Ok(apps) => (apps, None),
            Err(problem) => (Vec::new(), Some(problem)),
        };
        let games = games
            .into_iter()
            .map(|app| RemoteGame { id: format!("steam:{}", app.appid), source: "steam".to_string(), source_id: app.appid, title: app.title, has_cover: false, update: None, platform: String::new(), emulated: false, description: String::new() })
            .collect();
        json(&RemoteLibrary { games, free_bytes: (self.installs.free)(), problem })
    }

    /// Only a game of this desktop's own library, named by its id, is ever
    /// installed; the Ally cannot name anything else.
    fn install(&self, bytes: &[u8], now: i64) -> Answer {
        let wanted: Install = body(bytes)?;
        let offered = (self.installs.offered)().map_err(|e| (503, e))?;
        let Some(app) = offered.into_iter().find(|app| format!("steam:{}", app.appid) == wanted.game_id) else {
            // Asked twice, or Steam began by itself: that is what was wanted.
            if wanted.game_id.strip_prefix("steam:").is_some_and(|id| (self.installs.downloads)().iter().any(|d| d.source_id == id)) {
                return json(&Installing { asked_store: false });
            }
            return refuse(404, &format!("That game is not in the Steam library on {}, or is installed there already.", self.config.name));
        };
        // Steam has to start again to begin, which would disturb a game.
        if let Some(playing) = self.files.playing(now) {
            return refuse(409, &format!("{} is running on {}. Install {} once it is closed.", playing.title, self.config.name, app.title));
        }
        let started = (self.installs.start)(&app).map_err(|e| (503, e))?;
        // The next question about games must not get an older answer.
        *locked(&self.games) = None;
        json(&Installing { asked_store: started == Started::AskedSteam })
    }

    /// Give each stream a virtual screen of its own through this driver.
    pub fn with_virtual_screens(mut self, driver: Box<dyn Driver + Send + Sync>) -> Self {
        self.screens = Some(driver);
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

    #[cfg(test)]
    fn with_admin_hold(mut self, hold: Duration) -> Self {
        self.admin_hold = hold;
        self
    }

    /// An Ally asks whether the administrator wants something from it. The
    /// answer waits a while for a request to come, so the Ally can ask again
    /// at once without that costing anything.
    fn admin_next(&self, device: &str) -> Answer {
        let asked = Instant::now();
        loop {
            let now = crate::files::now();
            self.files.ally_listened(device, now);
            if let Some(command) = self.files.take_ally_ask(device, now) {
                return json(&Some(command));
            }
            if asked.elapsed() >= self.admin_hold {
                return json(&None::<carla_core::remote::Command>);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    fn admin_done(&self, device: &str, bytes: &[u8]) -> Answer {
        let done: carla_core::remote::Done = body(bytes)?;
        self.files.set_ally_answer(device, &done).map_err(|e| (500, e))?;
        json(&serde_json::json!({ "received": true }))
    }

    pub fn hello(&self) -> Hello {
        let approve = self.files.linking_until(crate::files::now()).is_some();
        Hello { name: self.config.name.clone(), id: self.config.id.clone(), version: VERSION.to_string(), api_version: API_VERSION, approve }
    }

    fn games(&self, fresh: bool) -> Vec<Found> {
        let mut games = locked(&self.games);
        match games.as_ref() {
            Some((at, list)) if !fresh && at.elapsed() < SCAN_AGE => list.clone(),
            _ => {
                let mut list = (self.scan)();
                // Game files of the consoles set up here, each starting its emulator.
                list.extend(self.files.consoles().iter().flat_map(emulation::scan).map(|game| ("emu", game)));
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
            platform: if g.emulated { g.platform } else { String::new() },
            emulated: g.emulated,
            update: g.update,
            description: String::new(),
        });
        let extras = self.files.extras().into_iter().filter(|e| e.exe.is_file()).map(|e| RemoteGame {
            id: format!("manual:{}", e.key),
            source: "manual".to_string(),
            source_id: e.key,
            title: e.title,
            has_cover: e.cover.is_some_and(|c| c.is_file()),
            platform: String::new(),
            emulated: false,
            update: None,
            description: e.description.as_deref().map(carla_core::library::clean_description).unwrap_or_default(),
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

    /// Close the game that is being streamed, because the player chose to
    /// on the Ally. The launcher that started it does the closing.
    fn end(&self, now: i64) -> Answer {
        let playing = self.files.playing(now).is_some();
        if playing {
            self.files.ask_end(now).map_err(|e| (500, e))?;
        }
        json(&serde_json::json!({ "ended": playing }))
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
            graphics: self.graphics.clone(),
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
        exchanges.retain(|_, w| w.fresh(now));
        exchanges.insert(enrol_id.clone(), Waiting { shared: Some(shared), device_name: name.to_string(), started: now, approve: None });
        json(&EnrolOffer { enrol_id, message })
    }

    /// A device asks to be linked without a code: someone here allows it.
    /// Only while linking is open, and one device at a time.
    fn enrol_ask(&self, bytes: &[u8], from: &str, now: i64) -> Answer {
        let ask: EnrolAsk = body(bytes)?;
        let name = ask.device_name.trim();
        if name.is_empty() || name.len() > 64 || !name.chars().all(|c| !c.is_control()) {
            return refuse(400, "The device needs a name.");
        }
        if ask.commit.len() != 64 || !ask.commit.bytes().all(|b| b.is_ascii_hexdigit()) {
            return refuse(400, "The request was not understood.");
        }
        let mut exchanges = locked(&self.exchanges);
        exchanges.retain(|_, w| w.fresh(now));
        if exchanges.values().any(|w| w.approve.is_some()) {
            return refuse(409, "Another device is asking to be linked. Answer that one at the desktop first.");
        }
        // Each request uses up one of the few tries of the open window, so
        // nobody can keep the desktop asking for ever.
        if self.files.use_code(now).is_none() {
            return refuse(403, "Linking is not open on the desktop right now. Open CarlaOS Desktop Host there first.");
        }
        let exchange = Exchange::start(Side::Desktop, enrol::APPROVE);
        let message = exchange.message();
        let enrol_id = link::random_hex(16);
        let approve = Approve { exchange: Some(exchange), commit: ask.commit.to_lowercase(), from: from.to_string() };
        exchanges.insert(enrol_id.clone(), Waiting { shared: None, device_name: name.to_string(), started: now, approve: Some(approve) });
        json(&EnrolOffer { enrol_id, message })
    }

    /// The device shows the message it committed to. From here both sides
    /// know the number to compare, and the desktop starts asking.
    fn enrol_reveal(&self, bytes: &[u8], now: i64) -> Answer {
        let reveal: EnrolReveal = body(bytes)?;
        let mut exchanges = locked(&self.exchanges);
        let Some(waiting) = exchanges.get_mut(&reveal.enrol_id).filter(|w| w.fresh(now)) else {
            return refuse(403, "That took too long. Start again.");
        };
        let Some((exchange, approve)) = waiting.approve.as_mut().and_then(|a| Some((a.exchange.take()?, a))) else {
            return refuse(400, "The request was not understood.");
        };
        let shared = if enrol::commit(&reveal.message) == approve.commit { exchange.finish(&reveal.message) } else { None };
        let Some(shared) = shared else {
            exchanges.remove(&reveal.enrol_id);
            return refuse(400, "The request was not understood.");
        };
        let asking = Asking { enrol_id: reveal.enrol_id.clone(), device_name: waiting.device_name.clone(), from: approve.from.clone(), check: shared.check(), at: now };
        waiting.shared = Some(shared);
        self.files.set_asking(&asking).map_err(|e| (500, e))?;
        json(&serde_json::json!({ "asking": true }))
    }

    fn enrol_finish(&self, bytes: &[u8], now: i64) -> Answer {
        let finish: EnrolFinish = body(bytes)?;
        let mut exchanges = locked(&self.exchanges);
        let waiting = exchanges.remove(&finish.enrol_id);
        let Some((waiting, shared)) = waiting.filter(|w| w.fresh(now)).and_then(|mut w| Some((w.shared.take()?, w)).map(|(s, w)| (w, s))) else {
            return refuse(403, "That took too long. Start again.");
        };
        if waiting.approve.is_some() {
            match self.files.asking_answer(&finish.enrol_id) {
                Some(true) => {}
                Some(false) => {
                    self.files.clear_code();
                    return refuse(403, "It was not allowed at the desktop.");
                }
                None => {
                    exchanges.insert(finish.enrol_id, Waiting { shared: Some(shared), ..waiting });
                    return refuse(409, enrol::WAITING);
                }
            }
        }
        if !shared.proof_ok(Side::Ally, &finish.proof) {
            return refuse(403, "That is not the code on the desktop.");
        }
        let device = self.files.add_device(&waiting.device_name, &shared.device_key()).map_err(|e| (500, e))?;
        self.files.clear_code();
        json(&EnrolDone { device_id: device.id, proof: shared.proof(Side::Desktop) })
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
                watch: extra.watch,
                screen: prepare.screen,
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
                watch: None,
                screen: prepare.screen,
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
        // Sunshine looks at the screens as soon as Moonlight connects, before
        // the launcher runs: the stream's own screen has to be there by then.
        // Without it the launcher switches the desktop's screen instead.
        if let (Some(driver), Some(want), true) = (&self.screens, prepare.screen, self.config.virtual_screen) {
            let _ = virtual_screen::enter(driver.as_ref(), &self.files, want);
        }
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

    /// Saves are left alone while a game runs here: it may be writing them.
    fn saves_free(&self, now: i64) -> Result<(), (u16, String)> {
        match self.files.playing(now) {
            Some(playing) => refuse(409, &format!("{} is still running on {}. Its saves are brought in step once it has closed.", playing.title, self.config.name)),
            None => Ok(()),
        }
    }

    /// The save folder the administrator named for this game: by its id, or
    /// by its name for a game that is not from a store.
    fn saves_find(&self, bytes: &[u8], now: i64) -> Answer {
        let wanted: SavesFind = body(bytes)?;
        let folders = self.files.save_folders();
        let title = wanted.title.trim().to_lowercase();
        let by_id = folders.iter().find(|f| f.game_id.as_deref() == Some(wanted.game_id.as_str()));
        let Some(found) = by_id.or_else(|| folders.iter().find(|f| !title.is_empty() && f.title.to_lowercase() == title)) else {
            return json(&None::<SavesFound>);
        };
        self.saves_free(now)?;
        let manifest = sync::manifest(&found.folder).map_err(|e| (503, e))?;
        json(&Some(SavesFound { key: found.key.clone(), manifest }))
    }

    /// One step of bringing a save folder in step: `key/what`.
    fn saves_step(&self, rest: &str, bytes: &[u8], now: i64) -> Answer {
        let unknown = || (404, "The companion does not know that request.".to_string());
        let (key, what) = rest.split_once('/').ok_or_else(unknown)?;
        let Some(SaveFolder { folder, .. }) = self.files.save_folders().into_iter().find(|f| f.key == key) else {
            return refuse(404, &format!("{} no longer keeps that game's saves in step.", self.config.name));
        };
        self.saves_free(now)?;
        let side = sync::Folder { folder, backups: Backups::new(&self.files.save_copies(), key), now };
        let done = || json(&serde_json::json!({ "done": true }));
        match what {
            "read" => {
                let file: SaveFile = body(bytes)?;
                side.read(&file.path).map(|contents| (contents, "application/octet-stream")).map_err(|e| (404, e))
            }
            "write" => {
                // One line that names the file, then its contents.
                let cut = bytes.iter().position(|b| *b == b'\n').ok_or((400, "The request was not understood.".to_string()))?;
                let file: SaveFile = body(&bytes[..cut])?;
                side.write(&file.path, &bytes[cut + 1..], file.modified).map_err(|e| (409, e))?;
                done()
            }
            "remove" => {
                let file: SaveFile = body(bytes)?;
                side.remove(&file.path).map_err(|e| (409, e))?;
                done()
            }
            "protect" => {
                side.protect().map_err(|e| (503, e))?;
                done()
            }
            _ => Err(unknown()),
        }
    }

    /// Whether a device with this id is linked. Decides how much of a request
    /// is read before its signature can be checked.
    pub fn knows(&self, device: &str) -> bool {
        self.files.devices().iter().any(|d| d.id == device)
    }

    fn signed(&self, req: &Request, now: i64) -> Answer {
        let device = req.device.unwrap_or_default();
        self.note_seen(device, req.from, now, req.path == "/v1/stream/prepare");
        match (req.method, req.path) {
            ("POST", "/v1/probe/burst") => self.burst(req.body, req.from, now),
            ("POST", "/v1/stream/fit") => self.fit(req.body),
            ("POST", "/v1/link/pulse") => self.pulse(device, req.body, now),
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
            ("GET", path) if path.starts_with("/v1/probe/") => match path["/v1/probe/".len()..].parse::<usize>() {
                Ok(bytes) if bytes <= quality::LARGEST_PROBE => Ok((filler(bytes), "application/octet-stream")),
                _ => refuse(400, "That is more than a measurement needs."),
            },
            ("GET", "/v1/library") => self.library(),
            ("POST", "/v1/install") => self.install(req.body, now),
            ("GET", "/v1/downloads") => json(&(self.installs.downloads)()),
            ("POST", "/v1/stream/prepare") => {
                let answer = self.prepare(req.body, now);
                if let (Ok(_), Ok(prepare)) = (&answer, body::<Prepare>(req.body)) {
                    self.asked_for(&prepare, now);
                }
                answer
            }
            ("POST", "/v1/stream/pair") => self.pair(req.body, req.from),
            ("POST", "/v1/power/sleep") => self.sleep(now),
            ("POST", "/v1/stream/end") => self.end(now),
            // Only the launcher of a stream that is going on listens.
            ("POST", "/v1/stream/reveal") => {
                let streaming = self.files.streaming(now);
                if streaming {
                    self.files.ask_reveal(now).map_err(|e| (500, e))?;
                }
                json(&serde_json::json!({ "revealed": streaming }))
            }
            // Read afresh: this is asked just before a game is started.
            ("GET", path) if path.starts_with("/v1/games/update/") => {
                let id = &path["/v1/games/update/".len()..];
                let fresh = self.games(true).into_iter().find(|(source, g)| format!("{source}:{}", g.source_id) == id);
                json(&fresh.and_then(|(_, g)| g.update))
            }
            ("GET", "/v1/admin/next") => self.admin_next(req.device.unwrap_or_default()),
            ("POST", "/v1/admin/done") => self.admin_done(req.device.unwrap_or_default(), req.body),
            ("POST", "/v1/update") => self.update(now),
            ("POST", "/v1/saves/find") => self.saves_find(req.body, now),
            ("POST", path) if path.starts_with("/v1/saves/") => self.saves_step(&path["/v1/saves/".len()..], req.body, now),
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
            ("POST", "/v1/enrol/ask") => Some(self.enrol_ask(req.body, req.from, now)),
            ("POST", "/v1/enrol/reveal") => Some(self.enrol_reveal(req.body, now)),
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
    // An Ally waiting for requests holds one of these, so there are spares.
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let (server, http) = (server.clone(), http.clone());
            std::thread::spawn(move || {
                while let Ok(mut request) = http.recv() {
                    let header = |name: &str| {
                        request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
                    };
                    let (device, time, nonce, signature) = (header(sign::DEVICE), header(sign::TIME), header(sign::NONCE), header(sign::SIGN));
                    let (host, token) = (header("host"), header("x-carla-token"));
                    let method = request.method().as_str().to_ascii_uppercase();
                    let path = request.url().split('?').next().unwrap_or_default().to_string();
                    // Nothing the Ally sends is large, except a save file, and
                    // that much is only read from a device that is linked.
                    let linked = device.as_deref().is_some_and(|d| server.knows(d));
                    let save = path.starts_with("/v1/saves/") && path.ends_with("/write");
                    let most = match (linked, save, path.as_str()) {
                        (true, true, _) => sync::LARGEST_FILE + 4096,
                        // The result of a request to the Ally, such as its list of games.
                        (true, _, "/v1/admin/done") => 2 * 1024 * 1024,
                        _ => 64 * 1024,
                    };
                    let mut bytes = Vec::new();
                    let read = std::io::Read::read_to_end(&mut std::io::Read::take(request.as_reader(), most), &mut bytes);
                    let from = request.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
                    // The dashboard: for this machine's own browser only.
                    let here = request.remote_addr().is_some_and(|a| a.ip().is_loopback());
                    let page = if here && read.is_ok() { server.local(&method, &path, host.as_deref(), token.as_deref(), &bytes, crate::files::now()) } else { None };
                    let reply = if let Some(page) = page {
                        page
                    } else if read.is_err() {
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
                    if reply.kind.starts_with("text/html") {
                        // The page holds a token: it is not kept, and not shown inside another page.
                        add("cache-control", "no-store");
                        add("x-frame-options", "DENY");
                        add("content-security-policy", "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src data:; frame-ancestors 'none'");
                    }
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
        fn paired(&self) -> Option<Vec<String>> {
            Some(vec!["ally".to_string()])
        }
    }

    /// What any test desktop's Steam was asked to install.
    static ASKED_FOR: Mutex<Vec<String>> = Mutex::new(Vec::new());

    #[test]
    fn a_linked_ally_installs_from_the_desktops_library_and_follows_the_download() {
        let desk = desk("install", false);
        let link = linked(&desk);
        let library = link.library().unwrap();
        assert_eq!(library.games.iter().map(|g| (g.id.as_str(), g.title.as_str())).collect::<Vec<_>>(), [("steam:40", "Not Here Yet"), ("steam:50", "Needs Steam's Window")]);
        assert_eq!((library.free_bytes, library.problem), (Some(1_000), None));

        assert!(!link.install("steam:40").unwrap().asked_store);
        assert!(link.install("steam:50").unwrap().asked_store, "the Ally is told when someone has to answer at the desktop");
        assert!(locked(&ASKED_FOR).contains(&"40".to_string()));
        // Only the library's own games, by id. Nothing else can be named.
        assert!(matches!(link.install("steam:10"), Err(Error::Host(text)) if text.contains("not in the Steam library")));
        assert!(matches!(link.install("steam:../../x"), Err(Error::Host(_))));
        // What Steam is already fetching counts as asked for.
        assert!(!link.install("steam:60").unwrap().asked_store);
        let fetching = link.downloads().unwrap();
        assert_eq!((fetching[0].source_id.as_str(), fetching[0].done, fetching[0].total), ("60", 5, 10));

        // Not while a game runs there, and never for a stranger.
        desk.files.set_playing(Some(&crate::files::Playing { game_id: "steam:10".into(), title: "First Game".into(), at: now_for_tests() }));
        assert_eq!(link.install("steam:40").err(), Some(Error::Host("First Game is running on HOME PC. Install Not Here Yet once it is closed.".into())));
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        assert_eq!(stranger.install("steam:40").err(), Some(Error::Refused));
        assert_eq!(stranger.library().err(), Some(Error::Refused));
    }
    /// What Steam on the test desktop still has to do for the second game.
    const UPDATE: carla_core::library::Update = carla_core::library::Update { state: carla_core::library::UpdateState::Needed, bytes: 1_000, done: 0 };

    /// How often any test desktop was put to sleep.
    static SLEEPS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    pub(super) struct Desk {
        pub files: Files,
        calls: Arc<Mutex<Vec<String>>>,
        pub port: u16,
    }

    /// A companion on this machine with two games, listening on a free port.
    pub(super) fn desk(name: &str, down: bool) -> Desk {
        let dir = std::env::temp_dir().join(format!("carla-companion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cover.jpg"), b"picture bytes").unwrap();
        let files = Files::new(dir.clone());
        let found_dir = dir.clone();
        let config = Config::new("HOME PC");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let sunshine = FakeSunshine { down, calls: calls.clone() };
        let scan = Box::new(move || {
            vec![
                ("steam", Scanned { source_id: "10".into(), title: "First Game".into(), install_path: dir.join("first"), cover: Some(dir.join("cover.jpg")), ..Scanned::default() }),
                ("steam", Scanned { source_id: "20".into(), title: "Second Game".into(), install_path: dir.join("second"), update: Some(UPDATE), ..Scanned::default() }),
                ("gog", Scanned { source_id: "30".into(), title: "Third Game".into(), install_path: dir.join("third"), exe: Some(dir.join("third").join("third.exe")), args: "-windowed".into(), ..Scanned::default() }),
            ]
        });
        // The measuring packets are answered on the same port number as the
        // API, as on a real desktop.
        let (http, udp) = loop {
            let http = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let port = http.server_addr().to_ip().unwrap().port();
            if let Ok(udp) = std::net::UdpSocket::bind(("127.0.0.1", port)) {
                break (http, udp);
            }
        };
        let port = http.server_addr().to_ip().unwrap().port();
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
        ).with_updater(|| Ok(Some("9.9.9".into()))).with_probe(udp.try_clone().unwrap()).with_graphics(vec!["AMD Radeon RX 9070 XT".into()]).with_dashboard(Vitals::default, || Ok(Some("9.9.9".into()))).with_finder(Box::new(move || vec![Candidate {
            title: "Pixel Quest".into(),
            exe: found_dir.join("Quest.exe"),
            folder: found_dir.clone(),
            kind: find::FoundKind::Launcher,
            score: 30,
            note: "A launcher".into(),
            watch_hint: Some("Quest-Win64-Shipping.exe".into()),
        }])).with_admin_hold(Duration::from_millis(400)).with_installs(Installs {
            offered: || Ok(vec![App { appid: "40".into(), title: "Not Here Yet".into(), installdir: "NotHere".into() }, App { appid: "50".into(), title: "Needs Steam's Window".into(), installdir: String::new() }]),
            start: |app| {
                locked(&ASKED_FOR).push(app.appid.clone());
                Ok(if app.installdir.is_empty() { Started::AskedSteam } else { Started::Quiet })
            },
            downloads: || vec![Download { source: "steam".into(), source_id: "60".into(), title: "Half Way".into(), phase: carla_core::library::downloads::Phase::Downloading, done: 5, total: 10, update: false }],
            free: || Some(1_000),
        }));
        let answering = server.clone();
        std::thread::spawn(move || carla_core::link::discover::answer(&udp, &|| answering.hello()));
        std::thread::spawn(move || serve(server, http));
        Desk { files, calls, port }
    }

    fn linked(desk: &Desk) -> Link {
        let code = desk.files.new_code().unwrap().code;
        client::enrol("127.0.0.1", desk.port, &code, "Ally").unwrap().1
    }

    #[test]
    fn the_ally_says_what_picture_it_will_ask_for_and_can_lift_the_curtain() {
        use carla_core::display::Mode;
        let desk = desk("picture", false);
        let link = linked(&desk);
        link.prepare_for("steam:10", Some(Mode::new(1920, 1200, 60))).unwrap();
        let pending = desk.files.take_pending(crate::files::now()).unwrap();
        assert_eq!(pending.screen, Some(Mode::new(1920, 1200, 60)));
        // An Ally from before this existed says nothing, which is fine.
        link.prepare("steam:10").unwrap();
        assert_eq!(desk.files.take_pending(crate::files::now()).unwrap().screen, None);
        // What Steam there still has to do, in the list and asked afresh.
        let games = link.games().unwrap();
        assert_eq!(games.iter().map(|g| g.update).collect::<Vec<_>>(), [None, Some(UPDATE), None]);
        assert_eq!(link.pending_update("steam:20").unwrap(), Some(UPDATE));
        assert_eq!(link.pending_update("steam:10").unwrap(), None);
        assert_eq!(link.pending_update("steam:99").unwrap(), None);
        // Nothing to lift without a stream.
        link.reveal().unwrap();
        assert!(!desk.files.take_reveal(crate::files::now()));
        desk.files.beat(crate::files::now());
        link.reveal().unwrap();
        assert!(desk.files.take_reveal(crate::files::now()));
        assert!(!desk.files.take_reveal(crate::files::now()), "asked once, done once");
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
        assert!(desk.files.add_extra("Paper Lanterns", std::path::Path::new("relative.exe"), "", None, None).is_err());
        assert!(desk.files.add_extra("  ", &exe, "", None, None).is_err());
        let added = desk.files.add_extra("Paper Lanterns", &exe, "-fullscreen", Some(&cover), Some("Lanterns-Win64-Shipping.exe")).unwrap();
        // Adding the same program again renames it instead of listing it twice.
        desk.files.add_extra("Paper Lanterns Deluxe", &exe, "-fullscreen", Some(&cover), Some("Lanterns-Win64-Shipping.exe")).unwrap();
        let id = format!("manual:{}", added.key);

        let games = link.games().unwrap();
        let listed: Vec<_> = games.iter().map(|g| (g.id.as_str(), g.source.as_str(), g.title.as_str(), g.has_cover)).collect();
        assert_eq!(listed, [("steam:10", "steam", "First Game", true), ("steam:20", "steam", "Second Game", false), ("gog:30", "gog", "Third Game", false), (id.as_str(), "manual", "Paper Lanterns Deluxe", true)]);
        assert_eq!(link.cover(&id).unwrap(), b"picture bytes");

        link.prepare(&id).unwrap();
        let pending = desk.files.take_pending(now_for_tests()).unwrap();
        assert_eq!((pending.source.as_str(), pending.exe.as_deref(), pending.args.as_str()), ("manual", Some(exe.as_path()), "-fullscreen"));
        assert_eq!(pending.install_path, desk.files.dir());
        assert_eq!(pending.watch.as_deref(), Some("Lanterns-Win64-Shipping.exe"));

        // A program that has gone is no longer offered.
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(link.games().unwrap().len(), 3);
        assert!(matches!(link.prepare(&id), Err(Error::Host(_))));
        assert_eq!(desk.files.drop_extra(&added.key).unwrap().as_deref(), Some("Paper Lanterns Deluxe"));
        assert_eq!(desk.files.drop_extra(&added.key).unwrap(), None);
    }

    #[test]
    fn a_description_crosses_the_link_and_the_ally_keeps_what_it_was_typed() {
        use carla_core::library::{Kind, ManualGame, Scanned, Store};
        let desk = desk("describe", false);
        let link = linked(&desk);
        let exe = desk.files.dir().join("Lanterns.exe");
        std::fs::write(&exe, b"program").unwrap();
        let added = desk.files.add_extra("Paper Lanterns", &exe, "", None, None).unwrap();
        let id = format!("manual:{}", added.key);
        let described = |link: &Link| link.games().unwrap().into_iter().find(|g| g.id == id).unwrap().description;
        assert_eq!(described(&link), "", "nothing is sent until there is something");

        let (_, kept) = desk.files.describe_extra(&added.key, "A quiet\nlantern   game.\u{7}").unwrap();
        assert_eq!(kept, "A quiet lantern   game.");
        assert_eq!(described(&link), "A quiet lantern   game.");
        // Adding the program again keeps it; a hand-edited file cannot send more than the limit.
        desk.files.add_extra("Paper Lanterns", &exe, "", None, None).unwrap();
        assert_eq!(described(&link), "A quiet lantern   game.");
        let mut extras = desk.files.extras();
        extras[0].description = Some("x".repeat(5000));
        std::fs::write(desk.files.dir().join("games.json"), serde_json::to_vec(&extras).unwrap()).unwrap();
        assert_eq!(described(&link).chars().count(), 1000);
        desk.files.describe_extra(&added.key, "From the desktop.").unwrap();

        // The Ally stores what came over the link.
        let remote = link.games().unwrap().into_iter().find(|g| g.id == id).unwrap();
        let scanned = |description: &str| Scanned { source_id: added.key.clone(), title: "Paper Lanterns".into(), description: description.into(), ..Scanned::default() };
        let mut store = Store::in_memory().unwrap();
        store.sync("desk", "manual", &[scanned(&remote.description)], 10).unwrap();
        assert_eq!(store.list().unwrap()[0].description, "From the desktop.");
        // A new text from the desktop replaces it, but clearing it there does not clear it here.
        store.sync("desk", "manual", &[scanned("Newer.")], 20).unwrap();
        assert_eq!(store.list().unwrap()[0].description, "Newer.");
        store.sync("desk", "manual", &[scanned("")], 30).unwrap();
        assert_eq!(store.list().unwrap()[0].description, "Newer.");
        // What the administrator typed on the Ally is never replaced.
        store.sync("desk", "manual", &[scanned("Newer.")], 40).unwrap();
        let own = ManualGame { title: "Paper Lanterns".into(), exe: exe.clone(), args: String::new(), kind: Kind::Game, cover: None, description: "Mine.".into(), origin: String::new() };
        store.add_manual(&own, 50).unwrap();
        store.sync("desk", "manual", &[scanned("Newer.")], 60).unwrap();
        assert_eq!(store.list().unwrap().iter().find(|g| g.title == "Paper Lanterns").map(|g| g.description.as_str()), Some("Mine."));
    }

    #[test]
    fn games_of_a_console_set_up_here_are_listed_and_streamed_through_its_emulator() {
        use carla_core::library::emulation::Console;
        let desk = desk("console", false);
        let link = linked(&desk);
        let base = desk.files.dir().join("Emulation");
        let (emulator, folder) = (base.join("Dolphin").join("Dolphin.exe"), base.join("GameCube"));
        std::fs::create_dir_all(emulator.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&emulator, b"program").unwrap();
        std::fs::write(folder.join("Tin Knight (USA).rvz"), b"x").unwrap();
        std::fs::write(folder.join("readme.txt"), b"x").unwrap();
        let typed = Console { id: String::new(), name: "GameCube".into(), emulator: emulator.clone(), options: "-b -e \"{game}\"".into(), folder: folder.clone(), endings: vec!["rvz".into()] };
        assert!(desk.files.add_console(Console { folder: "relative".into(), ..typed.clone() }).is_err());
        let console = desk.files.add_console(typed.clone()).unwrap();
        assert!(console.id.starts_with('d'), "never the same as a console of the Ally");
        // The same emulator and folder again replaces it.
        desk.files.add_console(Console { name: "Nintendo GameCube".into(), ..typed }).unwrap();
        assert_eq!(desk.files.consoles().len(), 1);

        let games = link.games().unwrap();
        let knight = games.iter().find(|g| g.source == "emu").expect("listed");
        assert_eq!((knight.title.as_str(), knight.platform.as_str(), knight.emulated), ("Tin Knight", "Nintendo GameCube", true));
        assert!(knight.id.starts_with(&format!("emu:{}-", console.id)));
        assert!(games.iter().filter(|g| g.source != "emu").all(|g| !g.emulated && g.platform.is_empty()));

        link.prepare(&knight.id).unwrap();
        let pending = desk.files.take_pending(now_for_tests()).unwrap();
        let file = folder.join("Tin Knight (USA).rvz").to_string_lossy().into_owned();
        assert_eq!((pending.source.as_str(), pending.exe.as_deref()), ("emu", Some(emulator.as_path())));
        assert_eq!(pending.args, format!("-b -e \"{file}\""));

        assert_eq!(desk.files.drop_console(&console.id).unwrap().as_deref(), Some("Nintendo GameCube"));
        assert!(link.games().unwrap().iter().all(|g| g.source != "emu"));
        assert!(matches!(link.prepare(&knight.id), Err(Error::Host(_))));
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

    #[test]
    fn saves_are_kept_in_step_with_a_desktop_only_where_its_administrator_said() {
        use carla_core::library::sync::{bring_in_step, Folder, Way};
        let desk = desk("saves", false);
        let link = linked(&desk);
        let there = desk.files.dir().join("desk-saves").join("Third Game");
        let here = desk.files.dir().join("ally").join("saves");
        std::fs::create_dir_all(&there).unwrap();
        std::fs::create_dir_all(&here).unwrap();
        let ally = Folder { folder: here.clone(), backups: Backups::new(&desk.files.dir().join("ally").join("copies"), "gog:30"), now: now_for_tests() };

        // Nothing was said on the desktop: no saves are offered, for any name.
        assert!(link.saves("gog:30", "Third Game").unwrap().is_none());
        assert!(desk.files.set_save_folder(Some("gog:30"), "Third Game", std::path::Path::new("relative")).is_err());
        assert!(desk.files.set_save_folder(Some("gog:30"), "Third Game", std::path::Path::new("/")).is_err());
        let entry = desk.files.set_save_folder(Some("gog:30"), "Third Game", &there).unwrap();
        assert!(link.saves("steam:10", "First Game").unwrap().is_none(), "other games stay private");

        // Played on the desktop first: the Ally receives.
        std::fs::write(there.join("slot1.sav"), b"desk day 1").unwrap();
        std::fs::create_dir_all(there.join("profile")).unwrap();
        std::fs::write(there.join("profile").join("big.bin"), vec![7u8; 300_000]).unwrap();
        let remote = link.saves("gog:30", "whatever it is called on the Ally").unwrap().unwrap();
        let first = bring_in_step(&ally, &remote, None).unwrap();
        assert_eq!((first.way, first.moved.files), (Way::Fetch, 2));
        assert_eq!(std::fs::read(here.join("profile").join("big.bin")).unwrap().len(), 300_000);

        // Played on the Ally: the desktop receives, larger than an ordinary
        // request may be, and keeps a safe copy of what it had.
        std::fs::write(here.join("slot1.sav"), b"ally day 2").unwrap();
        std::fs::write(here.join("profile").join("big.bin"), vec![9u8; 400_000]).unwrap();
        std::fs::write(here.join("slot2.sav"), b"line one\nline two").unwrap();
        let remote = link.saves("gog:30", "").unwrap().unwrap();
        let second = bring_in_step(&ally, &remote, Some(&first.agreed)).unwrap();
        assert_eq!((second.way, second.both_changed, second.moved.files), (Way::Send, false, 3));
        assert_eq!(std::fs::read(there.join("slot1.sav")).unwrap(), b"ally day 2");
        assert_eq!(std::fs::read(there.join("slot2.sav")).unwrap(), b"line one\nline two");
        assert_eq!(std::fs::read(there.join("profile").join("big.bin")).unwrap(), vec![9u8; 400_000]);
        let copies = Backups::new(&desk.files.save_copies(), &entry.key).list();
        assert_eq!(copies.len(), 1);
        let remote = link.saves("gog:30", "").unwrap().unwrap();
        assert_eq!(bring_in_step(&ally, &remote, Some(&second.agreed)).unwrap().way, Way::Same);

        // Nothing outside the named folder can be read or written.
        std::fs::write(desk.files.dir().join("desk-saves").join("secret.txt"), b"private").unwrap();
        use carla_core::library::sync::Side as _;
        assert!(remote.read("../secret.txt").is_err());
        assert!(remote.write("../secret.txt", b"x", 1).is_err());
        assert!(remote.remove("../secret.txt").is_err());
        assert_eq!(std::fs::read(desk.files.dir().join("desk-saves").join("secret.txt")).unwrap(), b"private");
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        assert_eq!(stranger.saves("gog:30", "").err(), Some(Error::Refused));

        // While a game runs on the desktop its saves are left alone.
        desk.files.set_playing(Some(&crate::files::Playing { game_id: "gog:30".into(), title: "Third Game".into(), at: now_for_tests() }));
        assert!(matches!(link.saves("gog:30", ""), Err(Error::Host(text)) if text.contains("still running")));
        assert!(remote.write("slot1.sav", b"x", 1).is_err());
        desk.files.set_playing(None);

        // A game that is in no store is found by its name.
        let loose = desk.files.dir().join("desk-saves").join("Lanterns");
        std::fs::create_dir_all(&loose).unwrap();
        desk.files.set_save_folder(None, "Paper Lanterns", &loose).unwrap();
        assert!(link.saves("manual:0123", " paper lanterns ").unwrap().is_some());
        assert!(link.saves("manual:0123", "Other").unwrap().is_none());

        // Once the administrator takes it back, nothing is offered again.
        assert_eq!(desk.files.clear_save_folder(&entry.key).unwrap().as_deref(), Some("Third Game"));
        assert!(link.saves("gog:30", "Third Game").unwrap().is_none());
        assert!(remote.read("slot1.sav").is_err());
    }

    #[test]
    fn the_ally_can_end_the_game_that_runs_on_the_desktop() {
        let desk = desk("end", false);
        let link = linked(&desk);
        // Nothing runs: nothing is asked of anyone.
        assert!(!link.end_game().unwrap());
        assert!(!desk.files.take_end(now_for_tests()));
        desk.files.set_playing(Some(&crate::files::Playing { game_id: "steam:10".into(), title: "First Game".into(), at: now_for_tests() }));
        assert!(link.end_game().unwrap());
        assert!(desk.files.take_end(now_for_tests()), "the launcher is told");
        assert!(!desk.files.take_end(now_for_tests()), "once");
        // A request nobody picked up does not close the next game.
        desk.files.ask_end(now_for_tests() - 60).unwrap();
        assert!(!desk.files.take_end(now_for_tests()));
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        assert_eq!(stranger.end_game().err(), Some(Error::Refused));
    }

    #[test]
    fn a_linked_ally_can_measure_the_connection_and_a_stranger_cannot() {
        use carla_core::link::quality::{self, Level, Setting};
        let desk = desk("probe", false);
        let link = linked(&desk);
        assert!(link.probe(0).is_ok());
        assert!(link.probe(300_000).is_ok());
        assert!(matches!(link.probe(quality::LARGEST_PROBE + 1), Err(Error::Host(_))), "no more than a measurement needs");
        // Over the loopback the whole measurement comes out as a fast, steady line.
        let measured = quality::measure(&|bytes| link.probe(bytes).ok(), false).expect("measured");
        assert!(measured.kbps.unwrap() > 50_000, "{measured:?}");
        assert_eq!(quality::choose(Setting::default(), Some(measured), false).level, Level::Sharp);
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        assert_eq!(stranger.probe(1024).err(), Some(Error::Refused));
        assert_eq!(quality::measure(&|bytes| stranger.probe(bytes).ok(), false), None);
    }

    #[test]
    fn the_administrator_reaches_the_ally_through_the_desktop_it_is_linked_to() {
        use carla_core::remote::{Command, Done};
        let desk = desk("admin", false);
        let link = linked(&desk);
        let device = desk.files.devices()[0].id.clone();

        // Nothing is wanted: the Ally is kept waiting a moment, then told so.
        let asked = Instant::now();
        assert_eq!(link.admin_next().unwrap(), None);
        assert!(asked.elapsed() >= Duration::from_millis(400));
        assert!(desk.files.ally_listening(&device).is_some(), "the desktop knows the Ally is listening");

        // A request typed at the desktop while the Ally waits reaches it at once.
        let command = Command { id: "c1".into(), op: "add_game".into(), args: serde_json::json!({ "title": "Paper Lanterns" }) };
        let files = desk.files.clone();
        let (later, for_device) = (command.clone(), device.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            files.ask_ally(&for_device, &later).unwrap();
        });
        assert_eq!(link.admin_next().unwrap(), Some(command));
        assert_eq!(link.admin_next().unwrap(), None, "handed out once");

        // The result comes back to whoever asked, even when it is long.
        assert!(desk.files.take_ally_answer(&device, "c1").is_none());
        let games: Vec<String> = (0..4000).map(|i| format!("A game with quite a long name, number {i}")).collect();
        let done = Done::of("c1", Ok(serde_json::json!(games)));
        link.admin_done(&done).unwrap();
        assert!(desk.files.take_ally_answer(&device, "other").is_none());
        assert_eq!(desk.files.take_ally_answer(&device, "c1"), Some(done));

        // A request for one Ally is never handed to another, or to a stranger.
        let second = linked(&desk);
        desk.files.ask_ally(&device, &Command { id: "c2".into(), op: "restart".into(), args: serde_json::Value::Null }).unwrap();
        assert_eq!(second.admin_next().unwrap(), None);
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        assert_eq!(stranger.admin_next().err(), Some(Error::Refused));
        assert_eq!(link.admin_next().unwrap().map(|c| c.id), Some("c2".to_string()));
    }

    #[test]
    fn an_ally_is_linked_by_being_allowed_at_the_desktop_without_a_code() {
        let desk = desk("approve", false);
        // Linking is closed until someone opens it at the desktop.
        assert!(!client::hello("127.0.0.1", desk.port).unwrap().approve);
        assert!(matches!(client::enrol_ask("127.0.0.1", desk.port, "Ally").err(), Some(Error::Host(text)) if text.contains("not open")));
        desk.files.new_code().unwrap();
        assert!(client::hello("127.0.0.1", desk.port).unwrap().approve);

        let asking = client::enrol_ask("127.0.0.1", desk.port, "ROGALLY").unwrap();
        let shown = desk.files.asking(crate::files::now()).expect("the desktop is asking");
        assert_eq!((shown.device_name.as_str(), shown.from.as_str()), ("ROGALLY", "127.0.0.1"));
        assert_eq!(shown.check, asking.check, "both screens show the same number");
        // Nothing is linked while nobody has answered, however often it asks.
        assert!(asking.finish().unwrap().is_none());
        assert!(asking.finish().unwrap().is_none());
        assert!(desk.files.devices().is_empty());
        // One device at a time.
        assert!(matches!(client::enrol_ask("127.0.0.1", desk.port, "Other").err(), Some(Error::Host(text)) if text.contains("Another device")));

        assert!(!desk.files.answer_asking("someone-else", true, crate::files::now()).unwrap());
        assert!(desk.files.answer_asking(&shown.enrol_id, true, crate::files::now()).unwrap());
        let (hello, link) = asking.finish().unwrap().expect("allowed");
        assert_eq!(hello.name, "HOME PC");
        assert_eq!(link.status().unwrap().name, "HOME PC");
        assert_eq!(desk.files.devices().len(), 1);
        // Linking closes with the first link, like a code that was used.
        assert!(!client::hello("127.0.0.1", desk.port).unwrap().approve);
        assert!(desk.files.asking(crate::files::now()).is_none());
    }

    #[test]
    fn a_link_that_is_refused_at_the_desktop_links_nothing() {
        let desk = desk("refuse", false);
        desk.files.new_code().unwrap();
        let asking = client::enrol_ask("127.0.0.1", desk.port, "Stranger").unwrap();
        let shown = desk.files.asking(crate::files::now()).unwrap();
        desk.files.answer_asking(&shown.enrol_id, false, crate::files::now()).unwrap();
        assert_eq!(asking.finish().err(), Some(Error::Host("It was not allowed at the desktop.".into())));
        assert!(desk.files.devices().is_empty());
        assert!(desk.files.linking_until(crate::files::now()).is_none(), "and linking is closed");
    }

    #[test]
    fn a_message_that_was_not_committed_to_is_refused() {
        use carla_core::link::enrol::{commit, Exchange, Side, APPROVE};
        let desk = desk("commit", false);
        desk.files.new_code().unwrap();
        let post = |path: &str, body: serde_json::Value| {
            let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();
            let mut reply = agent.post(&format!("http://127.0.0.1:{}{path}", desk.port)).send(body.to_string().as_bytes()).unwrap();
            (reply.status().as_u16(), serde_json::from_str::<serde_json::Value>(&reply.body_mut().read_to_string().unwrap()).unwrap())
        };
        let committed = Exchange::start(Side::Ally, APPROVE).message();
        let (status, offer) = post("/v1/enrol/ask", serde_json::json!({ "device_name": "Ally", "commit": commit(&committed) }));
        assert_eq!(status, 200);
        // Having seen the desktop's message, it tries another of its own.
        let other = Exchange::start(Side::Ally, APPROVE).message();
        assert_eq!(post("/v1/enrol/reveal", serde_json::json!({ "enrol_id": offer["enrol_id"], "message": other })).0, 400);
        assert!(desk.files.asking(crate::files::now()).is_none(), "the desktop never asks about it");
        assert_eq!(post("/v1/enrol/reveal", serde_json::json!({ "enrol_id": offer["enrol_id"], "message": committed })).0, 403, "and it is over");
    }

    #[test]
    fn the_connection_is_measured_with_packets_and_the_desktop_keeps_what_the_ally_reports() {
        use carla_core::display::Mode;
        use carla_core::link::health::{Grade, Health, Pulse};
        use carla_core::link::quality::{self, Depth, Setting};
        let desk = desk("health", false);
        let link = linked(&desk);
        let echo = link.echo(20).expect("packets come back");
        assert_eq!((echo.sent, echo.lost), (20, 0));
        let burst = link.burst(8_000, 200, Duration::from_millis(1)).expect("a burst arrives");
        assert_eq!((burst.asked_kbps, burst.lost), (8_000, 0), "{burst:?}");
        assert!(link.burst(carla_core::link::probe::MOST_KBPS + 1, 200, Duration::from_millis(1)).is_none(), "never a flood");
        let stranger = Link { key: vec![7; 32], ..link.clone() };
        assert!(stranger.burst(8_000, 200, Duration::from_millis(1)).is_none(), "and never for a stranger");

        // The whole measurement, as the check does it.
        let measured = quality::survey(&link, false, Depth::Full).expect("measured");
        assert!(measured.echo.is_some_and(|e| e.sent == 100) && measured.burst.is_some(), "{measured:?}");
        assert_eq!(measured.loss_permille(), Some(0));
        let choice = quality::choose(Setting::default(), Some(measured), false);
        let health = Health::of(measured, choice, Some(Mode::new(1920, 1080, 120)), "127.0.0.1", None, crate::files::now());
        assert!(health.grade >= Grade::Good, "{health:?}");

        // The desktop's graphics chip and the screen a stream would get.
        assert_eq!(link.status().unwrap().graphics, ["AMD Radeon RX 9070 XT"]);
        assert!(!link.fit(Mode::new(1920, 1080, 120)).unwrap().virtual_screen);

        // What the Ally reports is what the desktop's dashboard shows.
        let pulse = Pulse { version: "0.17.0".into(), moonlight: true, paired: Some(true), playing: None, battery: Some(71), charging: Some(true), screen: Some(Mode::new(1920, 1080, 120)), health: Some(health.clone()) };
        link.pulse(&pulse).unwrap();
        let ally = desk.files.allies(crate::files::now(), false).remove(0);
        assert_eq!((ally.name.as_str(), ally.connected, ally.address.as_deref()), ("Ally", true, Some("127.0.0.1")));
        assert_eq!(ally.health.as_ref(), Some(&health));
        assert_eq!(ally.pulse.as_ref().and_then(|p| p.battery), Some(71));

        // A pulse sent under a running game carries no measurement and keeps the last one.
        link.pulse(&Pulse { playing: Some("First Game".into()), health: None, ..pulse }).unwrap();
        let kept = desk.files.allies(crate::files::now(), false);
        assert_eq!(kept[0].health.as_ref(), Some(&health));
        assert_eq!(kept[0].pulse.as_ref().and_then(|p| p.playing.clone()), Some("First Game".into()));
        // Long silent, it is no longer connected; unless it is being streamed to.
        link.prepare("steam:10").unwrap();
        let later = crate::files::now() + 600;
        assert!(!desk.files.allies(later, false)[0].connected);
        let streaming = &desk.files.allies(later, true)[0];
        assert!(streaming.connected && streaming.streaming);
        // Forgetting it takes what was kept about it along.
        desk.files.forget_device(&ally.id).unwrap();
        assert!(desk.files.ally_pulse(&ally.id).is_none() && desk.files.allies(later, false).is_empty());
    }
}
