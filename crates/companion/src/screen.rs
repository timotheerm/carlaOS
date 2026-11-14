//! Switching this desktop's screen to the Ally's size for the length of a
//! stream, and making sure it always comes back.
//!
//! The launcher switches the screen when a stream starts and puts it back
//! when the game has closed. But Sunshine ends the launcher without warning
//! when a stream is quit, so three things stand behind that:
//!
//! - the switch is the temporary kind, which Windows itself takes back when
//!   the program that made it ends and never keeps across a sign-out;
//! - before switching, a note is left on disk, and the companion in the
//!   background puts the screen back when it finds the note with no stream
//!   going on any more, including after a crash or a restart;
//! - `carla-companion stream restore` does the same by hand.

use std::time::Duration;

use carla_core::display::{self, Mode};

use crate::files::{now, Files};

/// The screen, so tests can stand one in.
pub trait Screen {
    fn current(&self) -> Option<Mode>;
    fn modes(&self) -> Vec<Mode>;
    fn apply(&self, to: Mode) -> Result<(), String>;
    fn restore(&self) -> Result<(), String>;
}

/// This desktop's main screen.
pub struct Main;

impl Screen for Main {
    fn current(&self) -> Option<Mode> {
        display::current()
    }
    fn modes(&self) -> Vec<Mode> {
        display::modes()
    }
    fn apply(&self, to: Mode) -> Result<(), String> {
        display::apply(to)
    }
    fn restore(&self) -> Result<(), String> {
        display::restore()
    }
}

/// The picture a stream was started for, as Sunshine tells the program it
/// runs. Present for every stream, also one started from Moonlight itself.
pub fn asked_by_sunshine(var: impl Fn(&str) -> Option<String>) -> Option<Mode> {
    let number = |name: &str| var(name)?.trim().parse::<u32>().ok().filter(|n| *n > 0);
    let (width, height) = (number("SUNSHINE_CLIENT_WIDTH")?, number("SUNSHINE_CLIENT_HEIGHT")?);
    // Nothing a screen could be: better left alone.
    if !(320..=16_384).contains(&width) || !(200..=16_384).contains(&height) {
        return None;
    }
    Some(Mode { width, height, hz: number("SUNSHINE_CLIENT_FPS").unwrap_or(60).min(480) })
}

/// Switch the screen for a stream made for `want`. Says what it was switched
/// to, or `None` when it was left as it is.
pub fn switch(screen: &dyn Screen, files: &Files, want: Mode) -> Option<Mode> {
    let current = screen.current();
    let Some(to) = display::plan(current, &screen.modes(), want) else {
        if display::choose(&screen.modes(), want).is_none() {
            files.log(&format!("The screen has no mode for a {}x{} stream: left as it is.", want.width, want.height));
        }
        return None;
    };
    // The note first: if anything goes wrong from here, it is put back.
    if let Err(e) = files.set_switched(Some(to)) {
        files.log(&format!("The screen was left as it is, because that could not be noted: {e}"));
        return None;
    }
    match screen.apply(to) {
        Ok(()) => {
            let was = current.map(Mode::words).unwrap_or_else(|| "an unknown mode".to_string());
            files.log(&format!("Screen switched from {was} to {} for the stream.", to.words()));
            Some(to)
        }
        Err(e) => {
            let _ = files.set_switched(None);
            files.log(&e);
            None
        }
    }
}

/// Put the screen back if it was switched for a stream. Says whether it was.
pub fn put_back(screen: &dyn Screen, files: &Files) -> bool {
    if files.switched().is_none() {
        return false;
    }
    match screen.restore() {
        Ok(()) => {
            let _ = files.set_switched(None);
            files.log("Screen put back.");
            true
        }
        Err(e) => {
            // The note stays, so it is tried again.
            files.log(&e);
            false
        }
    }
}

/// One look by the companion in the background: a screen that was switched
/// for a stream that is no longer going on is put back.
pub fn tidy(screen: &dyn Screen, files: &Files, now: i64) -> bool {
    !files.streaming(now) && put_back(screen, files)
}

/// Keep looking for as long as the companion runs.
pub fn watch(files: Files) {
    std::thread::spawn(move || loop {
        tidy(&Main, &files, now());
        std::thread::sleep(Duration::from_secs(5));
    });
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// A 1440p monitor that remembers what was done to it.
    pub struct Fake {
        pub now: Cell<Mode>,
        pub refuses: bool,
        pub did: RefCell<Vec<String>>,
    }

    pub const HOME: Mode = Mode::new(2560, 1440, 165);

    impl Fake {
        pub fn new() -> Self {
            Self { now: Cell::new(HOME), refuses: false, did: RefCell::default() }
        }
    }

    impl Screen for Fake {
        fn current(&self) -> Option<Mode> {
            Some(self.now.get())
        }
        fn modes(&self) -> Vec<Mode> {
            [(2560, 1440), (1920, 1080), (1280, 720)].into_iter().flat_map(|(w, h)| [60, 120, 165].map(|hz| Mode::new(w, h, hz))).collect()
        }
        fn apply(&self, to: Mode) -> Result<(), String> {
            if self.refuses {
                return Err("Windows did not switch the screen.".to_string());
            }
            self.did.borrow_mut().push(format!("apply {}", to.words()));
            self.now.set(to);
            Ok(())
        }
        fn restore(&self) -> Result<(), String> {
            self.did.borrow_mut().push("restore".to_string());
            self.now.set(HOME);
            Ok(())
        }
    }

    fn files(name: &str) -> Files {
        let dir = std::env::temp_dir().join(format!("carla-screen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Files::new(dir)
    }

    #[test]
    fn sunshine_says_what_the_ally_asked_for() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string());
        let full = env(&[("SUNSHINE_CLIENT_WIDTH", "1920"), ("SUNSHINE_CLIENT_HEIGHT", "1080"), ("SUNSHINE_CLIENT_FPS", "120")]);
        assert_eq!(asked_by_sunshine(full), Some(Mode::new(1920, 1080, 120)));
        let no_rate = env(&[("SUNSHINE_CLIENT_WIDTH", "1280"), ("SUNSHINE_CLIENT_HEIGHT", " 800 ")]);
        assert_eq!(asked_by_sunshine(no_rate), Some(Mode::new(1280, 800, 60)));
        assert_eq!(asked_by_sunshine(env(&[("SUNSHINE_CLIENT_WIDTH", "1920")])), None);
        assert_eq!(asked_by_sunshine(env(&[("SUNSHINE_CLIENT_WIDTH", "0"), ("SUNSHINE_CLIENT_HEIGHT", "1080")])), None);
        assert_eq!(asked_by_sunshine(env(&[("SUNSHINE_CLIENT_WIDTH", "big"), ("SUNSHINE_CLIENT_HEIGHT", "1080")])), None);
        assert_eq!(asked_by_sunshine(env(&[("SUNSHINE_CLIENT_WIDTH", "99999"), ("SUNSHINE_CLIENT_HEIGHT", "1080")])), None);
        assert_eq!(asked_by_sunshine(|_| None), None);
    }

    #[test]
    fn the_screen_is_switched_for_a_stream_and_put_back_after() {
        let (files, screen) = (files("switch"), Fake::new());
        assert_eq!(switch(&screen, &files, Mode::new(1920, 1080, 60)), Some(Mode::new(1920, 1080, 60)));
        assert_eq!(files.switched(), Some(Mode::new(1920, 1080, 60)));
        assert!(put_back(&screen, &files));
        assert_eq!((screen.now.get(), files.switched()), (HOME, None));
        // Nothing to put back the second time: the screen is not touched.
        assert!(!put_back(&screen, &files));
        assert_eq!(*screen.did.borrow(), ["apply 1920x1080 at 60", "restore"]);
        let log = std::fs::read_to_string(files.dir().join("launcher.log")).unwrap();
        assert!(log.contains("Screen switched from 2560x1440 at 165 to 1920x1080 at 60"), "{log}");
    }

    #[test]
    fn a_screen_that_is_right_or_cannot_follow_is_left_alone() {
        let (files, screen) = (files("alone"), Fake::new());
        screen.now.set(Mode::new(1920, 1080, 60));
        assert_eq!(switch(&screen, &files, Mode::new(1920, 1080, 60)), None, "already right");
        assert_eq!(switch(&screen, &files, Mode::new(900, 1600, 60)), None, "no such mode");
        assert_eq!(files.switched(), None);
        assert!(screen.did.borrow().is_empty());
        let refusing = Fake { refuses: true, ..Fake::new() };
        assert_eq!(switch(&refusing, &files, Mode::new(1920, 1080, 60)), None);
        assert_eq!(files.switched(), None, "nothing to put back when nothing was switched");
    }

    #[test]
    fn the_companion_puts_the_screen_back_when_the_launcher_was_ended() {
        let (files, screen) = (files("tidy"), Fake::new());
        files.beat(1_000);
        switch(&screen, &files, Mode::new(1920, 1080, 60));
        // The stream is going on: hands off.
        assert!(!tidy(&screen, &files, 1_004));
        assert_eq!(screen.now.get(), Mode::new(1920, 1080, 60));
        // Sunshine ended the launcher; nothing has been heard for a while.
        assert!(!tidy(&screen, &files, 1_030), "a game that is loading can keep it quiet for a while");
        assert!(tidy(&screen, &files, 1_040));
        assert_eq!((screen.now.get(), files.switched()), (HOME, None));
        assert!(!tidy(&screen, &files, 1_050), "once");
    }
}
