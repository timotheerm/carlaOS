//! The administrator lock on the handheld.
//!
//! The lock keeps the player out of settings that can change Windows. It is a
//! guard rail, not a security boundary: the PIN file sits in the user's own
//! profile, and anything machine-wide still needs Windows' own administrator
//! approval. The PIN is stored salted and hashed so it is not readable at a
//! glance.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PIN_LENGTH: usize = 4;
const ROUNDS: u32 = 50_000;
const FREE_TRIES: u32 = 5;
const PAUSE: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize)]
struct Stored {
    salt: String,
    hash: String,
    rounds: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Status {
    pub pin_set: bool,
    pub unlocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Unlock {
    Unlocked,
    Wrong,
    /// Too many wrong tries; try again after this many seconds.
    Wait { seconds: u64 },
}

pub struct AdminLock {
    path: PathBuf,
    unlocked: bool,
    failures: u32,
    paused_until: Option<Instant>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn digest(salt: &str, pin: &str, rounds: u32) -> String {
    let mut out = Sha256::digest(format!("{salt}:{pin}").as_bytes());
    for _ in 1..rounds {
        out = Sha256::digest(out);
    }
    hex(&out)
}

fn valid(pin: &str) -> bool {
    pin.len() == PIN_LENGTH && pin.bytes().all(|b| b.is_ascii_digit())
}

fn new_salt() -> String {
    // The standard library seeds each RandomState from the operating system.
    let a = RandomState::new().build_hasher().finish();
    let b = RandomState::new().build_hasher().finish();
    format!("{a:016x}{b:016x}")
}

impl AdminLock {
    pub fn new(path: PathBuf) -> Self {
        Self { path, unlocked: false, failures: 0, paused_until: None }
    }

    fn stored(&self) -> Option<Stored> {
        serde_json::from_str(&fs::read_to_string(&self.path).ok()?).ok()
    }

    pub fn status(&self) -> Status {
        Status { pin_set: self.stored().is_some(), unlocked: self.unlocked }
    }

    /// Choose the PIN. Allowed the first time, and later only while unlocked.
    pub fn set_pin(&mut self, pin: &str) -> Result<(), String> {
        if !valid(pin) {
            return Err(format!("The PIN is {PIN_LENGTH} digits."));
        }
        if self.stored().is_some() && !self.unlocked {
            return Err("Unlock with the current PIN first.".to_string());
        }
        let salt = new_salt();
        let stored = Stored { hash: digest(&salt, pin, ROUNDS), salt, rounds: ROUNDS };
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string(&stored).map_err(|e| e.to_string())?;
        fs::write(&self.path, text).map_err(|e| e.to_string())?;
        self.unlocked = true;
        Ok(())
    }

    pub fn unlock(&mut self, pin: &str, now: Instant) -> Unlock {
        if let Some(until) = self.paused_until {
            if now < until {
                return Unlock::Wait { seconds: (until - now).as_secs().max(1) };
            }
            self.paused_until = None;
        }
        let right = self
            .stored()
            .is_some_and(|s| valid(pin) && digest(&s.salt, pin, s.rounds) == s.hash);
        if right {
            self.unlocked = true;
            self.failures = 0;
            return Unlock::Unlocked;
        }
        self.failures += 1;
        if self.failures >= FREE_TRIES {
            self.paused_until = Some(now + PAUSE);
            return Unlock::Wait { seconds: PAUSE.as_secs() };
        }
        Unlock::Wrong
    }

    pub fn lock(&mut self) {
        self.unlocked = false;
    }

    /// For commands that must only run for the administrator.
    pub fn require(&self) -> Result<(), String> {
        if self.unlocked {
            Ok(())
        } else {
            Err("This needs the administrator PIN.".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(name: &str) -> AdminLock {
        let dir = std::env::temp_dir().join(format!("carla-admin-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        AdminLock::new(dir.join("admin.json"))
    }

    #[test]
    fn first_pin_unlocks_and_later_needs_the_pin() {
        let mut a = lock("first");
        assert_eq!(a.status(), Status { pin_set: false, unlocked: false });
        assert!(a.require().is_err());
        assert!(a.set_pin("12a4").is_err());
        assert!(a.set_pin("123").is_err());
        a.set_pin("1234").unwrap();
        assert_eq!(a.status(), Status { pin_set: true, unlocked: true });
        assert!(a.require().is_ok());

        a.lock();
        assert!(a.require().is_err());
        assert!(a.set_pin("0000").is_err(), "a locked PIN cannot be replaced");
        let now = Instant::now();
        assert_eq!(a.unlock("0000", now), Unlock::Wrong);
        assert_eq!(a.unlock("1234", now), Unlock::Unlocked);
        a.set_pin("0000").unwrap();
        a.lock();
        assert_eq!(a.unlock("1234", now), Unlock::Wrong);
        assert_eq!(a.unlock("0000", now), Unlock::Unlocked);
    }

    #[test]
    fn the_pin_is_not_stored_in_the_clear() {
        let mut a = lock("clear");
        a.set_pin("4821").unwrap();
        let text = fs::read_to_string(&a.path).unwrap();
        assert!(!text.contains("4821"));
        // A second lock on the same file sees the PIN but starts locked.
        let mut b = AdminLock::new(a.path.clone());
        assert_eq!(b.status(), Status { pin_set: true, unlocked: false });
        assert_eq!(b.unlock("4821", Instant::now()), Unlock::Unlocked);
    }

    #[test]
    fn repeated_wrong_pins_force_a_pause() {
        let mut a = lock("pause");
        a.set_pin("1234").unwrap();
        a.lock();
        let now = Instant::now();
        for _ in 0..FREE_TRIES - 1 {
            assert_eq!(a.unlock("9999", now), Unlock::Wrong);
        }
        assert_eq!(a.unlock("9999", now), Unlock::Wait { seconds: 30 });
        // Even the right PIN waits.
        assert!(matches!(a.unlock("1234", now + Duration::from_secs(10)), Unlock::Wait { seconds: 20 }));
        assert_eq!(a.unlock("1234", now + PAUSE), Unlock::Unlocked);
    }
}
