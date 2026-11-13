//! What the pre-game view shows about one game: achievements, milestones and
//! saves. Also backups of saves, and looking up public details for games
//! added by hand.

use std::path::PathBuf;

use carla_core::library::achievements::{self, Achievement};
use carla_core::library::meta::{self, Match};
use carla_core::library::saves::{self, Backups, Overview};
use carla_core::library::LOCAL;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::admin::Admin;
use crate::library::{allow_pictures, locked, now, Library};

#[derive(Debug, Clone, Serialize)]
pub struct Details {
    /// The game's own achievements, when they are known.
    achievements: Vec<Achievement>,
    /// Where they were read: "steam" on this machine, or a desktop's name.
    achievements_from: Option<String>,
    /// CarlaOS milestones, which every game has.
    milestones: Vec<Achievement>,
    /// Saves on this machine, when CarlaOS knows where they are.
    saves: Option<Overview>,
    /// How the saves stand with each desktop that keeps them in step.
    sync: Vec<crate::savesync::SyncView>,
    /// Number of finished play sessions.
    sessions: usize,
}

fn steam_app(game_id: &str) -> Option<&str> {
    game_id.strip_prefix("steam:")
}

fn backups(library: &Library, game_id: &str) -> Backups {
    Backups::new(&library.0.data_dir.join("saves"), game_id)
}

/// Where this game's saves are on this machine: where the administrator
/// said, or Steam's own cloud folder for a Steam game.
fn save_dir(library: &Library, game_id: &str) -> Option<PathBuf> {
    if let Ok(Some(chosen)) = library.store().save_path(game_id) {
        return chosen.is_dir().then_some(chosen);
    }
    let root = locked(&library.0.steam_root).clone()?;
    saves::steam_folder(&root, steam_app(game_id)?)
}

fn overview(library: &Library, game_id: &str) -> Option<Overview> {
    backups(library, game_id).overview(&save_dir(library, game_id)?)
}

/// The game's own achievements: from Steam on this machine when it has been
/// played here, otherwise from a desktop it is installed on, otherwise what a
/// desktop last said.
fn own_achievements(library: &Library, game_id: &str, places: &[String]) -> (Vec<Achievement>, Option<String>) {
    let Some(app) = steam_app(game_id) else { return (Vec::new(), None) };
    let root = locked(&library.0.steam_root).clone();
    let here = root.and_then(|root| achievements::steam(&root, app));
    // Played here: this machine knows best.
    if let Some(list) = here.as_ref().filter(|list| list.iter().any(|a| a.unlocked.is_some())) {
        return (list.clone(), Some("steam".to_string()));
    }
    if places.iter().any(|p| p != LOCAL) {
        if let Some((list, host)) = crate::hosts::achievements(library, game_id, places) {
            if let Ok(json) = serde_json::to_string(&(&list, &host)) {
                let _ = library.store().cache_achievements(game_id, &json, now());
            }
            return (list, Some(host));
        }
        let cached = library.store().cached_achievements(game_id).ok().flatten();
        if let Some((list, host)) = cached.and_then(|(json, _)| serde_json::from_str::<(Vec<Achievement>, String)>(&json).ok()) {
            return (list, Some(host));
        }
    }
    match here {
        Some(list) => (list, Some("steam".to_string())),
        None => (Vec::new(), None),
    }
}

#[tauri::command(async)]
pub fn library_details(library: State<'_, Library>, game_id: String) -> Result<Details, String> {
    let places = library.store().list()?.into_iter().find(|g| g.id == game_id).map(|g| g.places).unwrap_or_default();
    let sessions = library.store().sessions(&game_id)?;
    let (own, from) = own_achievements(&library, &game_id, &places);
    Ok(Details {
        achievements: own,
        achievements_from: from,
        milestones: achievements::milestones(&sessions),
        saves: if places.iter().any(|p| p == LOCAL) { overview(&library, &game_id) } else { None },
        sync: crate::savesync::views(&library, &game_id),
        sessions: sessions.len(),
    })
}

/// What the stores still have to do before this game starts, in each place
/// it is installed, asked now rather than at the last look through the
/// library. The catalogue is brought up to date with the answer.
#[tauri::command(async)]
pub fn library_updates(library: State<'_, Library>, game_id: String) -> Result<Vec<carla_core::library::Waiting>, String> {
    let game = library.store().list()?.into_iter().find(|g| g.id == game_id);
    let places = game.map(|g| g.places).unwrap_or_default();
    for place in &places {
        let fresh = if place == LOCAL {
            let install = library.store().install(&game_id, LOCAL)?;
            let root = locked(&library.0.steam_root).clone();
            Some(carla_core::library::update_of(&install.source, &install.source_id, root.as_deref()))
        } else {
            crate::hosts::pending_update(&library, &game_id, place)
        };
        // A desktop that did not answer keeps what it last said.
        if let Some(update) = fresh {
            library.store().set_update(&game_id, place, update)?;
        }
    }
    Ok(library.store().list()?.into_iter().find(|g| g.id == game_id).map(|g| g.updates).unwrap_or_default())
}

/// Copy the saves aside after a play session, if they changed. Quiet: a
/// backup that cannot be made must never get in the way of playing.
pub(crate) fn backup_after_play(library: &Library, game_id: &str) {
    if let Some(dir) = save_dir(library, game_id) {
        let _ = backups(library, game_id).take(&dir, now(), true);
    }
}

/// Back up now. Anyone may: it only ever adds a copy.
#[tauri::command(async)]
pub fn saves_backup(library: State<'_, Library>, game_id: String) -> Result<Overview, String> {
    let dir = save_dir(&library, &game_id).ok_or("CarlaOS does not know where this game keeps its saves.")?;
    backups(&library, &game_id).take(&dir, now(), false)?;
    overview(&library, &game_id).ok_or_else(|| "The save folder could not be read.".to_string())
}

#[tauri::command(async)]
pub fn saves_restore(admin: State<'_, Admin>, library: State<'_, Library>, game_id: String, backup_id: String) -> Result<Overview, String> {
    admin.require()?;
    if locked(&library.0.active).is_some() {
        return Err("Close the game first.".to_string());
    }
    let dir = save_dir(&library, &game_id).ok_or("CarlaOS does not know where this game keeps its saves.")?;
    backups(&library, &game_id).restore(&dir, &backup_id, now())?;
    overview(&library, &game_id).ok_or_else(|| "The save folder could not be read.".to_string())
}

/// The administrator says where a game keeps its saves.
#[tauri::command(async)]
pub fn saves_pick_folder(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, game_id: String) -> Result<Option<Overview>, String> {
    admin.require()?;
    let Some(dir) = app.dialog().file().set_title("Choose the folder with this game's saves").blocking_pick_folder().and_then(|f| f.into_path().ok()) else {
        return Ok(None);
    };
    if !saves::sensible(&dir) {
        return Err("That folder is too general. Choose the folder that holds only this game's saves.".to_string());
    }
    if saves::measure(&dir).is_none() {
        return Err("That folder is too large to be a save folder.".to_string());
    }
    library.store().set_save_path(&game_id, Some(&dir))?;
    Ok(overview(&library, &game_id))
}

/// Forget a chosen save folder. Backups already made are kept.
#[tauri::command]
pub fn saves_forget_folder(admin: State<'_, Admin>, library: State<'_, Library>, game_id: String) -> Result<(), String> {
    admin.require()?;
    library.store().set_save_path(&game_id, None)
}

/// Search the public store for a game's details, by the name typed.
#[tauri::command(async)]
pub fn meta_search(admin: State<'_, Admin>, term: String) -> Result<Vec<Match>, String> {
    admin.require()?;
    meta::search(&term)
}

#[derive(Debug, Clone, Serialize)]
pub struct Fetched {
    name: String,
    description: String,
    /// The downloaded cover, as a file on this machine.
    cover: Option<String>,
}

#[tauri::command(async)]
pub fn meta_fetch(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<Fetched, String> {
    admin.require()?;
    let details = meta::details(&id)?;
    let dir = library.0.art_dir.join("fetched");
    let cover = meta::cover(&id, details.header.as_deref()).ok().and_then(|bytes| {
        let ext = if bytes.starts_with(&[0xFF, 0xD8]) { "jpg" } else { "png" };
        let path = dir.join(format!("{id}.{ext}"));
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, bytes).ok()?;
        Some(path.to_string_lossy().into_owned())
    });
    allow_pictures(&app, &library.0.art_dir);
    Ok(Fetched { name: details.name, description: details.description, cover })
}
