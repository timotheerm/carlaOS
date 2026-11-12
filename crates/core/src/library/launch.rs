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

use super::{steam, Install};

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

/// Start an install. Returns the started program when it is the game itself,
/// and nothing when a store client was asked to start it.
pub fn start(install: &Install, steam_root: Option<&Path>) -> io::Result<Option<Child>> {
    match install.source.as_str() {
        "steam" => {
            let root = steam_root.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Steam is not installed"))?;
            Command::new(steam::client(root)).arg(format!("steam://rungameid/{}", install.source_id)).spawn()?;
            Ok(None)
        }
        _ => {
            let exe = install
                .exe
                .as_deref()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "this game has no program to start"))?;
            let mut command = Command::new(exe);
            command.args(split_args(&install.args));
            if install.install_path.is_dir() {
                command.current_dir(&install.install_path);
            }
            command.spawn().map(Some)
        }
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
}
