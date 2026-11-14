//! A screen of its own for each stream.
//!
//! With the Virtual Display Driver installed (an open-source, signed driver
//! the administrator installs once: `carla-companion stream virtual
//! install`), a stream gets a virtual screen at exactly the Ally's size,
//! and for as long as the stream lasts it is the only screen that is on:
//!
//! - the picture fits the Ally exactly, whatever the desktop's monitor is;
//! - the game, the mouse and every window are on that screen, so nothing
//!   else of the desktop can be in the picture;
//! - the desktop's own monitor is dark meanwhile, so whoever is in the room
//!   does not see the game, and it is not left in a strange mode.
//!
//! The virtual screen only exists during a stream. It is made when the Ally
//! asks for a game (before Sunshine looks at the screens, which it does as
//! soon as Moonlight connects) and taken away when the stream is over, after
//! which Windows puts back the layout the user chose. A note on disk says a
//! virtual screen is on, so the companion in the background takes it away
//! when the stream was cut off, also after a crash or a restart.
//!
//! When anything here fails, the stream goes on the old way: the desktop's
//! own screen is switched to the nearest mode (see `screen`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use carla_core::display::{self, Mode};
use carla_core::layout::{self, Screen};

use crate::files::{now, Files};

/// How long a note may wait for its stream to start before the virtual
/// screen is taken away again: the Ally may have given up, or Moonlight may
/// not have started.
pub const GRACE: i64 = 90;

/// The driver and the screens, so tests can stand them in.
pub trait Driver {
    /// The driver is installed and answering.
    fn present(&self) -> bool;
    /// Ask the driver for this many virtual screens.
    fn set_count(&self, count: u32) -> Result<(), String>;
    /// Its settings file, where the sizes it offers are listed.
    fn settings(&self) -> Option<PathBuf>;
    fn screens(&self) -> Vec<Screen>;
    /// Switch every screen off but the virtual one.
    fn only_virtual(&self) -> Result<(), String>;
    /// Put back the layout the user chose.
    fn restore(&self) -> Result<(), String>;
    /// The modes of the main screen, and switching it.
    fn modes(&self) -> Vec<Mode>;
    fn apply(&self, to: Mode) -> Result<(), String>;
    /// Wait a moment for Windows to catch up.
    fn pause(&self, how_long: Duration);
}

/// Add a size, and a refresh rate, to the driver's settings when it does
/// not offer them yet. `None` when nothing needs to change.
pub fn with_mode(xml: &str, mode: Mode) -> Option<String> {
    let squeezed: String = xml.split_whitespace().collect();
    let mut out = xml.to_string();
    let mut changed = false;
    let size = format!("<width>{}</width><height>{}</height>", mode.width, mode.height);
    if !squeezed.contains(&size) {
        let at = out.find("</resolutions>")?;
        let entry = format!(
            "    <resolution>\n            <width>{}</width>\n            <height>{}</height>\n            <refresh_rate>{}</refresh_rate>\n        </resolution>\n    ",
            mode.width,
            mode.height,
            mode.hz.max(60)
        );
        out.insert_str(at, &entry);
        changed = true;
    }
    // Every size is offered at every rate listed under <global>.
    let rate = format!("<g_refresh_rate>{}</g_refresh_rate>", mode.hz);
    if mode.hz > 0 && !squeezed.contains(&rate) {
        if let Some(at) = out.find("</global>") {
            out.insert_str(at, &format!("    {rate}\n    "));
            changed = true;
        }
    }
    changed.then_some(out)
}

fn virtual_on(driver: &dyn Driver) -> bool {
    driver.screens().iter().any(|s| layout::is_virtual(s) && s.active)
}

fn wait_until(driver: &dyn Driver, limit: Duration, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        driver.pause(Duration::from_millis(250));
    }
}

/// Give the stream about to start a virtual screen of `want`'s size, the
/// only one that is on. Says what mode the screen got.
pub fn enter(driver: &dyn Driver, files: &Files, want: Mode) -> Result<Mode, String> {
    if !driver.present() {
        return Err("The Virtual Display Driver is not installed.".to_string());
    }
    let already = files.virtual_screen().is_some() && virtual_on(driver);
    // The note first: whatever happens from here, it is undone.
    files.set_virtual_screen(Some(now()))?;
    let result = (|| {
        if !already {
            if let Some(path) = driver.settings() {
                let xml = std::fs::read_to_string(&path).unwrap_or_default();
                if let Some(more) = with_mode(&xml, want) {
                    match std::fs::write(&path, more) {
                        Ok(()) => files.log(&format!("Added {} to the virtual screen's sizes.", want.words())),
                        Err(e) => files.log(&format!("The virtual screen's sizes could not be added to ({e}); the nearest one is used.")),
                    }
                }
            }
            driver.set_count(1)?;
            if !wait_until(driver, Duration::from_secs(10), || driver.screens().iter().any(layout::is_virtual)) {
                return Err("The virtual screen did not appear.".to_string());
            }
        }
        driver.only_virtual()?;
        // Windows needs a moment before the new screen can be set up.
        wait_until(driver, Duration::from_secs(3), || !driver.modes().is_empty());
        let to = display::choose(&driver.modes(), want).ok_or("The virtual screen offers no size near the Ally's.")?;
        driver.apply(to)?;
        Ok(to)
    })();
    match result {
        Ok(to) => {
            files.log(&format!("The stream has a virtual screen of its own, at {}.", to.words()));
            Ok(to)
        }
        Err(e) => {
            files.log(&format!("No virtual screen for this stream: {e}"));
            leave(driver, files);
            Err(e)
        }
    }
}

/// Take the virtual screen away and put back the layout the user chose.
/// Says whether there was one.
pub fn leave(driver: &dyn Driver, files: &Files) -> bool {
    if files.virtual_screen().is_none() {
        return false;
    }
    if let Err(e) = driver.set_count(0) {
        // The note stays, so it is tried again.
        files.log(&format!("The virtual screen could not be taken away: {e}"));
        return false;
    }
    wait_until(driver, Duration::from_secs(8), || !driver.screens().iter().any(layout::is_virtual));
    if let Err(e) = driver.restore() {
        files.log(&e);
    }
    let _ = files.set_virtual_screen(None);
    files.log("The virtual screen is gone and the desktop's screens are back.");
    true
}

/// One look by the companion in the background: a virtual screen whose
/// stream is over, or never started, is taken away.
pub fn tidy(driver: &dyn Driver, files: &Files, now: i64) -> bool {
    match files.virtual_screen() {
        Some(at) if !files.streaming(now) && now - at > GRACE => leave(driver, files),
        _ => false,
    }
}

/// When the companion starts: a virtual screen that is on without a stream
/// (the driver's own default is one, always on) is taken away, so it never
/// sits beside the desktop's screens.
pub fn settle(driver: &dyn Driver, files: &Files, now: i64) {
    if files.virtual_screen().is_some() || files.streaming(now) || !driver.present() {
        return;
    }
    if driver.screens().iter().any(layout::is_virtual) {
        files.log("A virtual screen was on without a stream: taking it away.");
        let _ = driver.set_count(0);
        wait_until(driver, Duration::from_secs(8), || !driver.screens().iter().any(layout::is_virtual));
        let _ = driver.restore();
    }
}

/// The Virtual Display Driver, through the pipe it listens on.
pub struct Vdd;

const PIPE: &str = r"\\.\pipe\MTTVirtualDisplayPipe";

#[cfg(windows)]
impl Vdd {
    fn folder() -> Option<PathBuf> {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let key = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\MikeTheTech\VirtualDisplayDriver").ok()?;
        Some(key.get_value::<String, _>("VDDPATH").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(r"C:\VirtualDisplayDriver")))
    }

    /// One command, as the driver reads them: UTF-16, at most 127 letters.
    fn send(command: &str) -> Result<(), String> {
        use std::io::{Read, Write};
        let end = Instant::now() + Duration::from_secs(5);
        let mut pipe = loop {
            match std::fs::OpenOptions::new().read(true).write(true).open(PIPE) {
                Ok(pipe) => break pipe,
                // Another program is talking to it: wait for a turn.
                Err(_) if Instant::now() < end => std::thread::sleep(Duration::from_millis(200)),
                Err(e) => return Err(format!("The Virtual Display Driver did not answer ({e}).")),
            }
        };
        let bytes: Vec<u8> = command.encode_utf16().flat_map(u16::to_le_bytes).collect();
        pipe.write_all(&bytes).map_err(|e| format!("The Virtual Display Driver did not take the request ({e})."))?;
        // It answers once it has done it; reading waits for that. Not for ever.
        let reader = std::thread::spawn(move || {
            let mut answer = [0u8; 512];
            let _ = pipe.read(&mut answer);
        });
        let waited = Instant::now();
        while !reader.is_finished() && waited.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Driver for Vdd {
    fn present(&self) -> bool {
        Self::folder().is_some()
    }
    fn set_count(&self, count: u32) -> Result<(), String> {
        Self::send(&format!("SETDISPLAYCOUNT {count}"))
    }
    fn settings(&self) -> Option<PathBuf> {
        Self::folder().map(|f| f.join("vdd_settings.xml")).filter(|p| p.is_file())
    }
    fn screens(&self) -> Vec<Screen> {
        layout::list()
    }
    fn only_virtual(&self) -> Result<(), String> {
        layout::only(&layout::is_virtual)
    }
    fn restore(&self) -> Result<(), String> {
        layout::restore()
    }
    fn modes(&self) -> Vec<Mode> {
        display::modes()
    }
    fn apply(&self, to: Mode) -> Result<(), String> {
        display::apply(to)
    }
    fn pause(&self, how_long: Duration) {
        std::thread::sleep(how_long);
    }
}

#[cfg(not(windows))]
impl Driver for Vdd {
    fn present(&self) -> bool {
        false
    }
    fn set_count(&self, _count: u32) -> Result<(), String> {
        Err(format!("There is no {PIPE} away from Windows."))
    }
    fn settings(&self) -> Option<PathBuf> {
        None
    }
    fn screens(&self) -> Vec<Screen> {
        Vec::new()
    }
    fn only_virtual(&self) -> Result<(), String> {
        Err("Screens can only be switched on Windows.".to_string())
    }
    fn restore(&self) -> Result<(), String> {
        Ok(())
    }
    fn modes(&self) -> Vec<Mode> {
        Vec::new()
    }
    fn apply(&self, _to: Mode) -> Result<(), String> {
        Err("Screens can only be switched on Windows.".to_string())
    }
    fn pause(&self, _how_long: Duration) {}
}

/// Install the driver with Windows' own package manager. Windows asks the
/// administrator to allow it.
pub fn install() -> Result<(), String> {
    let status = std::process::Command::new("winget")
        .args(["install", "--id", "VirtualDrivers.Virtual-Display-Driver", "--exact", "--source", "winget", "--accept-package-agreements", "--accept-source-agreements"])
        .status()
        .map_err(|e| format!("Windows' package manager (winget) could not be started: {e}. Install the driver from https://github.com/VirtualDrivers/Virtual-Display-Driver/releases instead."))?;
    if status.success() {
        Ok(())
    } else {
        Err("The driver was not installed. Install it from https://github.com/VirtualDrivers/Virtual-Display-Driver/releases instead.".to_string())
    }
}

/// Keep looking for as long as the companion runs.
pub fn watch(files: Files) {
    std::thread::spawn(move || {
        if files.config().is_some_and(|c| c.virtual_screen) {
            settle(&Vdd, &files, now());
        }
        loop {
            tidy(&Vdd, &files, now());
            std::thread::sleep(Duration::from_secs(5));
        }
    });
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    const VDD: &str = r"\\?\DISPLAY#MTT1337#1&1a2b3c4d&0&UID256#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}";

    /// A desktop with one monitor and the driver installed.
    pub struct Fake {
        pub installed: bool,
        pub count: Cell<u32>,
        pub only: Cell<bool>,
        pub mode: Cell<Option<Mode>>,
        pub refuse_only: bool,
        pub settings: Option<PathBuf>,
        pub did: RefCell<Vec<String>>,
    }

    impl Fake {
        pub fn new() -> Self {
            Self { installed: true, count: Cell::new(0), only: Cell::new(false), mode: Cell::new(None), refuse_only: false, settings: None, did: RefCell::default() }
        }
    }

    impl Driver for Fake {
        fn present(&self) -> bool {
            self.installed
        }
        fn set_count(&self, count: u32) -> Result<(), String> {
            self.did.borrow_mut().push(format!("count {count}"));
            self.count.set(count);
            if count == 0 {
                self.only.set(false);
            }
            Ok(())
        }
        fn settings(&self) -> Option<PathBuf> {
            self.settings.clone()
        }
        fn screens(&self) -> Vec<Screen> {
            let monitor = Screen { name: "ROG PG279Q".into(), path: "#AUS27D1#".into(), maker: 1, product: 2, active: !self.only.get() };
            let vdd = Screen { name: "VDD by MTT".into(), path: VDD.into(), maker: 0x3694, product: 0x1337, active: true };
            std::iter::once(monitor).chain((self.count.get() > 0).then_some(vdd)).collect()
        }
        fn only_virtual(&self) -> Result<(), String> {
            if self.refuse_only {
                return Err("Windows did not switch to the stream's screen.".to_string());
            }
            self.did.borrow_mut().push("only virtual".to_string());
            self.only.set(true);
            Ok(())
        }
        fn restore(&self) -> Result<(), String> {
            self.did.borrow_mut().push("restore".to_string());
            Ok(())
        }
        fn modes(&self) -> Vec<Mode> {
            // What the driver offers by default, at its usual rates.
            [(1280, 800), (1920, 1080), (2560, 1440), (3840, 2160)].into_iter().flat_map(|(w, h)| [60, 90, 120, 144].map(|hz| Mode::new(w, h, hz))).collect()
        }
        fn apply(&self, to: Mode) -> Result<(), String> {
            self.did.borrow_mut().push(format!("apply {}", to.words()));
            self.mode.set(Some(to));
            Ok(())
        }
        fn pause(&self, _how_long: Duration) {}
    }

    fn files(name: &str) -> Files {
        let dir = std::env::temp_dir().join(format!("carla-virtual-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Files::new(dir)
    }

    #[test]
    fn a_stream_gets_a_screen_of_its_own_and_gives_it_back() {
        let (files, driver) = (files("enter"), Fake::new());
        assert_eq!(enter(&driver, &files, Mode::new(1920, 1080, 120)), Ok(Mode::new(1920, 1080, 120)));
        assert!(files.virtual_screen().is_some());
        assert!(driver.only.get());
        assert!(leave(&driver, &files));
        assert_eq!(*driver.did.borrow(), ["count 1", "only virtual", "apply 1920x1080 at 120", "count 0", "restore"]);
        assert_eq!((driver.count.get(), files.virtual_screen()), (0, None));
        assert!(!leave(&driver, &files), "once");
    }

    #[test]
    fn a_second_ask_for_the_same_stream_does_not_make_another() {
        let (files, driver) = (files("again"), Fake::new());
        enter(&driver, &files, Mode::new(1920, 1080, 60)).unwrap();
        enter(&driver, &files, Mode::new(1920, 1080, 60)).unwrap();
        assert_eq!(driver.did.borrow().iter().filter(|d| d.starts_with("count")).count(), 1);
    }

    #[test]
    fn without_the_driver_or_when_windows_refuses_the_stream_goes_on_the_old_way() {
        let files = files("refused");
        let missing = Fake { installed: false, ..Fake::new() };
        assert!(enter(&missing, &files, Mode::new(1920, 1080, 60)).is_err());
        assert!(missing.did.borrow().is_empty() && files.virtual_screen().is_none());
        let refusing = Fake { refuse_only: true, ..Fake::new() };
        assert!(enter(&refusing, &files, Mode::new(1920, 1080, 60)).is_err());
        // What was started is undone.
        assert_eq!(*refusing.did.borrow(), ["count 1", "count 0", "restore"]);
        assert_eq!(files.virtual_screen(), None);
    }

    #[test]
    fn the_size_the_ally_needs_is_added_to_the_drivers_list() {
        let files = files("sizes");
        let xml = "<vdd_settings>\n<global>\n    <g_refresh_rate>60</g_refresh_rate>\n    <g_refresh_rate>120</g_refresh_rate>\n</global>\n<resolutions>\n    <resolution>\n        <width>1920</width>\n        <height>1080</height>\n        <refresh_rate>60</refresh_rate>\n    </resolution>\n</resolutions>\n</vdd_settings>\n";
        assert_eq!(with_mode(xml, Mode::new(1920, 1080, 120)), None, "already there");
        let more = with_mode(xml, Mode::new(1920, 1200, 144)).unwrap();
        let squeezed: String = more.split_whitespace().collect();
        assert!(squeezed.contains("<resolution><width>1920</width><height>1200</height><refresh_rate>144</refresh_rate></resolution></resolutions>"), "{more}");
        assert!(squeezed.contains("<g_refresh_rate>120</g_refresh_rate><g_refresh_rate>144</g_refresh_rate></global>"));
        assert_eq!(with_mode(&more, Mode::new(1920, 1200, 144)), None);
        assert_eq!(with_mode("<vdd_settings/>", Mode::new(1920, 1200, 60)), None, "a file it does not understand is left alone");

        // Written before the driver is asked for the screen, which makes it read the file.
        let path = files.dir().join("vdd_settings.xml");
        std::fs::create_dir_all(files.dir()).unwrap();
        std::fs::write(&path, xml).unwrap();
        let driver = Fake { settings: Some(path.clone()), ..Fake::new() };
        enter(&driver, &files, Mode::new(1280, 800, 60)).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("<width>1280</width>"));
    }

    #[test]
    fn the_companion_takes_away_a_screen_whose_stream_is_over() {
        let (files, driver) = (files("tidy"), Fake::new());
        enter(&driver, &files, Mode::new(1920, 1080, 60)).unwrap();
        let at = files.virtual_screen().unwrap();
        // Waiting for the stream to start: hands off.
        assert!(!tidy(&driver, &files, at + 30));
        // The stream is going on: hands off, however long.
        files.beat(at + 600);
        assert!(!tidy(&driver, &files, at + 610));
        // Sunshine ended the launcher.
        assert!(tidy(&driver, &files, at + 700));
        assert_eq!((driver.count.get(), files.virtual_screen()), (0, None));
        // Asked for and never streamed.
        files.stream_over();
        enter(&driver, &files, Mode::new(1920, 1080, 60)).unwrap();
        let at = files.virtual_screen().unwrap();
        assert!(tidy(&driver, &files, at + GRACE + 1));
    }

    #[test]
    fn a_virtual_screen_left_on_without_a_stream_is_taken_away_at_start() {
        let (files, driver) = (files("settle"), Fake::new());
        driver.count.set(1);
        settle(&driver, &files, now());
        assert_eq!(driver.count.get(), 0);
        // Not in the middle of a stream.
        driver.count.set(1);
        files.beat(now());
        settle(&driver, &files, now());
        assert_eq!(driver.count.get(), 1);
    }
}
