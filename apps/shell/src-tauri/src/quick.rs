//! The quick menu: a small menu on top of a running game, for the things a
//! player wants without leaving it. Sound, brightness, battery, and ending
//! the game.
//!
//! It opens when View and Menu (the two small buttons either side of the
//! middle) are held together for half a second, and closes the same way or
//! with B. While a game is in front CarlaOS does not get the controller, so a
//! watcher reads it directly, only while a game runs, and steers the menu
//! itself: the menu works even when Windows would not give it the keyboard.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::{Duration, Instant};

use carla_core::library::LOCAL;
use carla_core::quick::{Chord, Presses};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::device::{self, Battery, Sound};
use crate::library::{locked, Library};

const WINDOW: &str = "quick";
/// Sent to the menu for each step the controller takes through it.
const PRESS_EVENT: &str = "quick://press";
/// Sent to the menu each time it is shown, so it reads the levels afresh.
const OPEN_EVENT: &str = "quick://open";

static OPEN: AtomicBool = AtomicBool::new(false);
/// The window that was in front when the menu opened: the game.
static GAME_WINDOW: AtomicIsize = AtomicIsize::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct QuickState {
    /// The game the menu is over. Nothing when none runs any more.
    title: Option<String>,
    stream: bool,
    /// Unix seconds the game was started.
    since: Option<i64>,
    sound: Option<Sound>,
    brightness: Option<u8>,
    battery: Option<Battery>,
}

fn state(library: &Library) -> QuickState {
    let (title, stream, since) = match locked(&library.0.active).as_ref() {
        Some(active) => (Some(active.view.title.clone()), active.place != LOCAL, Some(active.since)),
        None => (None, false, None),
    };
    QuickState { title, stream, since, sound: device::sound(), brightness: device::brightness(), battery: device::battery() }
}

fn window_handle(window: &tauri::WebviewWindow) -> isize {
    #[cfg(windows)]
    {
        window.hwnd().map(|h| h.0 as isize).unwrap_or(0)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        0
    }
}

fn open(app: &AppHandle) {
    let window = match app.get_webview_window(WINDOW) {
        Some(window) => window,
        None => {
            // See-through and on top, so the game stays visible around it.
            let built = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::App("index.html#quick".into()))
                .title("CarlaOS quick menu")
                .transparent(true)
                .decorations(false)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .fullscreen(true)
                .visible(false)
                .build();
            match built {
                Ok(window) => window,
                Err(_) => return,
            }
        }
    };
    GAME_WINDOW.store(device::front(), Ordering::SeqCst);
    let _ = window.show();
    device::bring_forward(window_handle(&window));
    let _ = window.set_focus();
    OPEN.store(true, Ordering::SeqCst);
    let _ = window.emit(OPEN_EVENT, ());
}

fn close(app: &AppHandle) {
    if !OPEN.swap(false, Ordering::SeqCst) {
        return;
    }
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.hide();
    }
    // Back to the game, which most games need before they take input again.
    let game = GAME_WINDOW.swap(0, Ordering::SeqCst);
    if game != 0 {
        device::bring_forward(game);
    }
}

/// The game is over: the menu has nothing left to be over. Its window is
/// given up altogether, so CarlaOS is as light as before.
pub(crate) fn ended(app: &AppHandle) {
    OPEN.store(false, Ordering::SeqCst);
    GAME_WINDOW.store(0, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(WINDOW) {
        let _ = window.destroy();
    }
}

/// Watch the controller for as long as CarlaOS runs. Costs nothing while no
/// game is running.
pub fn watch(app: AppHandle, library: Library) {
    std::thread::spawn(move || {
        let mut chord = Chord::default();
        let mut presses = None::<Presses>;
        loop {
            let playing = locked(&library.0.active).as_ref().is_some_and(|a| a.view.phase == "running");
            let pad = if playing { device::pad() } else { None };
            let Some(pad) = pad else {
                chord = Chord::default();
                presses = None;
                std::thread::sleep(Duration::from_millis(if playing { 1000 } else { 500 }));
                continue;
            };
            let now = Instant::now();
            if chord.observe(pad.buttons, now) {
                if OPEN.load(Ordering::SeqCst) {
                    close(&app);
                } else {
                    open(&app);
                }
            }
            if OPEN.load(Ordering::SeqCst) {
                for press in presses.get_or_insert_with(Presses::opened).observe(pad, now) {
                    let _ = app.emit_to(WINDOW, PRESS_EVENT, press);
                }
                std::thread::sleep(Duration::from_millis(16));
            } else {
                presses = None;
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    });
}

#[tauri::command(async)]
pub fn quick_state(library: State<'_, Library>) -> QuickState {
    state(&library)
}

#[tauri::command(async)]
pub fn quick_set_sound(library: State<'_, Library>, level: u8, muted: bool) -> Result<QuickState, String> {
    device::set_sound(Sound { level, muted })?;
    Ok(state(&library))
}

#[tauri::command(async)]
pub fn quick_set_brightness(library: State<'_, Library>, level: u8) -> Result<QuickState, String> {
    // Never quite dark: the menu must stay readable to turn it back up.
    device::set_brightness(level.clamp(5, 100))?;
    Ok(state(&library))
}

#[tauri::command(async)]
pub fn quick_close(app: AppHandle) {
    close(&app);
}

/// Take away the loading screen the desktop shows during a stream, for when
/// something behind it needs an answer.
#[tauri::command(async)]
pub fn quick_show_desktop(app: AppHandle, library: State<'_, Library>) -> Result<(), String> {
    let place = locked(&library.0.active).as_ref().map(|a| a.place.clone()).filter(|p| p != LOCAL).ok_or("No game is being streamed.")?;
    crate::hosts::reveal(&library, &place)?;
    close(&app);
    Ok(())
}

/// Close the game. For a stream the desktop is asked to close it too, so it
/// does not keep running there unseen.
#[tauri::command(async)]
pub fn quick_end_game(app: AppHandle, library: State<'_, Library>) {
    let place = locked(&library.0.active).as_ref().map(|a| a.place.clone());
    if let Some(place) = place.filter(|p| p != LOCAL) {
        crate::hosts::end_game(&library, &place);
    }
    if let Some(active) = locked(&library.0.active).as_mut() {
        active.stop = true;
        active.end = true;
    }
    close(&app);
}
