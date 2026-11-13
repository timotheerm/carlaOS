//! Choosing a game's pictures, and looking at a program before it is added.
//! See `carla_core::library::art`. All of it is for the administrator: on
//! the Ally with the PIN, or from a desktop that was allowed.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use carla_core::library::art::{self, Candidate, Found, Shape, Wanted};
use carla_core::library::launch::split_args;
use carla_core::library::ports::{self, Inspected};
use carla_core::library::{manual_key, meta, Kind, LOCAL};
use carla_core::secret;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::admin::Admin;
use crate::library::{allow_pictures, Library};

/// The name the SteamGridDB key is kept under.
const KEY: &str = "steamgriddb_key";

/// The administrator's SteamGridDB key, when one was entered.
fn key(library: &Library) -> Option<String> {
    let sealed = library.store().setting(KEY).ok().flatten()?;
    String::from_utf8(secret::open(&sealed).ok()?).ok().filter(|k| art::key_shape(k))
}

/// Keep a SteamGridDB key, or forget it when it is empty. Says whether one
/// is kept afterwards.
pub(crate) fn set_key(library: &Library, key: &str) -> Result<bool, String> {
    let key = key.trim();
    if key.is_empty() {
        library.store().set_setting(KEY, None)?;
        return Ok(false);
    }
    if !art::key_shape(key) {
        return Err("That does not look like a SteamGridDB key. It is the long line of letters and digits under Preferences, API on steamgriddb.com.".to_string());
    }
    let sealed = secret::seal(key.as_bytes())?;
    library.store().set_setting(KEY, Some(&sealed))?;
    Ok(true)
}

/// The game file an emulated game starts, without its ending: the name box
/// art is filed under.
fn game_file(library: &Library, game_id: &str) -> Option<String> {
    let install = library.store().install(game_id, LOCAL).ok()?;
    split_args(&install.args).iter().rev().map(Path::new).find(|p| p.is_absolute() && p.extension().is_some())?.file_stem().map(|s| s.to_string_lossy().into_owned())
}

/// Look for pictures of one game. `term` searches for another name than the
/// game's own. Gives the game's title with what was found.
pub(crate) fn find(library: &Library, game_id: &str, term: &str) -> Result<(String, Found), String> {
    let game = library.store().list()?.into_iter().find(|g| g.id == game_id).ok_or("That game is not in the library any more.")?;
    let emulated = game.kind == Kind::Emulated;
    let wanted = Wanted {
        term: if term.trim().is_empty() { game.title.clone() } else { term.trim().to_string() },
        console: if emulated { game.platform.clone() } else { game.origin.clone() },
        // Only when looking for the game under its own name.
        file: if emulated && term.trim().is_empty() { game_file(library, game_id) } else { None },
        key: key(library),
    };
    Ok((game.title, art::find(&wanted)?))
}

fn chosen_dir(library: &Library) -> PathBuf {
    library.0.art_dir.join("chosen")
}

/// Keep picture bytes as a game's cover or wide picture. The file it
/// replaces is deleted when CarlaOS made it.
fn keep(library: &Library, game_id: &str, wide: bool, bytes: &[u8]) -> Result<(), String> {
    let ending = meta::picture_kind(bytes).ok_or("That is not a picture CarlaOS can show. Use a JPEG, PNG or WebP picture.")?;
    let dir = chosen_dir(library);
    let problem = |e: std::io::Error| format!("The picture could not be kept: {e}.");
    std::fs::create_dir_all(&dir).map_err(problem)?;
    // A new name each time, so the interface never shows a remembered copy
    // of the picture this one replaces.
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = dir.join(format!("{}-{}-{stamp:x}.{ending}", manual_key(game_id), if wide { "wide" } else { "cover" }));
    std::fs::write(&path, bytes).map_err(problem)?;
    match library.store().set_own_art(game_id, wide, Some(&path)) {
        Ok(old) => {
            forget(library, old);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(e)
        }
    }
}

/// Delete a picture nothing shows any more, if it is one CarlaOS made.
fn forget(library: &Library, old: Option<PathBuf>) {
    if let Some(old) = old.filter(|old| old.starts_with(chosen_dir(library))) {
        let _ = std::fs::remove_file(old);
    }
}

/// Give a game one of the pictures that were found.
pub(crate) fn apply(library: &Library, game_id: &str, candidate: &Candidate) -> Result<Shape, String> {
    if !candidate.trusted() {
        return Err("That picture is not from a place CarlaOS fetches pictures from.".to_string());
    }
    let bytes = meta::picture(&candidate.full)?;
    keep(library, game_id, candidate.shape == Shape::Wide, &bytes)?;
    Ok(candidate.shape)
}

/// Go back to the pictures the game had by itself.
pub(crate) fn clear(library: &Library, game_id: &str) -> Result<(), String> {
    for wide in [false, true] {
        let old = library.store().set_own_art(game_id, wide, None)?;
        forget(library, old);
    }
    Ok(())
}

/// One picture to choose, with a small copy on this machine to look at.
#[derive(Debug, Clone, Serialize)]
pub struct Shown {
    candidate: Candidate,
    preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Search {
    candidates: Vec<Shown>,
    notes: Vec<String>,
    /// Whether a SteamGridDB key is kept, so the page can offer to add one.
    key_set: bool,
}

/// A short steady name for an address.
fn name_of(url: &str) -> String {
    manual_key(url)
}

/// Fetch the small versions, a few at a time, into a folder that is emptied
/// at each search. A picture that does not load is not offered: this is how
/// guesses that do not exist drop out.
fn previews(library: &Library, candidates: Vec<Candidate>) -> Vec<Shown> {
    let dir = library.0.art_dir.join("looking");
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return Vec::new();
    }
    let fetch = |candidate: &Candidate| -> Option<Shown> {
        let bytes = meta::picture(&candidate.thumb).ok()?;
        let path = dir.join(format!("{}.{}", name_of(&candidate.thumb), meta::picture_kind(&bytes)?));
        std::fs::write(&path, bytes).ok()?;
        Some(Shown { candidate: candidate.clone(), preview: path.to_string_lossy().into_owned() })
    };
    let mut shown = Vec::new();
    for batch in candidates.chunks(8) {
        let got: Vec<Option<Shown>> = std::thread::scope(|scope| {
            let running: Vec<_> = batch.iter().map(|c| scope.spawn(|| fetch(c))).collect();
            running.into_iter().map(|r| r.join().ok().flatten()).collect()
        });
        shown.extend(got.into_iter().flatten());
        if shown.len() >= art::MOST {
            break;
        }
    }
    shown.truncate(art::MOST);
    shown
}

#[tauri::command(async)]
pub fn art_find(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, game_id: String, term: String) -> Result<Search, String> {
    admin.require()?;
    let (_, found) = find(&library, &game_id, &term)?;
    let candidates = previews(&library, found.candidates);
    allow_pictures(&app, &library.0.art_dir);
    Ok(Search { candidates, notes: found.notes, key_set: key(&library).is_some() })
}

#[tauri::command(async)]
pub fn art_use(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, game_id: String, candidate: Candidate) -> Result<Shape, String> {
    admin.require()?;
    let shape = apply(&library, &game_id, &candidate)?;
    allow_pictures(&app, &library.0.art_dir);
    Ok(shape)
}

/// Use a picture that is on this machine. Says whether one was chosen.
#[tauri::command(async)]
pub fn art_pick(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, game_id: String, wide: bool) -> Result<bool, String> {
    admin.require()?;
    let dialog = app.dialog().file().set_title(if wide { "Choose a wide picture" } else { "Choose a cover" }).add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]);
    let Some(path) = dialog.blocking_pick_file().and_then(|f| f.into_path().ok()) else { return Ok(false) };
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX);
    if size > 16 * 1024 * 1024 {
        return Err("That picture is too large. Use one under 16 MB.".to_string());
    }
    let bytes = std::fs::read(&path).map_err(|_| "That picture could not be read.".to_string())?;
    keep(&library, &game_id, wide, &bytes)?;
    allow_pictures(&app, &library.0.art_dir);
    Ok(true)
}

#[tauri::command]
pub fn art_clear(admin: State<'_, Admin>, library: State<'_, Library>, game_id: String) -> Result<(), String> {
    admin.require()?;
    clear(&library, &game_id)
}

/// Whether a SteamGridDB key is kept. The key itself never leaves the Rust side.
#[tauri::command]
pub fn art_key(admin: State<'_, Admin>, library: State<'_, Library>) -> Result<bool, String> {
    admin.require()?;
    Ok(key(&library).is_some())
}

#[tauri::command]
pub fn art_set_key(admin: State<'_, Admin>, library: State<'_, Library>, key: String) -> Result<bool, String> {
    admin.require()?;
    set_key(&library, &key)
}

/// What CarlaOS makes of a program or shortcut the administrator chose.
#[tauri::command]
pub fn library_inspect(admin: State<'_, Admin>, path: String) -> Result<Inspected, String> {
    admin.require()?;
    ports::inspect(Path::new(&path))
}
