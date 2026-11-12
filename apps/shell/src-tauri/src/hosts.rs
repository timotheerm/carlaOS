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
use carla_core::link::moonlight::{self, Picture};
use carla_core::link::{State as HostState, PORT};
use carla_core::secret;
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
    }
}

fn this_device() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty()).unwrap_or_else(|| "Ally".to_string())
}

/// Bring the catalogue in line with what a desktop has installed.
fn sync_games(library: &Library, host: &HostRow, link: &Link) -> Result<usize, String> {
    let games = link.games().map_err(|e| e.to_string())?;
    let have_cover: std::collections::HashSet<String> =
        library.store().list()?.into_iter().filter(|g| g.cover.is_some()).map(|g| g.id).collect();
    let dir = art_dir(library);
    let found: Vec<Scanned> = games
        .iter()
        .filter(|g| g.source == "steam")
        .map(|g| {
            // Fetch a picture only for games that have none here yet.
            let mut cover = None;
            if g.has_cover && !have_cover.contains(&g.id) && g.source_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                let path = dir.join(format!("steam-{}.jpg", g.source_id));
                if path.is_file() {
                    cover = Some(path);
                } else if let Ok(bytes) = link.cover(&g.id) {
                    if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, bytes).is_ok() {
                        cover = Some(path);
                    }
                }
            }
            Scanned { source_id: g.source_id.clone(), title: g.title.clone(), install_path: PathBuf::new(), cover, hero: None }
        })
        .collect();
    library.store().sync(&host.id, "steam", &found, now())?;
    Ok(found.len())
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

/// Ask one desktop how it is, and mirror its games while it is awake.
fn refresh(library: &Library, host: &HostRow) -> HostView {
    let mut out = view(host);
    let link = match link_of(host) {
        Ok(link) => link,
        Err(e) => {
            out.state = "not_ready";
            out.problem = Some(e);
            return out;
        }
    };
    let asked = Instant::now();
    match link.status() {
        Ok(status) => {
            out.latency_ms = Some(asked.elapsed().as_millis() as u64);
            out.state = match status.state {
                HostState::Ready => "ready",
                HostState::Busy => "busy",
                HostState::NotReady => "not_ready",
            };
            out.name = status.name.clone();
            out.hardware = status.hardware.clone();
            out.playing = status.playing;
            out.problem = status.problem;
            out.last_seen = Some(now());
            let _ = library.store().host_seen(&host.id, &status.name, &status.hardware, now());
            if sync_due(&host.id) {
                let _ = sync_games(library, host, &link);
            }
        }
        Err(Error::Unreachable) => out.state = "unreachable",
        Err(e) => {
            out.state = "not_ready";
            out.problem = Some(e.to_string());
        }
    }
    out
}

/// Linked desktops as last known, without asking them. Instant.
#[tauri::command]
pub fn hosts_list(library: State<'_, Library>) -> Result<Vec<HostView>, String> {
    Ok(library.store().hosts()?.iter().map(view).collect())
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
    let exe = moonlight::find().ok_or("Moonlight is not installed on this Ally yet. Install it, then link this desktop again.")?;
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
    let host = HostRow {
        id: hello.id,
        name: hello.name,
        address: link.address.clone(),
        port: link.port,
        device_id: link.device_id.clone(),
        key: secret::seal(&link.key)?,
        hardware: String::new(),
        last_seen: Some(now()),
    };
    library.store().save_host(&host, now())?;
    let warning = pair_moonlight(&link).err();
    let games = sync_games(&library, &host, &link).unwrap_or(0);
    allow_pictures(&app, &art_dir(&library));
    Ok(Linked { host: refresh(&library, &host), games, warning })
}

#[tauri::command]
pub fn hosts_remove(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<(), String> {
    admin.require()?;
    library.store().remove_host(&id)
}

/// Tell the desktop which game is wanted and start Moonlight on it.
pub(crate) fn start_stream(library: &Library, game_id: &str, place: &str) -> Result<Child, String> {
    let host = library.store().hosts()?.into_iter().find(|h| h.id == place).ok_or("That desktop is no longer set up.")?;
    let exe = moonlight::find().ok_or("Moonlight is not installed on this Ally.")?;
    let link = link_of(&host)?;
    link.prepare(game_id).map_err(|e| match e {
        Error::Unreachable => format!("{} did not answer. It may be off or asleep.", host.name),
        other => other.to_string(),
    })?;
    moonlight::stream(&exe, &host.address, Picture::default()).map_err(|e| format!("Moonlight could not be started: {e}."))
}
