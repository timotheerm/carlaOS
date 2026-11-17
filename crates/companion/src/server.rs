//! The companion's API. See `carla_core::link` for how requests are trusted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use carla_core::library::achievements::Achievement;
use carla_core::library::emulation;
use carla_core::library::find::{self, Candidate};
use carla_core::library::Scanned;
use carla_core::link::enrol::{self, Exchange, Shared, Side};
use carla_core::link::health::{Asking, Pulse, CONNECTED_FOR};
use carla_core::link::probe::{self, BurstAsk};
use carla_core::link::{EnrolAsk, EnrolReveal, FitAsk, ScreenFit};
use carla_core::link::quality;
use carla_core::link::sign::{self, Replay};
use carla_core::library::saves::Backups;
use carla_core::library::sync::{self, Side as _};
use carla_core::library::downloads::{Download, Started};
use carla_core::library::owned::App;
use carla_core::library::saveplace;
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
    /// Held while the disks are being scanned for games. The list above is
    /// not, so that asking what is known never waits for a scan.
    scanning: Mutex<()>,
    health: Mutex<Option<(Instant, Result<(), String>)>>,
    /// Held while Sunshine is being asked, for the same reason.
    checking: Mutex<()>,
    /// When the list of games was last thrown away (a game was installed): a
    /// scan that began before that is not kept.
    cleared: Mutex<Instant>,
    /// Lines of the log that would repeat for every stranger's request: per
    /// kind, when the window began, how many were left out, and who asked last.
    noise: Mutex<HashMap<String, (Instant, u32, String)>>,
    noise_window: Duration,
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

/// A device as the log names it: the first six letters of its id.
fn label(id: &str) -> String {
    id.chars().take(6).collect()
}

/// Remove the listed files from a save folder, each only if it still holds
/// exactly what was checked. One that changed since (written meanwhile, or
/// gone) stays where it is and is counted in `skipped`. Only plain files
/// inside the folder, never behind a link; the folder itself stays.
fn remove_checked(side: &sync::Folder, files: &[sync::Entry], backup: &str) -> Result<link::Retired, String> {
    let mut done = link::Retired { files: 0, bytes: 0, skipped: 0, backup: backup.to_string() };
    for file in files {
        if !side.read(&file.path).is_ok_and(|now| sync::fingerprint(&now) == file.hash) {
            done.skipped += 1;
            continue;
        }
        side.remove(&file.path).map_err(|e| format!("{e} Some saves may already be removed; the safe copy {backup} holds them all."))?;
        done.files += 1;
        done.bytes += file.size;
    }
    Ok(done)
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
            scanning: Mutex::default(),
            health: Mutex::default(),
            checking: Mutex::default(),
            cleared: Mutex::new(Instant::now()),
            noise: Mutex::default(),
            noise_window: Duration::from_secs(10),
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
        self.files.host(&format!("Installing {} (steam:{}) was asked for.", app.title, app.appid));
        // The next question about games must not get an older answer.
        *locked(&self.cleared) = Instant::now();
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
            Ok(Some(version)) => {
                self.files.host(&format!("Updating to {version} was asked for: the companion restarts."));
                json(&Updating { updating: true, version })
            }
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

    /// The installed games. `fresh` asks for a scan that began after this
    /// call; several asking at once share one scan rather than each making
    /// its own one after the other.
    fn games(&self, fresh: bool) -> Vec<Found> {
        let asked = Instant::now();
        let usable = |known: &Option<(Instant, Vec<Found>)>| match known {
            Some((at, list)) if (!fresh && at.elapsed() < SCAN_AGE) || (fresh && *at >= asked) => Some(list.clone()),
            _ => None,
        };
        if let Some(list) = usable(&locked(&self.games)) {
            return list;
        }
        // Waits for a scan that is running, whose answer may be what is wanted.
        let _scanning = locked(&self.scanning);
        if let Some(list) = usable(&locked(&self.games)) {
            return list;
        }
        let begun = Instant::now();
        let mut list = (self.scan)();
        // Game files of the consoles set up here, each starting its emulator.
        list.extend(self.files.consoles().iter().flat_map(emulation::scan).map(|game| ("emu", game)));
        // A scan that began before a game was installed does not show it: it is
        // answered to whoever asked, but not kept for those who ask next.
        if begun >= *locked(&self.cleared) {
            *locked(&self.games) = Some((Instant::now(), list.clone()));
        }
        list
    }

    /// The list for the Ally's request for the games. While a scan runs, the
    /// list from before is answered at once: a worker kept waiting for the
    /// disks is one less to answer the Ally's other requests.
    fn games_listing(&self) -> Vec<Found> {
        if matches!(self.scanning.try_lock(), Err(std::sync::TryLockError::WouldBlock)) {
            if let Some((_, list)) = locked(&self.games).as_ref() {
                return list.clone();
            }
        }
        self.games(true)
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
        let steam = self.games_listing().into_iter().map(|(source, g)| RemoteGame {
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
        self.files.host("The Ally asked this PC to go to sleep.");
        json(&serde_json::json!({ "sleeping": true }))
    }

    /// Close the game that is being streamed, because the player chose to
    /// on the Ally. The launcher that started it does the closing.
    fn end(&self, now: i64) -> Answer {
        let playing = self.files.playing(now);
        if let Some(playing) = &playing {
            self.files.ask_end(now).map_err(|e| (500, e))?;
            self.files.host(&format!("Closing {} was asked for; the launcher closes it.", playing.title));
        }
        json(&serde_json::json!({ "ended": playing.is_some() }))
    }

    fn streamer(&self) -> Result<&dyn Streamer, String> {
        self.streamer.as_deref().ok_or_else(|| "The companion has not been given Sunshine's user name and password. Run its set-up again.".to_string())
    }

    /// Whether Sunshine is all right, reused for a few seconds. While it is
    /// being asked (Sunshine may take seconds to answer), the answer from
    /// before is given at once, so that no worker waits for it.
    fn health(&self) -> Result<(), String> {
        let known = || locked(&self.health).clone();
        match known() {
            Some((at, result)) if at.elapsed() < HEALTH_AGE => return result,
            Some((_, result)) if matches!(self.checking.try_lock(), Err(std::sync::TryLockError::WouldBlock)) => return result,
            _ => {}
        }
        let _checking = locked(&self.checking);
        if let Some((at, result)) = known() {
            if at.elapsed() < HEALTH_AGE {
                return result;
            }
        }
        let result = self.streamer().and_then(Streamer::check);
        *locked(&self.health) = Some((Instant::now(), result.clone()));
        result
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
        self.files.host(&format!("A device was linked: {}.", label(&device.id)));
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
            self.files.host(&format!("Prepare: {} ({}) is already running; the stream picks it up again.", playing.title, playing.game_id));
            return json(&self.status(now));
        }
        self.streamer().and_then(|s| s.ensure_app(&self.launcher)).map_err(|e| (503, e))?;
        self.files.set_pending(&pending).map_err(|e| (500, e))?;
        let screen = pending.screen.map_or("the desktop's screen as it is".to_string(), |s| format!("a screen of {}", s.words()));
        self.files.host(&format!("Prepare: {} ({}) is ready to start when Moonlight connects, for {screen}.", pending.title, pending.game_id));
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
        self.files.host("Pairing with Moonlight began (the PIN is not written down).");
        let paired = streamer.ensure_app(&self.launcher).and_then(|_| streamer.pair(&pair.pin, &pair.name, from));
        match &paired {
            Ok(()) => self.files.host("Pairing with Moonlight worked."),
            Err(e) => self.files.host(&format!("Pairing with Moonlight failed: {e}")),
        }
        paired.map_err(|e| (503, e))?;
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
            // No folder here: at most a suggestion is noted. Nothing is read,
            // created or synced until the administrator accepts it.
            if let Some(place) = &wanted.place {
                self.suggest_saves(&wanted, place, now);
            }
            return json(&None::<SavesFound>);
        };
        self.saves_free(now)?;
        let manifest = sync::manifest(&found.folder).map_err(|e| (503, e))?;
        json(&Some(SavesFound { key: found.key.clone(), manifest, place: found.place.clone(), keep_clean: found.keep_clean }))
    }

    /// Note where this desktop's own folder for a game probably is, from
    /// where the Ally says it is on another desktop.
    fn suggest_saves(&self, wanted: &SavesFind, place: &saveplace::SavePlace, now: i64) {
        let listed = self.find(&wanted.game_id);
        let game_folder = listed.as_ref().map(|(_, g)| g.install_path.clone());
        let Some(folder) = saveplace::resolve(place, &self.files.place_env(), game_folder.as_deref()).filter(|f| !saveplace::loads_code(f, Some(place))) else { return };
        // Only where the folder, or the one it would be made in, is really there.
        if !folder.is_dir() && !folder.parent().is_some_and(Path::is_dir) {
            return;
        }
        let (game_id, title) = match &listed {
            Some((source, g)) => (Some(format!("{source}:{}", g.source_id)), g.title.clone()),
            None => (None, wanted.title.clone()),
        };
        let _ = self.files.suggest(game_id.as_deref(), &title, &folder, place, now);
    }

    /// One step of bringing a save folder in step: `key/what`.
    fn saves_step(&self, rest: &str, bytes: &[u8], now: i64) -> Answer {
        let unknown = || (404, "The companion does not know that request.".to_string());
        let (key, what) = rest.split_once('/').ok_or_else(unknown)?;
        let Some(entry) = self.files.save_folders().into_iter().find(|f| f.key == key) else {
            return refuse(404, &format!("{} no longer keeps that game's saves in step.", self.config.name));
        };
        // Every route that changes saves takes this one lock, so another
        // Ally's write cannot land between a retirement's check and its removal.
        static SAVES: Mutex<()> = Mutex::new(());
        let _changing = (what != "read").then(|| locked(&SAVES));
        if what == "retire" {
            return self.retire(&entry, bytes, now);
        }
        self.saves_free(now)?;
        let folder = entry.folder;
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

    /// Remove this desktop's live saves of a game, but only when its
    /// administrator asked for that (`saves clean KEY on`) and the Ally proves
    /// it holds exactly the same files. A safe copy is taken and checked
    /// first. Anything doubtful keeps everything where it is.
    fn retire(&self, entry: &SaveFolder, bytes: &[u8], now: i64) -> Answer {
        // Called with the saves lock held (see `saves_step`).
        if !entry.keep_clean {
            return refuse(403, &format!("{} is not set to keep that game's saves off the desktop.", self.config.name));
        }
        self.saves_free(now)?;
        let ally: sync::Manifest = body(bytes)?;
        if !ally.sound() {
            return refuse(400, "The Ally described its saves in a way the desktop does not trust. Nothing was removed.");
        }
        if ally.files.is_empty() {
            return refuse(409, "The Ally holds no saves of this game, so nothing was removed.");
        }
        let same = |a: &sync::Manifest, b: &sync::Manifest| {
            a.files.len() == b.files.len() && a.files.iter().zip(&b.files).all(|(x, y)| x.path == y.path && x.size == y.size && x.hash == y.hash)
        };
        let live = || sync::manifest(&entry.folder).map_err(|e| (503, e));
        let mine = live()?;
        if !same(&mine, &ally) {
            return refuse(409, "The saves on the desktop are not identical to the Ally's, so nothing was removed.");
        }
        let side = sync::Folder { folder: entry.folder.clone(), backups: Backups::new(&self.files.save_copies(), &entry.key), now };
        let backup = match side.backups.take_retired(&entry.folder, now) {
            Ok(Some(backup)) => backup,
            Ok(None) => return refuse(503, "The saves could not be backed up, so nothing was removed."),
            Err(e) => return refuse(503, &format!("{e} Nothing was removed.")),
        };
        // The safe copy must really hold what is about to go, and nothing
        // may have changed meanwhile.
        let copied = sync::manifest(&side.backups.path_of(&backup.id)).map_err(|e| (503, e))?;
        if !same(&copied, &mine) {
            return refuse(503, "The safe copy of the saves did not come out right, so nothing was removed.");
        }
        if !same(&live()?, &mine) {
            return refuse(409, "The saves on the desktop changed meanwhile, so nothing was removed.");
        }
        self.saves_free(now)?;
        let done = remove_checked(&side, &mine.files, &backup.id).map_err(|e| (409, e))?;
        json(&done)
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
                Err(Some(key)) => {
                    self.files.host(&format!("Refused a request of {}: it is too old, too new or repeated (the Ally's and the desktop's clocks differ).", label(req.device.unwrap_or_default())));
                    (refuse(409, "The Ally and the desktop disagree about the time. Check the clock on both."), Some(key))
                }
                Err(None) => {
                    let why = match req.device {
                        None => "it names no device".to_string(),
                        Some(device) if self.knows(device) => format!("device {} is linked, but the request's signature does not match (its key changed, or the request was altered)", label(device)),
                        Some(device) => format!("device {} is not linked here (forgotten, or linked to another desktop)", label(device)),
                    };
                    let linked = req.device.is_some_and(|d| self.knows(d));
                    if linked || self.noisy("refused detail", &who(req.device, false, false), Instant::now()) {
                        self.files.host(&format!("Refused {} {}: {why}.", carla_core::library::find::visible(req.method).chars().take(12).collect::<String>(), route_shape(req.path)));
                    }
                    (refuse(401, "This device is not linked to the desktop."), None)
                }
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

/// The workers that answer requests. Each takes one request at a time, and
/// one that waits for something holds its worker for as long as it waits.
const WORKERS: usize = 8;
/// How many of the workers may be on requests that can take long at once
/// (the saves, a look for games, pairing, an Ally waiting for a request from
/// the administrator). The rest are always free for the status, the games and
/// the pulse, so that a few slow requests cannot make the Ally look gone.
const LONG_AT_ONCE: usize = 4;
/// A request that takes longer than this is marked in the log.
const SLOW: Duration = Duration::from_secs(1);
/// How long one read or write on a connection may wait for the other side.
/// A client that went away, such as an Ally that dropped off Wi-Fi in the
/// middle of a request, is let go after this and frees its worker.
const IO_WAIT: Duration = Duration::from_secs(10);
/// How long a request's body may take to arrive, however slowly it trickles in.
const BODY_WITHIN: Duration = Duration::from_secs(30);
/// How often the Allies and Sunshine are looked at for the log.
const NOTICE_EVERY: Duration = Duration::from_secs(5);

/// Open the API's port. The sockets it accepts are given timeouts (see
/// [`IO_WAIT`]): `tiny_http` has none of its own.
pub fn bind(addr: impl std::net::ToSocketAddrs) -> Result<(tiny_http::Server, Result<(), String>), Box<dyn std::error::Error + Send + Sync + 'static>> {
    bind_within(addr, IO_WAIT)
}

/// The server, and whether sockets it accepts really have the timeouts: that
/// is tried here on a connection to the listener itself, not assumed. When
/// they do not, the body deadline is all that is left, and the caller says so.
fn bind_within(addr: impl std::net::ToSocketAddrs, wait: Duration) -> Result<(tiny_http::Server, Result<(), String>), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let listener = std::net::TcpListener::bind(addr)?;
    let timeouts = socket_timeouts(&listener, wait).map_err(|e| format!("could not set them ({e})")).and_then(|()| inherited(&listener, wait));
    Ok((tiny_http::Server::from_listener(listener, None)?, timeouts))
}

/// Connect to the listener, accept that connection and read back its timeout.
fn inherited(listener: &std::net::TcpListener, wait: Duration) -> Result<(), String> {
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let _client = std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(2)).map_err(|e| format!("could not be tried ({e})"))?;
    let (accepted, _) = listener.accept().map_err(|e| format!("could not be tried ({e})"))?;
    match accepted.read_timeout() {
        Ok(Some(got)) if got + Duration::from_millis(50) >= wait => Ok(()),
        _ => Err("accepted sockets do not inherit the timeouts".to_string()),
    }
}

/// Give the sockets that `listener` accepts a read and a write timeout. They
/// are inherited from the listening socket, which `std` has no call for.
#[cfg(windows)]
fn socket_timeouts(listener: &std::net::TcpListener, wait: Duration) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    #[link(name = "ws2_32")]
    extern "system" {
        fn setsockopt(socket: usize, level: i32, name: i32, value: *const u8, length: i32) -> i32;
    }
    const SOL_SOCKET: i32 = 0xffff;
    const SO_SNDTIMEO: i32 = 0x1005;
    const SO_RCVTIMEO: i32 = 0x1006;
    // Windows takes milliseconds, as a 32-bit number.
    let millis = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
    for option in [SO_RCVTIMEO, SO_SNDTIMEO] {
        // SAFETY: a valid socket of this process, and a 4-byte value as the option wants.
        let failed = unsafe { setsockopt(listener.as_raw_socket() as usize, SOL_SOCKET, option, (&millis as *const u32).cast(), 4) };
        if failed != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn socket_timeouts(_: &std::net::TcpListener, _: Duration) -> std::io::Result<()> {
    Ok(())
}

/// What the pool of workers may do (see `serve_with`).
#[derive(Clone, Copy)]
struct Limits {
    workers: usize,
    long: usize,
    body: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { workers: WORKERS, long: LONG_AT_ONCE, body: BODY_WITHIN }
    }
}

/// How many workers are busy, and how many of those on long requests.
struct Load {
    limits: Limits,
    busy: AtomicUsize,
    long: AtomicUsize,
    /// Threads that answer a request whose body never came whole.
    detached: AtomicUsize,
    warned: Mutex<Option<Instant>>,
    /// Where request lines go to be written by a thread of their own, so that
    /// a slow disk never keeps a worker (or its place) after the answer is sent.
    lines: Mutex<Option<std::sync::mpsc::SyncSender<String>>>,
}

/// How many such threads there can be: each waits for a client that went
/// quiet. Past that, the connection is let go without an answer.
const DETACHED_AT_ONCE: usize = 16;

/// A place taken in a count, given back when it goes.
struct Slot<'a>(&'a AtomicUsize);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Load {
    fn new(limits: Limits) -> Self {
        Self { limits, busy: AtomicUsize::new(0), long: AtomicUsize::new(0), detached: AtomicUsize::new(0), warned: Mutex::new(None), lines: Mutex::new(None) }
    }

    /// A worker takes a request. Also says whether that made every worker busy.
    fn start(&self) -> (Slot<'_>, bool) {
        let busy = self.busy.fetch_add(1, Ordering::SeqCst) + 1;
        (Slot(&self.busy), busy >= self.limits.workers)
    }

    /// A worker takes a request that can take long. `None` when enough
    /// workers are on such requests already.
    fn start_long(&self) -> Option<Slot<'_>> {
        if self.long.fetch_add(1, Ordering::SeqCst) >= self.limits.long {
            self.long.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Slot(&self.long))
    }

    /// A place for a thread that answers a client that went quiet. `false` when all are taken.
    /// Write a request's line down, without waiting for the disk. A full queue drops the line.
    fn say(&self, server: &Server, line: String) {
        match locked(&self.lines).as_ref() {
            Some(queue) => {
                let _ = queue.try_send(line);
            }
            None => server.files.host(&line),
        }
    }

    fn start_detached(&self) -> bool {
        if self.detached.fetch_add(1, Ordering::SeqCst) >= DETACHED_AT_ONCE {
            self.detached.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Whether to say again that every worker is busy: not more than every ten seconds.
    fn warn_due(&self) -> bool {
        let mut warned = locked(&self.warned);
        let due = warned.is_none_or(|at| at.elapsed() >= Duration::from_secs(10));
        if due {
            *warned = Some(Instant::now());
        }
        due
    }
}

/// Requests that can hold a worker for long.
fn takes_long(path: &str) -> bool {
    path.starts_with("/v1/saves/") || matches!(path, "/v1/admin/next" | "/v1/install" | "/v1/stream/pair" | "/local/find" | "/local/games")
}

/// A path as the log writes it: what kind of request it is, never which
/// game, save folder or file it is about.
fn route_shape(path: &str) -> String {
    const PLAIN: [&str; 21] = [
        "/v1/hello", "/v1/enrol/start", "/v1/enrol/finish", "/v1/enrol/ask", "/v1/enrol/reveal", "/v1/probe/burst", "/v1/stream/fit", "/v1/link/pulse", "/v1/status", "/v1/games", "/v1/library",
        "/v1/install", "/v1/downloads", "/v1/stream/prepare", "/v1/stream/pair", "/v1/power/sleep", "/v1/stream/end", "/v1/stream/reveal", "/v1/admin/next", "/v1/admin/done", "/v1/update",
    ];
    if path == "/" || path == "/v1/saves/find" || PLAIN.contains(&path) {
        return path.to_string();
    }
    if path.starts_with("/local/") {
        // Text of whoever asked: without characters that hide or reorder others, and short.
        return carla_core::library::find::visible(path).chars().filter(|c| !c.is_control()).take(40).collect();
    }
    for (prefix, rest) in [("/v1/cover/", ":game"), ("/v1/achievements/", ":game"), ("/v1/games/update/", ":game"), ("/v1/probe/", ":bytes")] {
        if path.starts_with(prefix) {
            return format!("{prefix}{rest}");
        }
    }
    if let Some((_, what)) = path.strip_prefix("/v1/saves/").and_then(|rest| rest.split_once('/')) {
        let what = if matches!(what, "read" | "write" | "remove" | "protect" | "retire") { what } else { "?" };
        return format!("/v1/saves/:folder/{what}");
    }
    "(unknown)".to_string()
}

/// Who a request is from, in the log: the device's short label, "stranger"
/// for an id that is not linked here, "unsigned" for no id, "this PC" for the dashboard.
fn who(device: Option<&str>, linked: bool, local: bool) -> String {
    match device {
        _ if local => "this PC".to_string(),
        Some(id) if linked => label(id),
        Some(_) => "stranger".to_string(),
        None => "unsigned".to_string(),
    }
}

/// The dashboard asks every two seconds: that is not written down, unless it went wrong.
fn too_common(method: &str, path: &str, status: u16) -> bool {
    method == "GET" && status == 200 && (path == "/" || path.starts_with("/local/"))
}

/// One line of the log for one request: no body, no header, no query, no key.
fn request_line(method: &str, path: &str, status: u16, took: Duration, who: &str, reason: Option<&str>) -> String {
    let long_wait = path == "/v1/admin/next";
    let mut line = format!("{method} {} -> {status} in {} ms ({who})", route_shape(path), took.as_millis());
    if took >= SLOW && !long_wait {
        line.push_str(" SLOW");
    }
    if let Some(reason) = reason {
        line.push_str(": ");
        line.push_str(reason);
    }
    line
}

/// How long a body of up to `most` bytes may take to come: little for what
/// is small, and a second per megabyte more for a save over weak Wi-Fi.
fn body_time(base: Duration, most: usize) -> Duration {
    if most <= 64 * 1024 {
        return base.min(Duration::from_secs(5));
    }
    (base + Duration::from_secs(most as u64 / (1024 * 1024))).min(Duration::from_secs(180).max(base))
}

/// Read a request's body, at most `most` bytes, and give up when it has not
/// come whole within `within`.
fn read_body(reader: &mut dyn std::io::Read, most: usize, within: Duration) -> std::io::Result<Vec<u8>> {
    let deadline = Instant::now() + within;
    let mut reader = std::io::Read::take(reader, most as u64);
    let (mut bytes, mut chunk) = (Vec::new(), [0u8; 16 * 1024]);
    loop {
        let got = std::io::Read::read(&mut reader, &mut chunk)?;
        if got == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..got]);
        if Instant::now() > deadline {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
    }
}

fn plain_reply(status: u16, error: &str) -> Reply {
    Reply { status, kind: "application/json", body: serde_json::to_vec(&Problem { error: error.to_string() }).expect("serialisable"), sign: None }
}

/// Answer one request. Everything that is written down about it is written here.
fn work(server: &Server, load: &std::sync::Arc<Load>, mut request: tiny_http::Request) {
    let started = Instant::now();
    let (_busy, crowded) = load.start();
    if crowded && load.warn_due() {
        server.files.host(&format!("All {} workers are busy: new requests wait until one is free.", load.limits.workers));
    }
    let header = |name: &str| request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string());
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
        // The Ally's description of its saves: a few hundred KB at most.
        (true, _, p) if p.starts_with("/v1/saves/") && p.ends_with("/retire") => 1024 * 1024,
        // The result of a request to the Ally, such as its list of games.
        (true, _, "/v1/admin/done") => 2 * 1024 * 1024,
        _ => 64 * 1024,
    };
    let from = request.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    // The dashboard: for this machine's own browser only.
    let here = request.remote_addr().is_some_and(|a| a.ip().is_loopback());
    // Only so many workers at once may be on requests that can take long, and
    // only a linked device or this PC may take one: someone else on the network
    // cannot use them up by sending half a request. What they may send is small
    // and has little time to come.
    let long_path = takes_long(&path);
    let trusted = linked || (here && path.starts_with("/local/"));
    let most = if long_path && !trusted { most.min(4096) } else { most };
    let long = (long_path && trusted).then(|| load.start_long());
    let too_many = matches!(long, Some(None));
    let within = body_time(load.limits.body, most as usize);
    let read = if too_many { Ok(Vec::new()) } else { read_body(request.as_reader(), most as usize, within) };
    let page = match &read {
        Ok(bytes) if here && !too_many => server.local(&method, &path, host.as_deref(), token.as_deref(), bytes, crate::files::now()),
        _ => None,
    };
    let reply = match (page, &read) {
        _ if too_many => {
            server.files.host(&format!("Too many long requests at once: {} {} was turned away.", method, route_shape(&path)));
            plain_reply(503, "This desktop is busy with other requests. Try again in a moment.")
        }
        (Some(page), _) => page,
        (None, Err(e)) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => plain_reply(408, "The request did not arrive in time."),
        (None, Err(_)) => plain_reply(400, "The request was not understood."),
        (None, Ok(bytes)) => server.handle(
            &Request { method: &method, path: &path, device: device.as_deref(), time: time.as_deref(), nonce: nonce.as_deref(), sign: signature.as_deref(), body: bytes, from: &from },
            crate::files::now(),
        ),
    };
    let status = reply.status;
    let reason = (status >= 400).then(|| serde_json::from_slice::<serde_json::Value>(&reply.body).ok().and_then(|v| v["error"].as_str().map(str::to_string))).flatten();
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
    if read.is_err() || too_many {
        // `tiny_http` reads what is left of a body before it lets the request go,
        // and from a client that went away that takes as long as a timeout. Another
        // thread waits for that, so that the worker is free at once.
        if load.start_detached() {
            let load = load.clone();
            std::thread::spawn(move || {
                let _ = request.respond(response);
                load.detached.fetch_sub(1, Ordering::SeqCst);
            });
        } else {
            // Too many already wait on clients that went quiet: let this one go.
            // Its connection is left open, and not answered, rather than held by a worker.
            std::mem::forget(request);
            server.files.host("Too many connections are stuck mid-request: one was let go without an answer.");
        }
    } else {
        let _ = request.respond(response);
    }
    // The answer is out: the places are free again before anything is written.
    drop(long);
    drop(_busy);
    if !too_common(&method, &path, status) {
        let local = here && (path == "/" || path.starts_with("/local/"));
        let who = who(device.as_deref(), linked, local);
        // Someone who is not linked, asking what is not there or being refused,
        // is written down once in a while with a count, not every time.
        let anonymous = !local && !linked && !path.starts_with("/v1/enrol/");
        let shape = route_shape(&path);
        let noise = anonymous && (status >= 400 || shape == "(unknown)" || path == "/v1/hello");
        let kind = if shape == "(unknown)" { "unknown route".to_string() } else { status.to_string() };
        if !noise || server.noisy(&kind, &who, Instant::now()) {
            load.say(server, request_line(&method, &path, status, started.elapsed(), &who, reason.as_deref()));
        }
    }
}

/// What the watcher last saw, to write down only what changed.
#[derive(Default)]
struct Noticed {
    sunshine: Option<bool>,
    /// Per linked device: connected, streaming.
    allies: HashMap<String, (bool, bool)>,
    /// Per linked device: whether Moonlight is paired.
    paired: HashMap<String, Option<bool>>,
}

impl Server {
    /// Whether to write this line in full: the first of its kind in a window
    /// of ten seconds. Later ones are only counted, and a line that says how
    /// many (and from whom last) follows when the window is over.
    fn noisy(&self, kind: &str, who: &str, now: Instant) -> bool {
        let mut noise = locked(&self.noise);
        let Some((begun, hidden, last)) = noise.get_mut(kind) else {
            noise.insert(kind.to_string(), (now, 0, who.to_string()));
            return true;
        };
        if now.saturating_duration_since(*begun) < self.noise_window {
            *hidden += 1;
            *last = who.to_string();
            return false;
        }
        let (count, last) = (std::mem::take(hidden), last.clone());
        *begun = now;
        drop(noise);
        self.say_hidden(kind, count, &last);
        true
    }

    fn say_hidden(&self, kind: &str, count: u32, last: &str) {
        if count > 0 {
            self.files.host(&format!("{count} more requests were refused or unknown ({kind}) in the last {} s, the last from {last}.", self.noise_window.as_secs()));
        }
    }

    /// Say what was left out, once its window is over, even when nothing of that kind comes again.
    fn flush_noise(&self, now: Instant) {
        let mut done = Vec::new();
        locked(&self.noise).retain(|kind, (begun, hidden, last)| {
            let over = now.saturating_duration_since(*begun) >= self.noise_window;
            if over && *hidden > 0 {
                done.push((kind.clone(), *hidden, last.clone()));
            }
            !over
        });
        for (kind, count, last) in done {
            self.say_hidden(&kind, count, &last);
        }
    }

    /// Write down what changed since the last look: Sunshine going up or
    /// down, an Ally being heard from or not for [`CONNECTED_FOR`] seconds, a
    /// stream starting or ending, Moonlight being paired. Changes only: an
    /// Ally's every pulse is not an event.
    fn notice(&self, before: &mut Noticed, now: i64) {
        let health = self.health();
        if before.sunshine.replace(health.is_ok()) != Some(health.is_ok()) {
            match &health {
                Ok(()) => self.files.host("Sunshine is ready."),
                Err(problem) => self.files.host(&format!("Sunshine is not ready: {problem}")),
            }
        }
        let streaming = self.files.playing(now).is_some();
        let linked = self.files.allies(now, streaming);
        let paired = if linked.is_empty() { None } else { self.paired_names() };
        for ally in &linked {
            let id = label(&ally.id);
            let state = (ally.connected, ally.streaming);
            match before.allies.insert(ally.id.clone(), state) {
                None => {
                    let heard = match ally.last_seen {
                        Some(at) => format!("last heard from {} s ago", (now - at).max(0)),
                        None => "never heard from".to_string(),
                    };
                    let words = if ally.streaming { "streaming".to_string() } else if ally.connected { "connected".to_string() } else { format!("not connected ({heard})") };
                    self.files.host(&format!("Ally {id}: {words}."));
                }
                Some(was) => {
                    if was.0 != state.0 {
                        if state.0 {
                            self.files.host(&format!("Ally {id} was heard from: connected."));
                        } else {
                            self.files.host(&format!("Ally {id} was not heard from for {CONNECTED_FOR} s: no longer connected."));
                        }
                    }
                    if was.1 != state.1 {
                        self.files.host(&format!("Ally {id}: streaming {}.", if state.1 { "started" } else { "ended" }));
                    }
                }
            }
            let is_paired = paired.as_ref().map(|names| names.iter().any(|n| n.eq_ignore_ascii_case(&ally.name)));
            if before.paired.insert(ally.id.clone(), is_paired) != Some(is_paired) {
                match is_paired {
                    Some(true) => self.files.host(&format!("Ally {id}: Moonlight is paired with Sunshine.")),
                    Some(false) => self.files.host(&format!("Ally {id}: Moonlight is not paired with Sunshine yet.")),
                    None => {}
                }
            }
        }
        before.allies.retain(|id, _| linked.iter().any(|a| &a.id == id));
        before.paired.retain(|id, _| linked.iter().any(|a| &a.id == id));
    }
}

/// Serve the API on an open listener until the process ends.
pub fn serve(server: std::sync::Arc<Server>, http: tiny_http::Server) {
    serve_with(server, http, Limits::default());
}

fn serve_with(server: std::sync::Arc<Server>, http: tiny_http::Server, limits: Limits) {
    let http = std::sync::Arc::new(http);
    let load = std::sync::Arc::new(Load::new(limits));
    let (queue, lines) = std::sync::mpsc::sync_channel::<String>(1024);
    *locked(&load.lines) = Some(queue);
    let writing = server.clone();
    std::thread::spawn(move || {
        for line in lines {
            writing.files.host(&line);
        }
    });
    let watching = server.clone();
    std::thread::spawn(move || {
        let mut before = Noticed::default();
        loop {
            watching.notice(&mut before, crate::files::now());
            watching.flush_noise(Instant::now());
            std::thread::sleep(NOTICE_EVERY);
        }
    });
    // An Ally waiting for requests holds one of these, so there are spares.
    let workers: Vec<_> = (0..limits.workers)
        .map(|_| {
            let (server, http, load) = (server.clone(), http.clone(), load.clone());
            std::thread::spawn(move || {
                while let Ok(request) = http.recv() {
                    work(&server, &load, request);
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
        desk_with(name, down, IO_WAIT, Limits::default())
    }

    fn desk_with(name: &str, down: bool, wait: Duration, limits: Limits) -> Desk {
        // Every desk gets a folder of its own, even when two tests use the same name.
        static DESKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("carla-companion-{name}-{}-{}", std::process::id(), DESKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("cover.jpg"), b"picture bytes").unwrap();
        // Its own profile, so that no test reads the real one.
        let env = carla_core::library::saveplace::Env { saved_games: Some(dir.join("profile").join("Saved Games")), ..Default::default() };
        let files = Files::new(dir.clone()).with_env(env);
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
            let (http, _) = bind_within("127.0.0.1:0", wait).unwrap();
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
        std::thread::spawn(move || serve_with(server, http, limits));
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
    fn a_second_desktop_learns_where_the_saves_are_and_waits_to_be_told_to_use_it() {
        use carla_core::library::saveplace::{SavePlace, SaveRoot};
        use carla_core::library::sync::{bring_in_step, Folder, Way};
        let first = desk("place-first", false);
        let second = desk("place-second", false);
        let (link1, link2) = (linked(&first), linked(&second));
        let saved_games = |d: &Desk| d.files.dir().join("profile").join("Saved Games");

        // Desktop 1: the administrator names the folder; it is also known by its place.
        let one = saved_games(&first).join("Third Game");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::write(one.join("slot1.sav"), b"desk one").unwrap();
        let entry = first.files.set_save_folder(Some("gog:30"), "Third Game", &one).unwrap();
        let place = SavePlace { root: SaveRoot::SavedGames, rel: "Third Game".into() };
        assert_eq!(entry.place.as_ref(), Some(&place));
        // The Ally learns it when desktop 1 answers.
        let learnt = link1.saves_placed("gog:30", "Third Game", None).unwrap().unwrap();
        assert_eq!(learnt.place.as_ref(), Some(&place));

        // A desktop that already has a folder ignores the place.
        assert!(link1.saves_placed("gog:30", "Third Game", Some(&SavePlace { root: SaveRoot::SavedGames, rel: "Elsewhere".into() })).unwrap().is_some());
        assert!(first.files.suggestions().is_empty());
        assert_eq!(first.files.save_folders()[0].folder, one);

        // Desktop 2 has no such folder yet, nor the Saved Games folder: nothing is noted.
        assert!(link2.saves_placed("gog:30", "Third Game", Some(&place)).unwrap().is_none());
        assert!(second.files.suggestions().is_empty());

        // Hostile places are refused, even where the root exists.
        std::fs::create_dir_all(saved_games(&second)).unwrap();
        for rel in ["../x", "..", "/abs", "C:/Windows", "a\\b", "", &"a".repeat(5000), &vec!["a"; 40].join("/")] {
            let bad = SavePlace { root: SaveRoot::SavedGames, rel: rel.to_string() };
            assert!(link2.saves_placed("gog:30", "Third Game", Some(&bad)).unwrap().is_none());
        }
        assert!(link2.saves_placed("gog:30", "Third Game", Some(&SavePlace { root: SaveRoot::ProgramData, rel: "Third Game".into() })).unwrap().is_none());
        assert!(second.files.suggestions().is_empty());

        // Now its parent exists: a suggestion is noted, nothing is created or offered.
        assert!(link2.saves_placed("gog:30", "Third Game", Some(&place)).unwrap().is_none());
        let wanted = saved_games(&second).join("Third Game");
        let list = second.files.suggestions();
        assert_eq!((list.len(), list[0].folder.clone(), list[0].game_id.as_deref()), (1, wanted.clone(), Some("gog:30")));
        assert!(!wanted.exists() && second.files.save_folders().is_empty());
        assert!(link2.saves_placed("gog:30", "Third Game", Some(&place)).unwrap().is_none());
        assert_eq!(second.files.suggestions().len(), 1, "one per game");

        // Accepting needs the folder to exist; then the usual sync takes over.
        let key = list[0].key.clone();
        assert!(second.files.accept_suggestion(&key, None).is_err());
        assert!(second.files.accept_suggestion("nothing", None).is_err());
        std::fs::create_dir_all(&wanted).unwrap();
        let accepted = second.files.accept_suggestion(&key, None).unwrap();
        assert_eq!((accepted.folder.clone(), accepted.place.clone()), (wanted.clone(), Some(place.clone())));
        assert!(second.files.suggestions().is_empty());

        let ally_dir = first.files.dir().join("ally");
        let here = ally_dir.join("saves");
        std::fs::create_dir_all(&here).unwrap();
        std::fs::write(here.join("slot1.sav"), b"ally newest").unwrap();
        let ally = Folder { folder: here, backups: Backups::new(&ally_dir.join("copies"), "gog:30"), now: now_for_tests() };
        let remote = link2.saves_placed("gog:30", "Third Game", Some(&place)).unwrap().unwrap();
        assert_eq!(remote.place.as_ref(), Some(&place));
        let done = bring_in_step(&ally, &remote, None).unwrap();
        assert_eq!(done.way, Way::Send);
        assert_eq!(std::fs::read(wanted.join("slot1.sav")).unwrap(), b"ally newest");
    }

    /// A desktop with a flagged game whose save folder the Ally holds an identical copy of.
    struct Retiring {
        desk: Desk,
        link: Link,
        key: String,
        there: PathBuf,
        here: PathBuf,
        ally: carla_core::library::sync::Folder,
    }

    fn retiring(tag: &str, flagged: bool) -> Retiring {
        use carla_core::library::sync::{bring_in_step, Folder};
        let desk = desk(tag, false);
        let link = linked(&desk);
        let there = desk.files.dir().join("desk-saves").join("Third Game");
        let here = desk.files.dir().join("ally").join("saves");
        std::fs::create_dir_all(there.join("profile")).unwrap();
        std::fs::write(there.join("slot1.sav"), b"slot one").unwrap();
        std::fs::write(there.join("profile").join("p.dat"), b"profile").unwrap();
        let key = desk.files.set_save_folder(Some("gog:30"), "Third Game", &there).unwrap().key;
        if flagged {
            assert_eq!(desk.files.set_keep_clean(&key, true).unwrap().as_deref(), Some("Third Game"));
        }
        let ally = Folder { folder: here.clone(), backups: Backups::new(&desk.files.dir().join("ally").join("copies"), "gog:30"), now: now_for_tests() };
        std::fs::create_dir_all(&here).unwrap();
        let remote = link.saves("gog:30", "Third Game").unwrap().unwrap();
        bring_in_step(&ally, &remote, None).unwrap();
        Retiring { desk, link, key, there, here, ally }
    }

    impl Retiring {
        fn offer(&self) -> Result<client::RetireAnswer, Error> {
            let mine = carla_core::library::sync::manifest(&self.here).unwrap();
            self.link.saves("gog:30", "Third Game").unwrap().unwrap().retire(&mine)
        }

        fn desktop_untouched(&self) {
            assert_eq!(std::fs::read(self.there.join("slot1.sav")).unwrap(), b"slot one");
            assert_eq!(std::fs::read(self.there.join("profile").join("p.dat")).unwrap(), b"profile");
            assert!(Backups::new(&self.desk.files.save_copies(), &self.key).list().is_empty(), "no safe copy was needed");
        }
    }

    fn kept(answer: Result<client::RetireAnswer, Error>) -> String {
        match answer {
            Ok(client::RetireAnswer::Kept(why)) => why,
            other => panic!("expected the desktop to keep its saves, got {other:?}"),
        }
    }

    #[test]
    fn a_desktop_clears_its_saves_only_when_flagged_and_the_ally_proves_an_identical_copy() {
        let r = retiring("retire-refuse", false);

        // Not flagged: refused, whatever the proof.
        assert_eq!(r.offer(), Ok(client::RetireAnswer::NotFlagged));
        r.desktop_untouched();
        r.desk.files.set_keep_clean(&r.key, true).unwrap();

        // One byte differs, an extra file, a missing file.
        std::fs::write(r.there.join("slot1.sav"), b"slot onE").unwrap();
        assert!(kept(r.offer()).contains("not identical"));
        std::fs::write(r.there.join("slot1.sav"), b"slot one").unwrap();
        std::fs::write(r.there.join("extra.sav"), b"x").unwrap();
        assert!(kept(r.offer()).contains("not identical"));
        std::fs::remove_file(r.there.join("extra.sav")).unwrap();
        std::fs::remove_file(r.there.join("profile").join("p.dat")).unwrap();
        assert!(kept(r.offer()).contains("not identical"));
        std::fs::write(r.there.join("profile").join("p.dat"), b"profile").unwrap();
        r.desktop_untouched();

        // An Ally with nothing proves nothing; a lie about paths is not believed.
        let answer = r.link.saves("gog:30", "").unwrap().unwrap();
        assert!(kept(answer.retire(&carla_core::library::sync::Manifest::default())).contains("no saves"));
        let lying = carla_core::library::sync::Manifest {
            files: vec![carla_core::library::sync::Entry { path: "../secret.txt".into(), size: 1, modified: 1, hash: carla_core::library::sync::fingerprint(b"x") }],
        };
        std::fs::write(r.there.parent().unwrap().join("secret.txt"), b"private").unwrap();
        assert!(answer.retire(&lying).is_err());
        assert_eq!(std::fs::read(r.there.parent().unwrap().join("secret.txt")).unwrap(), b"private");
        r.desktop_untouched();

        // A game that runs is left alone.
        let open = r.link.saves("gog:30", "").unwrap().unwrap();
        r.desk.files.set_playing(Some(&crate::files::Playing { game_id: "gog:30".into(), title: "Third Game".into(), at: now_for_tests() }));
        let mine = carla_core::library::sync::manifest(&r.here).unwrap();
        assert!(kept(open.retire(&mine)).contains("still running"));
        r.desk.files.set_playing(None);
        r.desktop_untouched();

        // A signed stranger gets nothing; an unlinked key is refused outright.
        let stranger = Link { key: vec![7; 32], ..r.link.clone() };
        assert_eq!(stranger.saves("gog:30", "").err(), Some(Error::Refused));

        // The safe copy cannot be made: nothing is removed.
        std::fs::write(r.desk.files.save_copies(), b"in the way").unwrap();
        assert!(matches!(r.offer(), Err(Error::Host(text)) if text.contains("Nothing was removed")));
        std::fs::remove_file(r.desk.files.save_copies()).unwrap();
        r.desktop_untouched();

        // An old desktop (or one that forgot the game) answers 404: not supported.
        let remote = r.link.saves("gog:30", "").unwrap().unwrap();
        r.desk.files.clear_save_folder(&r.key).unwrap();
        assert_eq!(remote.retire(&carla_core::library::sync::manifest(&r.here).unwrap()), Ok(client::RetireAnswer::Unsupported));
        r.desktop_untouched();
    }

    #[test]
    fn a_retired_desktop_keeps_its_folder_a_restorable_copy_and_nothing_outside() {
        let r = retiring("retire-done", true);
        let outside = r.there.parent().unwrap().join("secret.txt");
        std::fs::write(&outside, b"private").unwrap();
        let before = carla_core::library::sync::manifest(&r.there).unwrap();

        let done = match r.offer() {
            Ok(client::RetireAnswer::Done(done)) => done,
            other => panic!("expected the desktop to clear itself, got {other:?}"),
        };
        assert_eq!((done.files, done.bytes, done.skipped), (2, before.bytes(), 0));
        assert!(done.backup.contains("-retired"));
        assert!(r.there.is_dir(), "the folder itself stays");
        assert!(carla_core::library::sync::manifest(&r.there).unwrap().files.is_empty());
        assert_eq!(std::fs::read(&outside).unwrap(), b"private");
        assert_eq!(std::fs::read(r.here.join("slot1.sav")).unwrap(), b"slot one", "the Ally is untouched");

        // The safe copy is listed and `saves restore` brings everything back.
        let copies = Backups::new(&r.desk.files.save_copies(), &r.key);
        assert_eq!(copies.list().len(), 1);
        assert_eq!(copies.list()[0].id, done.backup);
        copies.restore(&r.there, &done.backup, now_for_tests() + 10).unwrap();
        assert_eq!(carla_core::library::sync::manifest(&r.there).unwrap().digest(), before.digest());

        // Retiring again, with nothing left to match against an unchanged Ally, still needs the proof.
        std::fs::write(r.there.join("slot1.sav"), b"changed").unwrap();
        assert!(kept(r.offer()).contains("not identical"));

        // Links inside the folder are neither listed nor followed (needs privileges on Windows).
        #[cfg(unix)]
        {
            std::fs::write(r.there.join("slot1.sav"), b"slot one").unwrap();
            std::os::unix::fs::symlink(outside.parent().unwrap(), r.there.join("escape")).unwrap();
            assert!(matches!(r.offer(), Ok(client::RetireAnswer::Done(_))));
            assert_eq!(std::fs::read(&outside).unwrap(), b"private");
        }
        // The same with a junction, which needs no privileges on Windows.
        #[cfg(windows)]
        {
            std::fs::write(r.there.join("slot1.sav"), b"slot one").unwrap();
            let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(r.there.join("escape")).arg(outside.parent().unwrap()).output().unwrap();
            assert!(made.status.success(), "mklink /J failed: {}", String::from_utf8_lossy(&made.stdout));
            let answer = r.offer();
            assert!(matches!(answer, Ok(client::RetireAnswer::Done(ref d)) if d.files == 2 && d.skipped == 0), "{answer:?}");
            assert_eq!(std::fs::read(&outside).unwrap(), b"private", "nothing behind the junction was touched");
            assert!(carla_core::library::sync::manifest(&r.there).unwrap().files.is_empty());
        }
    }

    #[test]
    fn the_whole_loop_leaves_the_desktop_clean_and_loses_nothing() {
        use carla_core::library::sync::{bring_in_step, manifest, Way};
        let r = retiring("retire-loop", true);
        let first = bring_in_step(&r.ally, &r.link.saves("gog:30", "").unwrap().unwrap(), None).unwrap();
        assert_eq!(first.way, Way::Same);

        // The player streams the game from the desktop and saves there.
        std::fs::write(r.there.join("slot1.sav"), b"played on the desktop").unwrap();
        std::fs::write(r.there.join("slot2.sav"), b"a new slot").unwrap();

        // After the stream: the Ally fetches, then offers its proof.
        let fetched = bring_in_step(&r.ally, &r.link.saves("gog:30", "").unwrap().unwrap(), Some(&first.agreed)).unwrap();
        assert_eq!(fetched.way, Way::Fetch);
        let answer = r.offer();
        assert!(matches!(answer, Ok(client::RetireAnswer::Done(ref d)) if d.files == 3), "{answer:?}");
        assert!(manifest(&r.there).unwrap().files.is_empty(), "the desktop is clean");
        assert_eq!(std::fs::read(r.here.join("slot1.sav")).unwrap(), b"played on the desktop");
        assert_eq!(Backups::new(&r.desk.files.save_copies(), &r.key).list().len(), 1, "what the desktop held is kept");

        // The next stream: the empty desktop never wipes the Ally, it receives.
        let next = bring_in_step(&r.ally, &r.link.saves("gog:30", "").unwrap().unwrap(), Some(&fetched.agreed)).unwrap();
        assert_eq!(next.way, Way::Send);
        assert_eq!(std::fs::read(r.there.join("slot1.sav")).unwrap(), b"played on the desktop");
        assert_eq!(std::fs::read(r.there.join("slot2.sav")).unwrap(), b"a new slot");
        assert_eq!(manifest(&r.there).unwrap().digest(), manifest(&r.here).unwrap().digest());
    }

    #[test]
    fn a_stream_from_a_keep_clean_desktop_waits_until_the_allys_saves_are_on_it() {
        use carla_core::library::sync::{bring_in_step, manifest, may_stream, Manifest, Side as _, Way, NOT_ON_DESKTOP};
        let r = retiring("retire-gate", true);
        let ally = manifest(&r.here).unwrap();
        assert!(matches!(r.offer(), Ok(client::RetireAnswer::Done(_))));
        assert!(manifest(&r.there).unwrap().files.is_empty());

        // The desktop says it keeps itself clean; its folder is empty now.
        let remote = r.link.saves("gog:30", "").unwrap().unwrap();
        assert!(remote.keep_clean);
        // The sync before the stream failed (nothing moved): the stream waits.
        assert_eq!(may_stream(remote.keep_clean, &ally, Some(&remote.manifest().unwrap())), Err(NOT_ON_DESKTOP.to_string()));
        // The desktop could not even be asked.
        assert_eq!(may_stream(true, &ally, None), Err(NOT_ON_DESKTOP.to_string()));
        // Half of it arrived: still waiting. The Ally's saves are untouched.
        remote.write("slot1.sav", b"slot one", 1).unwrap();
        let half = r.link.saves("gog:30", "").unwrap().unwrap();
        assert!(may_stream(half.keep_clean, &ally, Some(&half.manifest().unwrap())).is_err());
        assert_eq!(manifest(&r.here).unwrap().digest(), ally.digest());

        // Once the send is complete and read back, it may start.
        let sent = bring_in_step(&r.ally, &half, None).unwrap();
        assert_eq!(sent.way, Way::Send);
        let verified = r.link.saves("gog:30", "").unwrap().unwrap();
        assert_eq!(may_stream(verified.keep_clean, &ally, Some(&verified.manifest().unwrap())), Ok(()));

        // A game that is not flagged, or has no save on the Ally yet, is never held back.
        assert_eq!(may_stream(false, &ally, None), Ok(()));
        assert_eq!(may_stream(true, &Manifest::default(), None), Ok(()));
        let plain = retiring("retire-gate-plain", false);
        std::fs::remove_file(plain.there.join("slot1.sav")).unwrap();
        let theirs = plain.link.saves("gog:30", "").unwrap().unwrap();
        assert!(!theirs.keep_clean);
        assert_eq!(may_stream(theirs.keep_clean, &manifest(&plain.here).unwrap(), Some(&theirs.manifest().unwrap())), Ok(()));
        // An older desktop's answer has no such field.
        let old: link::SavesFound = serde_json::from_str(r#"{"key":"k","manifest":{"files":[]}}"#).unwrap();
        assert!(!old.keep_clean);
    }

    #[test]
    fn a_file_that_changed_after_the_check_is_skipped_and_counted_and_nothing_else_changes() {
        let r = retiring("retire-skip", true);
        let checked = carla_core::library::sync::manifest(&r.there).unwrap();
        // The check saw slot1.sav as "slot one"; it is written again before the removal.
        std::fs::write(r.there.join("slot1.sav"), b"written just now").unwrap();
        let side = carla_core::library::sync::Folder { folder: r.there.clone(), backups: Backups::new(&r.desk.files.save_copies(), &r.key), now: now_for_tests() };
        let done = remove_checked(&side, &checked.files, "b").unwrap();
        assert_eq!((done.files, done.skipped, done.bytes), (1, 1, 7));
        assert_eq!(std::fs::read(r.there.join("slot1.sav")).unwrap(), b"written just now");
        assert!(!r.there.join("profile").exists(), "the other file went, and its emptied folder with it");
        assert!(r.there.is_dir());
        assert_eq!(std::fs::read(r.here.join("slot1.sav")).unwrap(), b"slot one", "the Ally is untouched");
        // A file that is gone is skipped too.
        let gone = remove_checked(&side, &checked.files, "b").unwrap();
        assert_eq!((gone.files, gone.skipped), (0, 1 + 1));
    }

    #[test]
    fn saves_written_before_keep_clean_existed_load_as_not_kept_clean() {
        let dir = std::env::temp_dir().join(format!("carla-clean-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let files = Files::new(dir.clone());
        std::fs::write(dir.join("saves.json"), r#"[{"key":"k","game_id":"gog:1","title":"Old","folder":"C:/Saves/Old"}]"#).unwrap();
        assert!(!files.save_folders()[0].keep_clean);
        assert_eq!(files.set_keep_clean("k", true).unwrap().as_deref(), Some("Old"));
        assert!(files.save_folders()[0].keep_clean);
        assert_eq!(files.set_keep_clean("nope", true).unwrap(), None);
        files.set_keep_clean("k", false).unwrap();
        assert!(!files.save_folders()[0].keep_clean);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn suggestions_are_capped_dismissed_and_old_files_still_load() {
        use carla_core::library::saveplace::{SavePlace, SaveRoot};
        let dir = std::env::temp_dir().join(format!("carla-suggest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let files = Files::new(dir.clone());
        let place = SavePlace { root: SaveRoot::SavedGames, rel: "G".into() };
        for n in 0..250 {
            files.suggest(Some(&format!("gog:{n}")), &"t".repeat(500), &dir, &place, n).unwrap();
        }
        let list = files.suggestions();
        assert_eq!(list.len(), 200);
        assert!(list.iter().all(|s| s.title.len() == 80 && s.at >= 50));
        let key = list[0].key.clone();
        assert!(files.dismiss_suggestion(&key).unwrap().is_some());
        assert!(files.dismiss_suggestion(&key).unwrap().is_none());
        // A list of folders from before places existed loads.
        std::fs::write(dir.join("saves.json"), r#"[{"key":"k","game_id":"gog:1","title":"Old","folder":"C:/Saves/Old"}]"#).unwrap();
        let old = files.save_folders();
        assert_eq!((old.len(), old[0].place.clone()), (1, None));
        // So does a find request from an Ally that does not know places.
        let wanted: SavesFind = serde_json::from_str(r#"{"game_id":"gog:1","title":"Old"}"#).unwrap();
        assert!(wanted.place.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn suggestion_files(tag: &str) -> (Files, PathBuf) {
        use carla_core::library::saveplace::Env;
        let dir = std::env::temp_dir().join(format!("carla-sugg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Saved Games")).unwrap();
        let env = Env { saved_games: Some(dir.join("Saved Games")), ..Default::default() };
        (Files::new(dir.join("data")).with_env(env), dir)
    }

    #[test]
    fn suggestions_hold_no_hidden_characters_and_are_written_safely() {
        use carla_core::library::saveplace::{SavePlace, SaveRoot};
        let (files, dir) = suggestion_files("text");
        let place = SavePlace { root: SaveRoot::SavedGames, rel: "Fine\nGame\u{1b}[31m\u{202e}x".into() };
        files.suggest(Some("gog:\n1\u{202e}"), "Ti\ntle\u{1b}[2J\u{202e}gnp", &dir, &place, 5).unwrap();
        let s = &files.suggestions()[0];
        for text in [s.title.as_str(), s.game_id.as_deref().unwrap(), s.place.rel.as_str()] {
            assert!(!text.chars().any(|c| c.is_control() || c == '\u{202e}'), "{text:?}");
        }
        assert_eq!(s.title, "Title[2Jgnp");

        // Writing the same list again changes nothing, even where a write would fail.
        let file = files.dir().join("save-suggestions.json");
        let mut readonly = std::fs::metadata(&file).unwrap().permissions();
        readonly.set_readonly(true);
        std::fs::set_permissions(&file, readonly.clone()).unwrap();
        files.suggest(Some("gog:\n1\u{202e}"), "Ti\ntle\u{1b}[2J\u{202e}gnp", &dir, &place, 5).unwrap();
        #[allow(clippy::permissions_set_readonly_false)]
        readonly.set_readonly(false);
        std::fs::set_permissions(&file, readonly).unwrap();

        // Many at once: none is lost, and no half-written file is left.
        let files = std::sync::Arc::new(files);
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let (files, dir, place) = (files.clone(), dir.clone(), place.clone());
                std::thread::spawn(move || {
                    for n in 0..10 {
                        files.suggest(Some(&format!("gog:{t}-{n}")), "Game", &dir, &place, 100 + n).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(files.suggestions().len(), 81);
        assert!(std::fs::read_dir(files.dir()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".new")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_suggestion_in_a_place_that_runs_programs_cannot_be_accepted() {
        use carla_core::library::saveplace::{SavePlace, SaveRoot};
        let (files, dir) = suggestion_files("code");
        std::fs::create_dir_all(dir.join("Saved Games").join("Startup").join("G")).unwrap();
        std::fs::create_dir_all(dir.join("Saved Games").join("Fine")).unwrap();
        let bad = SavePlace { root: SaveRoot::SavedGames, rel: "Startup/G".into() };
        files.suggest(Some("gog:1"), "Bad", &dir.join("Saved Games").join("Startup").join("G"), &bad, 1).unwrap();
        let bad_key = files.suggestions()[0].key.clone();
        assert!(files.accept_suggestion(&bad_key, None).is_err());
        assert!(files.save_folders().is_empty());
        // A folder that is not where its place says is refused too.
        let fine = SavePlace { root: SaveRoot::SavedGames, rel: "Fine".into() };
        files.suggest(Some("gog:2"), "Moved", &dir, &fine, 2).unwrap();
        let moved_key = files.suggestions().iter().find(|s| s.title == "Moved").unwrap().key.clone();
        assert!(files.accept_suggestion(&moved_key, None).is_err());
        files.suggest(Some("gog:3"), "Fine", &dir.join("Saved Games").join("Fine"), &fine, 3).unwrap();
        let fine_key = files.suggestions().iter().find(|s| s.title == "Fine").unwrap().key.clone();
        assert!(files.accept_suggestion(&fine_key, None).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_below_the_root_cannot_be_accepted() {
        use carla_core::library::saveplace::{SavePlace, SaveRoot};
        let (files, dir) = suggestion_files("junction");
        std::fs::create_dir_all(dir.join("elsewhere").join("x")).unwrap();
        let link = dir.join("Saved Games").join("jn");
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(dir.join("elsewhere")).output().unwrap();
        assert!(made.status.success(), "mklink failed: {}", String::from_utf8_lossy(&made.stdout));
        let place = SavePlace { root: SaveRoot::SavedGames, rel: "jn/x".into() };
        files.suggest(Some("gog:9"), "Linked", &link.join("x"), &place, 1).unwrap();
        let key = files.suggestions()[0].key.clone();
        assert!(files.accept_suggestion(&key, None).is_err());
        assert!(files.save_folders().is_empty());
        let _ = std::process::Command::new("cmd").args(["/C", "rmdir"]).arg(&link).output();
        let _ = std::fs::remove_dir_all(&dir);
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

    // The log, and a server that stays answerable.

    /// Wait for a line to reach the log: it is written just after the answer is sent.
    fn logged(files: &Files, needle: &str) -> String {
        for _ in 0..100 {
            let text = files.host_log().text();
            if text.contains(needle) {
                return text;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("{needle:?} never reached the log:\n{}", files.host_log().text());
    }

    /// A server that is not listening, for calling its parts directly.
    fn server_with(name: &str, scan: Box<dyn Fn() -> Vec<Found> + Send + Sync>) -> Server {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("carla-companion-{name}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = carla_core::library::saveplace::Env { saved_games: Some(dir.join("Saved Games")), ..Default::default() };
        Server::new(Files::new(dir).with_env(env), Config::new("HOME PC"), Some(Box::new(FakeSunshine::default())), scan, "companion launch".into(), String::new(), || (Vec::new(), Vec::new()), |_| Vec::new(), || Ok(()))
    }

    #[test]
    fn every_request_is_logged_with_its_route_and_who_asked_but_never_a_key_or_a_body() {
        let desk = desk("log-requests", false);
        let link = linked(&desk);
        link.status().unwrap();
        let text = logged(&desk.files, "GET /v1/status -> 200");
        let me = label(&link.device_id);
        assert!(text.contains(&format!("GET /v1/status -> 200 in ")) && text.contains(&format!("({me})")), "{text}");
        assert!(text.contains(&format!("A device was linked: {me}.")), "{text}");

        // A route that names a game or a save folder is written as its shape.
        link.cover("steam:10").unwrap();
        assert!(logged(&desk.files, "/v1/cover/:game").contains("GET /v1/cover/:game -> 200"));
        assert!(!desk.files.host_log().text().contains("steam:10"));
        link.saves("steam:10", "First Game").unwrap();
        logged(&desk.files, "POST /v1/saves/find -> 200");

        // What the Ally asked to start, and a pairing whose PIN is not kept.
        link.prepare("steam:10").unwrap();
        logged(&desk.files, "Prepare: First Game (steam:10) is ready to start when Moonlight connects");
        link.pair("4821", "Ally").unwrap();
        assert!(logged(&desk.files, "Pairing with Moonlight worked.").contains("Pairing with Moonlight began"));
        assert!(!desk.files.host_log().text().contains("4821"));

        // A refusal says why, and who it was: a stranger with a made-up id, or a device whose signature fails.
        let wrong = Link { key: vec![7; 32], ..link.clone() };
        assert_eq!(wrong.status().err(), Some(Error::Refused));
        let text = logged(&desk.files, "does not match");
        assert!(text.contains(&format!("device {me} is linked, but the request's signature does not match")), "{text}");
        assert!(text.contains("-> 401") && text.contains("This device is not linked to the desktop."), "{text}");
        let stranger = Link { device_id: "ffeeddccbbaa9988".into(), ..link.clone() };
        assert_eq!(stranger.status().err(), Some(Error::Refused));
        let text = logged(&desk.files, "device ffeedd is not linked here");
        assert!(text.contains("(stranger)"), "{text}");

        // Nothing secret is ever in it.
        let text = desk.files.host_log().text();
        assert!(!text.contains(&hex_of(&link.key)) && !text.contains(&link.device_id), "{text}");
    }

    fn hex_of(bytes: &[u8]) -> String {
        crate::files::hex(bytes)
    }

    #[test]
    fn what_a_log_line_says_of_a_request() {
        assert_eq!(route_shape("/v1/status"), "/v1/status");
        assert_eq!(route_shape("/v1/saves/3fa9c0de/write"), "/v1/saves/:folder/write");
        assert_eq!(route_shape("/v1/saves/3fa9c0de/surprise"), "/v1/saves/:folder/?");
        assert_eq!(route_shape("/v1/saves/find"), "/v1/saves/find");
        assert_eq!(route_shape("/v1/cover/steam:10"), "/v1/cover/:game");
        assert_eq!(route_shape("/v1/games/update/gog:7"), "/v1/games/update/:game");
        assert_eq!(route_shape("/v1/probe/20000"), "/v1/probe/:bytes");
        assert_eq!(route_shape("/wp-login.php?x=1"), "(unknown)");
        assert_eq!((who(Some("0123456789abcdef"), true, false), who(Some("0123456789abcdef"), false, false), who(None, false, false), who(None, false, true)), ("012345".to_string(), "stranger".to_string(), "unsigned".to_string(), "this PC".to_string()));

        let quick = request_line("GET", "/v1/status", 200, Duration::from_millis(3), "012345", None);
        assert_eq!(quick, "GET /v1/status -> 200 in 3 ms (012345)");
        let slow = request_line("POST", "/v1/saves/abc123/write", 409, Duration::from_millis(2500), "012345", Some("Quest is still running."));
        assert_eq!(slow, "POST /v1/saves/:folder/write -> 409 in 2500 ms (012345) SLOW: Quest is still running.");
        // An Ally waiting for a request from the administrator is meant to wait.
        assert!(!request_line("GET", "/v1/admin/next", 200, Duration::from_secs(20), "012345", None).contains("SLOW"));
        // The dashboard's every-two-seconds question is not written down; a failure of it is.
        assert!(too_common("GET", "/local/overview", 200) && !too_common("GET", "/local/overview", 403) && !too_common("POST", "/local/code", 200));
    }

    #[test]
    fn the_watcher_writes_down_changes_only() {
        let server = server_with("watch", Box::new(Vec::new));
        let files = server.files.clone();
        let device = files.add_device("Zorba", &[1; 32]).unwrap();
        let id = label(&device.id);
        let now = crate::files::now();
        files.set_ally_seen(&device.id, &Seen { at: now, address: "192.168.1.5".into(), prepared: None });
        let mut before = Noticed::default();
        server.notice(&mut before, now);
        server.notice(&mut before, now + 5);
        server.notice(&mut before, now + 10);
        let text = files.host_log().text();
        assert_eq!(text.matches("Sunshine is ready.").count(), 1, "{text}");
        assert_eq!(text.matches(&format!("Ally {id}: connected.")).count(), 1, "pulses are not events:\n{text}");
        // Not heard from for a while: said once, when it turns.
        server.notice(&mut before, now + 120);
        server.notice(&mut before, now + 125);
        let text = files.host_log().text();
        assert_eq!(text.matches(&format!("Ally {id} was not heard from for 90 s")).count(), 1, "{text}");
        // Heard from again, and then a stream.
        files.set_ally_seen(&device.id, &Seen { at: now + 130, address: "192.168.1.5".into(), prepared: Some(now + 130) });
        server.notice(&mut before, now + 131);
        files.set_playing(Some(&crate::files::Playing { game_id: "steam:10".into(), title: "First Game".into(), at: now + 131 }));
        server.notice(&mut before, now + 132);
        files.set_playing(None);
        server.notice(&mut before, now + 400);
        let text = files.host_log().text();
        for line in [format!("Ally {id} was heard from: connected."), format!("Ally {id}: streaming started."), format!("Ally {id}: streaming ended.")] {
            assert_eq!(text.matches(&line).count(), 1, "{line}\n{text}");
        }
        // The device's name is the player's own words: it is not written down.
        assert!(!text.contains("Zorba"), "{text}");
    }

    #[test]
    fn asking_for_the_games_while_the_disks_are_scanned_does_not_wait_for_the_scan() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let scans = Arc::new(AtomicUsize::new(0));
        let counting = scans.clone();
        let scan = Box::new(move || {
            // The first look is quick, later ones take a while.
            if counting.fetch_add(1, Ordering::SeqCst) > 0 {
                std::thread::sleep(Duration::from_millis(400));
            }
            vec![("steam", Scanned { source_id: "10".into(), title: "First Game".into(), ..Scanned::default() })]
        });
        let server = Arc::new(server_with("scan", scan));
        assert_eq!(server.games_listing().len(), 1);
        assert_eq!(scans.load(Ordering::SeqCst), 1);

        // Four asking at once for a fresh look share one scan.
        let askers: Vec<_> = (0..4).map(|_| {
            let server = server.clone();
            std::thread::spawn(move || server.games(true).len())
        }).collect();
        std::thread::sleep(Duration::from_millis(100));
        // While it runs, the Ally's list is answered at once from what is known.
        let asked = Instant::now();
        assert_eq!(server.games_listing().len(), 1);
        assert!(asked.elapsed() < Duration::from_millis(200), "waited {:?} for the scan", asked.elapsed());
        for asker in askers {
            assert_eq!(asker.join().unwrap(), 1);
        }
        assert_eq!(scans.load(Ordering::SeqCst), 2, "one scan for all four");
    }

    #[test]
    fn long_requests_cannot_take_every_worker_and_all_busy_is_noticed() {
        let load = Load::new(Limits { workers: 3, long: 1, body: Duration::from_secs(1) });
        let first = load.start_long();
        assert!(first.is_some() && load.start_long().is_none(), "only one long request at a time");
        drop(first);
        assert!(load.start_long().is_some(), "the place is given back");
        let (a, crowded) = load.start();
        assert!(!crowded);
        let (b, crowded) = load.start();
        assert!(!crowded);
        let (c, crowded) = load.start();
        assert!(crowded, "the third of three");
        assert!(load.warn_due() && !load.warn_due(), "said once in a while, not for every request");
        drop((a, b, c));
        assert!(takes_long("/v1/saves/abc/write") && takes_long("/v1/admin/next") && takes_long("/local/find"));
        assert!(!takes_long("/v1/status") && !takes_long("/v1/games") && !takes_long("/v1/link/pulse") && !takes_long("/v1/stream/prepare"));
    }

    struct Trickle {
        left: usize,
        pause: Duration,
    }

    impl std::io::Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.left == 0 {
                return Ok(0);
            }
            std::thread::sleep(self.pause);
            self.left -= 1;
            buf[0] = b'x';
            Ok(1)
        }
    }

    #[test]
    fn a_body_that_trickles_in_is_given_up_on() {
        let quick = read_body(&mut Trickle { left: 5, pause: Duration::ZERO }, 100, Duration::from_secs(1)).unwrap();
        assert_eq!(quick.len(), 5);
        assert_eq!(read_body(&mut Trickle { left: 500, pause: Duration::ZERO }, 100, Duration::from_secs(1)).unwrap().len(), 100, "no more than allowed is read");
        let slow = read_body(&mut Trickle { left: 100, pause: Duration::from_millis(20) }, 1000, Duration::from_millis(100));
        assert_eq!(slow.err().map(|e| e.kind()), Some(std::io::ErrorKind::TimedOut));
    }

    /// A raw connection to a desk that says what it is told to, for clients that misbehave.
    fn connect(port: u16) -> std::net::TcpStream {
        std::net::TcpStream::connect(("127.0.0.1", port)).unwrap()
    }

    /// How long a plain, well-behaved request takes to be answered.
    fn hello_within(port: u16, most: Duration) -> Duration {
        use std::io::{Read, Write};
        let started = Instant::now();
        let mut stream = connect(port);
        stream.set_read_timeout(Some(most)).unwrap();
        write!(stream, "GET /v1/hello HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap_or_else(|e| panic!("no answer within {most:?}: {e}"));
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        started.elapsed()
    }

    #[test]
    fn a_client_that_connects_and_says_nothing_does_not_stop_the_others() {
        let desk = desk_with("silent", false, Duration::from_millis(500), Limits { workers: 2, long: 1, body: Duration::from_millis(500) });
        let _silent: Vec<_> = (0..6).map(|_| connect(desk.port)).collect();
        assert!(hello_within(desk.port, Duration::from_secs(3)) < Duration::from_secs(3));
    }

    #[test]
    fn a_body_that_trickles_in_does_not_hold_a_worker_for_ever() {
        use std::io::Write;
        // One worker only, so that a stuck one would stop everything.
        let desk = desk_with("trickle", false, Duration::from_secs(5), Limits { workers: 1, long: 1, body: Duration::from_millis(400) });
        let mut slow = connect(desk.port);
        write!(slow, "POST /v1/link/pulse HTTP/1.1\r\nHost: x\r\nContent-Length: 5000\r\n\r\n").unwrap();
        let dripping = std::thread::spawn(move || {
            for _ in 0..100 {
                if slow.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        // A well-behaved client is answered once the time limit of the stuck one is over.
        std::thread::sleep(Duration::from_millis(100));
        let took = hello_within(desk.port, Duration::from_secs(8));
        assert!(took < Duration::from_secs(3), "answered after {took:?}");
        let text = logged(&desk.files, "-> 408");
        assert!(text.contains("The request did not arrive in time."), "{text}");
        dripping.join().unwrap();
    }

    /// Sockets only have timeouts on Windows, which is where the companion runs.
    #[cfg(windows)]
    #[test]
    fn a_client_that_went_away_in_the_middle_of_a_request_frees_its_worker() {
        use std::io::Write;
        let desk = desk_with("went-away", false, Duration::from_millis(500), Limits { workers: 1, long: 1, body: Duration::from_secs(60) });
        // Says a body of 5000 bytes is coming, sends 10 of them, and is never heard from again.
        let mut dead = connect(desk.port);
        write!(dead, "POST /v1/link/pulse HTTP/1.1\r\nHost: x\r\nContent-Length: 5000\r\n\r\n0123456789").unwrap();
        std::thread::sleep(Duration::from_millis(150));
        // The body deadline is a minute: it is the socket's own timeout that lets go.
        let took = hello_within(desk.port, Duration::from_secs(8));
        assert!(took < Duration::from_secs(3), "answered after {took:?}");
        assert!(logged(&desk.files, "-> 408").contains("The request did not arrive in time."));
        drop(dead);
    }

    #[test]
    fn strangers_cannot_use_up_the_places_for_long_requests() {
        use std::io::Write;
        // One place for long requests, and four unsigned clients that each start a save write and trickle it.
        let desk = desk_with("strangers", false, Duration::from_secs(5), Limits { workers: 8, long: 1, body: Duration::from_secs(5) });
        let link = linked(&desk);
        let drips: Vec<_> = (0..4)
            .map(|_| {
                let mut stream = connect(desk.port);
                write!(stream, "POST /v1/saves/x/write HTTP/1.1\r\nHost: x\r\nContent-Length: 65536\r\n\r\n").unwrap();
                std::thread::spawn(move || {
                    for _ in 0..30 {
                        if stream.write_all(b"x").is_err() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                })
            })
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        // A linked device still gets the place, and is answered.
        assert!(link.saves("steam:10", "First Game").unwrap().is_none());
        for drip in drips {
            drip.join().unwrap();
        }
    }

    #[test]
    fn a_scanner_is_logged_once_in_a_while_with_a_count_and_the_linked_are_always_logged() {
        let desk = desk("scanner", false);
        let link = linked(&desk);
        for _ in 0..30 {
            use std::io::{Read, Write};
            let mut stream = connect(desk.port);
            write!(stream, "GET /v1/status HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
            let mut reply = String::new();
            let _ = stream.read_to_string(&mut reply);
        }
        link.status().unwrap();
        link.status().unwrap();
        // The second signed line is written just after its answer: wait for it too.
        for _ in 0..100 {
            if desk.files.host_log().text().matches("GET /v1/status -> 200").count() >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let text = desk.files.host_log().text();
        assert_eq!(text.lines().filter(|l| l.contains("GET /v1/status -> 401")).count(), 1, "{text}");
        assert_eq!(text.lines().filter(|l| l.contains("GET /v1/status -> 200") && l.contains(&format!("({})", label(&link.device_id)))).count(), 2, "{text}");
    }

    #[test]
    fn what_was_left_out_of_the_log_is_counted_when_its_window_is_over() {
        let mut server = server_with("noise", Box::new(Vec::new));
        server.noise_window = Duration::from_secs(10);
        let t0 = Instant::now();
        assert!(server.noisy("401", "stranger", t0));
        for _ in 0..999 {
            assert!(!server.noisy("401", "stranger", t0 + Duration::from_secs(1)));
        }
        assert!(server.noisy("404", "unsigned", t0), "another kind has its own window");
        server.flush_noise(t0 + Duration::from_secs(5));
        assert!(!server.files.host_log().text().contains("more requests"), "not before the window is over");
        server.flush_noise(t0 + Duration::from_secs(11));
        let text = server.files.host_log().text();
        assert!(text.contains("999 more requests were refused or unknown (401)") && text.contains("the last from stranger"), "{text}");
        assert!(text.lines().count() <= 3, "{text}");
    }

    #[test]
    fn a_scan_that_began_before_an_install_is_not_kept() {
        let server = Arc::new(server_with("cleared", Box::new(|| {
            std::thread::sleep(Duration::from_millis(250));
            vec![("steam", Scanned { source_id: "10".into(), title: "First Game".into(), ..Scanned::default() })]
        })));
        let scanning = server.clone();
        let scan = std::thread::spawn(move || scanning.games(true).len());
        std::thread::sleep(Duration::from_millis(80));
        *locked(&server.cleared) = Instant::now();
        assert_eq!(scan.join().unwrap(), 1, "answered to whoever asked");
        assert!(locked(&server.games).is_none(), "but not kept");
    }

    #[test]
    fn body_time_grows_with_what_may_come_and_is_short_for_what_is_small() {
        let base = Duration::from_secs(30);
        assert_eq!(body_time(base, 4096), Duration::from_secs(5));
        assert_eq!(body_time(base, 1024 * 1024 + 1), Duration::from_secs(60 - 29));
        assert_eq!(body_time(base, (sync::LARGEST_FILE + 4096) as usize), Duration::from_secs(62));
        assert_eq!(body_time(Duration::from_millis(400), 4096), Duration::from_millis(400));
        let load = Load::new(Limits::default());
        assert!((0..DETACHED_AT_ONCE).all(|_| load.start_detached()) && !load.start_detached(), "only so many threads wait on quiet clients");
    }

    #[test]
    fn the_timeouts_of_accepted_sockets_are_checked_not_assumed() {
        let (_http, timeouts) = bind_within("127.0.0.1:0", Duration::from_millis(700)).unwrap();
        #[cfg(windows)]
        assert_eq!(timeouts, Ok(()));
        #[cfg(not(windows))]
        assert!(timeouts.is_err(), "nothing sets them here, and that is said");
    }
}
