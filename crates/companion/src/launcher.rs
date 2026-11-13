//! What Sunshine runs when a stream starts.
//!
//! Sunshine knows one application, "CarlaOS", whose command is this launcher.
//! The Ally says which game it wants just before it starts the stream; the
//! launcher starts that game and stays alive until the game has closed, which
//! is how Sunshine knows when to end the stream.

use std::time::Duration;

use carla_core::library::launch::{self, Phase, Watch};
use carla_core::library::{steam, Install};

use crate::files::{now, Files, Pending, Playing};

const POLL: Duration = Duration::from_secs(2);

/// Sunshine starts the launcher in a console of its own, which would sit on
/// the desktop in the picture. Nobody reads it: what happens goes to the log.
#[cfg(windows)]
fn hide_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn FreeConsole() -> i32;
    }
    unsafe { FreeConsole() };
}

#[cfg(not(windows))]
fn hide_console() {}

/// Returns the process exit code.
pub fn run(files: &Files) -> i32 {
    hide_console();
    let pending = wait(files, POLL, || false).expect("waits until a game is asked for");
    play(files, pending)
}

/// The game this stream is for. A stream that was started from Moonlight
/// itself, not from CarlaOS, has none: leaving at once would make Sunshine end
/// the stream before its first picture, which Moonlight reports as a fault of
/// the desktop. So the launcher stays, the stream shows the desktop, and a
/// game asked for later from the Ally starts in it. Sunshine closes the
/// launcher when the stream is quit.
fn wait(files: &Files, poll: Duration, stop: impl Fn() -> bool) -> Option<Pending> {
    let mut said = false;
    loop {
        if let Some(pending) = files.take_pending(now()) {
            return Some(pending);
        }
        if !said {
            files.log("A stream started without a game being asked for: showing the desktop.");
            said = true;
        }
        if stop() {
            return None;
        }
        std::thread::sleep(poll);
    }
}

fn play(files: &Files, pending: Pending) -> i32 {
    let install = Install {
        game_id: pending.game_id.clone(),
        title: pending.title.clone(),
        source: pending.source,
        source_id: pending.source_id,
        install_path: pending.install_path,
        exe: pending.exe,
        args: pending.args,
    };
    let child = match launch::start(&install, steam::find_root().as_deref()) {
        Ok(child) => child,
        Err(e) => {
            files.log(&format!("{} could not be started: {e}", install.title));
            return 1;
        }
    };
    files.log(&format!("Started {}.", install.title));
    let mut watch = Watch::new(&install, child);
    // A request to close an earlier game is not for this one.
    files.take_end(now());
    loop {
        files.set_playing(Some(&Playing { game_id: pending.game_id.clone(), title: pending.title.clone(), at: now() }));
        std::thread::sleep(POLL);
        // The player chose "End game" on the Ally.
        if files.take_end(now()) {
            watch.end();
        }
        match watch.poll() {
            Phase::Finished => {
                files.log(&format!("{} has closed: ending the stream.", install.title));
                break;
            }
            Phase::NeverStarted => {
                files.log(&format!("{} never appeared: ending the stream.", install.title));
                break;
            }
            _ => {}
        }
    }
    files.set_playing(None);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::PathBuf;

    fn files(name: &str) -> Files {
        let dir = std::env::temp_dir().join(format!("carla-launcher-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Files::new(dir)
    }

    fn pending() -> Pending {
        Pending {
            game_id: "steam:1".to_string(),
            title: "A Game".to_string(),
            source: "steam".to_string(),
            source_id: "1".to_string(),
            install_path: PathBuf::from("games"),
            exe: None,
            args: String::new(),
            at: now(),
        }
    }

    #[test]
    fn a_stream_without_a_game_stays_on_the_desktop() {
        let files = files("none");
        let looks = Cell::new(0);
        let waited = wait(&files, Duration::ZERO, || {
            looks.set(looks.get() + 1);
            looks.get() == 3
        });
        assert_eq!(waited, None, "it only leaves when told to");
        let log = std::fs::read_to_string(files.dir().join("launcher.log")).unwrap();
        assert_eq!(log.matches("showing the desktop").count(), 1, "said once, not at every look");
    }

    #[test]
    fn a_game_asked_for_later_starts_in_the_same_stream() {
        let files = files("later");
        let looks = Cell::new(0);
        let waited = wait(&files, Duration::ZERO, || {
            looks.set(looks.get() + 1);
            if looks.get() == 2 {
                files.set_pending(&pending()).unwrap();
            }
            looks.get() == 5
        });
        assert_eq!(waited.map(|p| p.title).as_deref(), Some("A Game"));
    }

    #[test]
    fn a_game_asked_for_before_the_stream_starts_at_once() {
        let files = files("now");
        files.set_pending(&pending()).unwrap();
        let waited = wait(&files, Duration::ZERO, || panic!("no waiting"));
        assert!(waited.is_some());
        assert!(!files.dir().join("launcher.log").exists());
    }
}
