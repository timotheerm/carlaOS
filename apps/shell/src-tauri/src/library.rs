//! Commands for the game library: listing, scanning, adding games by hand,
//! starting a game and coming back when it has finished.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use carla_core::library::launch::{self, Phase, Watch};
use carla_core::library::{steam, GameView, ManualGame, Store, LOCAL};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::admin::Admin;

/// Event sent to the interface whenever the running game changes.
const SESSION_EVENT: &str = "library://session";
const POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    game_id: String,
    title: String,
    /// "starting" until the game's program has been seen, then "running".
    phase: &'static str,
}

struct Active {
    view: SessionView,
    /// Set when the player says they have finished and we could not tell.
    stop: bool,
}

struct Inner {
    store: Mutex<Store>,
    steam_root: Mutex<Option<PathBuf>>,
    art_dir: PathBuf,
    active: Mutex<Option<Active>>,
}

#[derive(Clone)]
pub struct Library(Arc<Inner>);

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Let the interface show pictures from this folder.
fn allow_pictures(app: &AppHandle, dir: &Path) {
    if dir.is_dir() {
        let _ = app.asset_protocol_scope().allow_directory(dir, true);
    }
}

impl Library {
    pub fn open(data_dir: &Path) -> Result<Self, String> {
        // A console that will not start is worse than one with an empty
        // library, so a damaged catalogue falls back to one kept in memory.
        let mut store = Store::open(&data_dir.join("library.sqlite")).or_else(|_| Store::in_memory())?;
        let _ = store.close_dangling_sessions();
        Ok(Self(Arc::new(Inner {
            store: Mutex::new(store),
            steam_root: Mutex::new(None),
            art_dir: data_dir.join("art"),
            active: Mutex::new(None),
        })))
    }

    fn store(&self) -> MutexGuard<'_, Store> {
        locked(&self.0.store)
    }

    /// Look at what is installed and bring the catalogue up to date.
    fn scan(&self, app: &AppHandle) -> Result<Vec<GameView>, String> {
        let root = steam::find_root();
        let found = root.as_deref().map(steam::scan).unwrap_or_default();
        if let Some(root) = &root {
            allow_pictures(app, &root.join("appcache").join("librarycache"));
        }
        allow_pictures(app, &self.0.art_dir);
        *locked(&self.0.steam_root) = root;
        let mut store = self.store();
        store.sync(LOCAL, "steam", &found, now())?;
        store.list()
    }
}

#[tauri::command(async)]
pub fn library_scan(app: AppHandle, library: State<'_, Library>) -> Result<Vec<GameView>, String> {
    library.scan(&app)
}

#[tauri::command]
pub fn library_list(library: State<'_, Library>) -> Result<Vec<GameView>, String> {
    library.store().list()
}

#[tauri::command]
pub fn library_set_favourite(library: State<'_, Library>, id: String, favourite: bool) -> Result<(), String> {
    library.store().set_favourite(&id, favourite)
}

#[tauri::command]
pub fn library_session(library: State<'_, Library>) -> Option<SessionView> {
    locked(&library.0.active).as_ref().map(|a| a.view.clone())
}

/// The player says the game is over. Used when we could not tell by ourselves.
#[tauri::command]
pub fn library_end_session(library: State<'_, Library>) {
    if let Some(active) = locked(&library.0.active).as_mut() {
        active.stop = true;
    }
}

#[tauri::command]
pub fn library_launch(app: AppHandle, library: State<'_, Library>, game_id: String, place: String) -> Result<SessionView, String> {
    if place != LOCAL {
        return Err("Streaming from a desktop is not set up yet.".to_string());
    }
    let mut active = locked(&library.0.active);
    if let Some(current) = active.as_ref() {
        return Err(format!("{} is still running.", current.view.title));
    }
    let install = library.store().install(&game_id, &place)?;
    let steam_root = locked(&library.0.steam_root).clone();
    let child = launch::start(&install, steam_root.as_deref())
        .map_err(|e| format!("{} could not be started: {e}.", install.title))?;
    let started = now();
    let session = library.store().start_session(&game_id, &place, started)?;
    let view = SessionView { game_id, title: install.title.clone(), phase: "starting" };
    *active = Some(Active { view: view.clone(), stop: false });
    drop(active);
    let _ = app.emit(SESSION_EVENT, Some(view.clone()));

    let library = library.inner().clone();
    std::thread::spawn(move || {
        let mut watch = Watch::new(&install, child);
        let mut played = false;
        loop {
            std::thread::sleep(POLL);
            let phase = watch.poll();
            let mut active = locked(&library.0.active);
            let Some(current) = active.as_mut() else { break };
            if current.stop {
                break;
            }
            match phase {
                Phase::Running if current.view.phase != "running" => {
                    played = true;
                    current.view.phase = "running";
                    let _ = app.emit(SESSION_EVENT, Some(current.view.clone()));
                }
                Phase::Running | Phase::Starting => {}
                Phase::Finished | Phase::NeverStarted => break,
            }
        }
        // A game that never appeared was not played.
        let ended = if played { now() } else { started };
        let _ = library.store().end_session(session, ended);
        *locked(&library.0.active) = None;
        let _ = app.emit(SESSION_EVENT, None::<SessionView>);
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    });
    Ok(view)
}

/// Ask the administrator to choose a file with the Windows file picker.
#[tauri::command(async)]
pub fn library_pick(app: AppHandle, admin: State<'_, Admin>, what: String) -> Result<Option<String>, String> {
    admin.require()?;
    let dialog = app.dialog().file();
    let dialog = match what.as_str() {
        "image" => dialog.set_title("Choose artwork").add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]),
        _ => dialog.set_title("Choose the game's program").add_filter("Programs", &["exe", "bat", "cmd", "lnk"]),
    };
    Ok(dialog.blocking_pick_file().and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().into_owned()))
}

/// Copy chosen artwork next to the catalogue, so it survives the original
/// being moved or deleted.
fn keep_artwork(art_dir: &Path, picture: &Path) -> Result<PathBuf, String> {
    let problem = |e: std::io::Error| format!("The artwork could not be copied: {e}.");
    std::fs::create_dir_all(art_dir).map_err(problem)?;
    let ext = picture.extension().and_then(|e| e.to_str()).unwrap_or("png").to_lowercase();
    let stem = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let kept = art_dir.join(format!("{stem:x}.{ext}"));
    std::fs::copy(picture, &kept).map_err(problem)?;
    Ok(kept)
}

#[tauri::command(async)]
pub fn library_add_manual(
    app: AppHandle,
    admin: State<'_, Admin>,
    library: State<'_, Library>,
    mut game: ManualGame,
) -> Result<String, String> {
    admin.require()?;
    if !game.exe.is_file() {
        return Err("That program could not be found.".to_string());
    }
    if let Some(picture) = game.cover.take() {
        game.cover = Some(keep_artwork(&library.0.art_dir, &picture)?);
        allow_pictures(&app, &library.0.art_dir);
    }
    library.store().add_manual(&game, now())
}

#[tauri::command]
pub fn library_remove_manual(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<(), String> {
    admin.require()?;
    library.store().remove_manual(&id)
}
