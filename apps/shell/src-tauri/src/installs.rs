//! Installing games of the Steam library, here or on a linked desktop, and
//! following what is being fetched. Steam does the fetching; see
//! `carla_core::library::downloads`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use carla_core::library::downloads::{self, Download, Phase, Started};
use carla_core::library::owned::{self, App};
use carla_core::library::{meta, steam, HostRow, Offered, LOCAL};
use carla_core::link::client::{Error, Link};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::library::{locked, Library};

/// Sent to the interface whenever what is being fetched changes.
const DOWNLOADS_EVENT: &str = "library://downloads";
const CHANGED_EVENT: &str = "library://changed";

/// One game being fetched somewhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DownloadView {
    game_id: String,
    /// "local" or a desktop's id.
    place: String,
    title: String,
    phase: Phase,
    done: u64,
    total: u64,
    /// An installed game being brought up to date.
    update: bool,
}

#[derive(Default)]
struct Watched {
    /// Desktops that were fetching something when last asked.
    desktops: HashSet<String>,
    last: Vec<DownloadView>,
}

static WATCHED: Mutex<Option<Watched>> = Mutex::new(None);
/// Set when something was just started, so the next look does not wait.
static LOOK_NOW: AtomicBool = AtomicBool::new(false);

fn view(place: &str, d: Download) -> DownloadView {
    DownloadView { game_id: format!("{}:{}", d.source, d.source_id), place: place.to_string(), title: d.title, phase: d.phase, done: d.done, total: d.total, update: d.update }
}

fn steam_root(library: &Library) -> Option<PathBuf> {
    let known = locked(&library.0.steam_root).clone();
    known.or_else(steam::find_root)
}

/// Bring the catalogue in line with what Steam could install on this Ally.
pub(crate) fn sync_local(library: &Library, root: Option<&std::path::Path>) {
    let Some(root) = root else { return };
    // When Steam's files cannot be read just now, what was known stays.
    let Ok(apps) = owned::not_installed(root) else { return };
    let offered: Vec<Offered> = apps.into_iter().map(|app| Offered { cover: steam::cover(root, &app.appid), source_id: app.appid, title: app.title }).collect();
    let _ = library.store().sync_available(LOCAL, "steam", &offered);
}

/// The same for a desktop, and note whether it is fetching something.
pub(crate) fn sync_host(library: &Library, host: &HostRow, link: &Link) {
    let offered: Vec<Offered> = match link.library() {
        Ok(there) => there.games.into_iter().filter(|g| g.source == "steam" && g.id == format!("steam:{}", g.source_id)).map(|g| Offered { source_id: g.source_id, title: g.title, cover: None }).collect(),
        // A companion from before installs existed offers nothing.
        Err(Error::Host(_)) => Vec::new(),
        Err(_) => return,
    };
    let _ = library.store().sync_available(&host.id, "steam", &offered);
    if link.downloads().is_ok_and(|list| !list.is_empty()) {
        locked(&WATCHED).get_or_insert_with(Watched::default).desktops.insert(host.id.clone());
        LOOK_NOW.store(true, Ordering::SeqCst);
    }
}

/// Fetch covers for games that are not installed anywhere, so the library
/// does not look half empty. One at a time, in the background, and only
/// pictures from Steam's public store.
pub(crate) fn fetch_covers(app: &AppHandle, library: &Library) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, library) = (app.clone(), library.clone());
    std::thread::spawn(move || {
        let dir = library.0.art_dir.join("steam");
        let wanted = library.store().coverless("steam").unwrap_or_default();
        let (mut misses, mut new) = (0, 0);
        for (game_id, appid) in wanted.into_iter().take(800) {
            if !appid.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let path = dir.join(format!("{appid}.jpg"));
            if !path.is_file() {
                match meta::cover(&appid, None) {
                    Ok(bytes) if std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, &bytes).is_ok() => misses = 0,
                    _ => {
                        // Some games have no cover; several in a row means no connection.
                        misses += 1;
                        if misses >= 8 {
                            break;
                        }
                        continue;
                    }
                }
            }
            let _ = library.store().set_cover_if_none(&game_id, &path);
            new += 1;
            if new % 12 == 0 {
                let _ = app.emit(CHANGED_EVENT, ());
            }
        }
        if new % 12 != 0 {
            let _ = app.emit(CHANGED_EVENT, ());
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

#[derive(Debug, Clone, Serialize)]
pub struct StartedView {
    /// Steam is showing its own window and waits for an answer there.
    asked_store: bool,
}

fn find_host(library: &Library, id: &str) -> Result<HostRow, String> {
    library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or_else(|| "That desktop is no longer set up.".to_string())
}

/// Ask Steam to install a game of the library, on this Ally or on a desktop.
#[tauri::command(async)]
pub fn installs_start(app: AppHandle, library: State<'_, Library>, game_id: String, place: String) -> Result<StartedView, String> {
    let (source, source_id, title) = library.store().offered(&game_id, &place)?;
    if source != "steam" {
        return Err("Only Steam games can be installed from CarlaOS so far.".to_string());
    }
    let asked_store = if place == LOCAL {
        let root = steam_root(&library).ok_or("Steam is not installed on this Ally.")?;
        // Steam has to start again to begin, which would disturb a game.
        if let Some(running) = locked(&library.0.active).as_ref() {
            if downloads::restart_closes_steam() {
                return Err(format!("{} is still running. Install {title} once it is closed.", running.view.title));
            }
        }
        let known = owned::library(&root).ok().and_then(|apps| apps.into_iter().find(|a| a.appid == source_id));
        let wanted = known.unwrap_or(App { appid: source_id, title, installdir: String::new() });
        downloads::start_here(&root, &wanted)? == Started::AskedSteam
    } else {
        let host = find_host(&library, &place)?;
        let (link, ..) = crate::hosts::reach(&host).map_err(|e| match e {
            Error::Unreachable => format!("{} did not answer. It has to be on to install a game there.", host.name),
            other => other.to_string(),
        })?;
        let started = link.install(&game_id).map_err(|e| e.to_string())?;
        locked(&WATCHED).get_or_insert_with(Watched::default).desktops.insert(host.id);
        started.asked_store
    };
    LOOK_NOW.store(true, Ordering::SeqCst);
    let _ = app.emit(DOWNLOADS_EVENT, look(&app, &library));
    Ok(StartedView { asked_store })
}

/// What is being fetched, as last seen.
#[tauri::command]
pub fn installs_list() -> Vec<DownloadView> {
    locked(&WATCHED).as_ref().map(|w| w.last.clone()).unwrap_or_default()
}

/// Room for new games on this Ally, in bytes.
#[tauri::command(async)]
pub fn installs_room(library: State<'_, Library>) -> Option<u64> {
    steam_root(&library).and_then(|root| downloads::roomiest(&root).1)
}

/// Look at everything that is being fetched, here and on the desktops that
/// were fetching. When something has finished, the library is read again.
fn look(app: &AppHandle, library: &Library) -> Vec<DownloadView> {
    let mut now: Vec<DownloadView> = steam_root(library).map(|root| downloads::steam(&root)).unwrap_or_default().into_iter().map(|d| view(LOCAL, d)).collect();
    let desktops: Vec<String> = locked(&WATCHED).get_or_insert_with(Watched::default).desktops.iter().cloned().collect();
    let mut links: HashMap<String, (HostRow, Link)> = HashMap::new();
    let mut quiet = Vec::new();
    for id in desktops {
        let there = find_host(library, &id).ok().and_then(|host| crate::hosts::reach(&host).ok().map(|(link, ..)| (host, link)));
        match there.as_ref().map(|(_, link)| link.downloads()) {
            Some(Ok(list)) if !list.is_empty() => now.extend(list.into_iter().map(|d| view(&id, d))),
            // Nothing left to fetch, or the desktop went away.
            _ => quiet.push(id.clone()),
        }
        if let Some(there) = there {
            links.insert(id, there);
        }
    }
    let before = {
        let mut watched = locked(&WATCHED);
        let watched = watched.get_or_insert_with(Watched::default);
        quiet.iter().for_each(|id| {
            watched.desktops.remove(id);
        });
        std::mem::replace(&mut watched.last, now.clone())
    };
    // A first install that is no longer being fetched is a game to play.
    let gone: Vec<&DownloadView> = before.iter().filter(|b| !now.iter().any(|n| n.game_id == b.game_id && n.place == b.place)).collect();
    let mut changed = false;
    if gone.iter().any(|d| d.place == LOCAL) {
        changed = library.scan(app).is_ok();
    }
    let finished: HashSet<&str> = gone.iter().filter(|d| d.place != LOCAL).map(|d| d.place.as_str()).collect();
    for place in finished {
        if let Some((host, link)) = links.get(place) {
            changed |= crate::hosts::sync_games(library, host, link).is_ok();
        }
    }
    if changed {
        let _ = app.emit(CHANGED_EVENT, ());
    }
    now
}

/// Called once when CarlaOS starts: keep an eye on what is being fetched.
pub fn watch(app: AppHandle, library: Library) {
    std::thread::spawn(move || {
        let mut shown: Vec<DownloadView> = Vec::new();
        loop {
            let now = look(&app, &library);
            if now != shown {
                let _ = app.emit(DOWNLOADS_EVENT, now.clone());
                shown = now;
            }
            // Often while something is moving, rarely otherwise, and hardly
            // at all while a game is being played.
            let playing = locked(&library.0.active).is_some();
            let moving = shown.iter().any(|d| d.phase != Phase::Paused);
            let wait = if playing { 60 } else if moving { 4 } else { 30 };
            for _ in 0..wait * 2 {
                if LOOK_NOW.swap(false, Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    });
}
