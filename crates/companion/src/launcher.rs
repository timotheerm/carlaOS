//! What Sunshine runs when a stream starts.
//!
//! Sunshine knows one application, "CarlaOS", whose command is this launcher.
//! The Ally says which game it wants just before it starts the stream; the
//! launcher starts that game and stays alive until the game has closed, which
//! is how Sunshine knows when to end the stream.

use std::time::Duration;

use carla_core::library::launch::{self, Phase, Watch};
use carla_core::library::{steam, Install};

use carla_core::display::Mode;

use crate::files::{now, Files, Pending, Playing};
use crate::screen::{self, Screen};
use crate::stage::Curtain;

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

/// What a store is called on the loading screen.
fn store_name(source: &str) -> &'static str {
    match source {
        "steam" => "Steam",
        "epic" => "The Epic Games Launcher",
        "gog" => "GOG",
        _ => "The desktop",
    }
}

/// Returns the process exit code.
pub fn run(files: &Files) -> i32 {
    hide_console();
    let config = files.config();
    let match_screen = config.as_ref().is_none_or(|c| c.match_screen);
    let curtain = config.as_ref().is_none_or(|c| c.curtain);
    files.beat(now());
    // Sunshine says what the Ally asked for, also for a stream that was
    // started from Moonlight itself.
    let asked = screen::asked_by_sunshine(|name| std::env::var(name).ok());
    let mut switched = false;
    if let Some(want) = asked.filter(|_| match_screen) {
        switched = screen::switch(&screen::Main, files, want).is_some();
    }
    let pending = wait(files, POLL, || false).expect("waits until a game is asked for");
    // An older Sunshine may not say: then the Ally's own word is used.
    if let Some(want) = pending.screen.filter(|_| match_screen && asked.is_none() && !switched) {
        screen::switch(&screen::Main, files, want);
    }
    let code = play(files, pending, curtain);
    screen::put_back(&screen::Main, files);
    files.stream_over();
    code
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
        files.beat(now());
        std::thread::sleep(poll);
    }
}

fn play(files: &Files, pending: Pending, curtain: bool) -> i32 {
    let install = Install {
        game_id: pending.game_id.clone(),
        title: pending.title.clone(),
        source: pending.source,
        source_id: pending.source_id,
        install_path: pending.install_path,
        exe: pending.exe,
        args: pending.args,
    };
    let steam_root = steam::find_root();
    // Up before the game is asked for, so the desktop is never in the picture.
    let curtain = if curtain { Curtain::open(&install.title, store_name(&install.source), &install.install_path, files) } else { Curtain::none() };
    let child = match launch::start(&install, steam_root.as_deref()) {
        Ok(child) => child,
        Err(e) => {
            files.log(&format!("{} could not be started: {e}", install.title));
            curtain.close();
            return 1;
        }
    };
    files.log(&format!("Started {}.", install.title));
    let mut watch = Watch::new(&install, child);
    // Requests meant for an earlier game are not for this one.
    files.take_end(now());
    files.take_reveal(now());
    let mut updating = false;
    let mut phase = Phase::Starting;
    loop {
        files.set_playing(Some(&Playing { game_id: pending.game_id.clone(), title: pending.title.clone(), at: now() }));
        files.beat(now());
        std::thread::sleep(POLL);
        // The player chose "End game" on the Ally.
        if files.take_end(now()) {
            watch.end();
        }
        if files.take_reveal(now()) {
            curtain.reveal();
        }
        // A store that is updating the game first is not a game that failed
        // to start: wait for it, and say on the loading screen how far it is.
        if phase == Phase::Starting {
            let update = carla_core::library::update_of(&install.source, &install.source_id, steam_root.as_deref());
            if update.is_some() != updating {
                updating = update.is_some();
                files.log(&format!("{} {}", install.title, if updating { "is being updated first." } else { "has been updated." }));
            }
            if updating {
                watch.still_preparing();
            }
            curtain.update(update);
        }
        phase = watch.poll();
        match phase {
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
    curtain.closing();
    // Long enough to be read, and for the game's last window to go.
    std::thread::sleep(Duration::from_millis(1500));
    curtain.close();
    0
}

/// `carla-companion stream test`: do to this desktop what a stream does,
/// for a few seconds, so the administrator can see it without an Ally.
pub fn rehearse(files: &Files, display: &dyn Screen, want: Mode, hold: Duration) -> Result<Option<Mode>, String> {
    if files.streaming(now()) {
        return Err("A stream is going on. Try this when it is over.".to_string());
    }
    files.beat(now());
    let to = screen::switch(display, files, want);
    let curtain = Curtain::open("a test", "Steam", std::path::Path::new(""), files);
    let end = std::time::Instant::now() + hold;
    while std::time::Instant::now() < end {
        files.beat(now());
        std::thread::sleep(Duration::from_millis(500).min(hold));
    }
    curtain.closing();
    std::thread::sleep(Duration::from_millis(1500).min(hold));
    curtain.close();
    screen::put_back(display, files);
    files.stream_over();
    Ok(to)
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
            screen: None,
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
    fn a_rehearsal_switches_the_screen_and_puts_it_back() {
        let files = files("rehearse");
        let display = crate::screen::tests::Fake::new();
        let to = rehearse(&files, &display, Mode::new(1920, 1080, 60), Duration::ZERO).unwrap();
        assert_eq!(to, Some(Mode::new(1920, 1080, 60)));
        assert_eq!(*display.did.borrow(), ["apply 1920x1080 at 60", "restore"]);
        assert!(!files.streaming(now()) && files.switched().is_none());
        // Not in the middle of a real stream.
        files.beat(now());
        assert!(rehearse(&files, &display, Mode::new(1920, 1080, 60), Duration::ZERO).is_err());
    }

    #[test]
    fn a_stream_says_it_is_going_on_while_it_waits_for_a_game() {
        let files = files("beat");
        assert!(!files.streaming(now()));
        wait(&files, Duration::ZERO, || true);
        files.beat(now());
        assert!(files.streaming(now()));
        assert!(!files.streaming(now() + 60), "silence means the launcher is gone");
        files.stream_over();
        assert!(!files.streaming(now()));
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
