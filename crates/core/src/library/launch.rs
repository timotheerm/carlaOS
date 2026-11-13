//! Starting a game and knowing when it has finished.
//!
//! Store clients make "finished" awkward: asking Steam to start a game returns
//! at once, and many games start through a small launcher that exits straight
//! away. So a game counts as running while any program whose file lives inside
//! the game's folder is running, and as finished once none has been seen twice
//! in a row.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::{shortcut, steam, Install};

/// How long a game may take to appear after being asked to start. Steam can
/// sit on "updating" or "syncing saves" for a while first.
pub const START_GRACE: Duration = Duration::from_secs(180);

/// Split an arguments line the way a shortcut would: spaces separate, double
/// quotes keep spaces together.
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(current);
    }
    out
}

const EPIC_ADDRESS: &str = "com.epicgames.launcher://apps/";

/// Hand an address to the program registered for it, without a shell in
/// between that could read `&` or `%` as something else.
fn open_address(address: &str) -> io::Result<()> {
    if cfg!(windows) {
        Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(address).spawn().map(|_| ())
    } else {
        Command::new("xdg-open").arg(address).spawn().map(|_| ())
    }
}

/// Start an install. Returns the started program when it is the game itself,
/// and nothing when a store client was asked to start it.
pub fn start(install: &Install, steam_root: Option<&Path>) -> io::Result<Option<Child>> {
    match install.source.as_str() {
        "steam" => {
            let root = steam_root.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Steam is not installed"))?;
            Command::new(steam::client(root)).arg(format!("steam://rungameid/{}", install.source_id)).spawn()?;
            Ok(None)
        }
        "epic" => {
            // The address was built from the launcher's own manifest. Anything
            // else in its place is not handed to Windows to open.
            if !install.args.starts_with(EPIC_ADDRESS) || install.args.contains(char::is_whitespace) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "the Epic Games Launcher does not know this game any more"));
            }
            open_address(&install.args)?;
            Ok(None)
        }
        _ => {
            let exe = install
                .exe
                .as_deref()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "this game has no program to start"))?;
            // Windows cannot start a shortcut as a program. Games are added by
            // the program behind their shortcut; one that still names the
            // shortcut is followed here.
            let followed = if shortcut::is_shortcut(exe) {
                Some(shortcut::read(exe).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?)
            } else {
                None
            };
            let (exe, args) = match &followed {
                Some(link) => (link.target.as_path(), if install.args.trim().is_empty() { link.args.as_str() } else { install.args.as_str() }),
                None => (exe, install.args.as_str()),
            };
            let dir = [followed.as_ref().and_then(|l| l.dir.as_deref()), Some(install.install_path.as_path()), exe.parent()].into_iter().flatten().find(|d| d.is_dir());
            let mut command = Command::new(exe);
            command.args(split_args(args));
            if let Some(dir) = dir {
                command.current_dir(dir);
            }
            match command.spawn() {
                Ok(child) => Ok(Some(child)),
                // Some programs can only be started the way the desktop starts
                // them: one that asks for administrator rights, or a file that
                // is opened by another program.
                Err(e) if cfg!(windows) && needs_the_desktop(&e) => open_program(exe, args, dir).map(|()| None),
                Err(e) => Err(e),
            }
        }
    }
}

/// Windows' "this needs to be elevated" and "this is not a program".
fn needs_the_desktop(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(740 | 193))
}

/// Start a program the way a double click does. The path and the options
/// travel in the environment, not in the command, so nothing in them is read
/// as part of the command.
fn open_program(exe: &Path, args: &str, dir: Option<&Path>) -> io::Result<()> {
    let mut script = "Start-Process -FilePath $env:CARLA_START".to_string();
    if !args.trim().is_empty() {
        script.push_str(" -ArgumentList $env:CARLA_START_ARGS");
    }
    if dir.is_some() {
        script.push_str(" -WorkingDirectory $env:CARLA_START_DIR");
    }
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script]).env("CARLA_START", exe).env("CARLA_START_ARGS", args.trim());
    if let Some(dir) = dir {
        command.env("CARLA_START_DIR", dir);
    }
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("Windows would not start it"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Asked to start, nothing seen yet.
    Starting,
    Running,
    Finished,
    /// Never appeared within [`START_GRACE`].
    NeverStarted,
}

/// Decides the phase from one observation at a time. Kept free of real
/// processes and clocks so it can be tested.
#[derive(Debug)]
pub struct Tracker {
    asked: Instant,
    seen: bool,
    misses: u8,
}

impl Tracker {
    pub fn new(now: Instant) -> Self {
        Self { asked: now, seen: false, misses: 0 }
    }

    pub fn observe(&mut self, running: bool, now: Instant) -> Phase {
        if running {
            self.seen = true;
            self.misses = 0;
            Phase::Running
        } else if !self.seen {
            if now.duration_since(self.asked) > START_GRACE {
                Phase::NeverStarted
            } else {
                Phase::Starting
            }
        } else {
            self.misses += 1;
            if self.misses >= 2 {
                Phase::Finished
            } else {
                Phase::Running
            }
        }
    }
}

/// Looks at the programs running on this machine.
pub struct Processes {
    /// `None` when the process list is not available; nothing then counts as
    /// running, and a game ends by its own program exiting or by hand.
    system: Option<System>,
}

impl Default for Processes {
    fn default() -> Self {
        Self { system: System::new().ok() }
    }
}

fn normal(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

impl Processes {
    /// Is any program whose file is inside `dir` running?
    pub fn any_inside(&mut self, dir: &Path) -> bool {
        let mut prefix = normal(dir);
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        // A folder this short would match half the machine.
        if prefix.len() < 4 {
            return false;
        }
        let Some(system) = self.system.as_mut() else { return false };
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        system.processes().values().any(|p| p.exe().is_some_and(|exe| normal(exe).starts_with(&prefix)))
    }
}

impl Processes {
    /// End every program whose file is inside `dir`. Says how many.
    pub fn end_inside(&mut self, dir: &Path) -> usize {
        // Looked at afresh, and with the same guard against a folder that is
        // too general.
        if !self.any_inside(dir) {
            return 0;
        }
        let mut prefix = normal(dir);
        if !prefix.ends_with('/') {
            prefix.push('/');
        }
        let Some(system) = self.system.as_ref() else { return 0 };
        system.processes().values().filter(|p| p.exe().is_some_and(|exe| normal(exe).starts_with(&prefix))).filter(|p| p.kill()).count()
    }
}

/// One running game: the tracker plus what it watches.
pub struct Watch {
    dir: PathBuf,
    child: Option<Child>,
    tracker: Tracker,
    processes: Processes,
}

impl Watch {
    pub fn new(install: &Install, child: Option<Child>) -> Self {
        Self { dir: install.install_path.clone(), child, tracker: Tracker::new(Instant::now()), processes: Processes::default() }
    }

    /// Close the game because the player asked. It is not asked nicely:
    /// whatever it had not saved is lost.
    pub fn end(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.processes.end_inside(&self.dir);
    }

    pub fn poll(&mut self) -> Phase {
        let child_alive = match self.child.as_mut().map(Child::try_wait) {
            Some(Ok(None)) => true,
            Some(_) => {
                self.child = None;
                false
            }
            None => false,
        };
        let running = child_alive || self.processes.any_inside(&self.dir);
        self.tracker.observe(running, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_split_like_a_shortcut() {
        assert_eq!(split_args(r#"-windowed  --profile "My Save" -x"#), ["-windowed", "--profile", "My Save", "-x"]);
        assert_eq!(split_args(r#""" a"#), ["", "a"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn an_epic_game_is_only_started_through_the_launchers_own_address() {
        let install = |args: &str| Install {
            game_id: "epic:Jade".into(),
            title: "Velvet Circuit".into(),
            source: "epic".into(),
            source_id: "Jade".into(),
            install_path: PathBuf::new(),
            exe: None,
            args: args.into(),
        };
        assert!(start(&install("https://example.com/x"), None).is_err());
        assert!(start(&install("com.epicgames.launcher://apps/Jade?action=launch & calc.exe"), None).is_err());
        assert!(start(&install(""), None).is_err());
    }

    #[test]
    fn a_game_finishes_only_after_it_was_seen_and_then_missed_twice() {
        let t0 = Instant::now();
        let mut t = Tracker::new(t0);
        assert_eq!(t.observe(false, t0), Phase::Starting);
        assert_eq!(t.observe(true, t0), Phase::Running);
        // A launcher handing over to the real game can leave a short gap.
        assert_eq!(t.observe(false, t0), Phase::Running);
        assert_eq!(t.observe(true, t0), Phase::Running);
        assert_eq!(t.observe(false, t0), Phase::Running);
        assert_eq!(t.observe(false, t0), Phase::Finished);
    }

    #[test]
    fn a_game_that_never_appears_is_given_up_on() {
        let t0 = Instant::now();
        let mut t = Tracker::new(t0);
        assert_eq!(t.observe(false, t0 + START_GRACE), Phase::Starting);
        assert_eq!(t.observe(false, t0 + START_GRACE + Duration::from_secs(1)), Phase::NeverStarted);
    }

    #[test]
    fn finds_this_test_program_by_its_folder() {
        let exe = std::env::current_exe().unwrap();
        let mut p = Processes::default();
        assert!(p.any_inside(exe.parent().unwrap()));
        assert!(!p.any_inside(Path::new("/definitely/not/a/real/folder")));
        assert!(!p.any_inside(Path::new("/")));
    }

    #[cfg(unix)]
    #[test]
    fn a_game_that_still_names_its_shortcut_starts_the_program_behind_it() {
        let dir = std::env::temp_dir().join(format!("carla-launch-lnk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("Sleep.lnk");
        std::fs::write(&link, shortcut::sample("/bin/sleep", "0.2", "", true)).unwrap();
        let install = Install {
            game_id: "manual:x".into(),
            title: "Sleep".into(),
            source: "manual".into(),
            source_id: "x".into(),
            install_path: dir.clone(),
            exe: Some(link.clone()),
            args: String::new(),
        };
        let mut child = start(&install, None).unwrap().expect("the program itself");
        assert!(child.wait().unwrap().success(), "started with the shortcut's own options");
        // A file that only looks like a shortcut is refused, not run.
        std::fs::write(&link, b"#!/bin/sh\nexit 0\n").unwrap();
        assert!(start(&install, None).is_err());
        assert!(needs_the_desktop(&io::Error::from_raw_os_error(740)) && !needs_the_desktop(&io::Error::from_raw_os_error(2)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_started_program_is_watched_until_it_exits() {
        let install = Install {
            game_id: "manual:x".into(),
            title: "Sleep".into(),
            source: "manual".into(),
            source_id: "x".into(),
            install_path: PathBuf::from("/nonexistent-carla-dir"),
            exe: Some(PathBuf::from("/bin/sleep")),
            args: "0.3".into(),
        };
        let child = start(&install, None).unwrap();
        let mut watch = Watch::new(&install, child);
        assert_eq!(watch.poll(), Phase::Running);
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(watch.poll(), Phase::Running); // first miss
        assert_eq!(watch.poll(), Phase::Finished);
    }

    #[cfg(unix)]
    #[test]
    fn a_game_the_player_ends_is_gone_at_once() {
        let install = Install {
            game_id: "manual:x".into(),
            title: "Sleep".into(),
            source: "manual".into(),
            source_id: "x".into(),
            install_path: PathBuf::from("/nonexistent-carla-dir"),
            exe: Some(PathBuf::from("/bin/sleep")),
            args: "30".into(),
        };
        let mut watch = Watch::new(&install, start(&install, None).unwrap());
        assert_eq!(watch.poll(), Phase::Running);
        watch.end();
        assert_eq!(watch.poll(), Phase::Running); // first miss
        assert_eq!(watch.poll(), Phase::Finished);
        // Nothing outside the game's own folder is ever ended.
        assert_eq!(Processes::default().end_inside(Path::new("/")), 0);
    }
}
