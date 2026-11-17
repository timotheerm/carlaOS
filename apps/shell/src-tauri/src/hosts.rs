//! Commands for linked desktops: finding and linking them, knowing whether
//! they are ready, mirroring their games into the catalogue, and starting a
//! stream. Linking and removing are for the administrator only.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::process::Child;
use std::time::{Duration, Instant};

use carla_core::library::{HostRow, Scanned};
use carla_core::link::client::{self, Error, Link};
use carla_core::link::discover::{self, Found};
use carla_core::library::Streamed;
use carla_core::link::check::{self, Facts, Fix, Outcome, Report};
use carla_core::link::moonlight;
use carla_core::link::health::{Health, Pulse};
use carla_core::link::quality::{self, Choice, Depth, Setting};
use carla_core::link::{wake, State as HostState, Status, Updating, PORT};
use carla_core::{secret, VERSION};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::admin::Admin;
use crate::library::{allow_pictures, now, Library};

/// One desktop as the interface shows it.
#[derive(Debug, Clone, Serialize)]
pub struct HostView {
    id: String,
    name: String,
    /// "ready", "busy", "not_ready" or "unreachable"; "unknown" before the
    /// first answer.
    state: &'static str,
    /// What is running there, when busy.
    playing: Option<String>,
    /// One line on what is wrong, when not ready.
    problem: Option<String>,
    latency_ms: Option<u64>,
    hardware: String,
    /// Unix seconds of the last answer.
    last_seen: Option<i64>,
    address: String,
    /// "local" on the home network, "remote" through Tailscale.
    route: Option<&'static str>,
    /// The desktop told us how to wake it.
    can_wake: bool,
    /// The companion's version, once the desktop has answered.
    version: Option<String>,
    /// The connection as it was last measured: ping, jitter, lost packets,
    /// a grade and the picture streams get.
    health: Option<Health>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Linked {
    host: HostView,
    games: usize,
    /// Set when the desktop is linked but streaming still needs attention.
    warning: Option<String>,
}

pub(crate) fn art_dir(library: &Library) -> PathBuf {
    library.0.art_dir.join("desktops")
}

fn link_of(host: &HostRow) -> Result<Link, String> {
    Ok(Link { address: host.address.clone(), port: host.port, device_id: host.device_id.clone(), key: secret::open(&host.key)? })
}

fn view(host: &HostRow) -> HostView {
    HostView {
        id: host.id.clone(),
        name: host.name.clone(),
        state: "unknown",
        playing: None,
        problem: None,
        latency_ms: None,
        hardware: host.hardware.clone(),
        last_seen: host.last_seen,
        address: host.address.clone(),
        route: None,
        can_wake: !host.macs.is_empty(),
        version: None,
        health: None,
    }
}

/// The same with the last measurement of the connection.
fn view_with_health(library: &Library, host: &HostRow) -> HostView {
    HostView { health: library.store().host_health(&host.id).ok().flatten(), ..view(host) }
}

/// The picture setting of a desktop and what its last stream got.
#[derive(Debug, Clone, Serialize)]
pub struct PictureView {
    /// "auto", "auto120" or a level's name.
    setting: &'static str,
    /// Every setting, in the order the interface steps through them.
    settings: Vec<&'static str>,
    last: Option<LastPicture>,
}

#[derive(Debug, Clone, Serialize)]
struct LastPicture {
    level: &'static str,
    automatic: bool,
    height: u32,
    fps: u32,
    mbps: u32,
    latency_ms: Option<u32>,
    link_kbps: Option<u32>,
    away: bool,
    at: i64,
}

fn picture_view(library: &Library, id: &str) -> Result<PictureView, String> {
    let (setting, streamed) = library.store().host_quality(id)?;
    Ok(PictureView {
        setting: setting.name(),
        settings: Setting::all().into_iter().map(Setting::name).collect(),
        last: streamed.map(|Streamed { choice, at }| {
            let p = choice.picture_on(carla_core::display::current());
            LastPicture {
                level: choice.level.name(),
                automatic: choice.automatic,
                height: p.height,
                fps: p.fps,
                mbps: p.kbps / 1000,
                latency_ms: choice.measure.map(|m| m.latency_ms),
                link_kbps: choice.measure.and_then(|m| m.kbps),
                away: choice.measure.is_some_and(|m| m.away),
                at,
            }
        }),
    })
}

/// A desktop's picture setting and what its last stream got.
#[tauri::command]
pub fn hosts_picture(library: State<'_, Library>, id: String) -> Result<PictureView, String> {
    picture_view(&library, &id)
}

/// Choose the picture of streams from a desktop.
#[tauri::command]
pub fn hosts_set_picture(admin: State<'_, Admin>, library: State<'_, Library>, id: String, setting: String) -> Result<PictureView, String> {
    admin.require()?;
    let setting = Setting::parse(&setting).ok_or("That is not a picture setting.")?;
    library.store().set_host_quality(&id, setting)?;
    picture_view(&library, &id)
}

pub(crate) fn this_device() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty()).unwrap_or_else(|| "Ally".to_string())
}

/// Bring the catalogue in line with what a desktop has installed.
pub(crate) fn sync_games(library: &Library, host: &HostRow, link: &Link) -> Result<usize, String> {
    let games = link.games().map_err(|e| e.to_string())?;
    let have_cover: std::collections::HashSet<String> =
        library.store().list()?.into_iter().filter(|g| g.cover.is_some()).map(|g| g.id).collect();
    let dir = art_dir(library);
    // Games from the stores, programs the administrator added on the desktop,
    // and game files of the consoles set up there.
    let from = |source: &'static str| -> Vec<Scanned> {
        games
        .iter()
        .filter(|g| g.source == source && g.id == format!("{source}:{}", g.source_id) && g.emulated == (source == "emu"))
        .map(|g| {
            // Fetch a picture only for games that have none here yet.
            let mut cover = None;
            if g.has_cover && !have_cover.contains(&g.id) && g.source_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                let path = dir.join(format!("{source}-{}.jpg", g.source_id));
                if path.is_file() {
                    cover = Some(path);
                } else if let Ok(bytes) = link.cover(&g.id) {
                    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, bytes).is_ok() {
                        cover = Some(path);
                    }
                }
            }
            // A console's name comes from the desktop; only an emulated game has one.
            let platform = if g.emulated { g.platform.chars().take(40).collect() } else { String::new() };
            Scanned { source_id: g.source_id.clone(), title: g.title.clone(), cover, platform, emulated: g.emulated, update: g.update, ..Scanned::default() }
        })
        .collect()
    };
    let mut total = 0;
    for source in ["steam", "epic", "gog", "manual", "emu"] {
        let found = from(source);
        total += found.len();
        library.store().sync(&host.id, source, &found, now())?;
    }
    // And what the desktop's Steam could install there.
    crate::installs::sync_host(library, host, link);
    Ok(total)
}

/// A desktop's game list changes rarely, and reading it makes the desktop
/// look through its disks, so it is mirrored at most this often.
const SYNC_EVERY: Duration = Duration::from_secs(300);

fn sync_due(host_id: &str) -> bool {
    static LAST: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let last = last.get_or_insert_with(HashMap::new);
    if last.get(host_id).is_some_and(|t| t.elapsed() < SYNC_EVERY) {
        return false;
    }
    last.insert(host_id.to_string(), Instant::now());
    true
}

/// Find the address a desktop answers at. Every known address is asked at
/// once, so a desktop that has moved (home network to Tailscale, or a new
/// address from the router) costs no extra waiting. The home network wins
/// when several answer.
pub(crate) fn reach(host: &HostRow) -> Result<(Link, Status, u64), Error> {
    let base = link_of(host).map_err(Error::Host)?;
    let asking: Vec<_> = wake::candidates(&host.address, &host.addresses)
        .into_iter()
        .map(|address| {
            let link = Link { address, ..base.clone() };
            std::thread::spawn(move || {
                let asked = Instant::now();
                link.status().map(|status| (link, status, asked.elapsed().as_millis() as u64))
            })
        })
        .collect();
    let mut answers: Vec<_> = asking.into_iter().filter_map(|t| t.join().ok()).collect();
    // Candidates are in order of preference, except that a Tailscale address
    // that worked last time must not beat a home address that works now.
    answers.sort_by_key(|a| a.as_ref().map(|(link, ..)| wake::is_tailscale(&link.address)).unwrap_or(true));
    let mut problem = Error::Unreachable;
    for answer in answers {
        match answer {
            Ok(found) => return Ok(found),
            // "Refused" or a problem in the desktop's words says more than silence.
            Err(e) if problem == Error::Unreachable => problem = e,
            Err(_) => {}
        }
    }
    Err(problem)
}

/// Record what a desktop said, including where it answered.
fn seen(library: &Library, host: &HostRow, link: &Link, status: &Status) -> HostRow {
    let now_known = HostRow {
        name: status.name.clone(),
        hardware: status.hardware.clone(),
        address: link.address.clone(),
        // An older companion reports neither; keep what we had.
        addresses: if status.addresses.is_empty() { host.addresses.clone() } else { status.addresses.clone() },
        macs: if status.macs.is_empty() { host.macs.clone() } else { status.macs.clone() },
        last_seen: Some(now()),
        ..host.clone()
    };
    let _ = library.store().host_seen(&now_known, now());
    now_known
}

/// Ask one desktop how it is, and mirror its games while it is awake.
fn refresh(library: &Library, host: &HostRow) -> HostView {
    match reach(host) {
        Ok((link, status, latency)) => {
            let host = seen(library, host, &link, &status);
            let mut out = view_with_health(library, &host);
            out.latency_ms = Some(latency);
            out.route = Some(if wake::is_tailscale(&link.address) { "remote" } else { "local" });
            out.state = match status.state {
                HostState::Ready => "ready",
                HostState::Busy => "busy",
                HostState::NotReady => "not_ready",
            };
            out.version = Some(status.version.clone());
            out.playing = status.playing;
            out.problem = status.problem;
            if sync_due(&host.id) {
                let _ = sync_games(library, &host, &link);
            }
            out
        }
        Err(Error::Unreachable) => HostView { state: "unreachable", ..view_with_health(library, host) },
        Err(e) => HostView { state: "not_ready", problem: Some(e.to_string()), ..view_with_health(library, host) },
    }
}

/// Linked desktops as last known, without asking them. Instant.
#[tauri::command]
pub fn hosts_list(library: State<'_, Library>) -> Result<Vec<HostView>, String> {
    let hosts = library.store().hosts()?;
    Ok(hosts.iter().map(|host| view_with_health(&library, host)).collect())
}

/// Ask every desktop at once how it is.
#[tauri::command(async)]
pub fn hosts_refresh(app: AppHandle, library: State<'_, Library>) -> Result<Vec<HostView>, String> {
    let hosts = library.store().hosts()?;
    let asking: Vec<_> = hosts
        .into_iter()
        .map(|host| {
            let library = library.inner().clone();
            std::thread::spawn(move || refresh(&library, &host))
        })
        .collect();
    let views = asking.into_iter().filter_map(|t| t.join().ok()).collect();
    allow_pictures(&app, &art_dir(&library));
    Ok(views)
}

/// Look for companions on this network.
#[tauri::command(async)]
pub fn hosts_discover(admin: State<'_, Admin>) -> Result<Vec<Found>, String> {
    admin.require()?;
    Ok(discover::search(Duration::from_millis(2500)))
}

/// Pair Moonlight on this Ally with Sunshine on the desktop, without anyone
/// typing a PIN: we choose it, give it to Moonlight, and hand it to the
/// desktop over the link that was just made.
fn pair_moonlight(link: &Link) -> Result<(), String> {
    // Without Moonlight, install it first: one thing less to do by hand.
    let exe = match moonlight::find() {
        Some(exe) => exe,
        None => install_moonlight()?,
    };
    if moonlight::paired(&exe, &link.address) {
        return Ok(());
    }
    let pin = moonlight::new_pin();
    let mut asking = moonlight::pair(&exe, &link.address, &pin).map_err(|e| format!("Moonlight could not be started: {e}."))?;
    // Give Moonlight a moment to reach the desktop before the PIN arrives.
    std::thread::sleep(Duration::from_secs(2));
    let handed = link.pair(&pin, &this_device());
    // Moonlight closes its window when pairing worked; do not leave it open
    // in front of CarlaOS when it did not.
    moonlight::finished(&mut asking, Duration::from_secs(if handed.is_ok() { 15 } else { 1 }));
    handed.map_err(|e| e.to_string())?;
    if moonlight::paired(&exe, &link.address) {
        Ok(())
    } else {
        Err("Moonlight and the desktop did not finish pairing. Link this desktop again.".to_string())
    }
}

/// Install Moonlight with Windows' own package manager. Windows asks once
/// to allow it. Returns where it then is.
fn install_moonlight() -> Result<std::path::PathBuf, String> {
    let by_hand = "Install Moonlight from moonlight-stream.org on the Ally, then choose \"Check streaming from it\".";
    let mut command = std::process::Command::new("winget");
    command.args(["install", "--id", "MoonlightGameStreamingProject.Moonlight", "--exact", "--source", "winget", "--silent", "--accept-package-agreements", "--accept-source-agreements"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window in front of CarlaOS.
        command.creation_flags(0x0800_0000);
    }
    let status = command.status().map_err(|e| format!("Windows' package manager (winget) could not be started: {e}. {by_hand}"))?;
    moonlight::find().ok_or_else(|| if status.success() { format!("Moonlight was installed but CarlaOS cannot find it. {by_hand}") } else { format!("Moonlight was not installed. {by_hand}") })
}

/// What this Ally tells a desktop about itself.
fn pulse_of(library: &Library, address: &str, health: Option<Health>) -> Pulse {
    let battery = crate::device::battery();
    Pulse {
        version: VERSION.to_string(),
        moonlight: moonlight::find().is_some(),
        // Only what is known already: asking Moonlight would start it.
        paired: known_paired(address).then_some(true),
        playing: crate::library::locked(&library.0.active).as_ref().map(|a| a.view.title.clone()),
        battery: battery.as_ref().map(|b| b.percent),
        charging: battery.as_ref().map(|b| b.charging),
        screen: carla_core::display::current(),
        health,
    }
}

/// Measure the connection to a desktop, keep the result for its card and
/// tell the desktop, whose dashboard shows it. `None` when it could not be
/// measured.
fn measure_host(library: &Library, host: &HostRow, link: &Link, status: &Status, depth: Depth) -> Option<Health> {
    let away = wake::is_tailscale(&link.address);
    let measure = quality::survey(link, away, depth)?;
    let setting = library.store().host_quality(&host.id).map(|(s, _)| s).unwrap_or_default();
    let choice = quality::choose(setting, Some(measure), away);
    let codec = carla_core::link::codec::fit(&status.graphics, &carla_core::link::codec::graphics());
    let codec = (!status.graphics.is_empty()).then_some(codec);
    let health = Health::of(measure, choice, carla_core::display::current(), &link.address, codec, now());
    let _ = library.store().set_host_health(&host.id, &health);
    let _ = link.pulse(&pulse_of(library, &link.address, Some(health.clone())));
    Some(health)
}

/// Measure the connection to a desktop now, thoroughly. A few seconds.
#[tauri::command(async)]
pub fn hosts_measure(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<Health, String> {
    admin.require()?;
    if crate::library::locked(&library.0.active).is_some() {
        return Err("A game is running. The connection is measured when it is closed.".to_string());
    }
    let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
    let (link, status, _) = reach(&host).map_err(|e| match e {
        Error::Unreachable => format!("{} did not answer. It may be off or asleep.", host.name),
        e => e.to_string(),
    })?;
    let host = seen(&library, &host, &link, &status);
    measure_host(&library, &host, &link, &status, Depth::Full).ok_or_else(|| format!("The connection to {} could not be measured.", host.name))
}

/// How often the connection to each desktop is measured in the background.
const PULSE_EVERY: Duration = Duration::from_secs(45);

/// For as long as CarlaOS runs: measure the connection to every desktop
/// that is on, lightly, and tell each how this Ally is. While a game runs
/// nothing is measured, so a stream never shares the connection with it.
pub fn watch(library: Library) {
    std::thread::spawn(move || {
        // Whether each desktop answered last time, to write down a change only.
        let mut answered: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        loop {
            std::thread::sleep(PULSE_EVERY);
            let playing = crate::library::locked(&library.0.active).is_some();
            for host in library.store().hosts().unwrap_or_default() {
                let reached = reach(&host);
                let up = reached.is_ok();
                if answered.insert(host.id.clone(), up).is_some_and(|before| before != up) {
                    crate::crash::note(&format!("desktop {} {} (a game or stream is running: {playing})", host.id, if up { "answers again" } else { "stopped answering" }));
                }
                // A desktop that is off is left alone: asking costs seconds.
                let Ok((link, status, _)) = reached else { continue };
                if playing {
                    let _ = link.pulse(&pulse_of(&library, &link.address, None));
                } else {
                    let host = seen(&library, &host, &link, &status);
                    if measure_host(&library, &host, &link, &status, Depth::Quick).is_none() {
                        let _ = link.pulse(&pulse_of(&library, &link.address, None));
                    }
                }
            }
        }
    });
}

/// Desktops Moonlight was seen paired with since CarlaOS started, by address.
/// Asking Moonlight costs a second or two, so it is asked once per desktop,
/// and again after a stream from it failed to start.
fn paired_cache() -> std::sync::MutexGuard<'static, Option<std::collections::HashSet<String>>> {
    static PAIRED: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);
    PAIRED.lock().unwrap_or_else(|e| e.into_inner())
}

fn known_paired(address: &str) -> bool {
    paired_cache().as_ref().is_some_and(|set| set.contains(address))
}

fn note_paired(address: &str, paired: bool) {
    let mut cache = paired_cache();
    let set = cache.get_or_insert_with(Default::default);
    if paired {
        set.insert(address.to_string());
    } else {
        set.remove(address);
    }
}

/// Make sure Moonlight is paired with a desktop before streaming from it,
/// pairing it again over the link when it is not: after Sunshine was
/// reinstalled, its clients cleared, or Moonlight set up afresh. The player
/// then never meets Moonlight's own PIN screen.
fn ensure_paired(exe: &std::path::Path, link: &Link) -> Result<(), String> {
    if known_paired(&link.address) {
        return Ok(());
    }
    if moonlight::paired(exe, &link.address) {
        note_paired(&link.address, true);
        return Ok(());
    }
    pair_moonlight(link).map_err(|e| format!("Streaming from this desktop needs pairing again, and it did not work: {e}"))?;
    note_paired(&link.address, true);
    Ok(())
}

/// Forget that a desktop was paired, after a stream from it ended at once.
pub(crate) fn stream_failed(library: &Library, place: &str) {
    crate::crash::note(&format!("stream from {place} ended before it started: pairing will be asked again"));
    if let Some(host) = library.store().hosts().ok().and_then(|hosts| hosts.into_iter().find(|h| h.id == place)) {
        let mut cache = paired_cache();
        if let Some(set) = cache.as_mut() {
            set.remove(&host.address);
            for a in &host.addresses {
                set.remove(a);
            }
        }
    }
}

/// Check one desktop step by step. With `repair`, also fix what CarlaOS can
/// fix by itself (wake it, pair streaming, update it) and check again.
pub(crate) fn check_host(library: &Library, host: &HostRow, repair: bool) -> Report {
    let gather = |host: &HostRow| {
        let exe = moonlight::find();
        let answer = reach(host);
        let (answer, link) = match answer {
            Ok((link, status, ms)) => {
                seen(library, host, &link, &status);
                (Ok((status, link.address.clone(), ms)), Some(link))
            }
            Err(e) => (Err(e), None),
        };
        let paired = match (&exe, &link) {
            (Some(exe), Some(link)) => {
                let paired = moonlight::paired(exe, &link.address);
                note_paired(&link.address, paired);
                Some(paired)
            }
            _ => None,
        };
        // Not under a running game: the measuring packets would share its stream.
        let playing = crate::library::locked(&library.0.active).is_some();
        let measure = link.as_ref().filter(|_| !playing).and_then(|link| quality::survey(link, wake::is_tailscale(&link.address), Depth::Full));
        let setting = library.store().host_quality(&host.id).map(|(s, _)| s).unwrap_or_default();
        let fresh = library.store().hosts().ok().and_then(|hs| hs.into_iter().find(|h| h.id == host.id)).unwrap_or_else(|| host.clone());
        let screen = carla_core::display::current();
        let ports = link.as_ref().map(|link| {
            client::SUNSHINE_PORTS.iter().map(|port| (*port, client::port_open(&link.address, *port, Duration::from_secs(2)).is_some())).collect()
        });
        let fit = match (&link, screen) {
            (Some(link), Some(screen)) => link.fit(screen).ok(),
            _ => None,
        };
        let facts = Facts {
            ally_version: VERSION,
            answer,
            can_wake: !fresh.macs.is_empty(),
            moonlight: exe.is_some(),
            paired,
            measure,
            setting,
            ports,
            screen,
            fit,
            graphics: carla_core::link::codec::graphics(),
            now: now(),
        };
        let health = facts.health();
        if let (Some(health), Some(link)) = (&health, &link) {
            let _ = library.store().set_host_health(&fresh.id, health);
            let _ = link.pulse(&pulse_of(library, &link.address, Some(health.clone())));
        }
        (check::steps(&facts), link, fresh, health)
    };
    let (mut steps, mut link, mut fresh, mut health) = gather(host);
    let mut repaired = Vec::new();
    if repair {
        let failing = |steps: &[check::Step], fix: Fix| steps.iter().any(|s| s.fix == Some(fix) && matches!(s.outcome, Outcome::Fail | Outcome::Warn));
        let mut again = false;
        if failing(&steps, Fix::Wake) {
            match wake_and_wait(&fresh, &|| false) {
                Ok(_) => repaired.push(format!("Woke {}.", fresh.name)),
                Err(e) => repaired.push(format!("Could not wake it: {e}")),
            }
            again = true;
        }
        if failing(&steps, Fix::Moonlight) {
            match install_moonlight() {
                Ok(_) => repaired.push("Installed Moonlight on the Ally.".to_string()),
                Err(e) => repaired.push(e),
            }
            again = true;
        }
        if again {
            (steps, link, fresh, health) = gather(&fresh);
            again = false;
        }
        if let Some(l) = link.as_ref().filter(|_| failing(&steps, Fix::Pair)) {
            match pair_moonlight(l) {
                Ok(()) => {
                    note_paired(&l.address, true);
                    repaired.push("Paired Moonlight with Sunshine there again.".to_string());
                }
                Err(e) => repaired.push(format!("Pairing did not work: {e}")),
            }
            again = true;
        }
        if let Some(l) = link.as_ref().filter(|_| failing(&steps, Fix::Update)) {
            match l.update() {
                Ok(u) if u.updating => repaired.push(format!("The desktop is updating to {}; it is away for a minute.", u.version)),
                Ok(u) => repaired.push(format!("The desktop already has the newest release, {}.", u.version)),
                Err(e) => repaired.push(format!("The desktop did not update: {e}")),
            }
        }
        if again {
            (steps, _, fresh, health) = gather(&fresh);
        }
    }
    Report { id: fresh.id.clone(), name: fresh.name.clone(), steps, repaired, health }
}

/// Check a desktop step by step: what works, what does not, and what would fix it.
#[tauri::command(async)]
pub fn hosts_check(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<Report, String> {
    admin.require()?;
    let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
    Ok(check_host(&library, &host, false))
}

/// Do one of the fixes a check offered, then check again.
#[tauri::command(async)]
pub fn hosts_repair(admin: State<'_, Admin>, library: State<'_, Library>, id: String, fix: Fix) -> Result<Report, String> {
    admin.require()?;
    let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
    let note = match fix {
        Fix::Wake => match wake_and_wait(&host, &|| false) {
            Ok(_) => format!("Woke {}.", host.name),
            Err(e) => return Err(e),
        },
        Fix::Pair => {
            let (link, ..) = reach(&host).map_err(|e| e.to_string())?;
            pair_moonlight(&link)?;
            note_paired(&link.address, true);
            "Paired Moonlight with Sunshine there again.".to_string()
        }
        Fix::Update => {
            let u = hosts_update(admin, library.clone(), id)?;
            if u.updating {
                format!("The desktop is updating to {}; it is away for a minute.", u.version)
            } else {
                format!("The desktop already has the newest release, {}.", u.version)
            }
        }
        Fix::Moonlight => {
            install_moonlight()?;
            "Installed Moonlight on the Ally.".to_string()
        }
        Fix::Relink => return Err("Linking again starts at the desktop: open CarlaOS Desktop Host there, then choose Add a desktop here.".to_string()),
    };
    let mut report = check_host(&library, &host, false);
    report.repaired.push(note);
    Ok(report)
}

#[tauri::command(async)]
pub fn hosts_link(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, address: String, code: String) -> Result<Linked, String> {
    admin.require()?;
    let address = address.trim();
    if address.is_empty() {
        return Err("Say which desktop.".to_string());
    }
    let (hello, link) = client::enrol(address, PORT, &code, &this_device()).map_err(|e| match e {
        Error::Unreachable => "Nothing answered at that address. Is the companion running on the desktop?".to_string(),
        other => other.to_string(),
    })?;
    finish_link(&app, &library, hello, link)
}

/// A link that waits to be allowed at the desktop. One at a time.
static ASKING: Mutex<Option<std::sync::Arc<client::Asking>>> = Mutex::new(None);

/// Ask a desktop to link without a code. Answers with the number both
/// screens show; `hosts_link_wait` then waits for someone to allow it there.
#[tauri::command(async)]
pub fn hosts_link_ask(admin: State<'_, Admin>, address: String) -> Result<String, String> {
    admin.require()?;
    let asking = client::enrol_ask(address.trim(), PORT, &this_device()).map_err(|e| match e {
        Error::Unreachable => "Nothing answered at that address. Is the companion running on the desktop?".to_string(),
        other => other.to_string(),
    })?;
    let check = asking.check.clone();
    *ASKING.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::sync::Arc::new(asking));
    Ok(check)
}

/// Wait until the link that was asked for is allowed at the desktop, then
/// finish it as a code would.
#[tauri::command(async)]
pub fn hosts_link_wait(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>) -> Result<Linked, String> {
    admin.require()?;
    let asking = ASKING.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or("Choose the desktop again.")?;
    let still_ours = || ASKING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|a| std::sync::Arc::ptr_eq(a, &asking));
    let end = Instant::now() + Duration::from_secs(carla_core::link::enrol::APPROVE_LIFETIME as u64);
    let (hello, link) = loop {
        match asking.finish() {
            Ok(Some(done)) => break done,
            Ok(None) if Instant::now() < end && still_ours() => std::thread::sleep(Duration::from_secs(1)),
            Ok(None) => return Err("Nobody allowed it at the desktop in time. Choose the desktop again.".to_string()),
            Err(Error::Unreachable) => return Err("The desktop stopped answering.".to_string()),
            Err(e) => return Err(e.to_string()),
        }
    };
    *ASKING.lock().unwrap_or_else(|e| e.into_inner()) = None;
    finish_link(&app, &library, hello, link)
}

/// Stop waiting for a link to be allowed.
#[tauri::command]
pub fn hosts_link_cancel() {
    *ASKING.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// What follows every new link, however it was made: keep it, pair
/// Moonlight, fetch the desktop's games and measure the connection.
fn finish_link(app: &AppHandle, library: &Library, hello: carla_core::link::Hello, link: Link) -> Result<Linked, String> {
    let host = HostRow {
        id: hello.id,
        name: hello.name,
        address: link.address.clone(),
        port: link.port,
        device_id: link.device_id.clone(),
        key: secret::seal(&link.key)?,
        hardware: String::new(),
        last_seen: Some(now()),
        addresses: Vec::new(),
        macs: Vec::new(),
    };
    library.store().save_host(&host, now())?;
    let warning = pair_moonlight(&link).err();
    note_paired(&link.address, warning.is_none());
    let games = sync_games(library, &host, &link).unwrap_or(0);
    allow_pictures(app, &art_dir(library));
    if let Ok(status) = link.status() {
        measure_host(library, &host, &link, &status, Depth::Full);
    }
    Ok(Linked { host: refresh(library, &host), games, warning })
}

#[tauri::command]
pub fn hosts_remove(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<(), String> {
    admin.require()?;
    library.store().remove_host(&id)
}

/// Look through a desktop's games again now, without waiting for the next
/// regular look. For after a game was installed or added there.
#[tauri::command(async)]
pub fn hosts_rescan(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<usize, String> {
    admin.require()?;
    let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
    let (link, status, _) = reach(&host).map_err(|e| match e {
        Error::Unreachable => format!("{} did not answer. It may be off or asleep.", host.name),
        e => e.to_string(),
    })?;
    let host = seen(&library, &host, &link, &status);
    let games = sync_games(&library, &host, &link)?;
    allow_pictures(&app, &art_dir(&library));
    Ok(games)
}

/// Send a desktop to sleep. It refuses while a game runs there.
#[tauri::command(async)]
pub fn hosts_sleep(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<(), String> {
    admin.require()?;
    let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
    match reach(&host) {
        Ok((link, ..)) => link.sleep().map_err(|e| e.to_string()),
        Err(Error::Unreachable) => Err(format!("{} is not answering. It may already be asleep.", host.name)),
        Err(e) => Err(e.to_string()),
    }
}

/// After a stream for which CarlaOS woke the desktop: let it sleep again.
/// Quiet, and the desktop itself refuses while the game is still open there.
pub(crate) fn sleep_again(library: &Library, place: &str) {
    let Some(host) = library.store().hosts().ok().and_then(|hosts| hosts.into_iter().find(|h| h.id == place)) else { return };
    // The game needs a moment to close after the stream has ended.
    std::thread::sleep(Duration::from_secs(20));
    if let Ok((link, status, _)) = reach(&host) {
        if status.state != HostState::Busy {
            let _ = link.sleep();
        }
    }
}

/// Close the game a desktop is streaming, because the player chose to.
/// Quiet: the stream is stopped either way.
pub(crate) fn end_game(library: &Library, place: &str) {
    let Some(host) = library.store().hosts().ok().and_then(|hosts| hosts.into_iter().find(|h| h.id == place)) else { return };
    if let Ok((link, ..)) = reach(&host) {
        let _ = link.end_game();
    }
}

/// Take away the loading screen a desktop shows over its desktop during a
/// stream, so the player can see and answer what is behind it.
pub(crate) fn reveal(library: &Library, place: &str) -> Result<(), String> {
    let host = library.store().hosts()?.into_iter().find(|h| h.id == place).ok_or("That desktop is no longer set up.")?;
    let (link, ..) = reach(&host).map_err(|e| e.to_string())?;
    link.reveal().map_err(|e| e.to_string())
}

/// What the store on a desktop still has to do for a game, asked now.
/// `None` when the desktop does not answer or is too old to know.
pub(crate) fn pending_update(library: &Library, game_id: &str, place: &str) -> Option<Option<carla_core::library::Update>> {
    let host = library.store().hosts().ok()?.into_iter().find(|h| h.id == place)?;
    let (link, ..) = reach(&host).ok()?;
    link.pending_update(game_id).ok()
}

/// A game's achievements from the first desktop that has it and answers,
/// with that desktop's name.
pub(crate) fn achievements(library: &Library, game_id: &str, places: &[String]) -> Option<(Vec<carla_core::library::achievements::Achievement>, String)> {
    let hosts = library.store().hosts().ok()?;
    hosts.iter().filter(|h| places.contains(&h.id)).find_map(|host| {
        let (link, ..) = reach(host).ok()?;
        let list = link.achievements(game_id).ok()?;
        (!list.is_empty()).then(|| (list, host.name.clone()))
    })
}

/// Send the wake-up packet to a desktop. Anyone may: it only turns on a
/// computer the administrator linked. Says whether there was anything to send.
#[tauri::command(async)]
pub fn hosts_wake(library: State<'_, Library>, id: String) -> Result<(), String> {
    let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
    if wake::wake(&host.macs) == 0 {
        return Err(format!("{} has not said how it can be woken yet. Turn it on by hand once.", host.name));
    }
    Ok(())
}

/// Ask a desktop to update its companion to the newest release. It answers
/// with the version it is moving to, then is away for a moment.
#[tauri::command(async)]
pub fn hosts_update(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<Updating, String> {
    admin.require()?;
    let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
    let (link, ..) = reach(&host).map_err(|e| match e {
        Error::Unreachable => format!("{} did not answer. It has to be on to be updated.", host.name),
        other => other.to_string(),
    })?;
    link.update().map_err(|e| match e {
        // A companion from before updating existed does not know the request.
        Error::Host(text) if text.contains("does not know that request") => {
            format!("{} has an older CarlaOS that cannot update itself yet. Install the newest Desktop Host on it once by hand.", host.name)
        }
        other => other.to_string(),
    })
}

/// How long a desktop gets to wake up before we say it did not.
const WAKE_LIMIT: Duration = Duration::from_secs(90);

/// Wake a desktop and wait until it answers. `stopped` lets the player give up.
fn wake_and_wait(host: &HostRow, stopped: &dyn Fn() -> bool) -> Result<(Link, Status, u64), String> {
    let started = Instant::now();
    let mut shouted = None::<Instant>;
    while started.elapsed() < WAKE_LIMIT {
        // Again every so often: the first packet can arrive before the
        // adapter is listening.
        if shouted.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
            wake::wake(&host.macs);
            shouted = Some(Instant::now());
        }
        std::thread::sleep(Duration::from_secs(2));
        if stopped() {
            return Err("Stopped.".to_string());
        }
        match reach(host) {
            Ok(found) => return Ok(found),
            Err(Error::Unreachable) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("{} did not wake up. It may be switched off at the wall, or waking is not turned on for it.", host.name))
}

/// Decide the picture of the stream that is about to start, measuring the
/// connection unless the administrator fixed the picture, and note it.
fn picture_for(library: &Library, host: &HostRow, link: &Link) -> Choice {
    let setting = library.store().host_quality(&host.id).map(|(setting, _)| setting).unwrap_or_default();
    let away = wake::is_tailscale(&link.address);
    let measure = match setting {
        Setting::Fixed(_) => None,
        Setting::Auto { .. } => quality::survey(link, away, Depth::Quick),
    };
    let choice = quality::choose(setting, measure, away);
    let _ = library.store().set_host_streamed(&host.id, &Streamed { choice, at: now() });
    choice
}

/// Tell the desktop which game is wanted and start Moonlight on it. A desktop
/// that does not answer is woken first; `waking` is called when that starts.
/// Also says whether the desktop had to be woken.
///
/// With `resuming` the game is already open on the desktop and only the
/// picture is fetched again: saves are left alone, since the game there is
/// using them.
pub(crate) fn start_stream(library: &Library, game_id: &str, place: &str, waking: &dyn Fn(), stopped: &dyn Fn() -> bool, resuming: bool) -> Result<(Child, bool), String> {
    let host = library.store().hosts()?.into_iter().find(|h| h.id == place).ok_or("That desktop is no longer set up.")?;
    let exe = moonlight::find().ok_or("Moonlight is not installed on this Ally.")?;
    let mut woke = false;
    let (link, status, _) = match reach(&host) {
        Ok(found) => found,
        Err(Error::Unreachable) if !host.macs.is_empty() => {
            waking();
            woke = true;
            wake_and_wait(&host, stopped)?
        }
        Err(Error::Unreachable) => return Err(format!("{} did not answer. It may be off or asleep.", host.name)),
        Err(e) => return Err(e.to_string()),
    };
    seen(library, &host, &link, &status);
    // What was played on the Ally since goes to the desktop first.
    if !resuming {
        crate::savesync::before_stream(library, game_id, &host, &link);
    }
    // A desktop that has just woken may need a moment before Sunshine is up.
    // The picture is made for this Ally's own screen, and the desktop is told
    // its size so it can switch its screen to match.
    let choice = picture_for(library, &host, &link);
    let picture = choice.picture_on(carla_core::display::current());
    let screen = carla_core::display::Mode::new(picture.width, picture.height, picture.fps);
    let mut tries = 0;
    loop {
        match link.prepare_for(game_id, Some(screen)) {
            Ok(_) => break,
            Err(Error::Host(text)) if text.contains("Sunshine is not running") && tries < 10 && !stopped() => {
                tries += 1;
                std::thread::sleep(Duration::from_secs(3));
            }
            Err(Error::Unreachable) => return Err(format!("{} stopped answering.", host.name)),
            Err(e) => return Err(e.to_string()),
        }
    }
    ensure_paired(&exe, &link)?;
    let moonlight = moonlight::stream(&exe, &link.address, picture).map_err(|e| format!("Moonlight could not be started: {e}."))?;
    Ok((moonlight, woke))
}
