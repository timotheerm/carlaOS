//! Driving stock Moonlight on the Ally through its command line.
//!
//! Moonlight is not modified or bundled; the administrator installs it once.
//! The player never sees its own menus: CarlaOS starts it straight into a
//! stream and comes back to the front when it exits.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::{random, SUNSHINE_APP};

/// Picture settings for a stream. The Ally's screen is 1080p.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for Picture {
    fn default() -> Self {
        Self { width: 1920, height: 1080, fps: 60 }
    }
}

/// Where Moonlight is installed. `CARLA_MOONLIGHT` overrides the search.
pub fn find() -> Option<PathBuf> {
    let given = std::env::var_os("CARLA_MOONLIGHT").map(PathBuf::from);
    let folders = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)", "LOCALAPPDATA"];
    let usual = folders.iter().filter_map(std::env::var_os).map(|dir| PathBuf::from(dir).join("Moonlight Game Streaming").join("Moonlight.exe"));
    given.into_iter().chain(usual).find(|p| p.is_file())
}

/// A four-digit PIN for one pairing.
pub fn new_pin() -> String {
    let n = u32::from_le_bytes(random(4).try_into().expect("four bytes"));
    format!("{:04}", n % 10_000)
}

pub fn stream_args(address: &str, picture: Picture) -> Vec<String> {
    vec![
        "stream".to_string(),
        address.to_string(),
        SUNSHINE_APP.to_string(),
        "--resolution".to_string(),
        format!("{}x{}", picture.width, picture.height),
        "--fps".to_string(),
        picture.fps.to_string(),
        "--display-mode".to_string(),
        "fullscreen".to_string(),
        // Leaving the stream must not close the game on the desktop: the
        // player may have unsaved progress and can pick it up again.
        "--no-quit-after".to_string(),
    ]
}

fn command(exe: &Path, args: &[String]) -> Command {
    let mut command = Command::new(exe);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command
}

/// Start streaming the CarlaOS application from a desktop.
pub fn stream(exe: &Path, address: &str, picture: Picture) -> std::io::Result<Child> {
    command(exe, &stream_args(address, picture)).spawn()
}

/// Start pairing with a desktop using a PIN we chose.
pub fn pair(exe: &Path, address: &str, pin: &str) -> std::io::Result<Child> {
    command(exe, &["pair".to_string(), address.to_string(), "--pin".to_string(), pin.to_string()]).spawn()
}

/// Wait for a program to exit; stop it when it takes longer than `limit`.
/// Returns whether it exited by itself with success.
pub fn finished(child: &mut Child, limit: Duration) -> bool {
    let end = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Is this Ally already paired with the desktop? Moonlight can only list a
/// desktop's applications when it is.
pub fn paired(exe: &Path, address: &str) -> bool {
    match command(exe, &["list".to_string(), address.to_string()]).spawn() {
        Ok(mut child) => finished(&mut child, Duration::from_secs(25)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stream_goes_full_screen_through_the_one_application() {
        let args = stream_args("192.168.1.20", Picture::default());
        assert_eq!(args[..3], ["stream", "192.168.1.20", "CarlaOS"]);
        assert!(args.windows(2).any(|w| w == ["--resolution", "1920x1080"]));
        assert!(args.windows(2).any(|w| w == ["--fps", "60"]));
        assert!(args.windows(2).any(|w| w == ["--display-mode", "fullscreen"]));
        assert!(args.contains(&"--no-quit-after".to_string()));
    }

    #[test]
    fn pins_are_four_digits() {
        for _ in 0..50 {
            let pin = new_pin();
            assert!(pin.len() == 4 && pin.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_program_that_hangs_is_stopped() {
        let mut quick = Command::new("true").spawn().unwrap();
        assert!(finished(&mut quick, Duration::from_secs(5)));
        let mut failing = Command::new("false").spawn().unwrap();
        assert!(!finished(&mut failing, Duration::from_secs(5)));
        let mut hanging = Command::new("sleep").arg("30").spawn().unwrap();
        let started = Instant::now();
        assert!(!finished(&mut hanging, Duration::from_millis(300)));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
