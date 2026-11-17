//! The Ally itself, for the side menu: sound, brightness, battery, Wi-Fi,
//! and sleeping, restarting or turning off. And noticing that the Ally has
//! been asleep, so everything is put right on the way back.

use std::process::Command;
use std::time::Duration;

use carla_core::nap::Napper;
use carla_core::network::{self, Wifi};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::device::{self, Battery, Sound};
use crate::library::{locked, now_ms, Library};

/// Sent to the interface when the Ally has woken, with the seconds it slept.
const WOKE_EVENT: &str = "system://woke";

#[derive(Debug, Clone, Serialize)]
pub struct SystemState {
    sound: Option<Sound>,
    brightness: Option<u8>,
    battery: Option<Battery>,
    wifi: Option<Wifi>,
}

fn state() -> SystemState {
    SystemState { sound: device::sound(), brightness: device::brightness(), battery: device::battery(), wifi: network::wifi() }
}

/// Notice every sleep for as long as CarlaOS runs. One look at the clock a
/// second.
pub fn watch(app: AppHandle, library: Library) {
    std::thread::spawn(move || {
        let mut napper = Napper::new(now_ms());
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let Some(nap) = napper.observe(now_ms()) else { continue };
            // A running game puts its own power level back (see the library's
            // watcher). Between games the menus go back to resting.
            if locked(&library.0.active).is_none() {
                crate::power::rest();
            }
            let _ = app.emit(WOKE_EVENT, nap.seconds());
        }
    });
}

#[tauri::command(async)]
pub fn system_state() -> SystemState {
    state()
}

#[tauri::command(async)]
pub fn system_set_sound(level: u8, muted: bool) -> Result<SystemState, String> {
    device::set_sound(Sound { level, muted })?;
    Ok(state())
}

#[tauri::command(async)]
pub fn system_set_brightness(level: u8) -> Result<SystemState, String> {
    // Never quite dark: the menu must stay readable to turn it back up.
    device::set_brightness(level.clamp(5, 100))?;
    Ok(state())
}

#[cfg(windows)]
fn hidden(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hidden(_: &mut Command) {}

/// Windows' own way of restarting or turning off, at once.
fn shutdown(flag: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Only the Ally can be turned off from here.".to_string());
    }
    let mut command = Command::new("shutdown.exe");
    command.args([flag, "/t", "0"]);
    hidden(&mut command);
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err("Windows would not do that just now.".to_string()),
        Err(e) => Err(format!("Windows would not do that: {e}.")),
    }
}

/// Sleep, restart or turn off the Ally. The player chose it from the side
/// menu and was asked once.
#[tauri::command(async)]
pub fn system_power(library: State<'_, Library>, action: String) -> Result<(), String> {
    match action.as_str() {
        "sleep" => device::sleep(),
        "restart" | "off" => {
            // A game that is open is closed by Windows with everything else;
            // its time played is counted up to now.
            if let Some(active) = locked(&library.0.active).as_mut() {
                active.stop = true;
            }
            shutdown(if action == "restart" { "/r" } else { "/s" })
        }
        _ => Err("CarlaOS does not know that.".to_string()),
    }
}
