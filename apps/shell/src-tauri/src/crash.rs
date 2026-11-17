//! Crash reports on the Ally: wiring for `carla_core::crash`.
//!
//! The reports live in `crash` next to the interface's other files and never
//! leave the device by themselves. Settings, Crash reports lets the player or
//! administrator copy one to send for diagnosis.

use std::path::PathBuf;
use std::sync::OnceLock;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use carla_core::crash::{Beat, Crash, Heartbeat, ReportInfo};
use carla_core::{AppInfo, Role};
use tauri::{AppHandle, Manager, State};

use crate::library::Library;

static CRASH: OnceLock<Crash> = OnceLock::new();
static LIBRARY: OnceLock<Library> = OnceLock::new();

pub struct Reports(pub Crash);

static HEART: Heartbeat = Heartbeat::new();
/// How often the watchdog looks, and how long the interface may stay silent.
const WATCH_EVERY: Duration = Duration::from_secs(5);
const SILENT_FOR: i64 = 30;
/// What the page said about itself with its last beat: a hidden page (covered,
/// on another desktop, screen off) has its timers slowed by the browser engine,
/// so it is not judged. A frozen page cannot say anything, so it still is.
static PAGE_HIDDEN: AtomicBool = AtomicBool::new(false);

/// The interface says it is alive (every few seconds).
#[tauri::command]
pub fn crash_alive(hidden: bool) {
    PAGE_HIDDEN.store(hidden, Ordering::SeqCst);
    if HEART.beat(crate::library::now()) {
        note("interface responding again");
    }
}

/// Report an interface that stops answering while the program runs on.
/// Only a window the player can see is judged (not minimised, and the page
/// itself says it is not hidden), also while a game or stream is marked as running.
/// A sleep in between, or the window coming back, starts the count again.
pub fn watch_ui(app: AppHandle, _library: Library) {
    std::thread::spawn(move || {
        let mut last_tick = crate::library::now();
        let mut judging = false;
        loop {
            std::thread::sleep(WATCH_EVERY);
            let now = crate::library::now();
            let slept = now - last_tick > 3 * WATCH_EVERY.as_secs() as i64;
            last_tick = now;
            let visible = app.get_webview_window("main").is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(true));
            // A session still marked active does not excuse a window that stopped
            // answering: that is how a session that never ended looks. A covered
            // window reports itself hidden and is not judged.
            let judge = visible && !PAGE_HIDDEN.load(Ordering::SeqCst);
            if slept || (judge && !judging) {
                HEART.reset(now);
            }
            judging = judge;
            if !judge {
                continue;
            }
            if let Beat::Froze(secs) = HEART.check(now, SILENT_FOR) {
                note(&format!("interface not responding for {secs} seconds"));
                if let Some(crash) = CRASH.get() {
                    crash.write_hang(secs, &state());
                }
            }
        }
    });
}

/// Open the report folder. Worked out by hand, like the boot record, because
/// this runs before the interface exists.
pub fn open(identifier: &str) -> Crash {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let crash = Crash::new(base.join(identifier).join("crash"), AppInfo::current(Role::Shell).version);
    let _ = CRASH.set(crash.clone());
    crash
}

/// Write down what the program is doing. Does nothing before `open`.
pub fn note(text: &str) {
    if let Some(crash) = CRASH.get() {
        crash.note(text);
    }
}

/// The newest notes of the trail, oldest first. Empty before `open`.
pub fn trail(count: usize) -> Vec<String> {
    CRASH.get().map(|crash| crash.trail(count)).unwrap_or_default()
}

/// The program ends normally: call before anything that ends it without
/// `RunEvent::Exit`, such as asking Windows to restart or switch off.
pub fn ended() {
    if let Some(crash) = CRASH.get() {
        crash.end();
    }
}

/// An ending announced with `ended` did not happen: the program runs on.
pub fn resumed() {
    if let Some(crash) = CRASH.get() {
        crash.begin();
    }
}

/// Let a panic report say what was running. Called once the library exists.
pub fn watch_state(library: &Library) {
    let _ = LIBRARY.set(library.clone());
}

/// A line for a panic report. Runs inside the panic hook, so it never waits
/// for a lock the panicking thread may be holding.
pub fn state() -> String {
    let Some(library) = LIBRARY.get() else { return "the library was not open yet".to_string() };
    match library.0.active.try_lock() {
        Ok(active) => match active.as_ref() {
            Some(a) => format!("session: \"{}\" at {}, phase {}", a.view.title, a.place, a.view.phase),
            None => "no game or stream was running".to_string(),
        },
        Err(_) => "the session state was being changed".to_string(),
    }
}

#[tauri::command]
pub fn crash_reports(reports: State<'_, Reports>) -> Vec<ReportInfo> {
    reports.0.reports()
}

#[tauri::command]
pub fn crash_report_text(reports: State<'_, Reports>, name: String) -> Result<String, String> {
    reports.0.read(&name)
}

/// The reports on screen need not be announced again.
#[tauri::command]
pub fn crash_seen(reports: State<'_, Reports>) {
    reports.0.mark_seen();
}

/// A note from the interface (an error it caught). Clipped by the writer.
#[tauri::command]
pub fn crash_note(text: String) {
    note(&format!("interface: {text}"));
}

#[tauri::command]
pub fn crash_open_folder(reports: State<'_, Reports>) -> Result<(), String> {
    let dir = reports.0.dir();
    std::fs::create_dir_all(dir).map_err(|_| "The folder could not be made.".to_string())?;
    std::process::Command::new("explorer.exe").arg(dir).spawn().map(|_| ()).map_err(|_| "The folder could not be opened.".to_string())
}

