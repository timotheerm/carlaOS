//! Pictures for every game, found without anyone asking. See
//! `carla_core::library::autoart`. Runs in the background after each look
//! through the library, never while a game is being played.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use carla_core::library::autoart::{self, Subject, TRY_AGAIN_AFTER};
use carla_core::library::{manual_key, meta, Kind};
use tauri::{AppHandle, Emitter, State};

use crate::admin::Admin;
use crate::library::{allow_pictures, locked, now, Library};

const CHANGED_EVENT: &str = "library://changed";
/// The setting that turns this off: kept as "0".
const SETTING: &str = "auto_art";
/// So many games are looked up after one look through the library.
const MOST: usize = 400;

pub(crate) fn enabled(library: &Library) -> bool {
    library.store().setting(SETTING).ok().flatten().as_deref() != Some(b"0")
}

/// Keep picture bytes for a game and show them where it has none.
fn keep(library: &Library, game_id: &str, wide: bool, bytes: &[u8]) -> bool {
    let Some(ending) = meta::picture_kind(bytes) else { return false };
    let dir = library.0.art_dir.join("auto");
    let path = dir.join(format!("{}-{}.{ending}", manual_key(game_id), if wide { "wide" } else { "cover" }));
    std::fs::create_dir_all(&dir).is_ok() && std::fs::write(&path, bytes).is_ok() && library.store().set_art_if_none(game_id, wide, &path).is_ok()
}

/// Look up the games that have no cover or no wide picture, one at a time.
pub(crate) fn run(app: &AppHandle, library: &Library) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    if !enabled(library) || BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, library) = (app.clone(), library.clone());
    std::thread::spawn(move || {
        let wanting = library.store().artless(now() - TRY_AGAIN_AFTER).unwrap_or_default();
        let key = crate::art::key(&library);
        let (mut unreachable, mut new) = (0, 0);
        for game in wanting.into_iter().take(MOST) {
            // The connection and the disk belong to the game being played.
            while locked(&library.0.active).is_some() {
                std::thread::sleep(Duration::from_secs(20));
            }
            if !enabled(&library) {
                break;
            }
            let emulated = game.kind == Kind::Emulated;
            let subject = Subject {
                file: if emulated { crate::art::game_file(&library, &game.id) } else { None },
                console: if emulated { game.platform.clone() } else { game.origin.clone() },
                id: game.id.clone(),
                title: game.title.clone(),
                need_cover: game.need_cover,
                need_hero: game.need_hero,
            };
            match autoart::fill(&subject, key.as_deref()) {
                Ok(filled) => {
                    unreachable = 0;
                    let mut got = false;
                    if let Some(bytes) = &filled.cover {
                        got |= keep(&library, &game.id, false, bytes);
                    }
                    if let Some(bytes) = &filled.hero {
                        got |= keep(&library, &game.id, true, bytes);
                    }
                    // Found or not, it is not asked about again for a week.
                    let _ = library.store().art_tried(&game.id, now());
                    if got {
                        new += 1;
                        allow_pictures(&app, &library.0.art_dir);
                        if new % 4 == 0 {
                            let _ = app.emit(CHANGED_EVENT, ());
                        }
                    }
                }
                // Nothing answered: several in a row means no connection.
                Err(_) => {
                    unreachable += 1;
                    if unreachable >= 3 {
                        break;
                    }
                }
            }
        }
        if new % 4 != 0 {
            let _ = app.emit(CHANGED_EVENT, ());
        }
        BUSY.store(false, Ordering::SeqCst);
    });
}

/// Whether pictures are looked for by themselves.
#[tauri::command]
pub fn art_auto(library: State<'_, Library>) -> bool {
    enabled(&library)
}

#[tauri::command]
pub fn art_set_auto(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, on: bool) -> Result<bool, String> {
    admin.require()?;
    library.store().set_setting(SETTING, if on { None } else { Some(b"0") })?;
    if on {
        run(&app, &library);
    }
    Ok(on)
}

/// Look every game without a picture up again now, whatever was found before.
#[tauri::command]
pub fn art_auto_again(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>) -> Result<(), String> {
    admin.require()?;
    library.store().forget_art_tried()?;
    run(&app, &library);
    Ok(())
}
