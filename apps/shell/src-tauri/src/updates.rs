//! Updating CarlaOS on this machine from its GitHub releases: the Ally
//! interface, and the desktop host too when this machine has both halves.
//! Looking is open to anyone; installing is for the administrator.

use carla_core::update::{self, Available, Part};
use carla_core::VERSION;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::admin::Admin;

#[derive(Debug, Clone, Serialize)]
pub struct Updates {
    /// The version of the interface that is running.
    current: &'static str,
    /// A newer interface, if there is one.
    available: Option<Available>,
    /// The desktop host installed on this same machine, if any.
    host_here: Option<HostHere>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostHere {
    current: String,
    available: Option<Available>,
}

fn look() -> Result<Updates, String> {
    let available = update::check(Part::Ally)?;
    let host_here = match update::desktop_host_here() {
        Some((current, _)) => Some(HostHere { available: update::check_from(Part::DesktopHost, &current)?, current }),
        None => None,
    };
    Ok(Updates { current: VERSION, available, host_here })
}

/// Ask GitHub whether there is anything newer for this machine.
#[tauri::command(async)]
pub fn update_check() -> Result<Updates, String> {
    look()
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    received: u64,
    total: u64,
}

/// Download and install everything newer for this machine. The interface
/// closes for a moment and comes back as the new version. Says "nothing to do"
/// by returning false.
#[tauri::command(async)]
pub fn update_install(app: AppHandle, admin: State<'_, Admin>) -> Result<bool, String> {
    admin.require()?;
    // Looked up again here, so the page cannot ask for anything but our releases.
    let found = look()?;
    let host = found.host_here.and_then(|h| h.available);
    let total = found.available.as_ref().map_or(0, |u| u.size) + host.as_ref().map_or(0, |u| u.size);
    let report = |before: u64| {
        let app = app.clone();
        move |received: u64| {
            let _ = app.emit("update-progress", Progress { received: before + received, total });
        }
    };
    let dir = update::download_dir();
    let mut changed = false;
    // The desktop host first: its installer works unseen and restarts the
    // companion itself, while the interface's ends this program.
    if let Some(host) = &host {
        let installer = update::download(host, &dir, &report(0))?;
        update::install(Part::DesktopHost, &installer)?;
        changed = true;
    }
    if let Some(ally) = &found.available {
        let installer = update::download(ally, &dir, &report(host.as_ref().map_or(0, |u| u.size)))?;
        update::install(Part::Ally, &installer)?;
        app.exit(0);
        changed = true;
    }
    Ok(changed)
}
