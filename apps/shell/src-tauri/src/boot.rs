//! Starting with the Ally, and the ways back to the desktop. See
//! `carla_core::boot`.

use std::path::PathBuf;
use std::time::Duration;

use carla_core::boot::{self, Guard, Start, BOOT_ARG};
use carla_core::quick::{MENU, VIEW};
use serde::Serialize;
use tauri::State;

use crate::admin::Admin;

pub struct Boot(pub Guard);

/// Where the record of the last starts is kept. Needed before the rest of
/// CarlaOS exists, so it is worked out by hand: the same folder the
/// interface's other files are in.
pub fn record_path(identifier: &str) -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join(identifier).join("boot.json")
}

/// Whether View alone is being held. Looked at for a moment, because a
/// controller can take that long to appear after Windows has started.
fn view_held() -> bool {
    (0..8).any(|_| {
        let held = crate::device::pad().is_some_and(|pad| pad.buttons & VIEW != 0 && pad.buttons & MENU == 0);
        if !held {
            std::thread::sleep(Duration::from_millis(100));
        }
        held
    })
}

/// Called first thing. When Windows started CarlaOS, decide whether to show
/// it at all; `false` means leave the desktop alone this time.
pub fn should_start(guard: &Guard) -> bool {
    if !std::env::args().any(|a| a == BOOT_ARG) {
        return true;
    }
    if view_held() {
        guard.skipped();
        return false;
    }
    guard.begin() == Start::Proceed
}

#[derive(Debug, Clone, Serialize)]
pub struct BootStatus {
    /// CarlaOS starts when the Ally is turned on.
    enabled: bool,
    /// It stayed out of the way once since this was last asked, because it
    /// had failed to start twice.
    stood_down: bool,
}

/// The library is on the screen. Also says how starting is set up.
#[tauri::command]
pub fn boot_ready(boot: State<'_, Boot>) -> BootStatus {
    boot.0.ready();
    BootStatus { enabled: boot::enabled(), stood_down: boot.0.take_stood_down() }
}

#[tauri::command]
pub fn boot_set(admin: State<'_, Admin>, boot: State<'_, Boot>, enabled: bool) -> Result<bool, String> {
    admin.require()?;
    boot::set(&boot.0, enabled)?;
    Ok(boot::enabled())
}
