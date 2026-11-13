//! Commands behind the administrator lock: the lock itself and the Windows
//! clean-up. Every clean-up command checks the lock here, on the Rust side, so
//! the UI cannot skip it.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use carla_core::admin::{AdminLock, Status, Unlock};
use carla_core::debloat::{Debloat, Outcome, PowerShell, Report};
use tauri::State;

pub struct Admin {
    lock: Mutex<AdminLock>,
    /// Holds the helper script, its request files and the change journal.
    dir: PathBuf,
    /// One clean-up operation at a time.
    busy: Mutex<()>,
}

impl Admin {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            lock: Mutex::new(AdminLock::new(data_dir.join("admin.json"))),
            dir: data_dir.join("cleanup"),
            busy: Mutex::new(()),
        }
    }

    /// For commands elsewhere that only the administrator may run.
    pub fn require(&self) -> Result<(), String> {
        self.with_lock(|lock| lock.require())
    }

    fn with_lock<T>(&self, f: impl FnOnce(&mut AdminLock) -> T) -> T {
        f(&mut self.lock.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Run a clean-up operation, if the administrator has unlocked.
    fn cleanup<T>(&self, f: impl FnOnce(&Debloat<PowerShell>) -> Result<T, carla_core::debloat::Error>) -> Result<T, String> {
        self.with_lock(|lock| lock.require())?;
        let _one_at_a_time = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        let debloat = Debloat::new(PowerShell::new(self.dir.clone()), self.dir.join("journal.json"));
        f(&debloat).map_err(|e| e.to_string())
    }

    /// A clean-up operation asked for by a desktop that the administrator
    /// allowed to administer this Ally. The lock on the screen is not asked:
    /// allowing the desktop, which needed the PIN, was the permission.
    pub(crate) fn cleanup_remote<T>(&self, f: impl FnOnce(&Debloat<PowerShell>) -> Result<T, carla_core::debloat::Error>) -> Result<T, String> {
        let _one_at_a_time = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        let debloat = Debloat::new(PowerShell::new(self.dir.clone()), self.dir.join("journal.json"));
        f(&debloat).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn admin_status(admin: State<'_, Admin>) -> Status {
    admin.with_lock(|lock| lock.status())
}

#[tauri::command]
pub fn admin_set_pin(admin: State<'_, Admin>, pin: String) -> Result<(), String> {
    admin.with_lock(|lock| lock.set_pin(&pin))
}

#[tauri::command]
pub fn admin_unlock(admin: State<'_, Admin>, pin: String) -> Unlock {
    admin.with_lock(|lock| lock.unlock(&pin, Instant::now()))
}

#[tauri::command]
pub fn admin_lock(admin: State<'_, Admin>) {
    admin.with_lock(|lock| lock.lock());
}

// The clean-up commands wait on PowerShell, and applying can wait on the
// Windows approval prompt, so they run off the main thread.

#[tauri::command(async)]
pub fn cleanup_scan(admin: State<'_, Admin>) -> Result<Report, String> {
    admin.cleanup(|d| d.scan())
}

#[tauri::command(async)]
pub fn cleanup_apply(admin: State<'_, Admin>, ids: Vec<String>) -> Result<Outcome, String> {
    admin.cleanup(|d| d.apply(&ids))
}

#[tauri::command(async)]
pub fn cleanup_undo(admin: State<'_, Admin>, ids: Vec<String>) -> Result<Outcome, String> {
    admin.cleanup(|d| d.undo(&ids))
}
