//! Knowing what is being played, also when CarlaOS did not start it.
//!
//! Two things live here. A watcher that notices a game of the library that
//! is running without CarlaOS having started it, and takes it on (see
//! `carla_core::presence`). And the way back to a running game from the
//! library: its window is remembered while it is in front, so "Back to the
//! game" can put it in front again.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use carla_core::library::launch::Processes;
use carla_core::library::LOCAL;
use carla_core::presence::{spot, Spotter};
use tauri::{AppHandle, Manager, State};

use crate::device;
use crate::library::{locked, Library};

/// How often the running programs are looked at while no game is known to
/// be running. It is one cheap look; nothing is looked at during a game.
const LOOK_EVERY: Duration = Duration::from_secs(5);

/// The last window that was in front during a game and was not CarlaOS's
/// own: the game.
static GAME_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Called often while a game runs: remember the game's window.
pub(crate) fn note_front() {
    let front = device::front();
    if front != 0 && !device::is_ours(front) {
        GAME_WINDOW.store(front, Ordering::SeqCst);
    }
}

/// The game is over: its window is gone with it.
pub(crate) fn ended() {
    GAME_WINDOW.store(0, Ordering::SeqCst);
}

/// Put the running game back in front. When its window is not known, CarlaOS
/// steps aside instead, which shows whatever is behind it: the game.
pub(crate) fn back_to_game(app: &AppHandle) {
    let game = GAME_WINDOW.load(Ordering::SeqCst);
    if game != 0 && device::is_window(game) {
        device::bring_forward(game);
    } else if let Some(window) = app.get_webview_window("main") {
        let _ = window.minimize();
    }
}

/// Put CarlaOS itself in front of a running game, which keeps running.
pub(crate) fn to_carlaos(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        #[cfg(windows)]
        if let Ok(handle) = window.hwnd() {
            device::bring_forward(handle.0 as isize);
        }
        let _ = window.set_focus();
    }
}

/// Look for a game that is running without CarlaOS having started it, for
/// as long as CarlaOS runs.
pub fn watch(app: AppHandle, library: Library) {
    std::thread::spawn(move || {
        let mut processes = Processes::default();
        let mut spotter = Spotter::default();
        loop {
            std::thread::sleep(LOOK_EVERY);
            // One game at a time, and the one that runs is already watched.
            if locked(&library.0.active).is_some() {
                spotter = Spotter::default();
                continue;
            }
            let candidates = library.candidates();
            if candidates.is_empty() {
                continue;
            }
            let found = spot(&candidates, &processes.programs()).map(|c| c.game_id.clone());
            let Some(game_id) = spotter.observe(found.as_deref()) else { continue };
            let install = library.store().install(&game_id, LOCAL);
            if let Ok(install) = install {
                // Losing the race against a game started from CarlaOS at
                // this very moment is fine: that one is watched already.
                let _ = crate::library::adopt(&app, &library, install);
            }
        }
    });
}

/// Back to the running game, from the library.
#[tauri::command(async)]
pub fn session_resume(app: AppHandle, library: State<'_, Library>) -> Result<(), String> {
    if locked(&library.0.active).is_none() {
        return Err("No game is running any more.".to_string());
    }
    crate::quick::hide(&app);
    back_to_game(&app);
    Ok(())
}
