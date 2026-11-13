//! Starting straight into CarlaOS when the Ally is turned on, with safe ways
//! back to the Windows desktop.
//!
//! CarlaOS starts with Windows for the signed-in account. It never replaces
//! the Windows shell, so the desktop is always still there underneath: Exit
//! to Windows shows it at once.
//!
//! Two things guard against a CarlaOS that will not start any more and would
//! otherwise be in the way at every start:
//!
//! - Holding **View** (the button with two squares) while it starts skips
//!   CarlaOS that one time.
//! - After two starts in a row that never got as far as showing the library,
//!   the next start stands down by itself and leaves the desktop.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The argument Windows starts CarlaOS with, as opposed to a person.
pub const BOOT_ARG: &str = "--boot";

/// Starts in a row that may fail before CarlaOS stays out of the way.
pub const FAILED_STARTS: u32 = 2;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Record {
    /// Starts by Windows that have not shown the library yet.
    #[serde(default)]
    unfinished: u32,
    /// CarlaOS stood down and has not said so yet.
    #[serde(default)]
    stood_down: bool,
    /// CarlaOS turned off Windows' own wait before start-up programs, and
    /// should turn it back on when it no longer starts with Windows.
    #[serde(default)]
    removed_delay: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Go on and show CarlaOS.
    Proceed,
    /// Do not start: it failed too often. The desktop is left as it is.
    StandDown,
}

/// Remembers how the last starts went.
pub struct Guard {
    path: PathBuf,
}

impl Guard {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn read(&self) -> Record {
        // A record that cannot be read counts as a clean one: the guard must
        // never be the reason CarlaOS does not start.
        fs::read_to_string(&self.path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn write(&self, record: &Record) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(&self.path, serde_json::to_string(record).expect("serialisable"));
    }

    /// Windows is starting CarlaOS. Decide whether to go on.
    pub fn begin(&self) -> Start {
        let mut record = self.read();
        if record.unfinished >= FAILED_STARTS {
            // Stand down once, then try again next time: whatever was wrong
            // may have been put right in between.
            record.unfinished = 0;
            record.stood_down = true;
            self.write(&record);
            return Start::StandDown;
        }
        record.unfinished += 1;
        self.write(&record);
        Start::Proceed
    }

    /// The library is on the screen: this start worked.
    pub fn ready(&self) {
        let mut record = self.read();
        if record.unfinished != 0 {
            record.unfinished = 0;
            self.write(&record);
        }
    }

    /// The player chose the desktop this time. Not a failure.
    pub fn skipped(&self) {
        self.ready();
    }

    /// Whether CarlaOS stood down since this was last asked. Asked once.
    pub fn take_stood_down(&self) -> bool {
        let mut record = self.read();
        let stood_down = std::mem::take(&mut record.stood_down);
        if stood_down {
            self.write(&record);
        }
        stood_down
    }

    fn removed_delay(&self) -> bool {
        self.read().removed_delay
    }

    fn set_removed_delay(&self, removed: bool) {
        let mut record = self.read();
        record.removed_delay = removed;
        self.write(&record);
    }
}

#[cfg(windows)]
mod os {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "CarlaOS";
    /// Windows waits some seconds before it starts start-up programs unless
    /// this value is zero.
    const SERIALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize";
    const DELAY: &str = "StartupDelayInMSec";

    fn problem(e: std::io::Error) -> String {
        format!("Windows did not accept the change: {e}.")
    }

    fn command() -> Result<String, String> {
        let exe = std::env::current_exe().map_err(problem)?;
        Ok(format!("\"{}\" {}", exe.display(), super::BOOT_ARG))
    }

    /// Whether Windows starts this very program at sign-in.
    pub fn enabled() -> bool {
        let Ok(run) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN) else { return false };
        let set: Option<String> = run.get_value(NAME).ok();
        set.is_some() && set == command().ok()
    }

    pub fn set(on: bool) -> Result<(), String> {
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN).map_err(problem)?;
        if on {
            run.set_value(NAME, &command()?).map_err(problem)
        } else {
            match run.delete_value(NAME) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(problem(e)),
                _ => Ok(()),
            }
        }
    }

    /// Turn off Windows' wait before start-up programs. Says whether this
    /// changed anything, so it is only undone if CarlaOS did it.
    pub fn remove_delay() -> bool {
        let Ok((key, _)) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(SERIALIZE) else { return false };
        if key.get_value::<u32, _>(DELAY).is_ok() {
            return false; // someone chose a value already; leave it
        }
        key.set_value(DELAY, &0u32).is_ok()
    }

    pub fn restore_delay() {
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(SERIALIZE, winreg::enums::KEY_SET_VALUE) {
            let _ = key.delete_value(DELAY);
        }
    }
}

#[cfg(not(windows))]
mod os {
    pub fn enabled() -> bool {
        false
    }
    pub fn set(_on: bool) -> Result<(), String> {
        Err("Starting with the machine is set up on the Ally itself.".to_string())
    }
    pub fn remove_delay() -> bool {
        false
    }
    pub fn restore_delay() {}
}

/// Whether CarlaOS starts when the Ally is turned on.
pub fn enabled() -> bool {
    os::enabled()
}

/// Turn starting with Windows on or off. Everything it changes is put back
/// when it is turned off.
pub fn set(guard: &Guard, on: bool) -> Result<(), String> {
    os::set(on)?;
    if on {
        if os::remove_delay() {
            guard.set_removed_delay(true);
        }
        // A fresh start for the guard too.
        guard.ready();
    } else if guard.removed_delay() {
        os::restore_delay();
        guard.set_removed_delay(false);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard(name: &str) -> Guard {
        let path = std::env::temp_dir().join(format!("carla-boot-{name}-{}", std::process::id())).join("boot.json");
        let _ = fs::remove_file(&path);
        Guard::new(path)
    }

    #[test]
    fn starts_that_work_never_stand_down() {
        let g = guard("fine");
        for _ in 0..10 {
            assert_eq!(g.begin(), Start::Proceed);
            g.ready();
        }
        assert!(!g.take_stood_down());
    }

    #[test]
    fn after_two_failed_starts_the_next_one_leaves_the_desktop_alone_once() {
        let g = guard("failing");
        assert_eq!(g.begin(), Start::Proceed); // never got to the library
        assert_eq!(g.begin(), Start::Proceed); // nor this time
        assert_eq!(g.begin(), Start::StandDown);
        // Then it tries again, and says once what happened.
        assert_eq!(g.begin(), Start::Proceed);
        g.ready();
        assert!(g.take_stood_down());
        assert!(!g.take_stood_down());
        // One bad start between good ones is not held against it.
        assert_eq!(g.begin(), Start::Proceed);
        assert_eq!(g.begin(), Start::Proceed);
        g.ready();
        assert_eq!(g.begin(), Start::Proceed);
        assert_eq!(g.begin(), Start::Proceed);
        assert_eq!(g.begin(), Start::StandDown);
    }

    #[test]
    fn choosing_the_desktop_at_the_start_is_not_a_failure() {
        let g = guard("skipped");
        for _ in 0..5 {
            assert_eq!(g.begin(), Start::Proceed);
            g.skipped();
        }
        // A record that is damaged never keeps CarlaOS from starting.
        fs::write(&g.path, "not json").unwrap();
        assert_eq!(g.begin(), Start::Proceed);
    }
}
