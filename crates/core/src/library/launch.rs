//! Starting a game and knowing when it has finished.
//!
//! Store clients make "finished" awkward: asking Steam to start a game returns
//! at once, and many games start through a small launcher that exits straight
//! away. So a game counts as running while any program whose file lives inside
//! the game's folder is running, and as finished once none has been seen twice
//! in a row.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::{cloud, shortcut, steam, Install};

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

/// A program started from here gets no standard handles of its own.
///
/// The launcher that Sunshine runs frees its console so it does not sit in
/// the stream's picture. Afterwards the handles Windows passes down by default
/// are invalid, and starting any program fails with "The handle is invalid"
/// (os error 6): the game never started and the stream ended at once.
fn quiet(command: &mut Command) -> &mut Command {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
}

/// Hand an address to the program registered for it, without a shell in
/// between that could read `&` or `%` as something else.
fn open_address(address: &str) -> io::Result<()> {
    if cfg!(windows) {
        quiet(Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(address)).spawn().map(|_| ())
    } else {
        quiet(Command::new("xdg-open").arg(address)).spawn().map(|_| ())
    }
}

/// Start an install. Returns the started program when it is the game itself,
/// and nothing when a store client was asked to start it.
pub fn start(install: &Install, steam_root: Option<&Path>) -> io::Result<Option<Child>> {
    match install.source.as_str() {
        "steam" => {
            let root = steam_root.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Steam is not installed"))?;
            quiet(Command::new(steam::client(root)).arg(format!("steam://rungameid/{}", install.source_id))).spawn()?;
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
        "cloud" => {
            // Checked again here: only a page of a known service is opened.
            let (_, link) = cloud::checked(&install.args).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            if !install.install_path.is_absolute() {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "the cloud browser profile has no folder"));
            }
            let browser = cloud::find_browser().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Microsoft Edge was not found"))?;
            std::fs::create_dir_all(&install.install_path)?;
            // The browser stays open for as long as the game is played: it
            // is what is watched, and what is closed to end the game.
            quiet(Command::new(browser).args(cloud::browser_args(&install.install_path, &link))).spawn().map(Some)
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
            quiet(&mut command);
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
    quiet(&mut command);
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

/// How long a store may spend updating a game before it starts. Past this
/// nobody is still waiting in front of it.
pub const UPDATE_GRACE: Duration = Duration::from_secs(3 * 60 * 60);

/// Decides the phase from one observation at a time. Kept free of real
/// processes and clocks so it can be tested.
#[derive(Debug)]
pub struct Tracker {
    asked: Instant,
    first: Instant,
    seen: bool,
    misses: u8,
}

impl Tracker {
    pub fn new(now: Instant) -> Self {
        Self { asked: now, first: now, seen: false, misses: 0 }
    }

    /// The store is still getting the game ready (updating it), so it is
    /// not late yet: the wait for it to appear starts again.
    pub fn still_preparing(&mut self, now: Instant) {
        if !self.seen && now.duration_since(self.first) <= UPDATE_GRACE {
            self.asked = now;
        }
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
    /// The file of every program that is running, looked at afresh.
    pub fn programs(&mut self) -> Vec<PathBuf> {
        let Some(system) = self.system.as_mut() else { return Vec::new() };
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet));
        system.processes().values().filter_map(|p| p.exe().map(Path::to_path_buf)).collect()
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

/// A program name to watch, reduced to its bare file name. `None` when it
/// names nothing.
pub fn watch_name(name: &str) -> Option<String> {
    let bare = name.trim().trim_matches('"').rsplit(['/', '\\']).next().unwrap_or_default().trim();
    (!bare.is_empty()).then(|| bare.to_string())
}

/// Programs that are never watched or closed by name: Windows' own, and the
/// stores' and browsers', which are not "the game".
const NEVER: [&str; 25] = [
    "explorer", "svchost", "csrss", "winlogon", "lsass", "services", "wininit", "dwm", "taskmgr", "cmd", "powershell", "pwsh", "conhost", "steam", "steamwebhelper", "epicgameslauncher", "galaxyclient", "origin",
    "eadesktop", "ubisoftconnect", "upc", "battle.net", "msedge", "chrome", "firefox",
];

fn windows_dirs() -> Vec<PathBuf> {
    ["SystemRoot", "windir"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect()
}

/// A program name that may be watched, and closed with the game: an `.exe`
/// file name only, of plain characters, that is not one of Windows' own or a
/// store's or browser's. Anything else is refused, because the game "runs"
/// for as long as such a program does and "End game" closes it.
pub fn valid_watch(name: &str) -> Option<String> {
    let bare = watch_name(name)?;
    let lower = bare.to_lowercase();
    let stem = lower.strip_suffix(".exe")?;
    let plain = bare.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '(' | ')' | '+' | '-'));
    let own = windows_dirs().iter().any(|dir| dir.join(&bare).exists() || dir.join("System32").join(&bare).exists());
    // CarlaOS itself, its streaming parts, and whatever program is running this code.
    let ours = stem.starts_with("carla") || ["sunshine", "sunshinesvc", "moonlight"].contains(&stem) || std::env::current_exe().ok().and_then(|p| p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(&bare))).unwrap_or(false);
    (bare == name && bare.len() >= 7 && plain && !NEVER.contains(&stem) && !own && !ours).then_some(bare)
}

/// Whether a running program's file is somewhere that is never closed here.
fn protected(exe: &Path) -> bool {
    let path = normal(exe);
    path.contains("/windowsapps/") || windows_dirs().iter().any(|dir| path.starts_with(&format!("{}/", normal(dir).trim_end_matches('/'))))
}

/// Whether a running program is the one called `wanted`: the file name only,
/// and without regard to case.
fn is_image(exe: Option<&Path>, name: &std::ffi::OsStr, wanted: &str) -> bool {
    exe.and_then(Path::file_name).is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(wanted)) || name.to_string_lossy().eq_ignore_ascii_case(wanted)
}

impl Processes {
    /// Is any program called `name` running? A game that starts through a
    /// launcher in another folder is found this way.
    pub fn any_named(&mut self, name: &str) -> bool {
        let Some(system) = self.system.as_mut() else { return false };
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet));
        system.processes().values().any(|p| is_image(p.exe(), p.name(), name))
    }

    /// End every program called `name`. Says how many.
    /// Only a name that passes [`valid_watch`] is ever acted on, and never a
    /// program that lives in Windows' own folders.
    pub fn end_named(&mut self, name: &str) -> usize {
        if valid_watch(name).is_none() || !self.any_named(name) {
            return 0;
        }
        let Some(system) = self.system.as_ref() else { return 0 };
        system.processes().values().filter(|p| is_image(p.exe(), p.name(), name)).filter(|p| p.pid().as_u32() != std::process::id())
            // A program whose file cannot be seen (access denied) is not ours to close.
            .filter(|p| p.exe().is_some_and(|exe| !protected(exe)))
            .filter(|p| p.kill())
            .count()
    }
}

/// One running game: the tracker plus what it watches.
pub struct Watch {
    dir: PathBuf,
    child: Option<Child>,
    /// A program name that also counts as the game running.
    named: Option<String>,
    tracker: Tracker,
    processes: Processes,
}

impl Watch {
    pub fn new(install: &Install, child: Option<Child>) -> Self {
        Self { dir: install.install_path.clone(), child, named: None, tracker: Tracker::new(Instant::now()), processes: Processes::default() }
    }

    /// Also count the game as running while a program of this name runs,
    /// wherever its file is: for a launcher that hands over to a game that
    /// lives in another folder. Both the wait for the game to appear and the
    /// wait for it to end follow it.
    pub fn watching(mut self, name: Option<&str>) -> Self {
        self.named = name.and_then(valid_watch);
        self
    }

    /// Watch a game that is already running: one that CarlaOS did not start
    /// itself. It is never waited for, only followed until it has gone.
    pub fn adopted(install: &Install) -> Self {
        let mut watch = Self::new(install, None);
        watch.tracker.seen = true;
        watch
    }

    /// Close the game because the player asked. It is not asked nicely:
    /// whatever it had not saved is lost.
    pub fn end(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.processes.end_inside(&self.dir);
        if let Some(name) = &self.named {
            self.processes.end_named(name);
        }
    }

    /// See [`Tracker::still_preparing`].
    pub fn still_preparing(&mut self) {
        self.tracker.still_preparing(Instant::now());
    }

    /// The folder whose programs count as the game.
    pub fn dir(&self) -> &Path {
        &self.dir
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
        let running = child_alive || self.processes.any_inside(&self.dir) || self.named.as_deref().is_some_and(|n| self.processes.any_named(n));
        self.tracker.observe(running, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The launcher frees its console, so the handles a program would inherit
    /// are invalid. Run in a child process so no other test is disturbed.
    #[cfg(windows)]
    #[test]
    fn a_program_starts_even_when_the_inherited_handles_are_invalid() {
        // Not 0: a child that ran no test at all (the name below is stale) exits 0.
        const FIXED: i32 = 42;
        if std::env::var_os("CARLA_BAD_HANDLES").is_some() {
            #[link(name = "kernel32")]
            extern "system" {
                fn SetStdHandle(which: u32, handle: isize) -> i32;
            }
            // STD_INPUT_HANDLE, pointing at nothing.
            unsafe { SetStdHandle(-10i32 as u32, 0x7777) };
            let plain = Command::new("cmd.exe").args(["/c", "exit"]).spawn();
            let mut command = Command::new("cmd.exe");
            command.args(["/c", "exit"]);
            quiet(&mut command);
            let fixed = command.spawn();
            let code = match (plain, fixed) {
                (Err(e), Ok(mut child)) if e.raw_os_error() == Some(6) => {
                    let _ = child.wait();
                    FIXED
                }
                // The broken state was not reproduced, so this proves nothing.
                (Ok(_), _) => 3,
                _ => 1,
            };
            std::process::exit(code);
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "library::launch::tests::a_program_starts_even_when_the_inherited_handles_are_invalid", "--test-threads=1"])
            .env("CARLA_BAD_HANDLES", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(FIXED), "{FIXED} = fixed, 3 = not reproduced, 1 = still failing, 0 = the child ran no test");
    }

    #[test]
    fn a_game_also_runs_while_a_program_of_its_name_runs_wherever_it_is() {
        assert_eq!(watch_name(" \"C:\\Games\\X\\Game-Win64-Shipping.exe\" ").as_deref(), Some("Game-Win64-Shipping.exe"));
        assert_eq!(watch_name("/opt/x/game.bin").as_deref(), Some("game.bin"));
        assert_eq!(watch_name("  "), None);
        let name = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        // Case does not matter, the folder is not part of the name.
        let mut processes = Processes::default();
        assert!(processes.any_named(&name.to_uppercase()));
        assert!(!processes.any_named("definitely-not-running.exe"));
        let install = Install {
            game_id: "manual:x".into(),
            title: "Game".into(),
            source: "manual".into(),
            source_id: "x".into(),
            install_path: PathBuf::from("/nonexistent-carla-dir"),
            exe: None,
            args: String::new(),
        };
        // Not started yet, and no program of its folder: only the name shows it is there.
        assert_eq!(Watch::new(&install, None).poll(), Phase::Starting);
        // This program's own name is never accepted as a game to watch.
        assert_eq!(Watch::new(&install, None).watching(Some(&name.to_uppercase())).poll(), Phase::Starting);
        assert_eq!(Watch::new(&install, None).watching(Some("definitely-not-running.exe")).poll(), Phase::Starting);
        // Once it has gone, the game has finished.
        let mut gone = Watch::adopted(&install).watching(Some("definitely-not-running.exe"));
        assert_eq!(gone.poll(), Phase::Running);
        assert_eq!(gone.poll(), Phase::Finished);
    }

    #[test]
    fn only_a_plain_game_program_name_may_be_watched_or_closed() {
        for ok in ["Game-Win64-Shipping.exe", "Quest.EXE", "My Game (x64).exe", "a_b+c.exe"] {
            assert_eq!(valid_watch(ok).as_deref(), Some(ok), "{ok}");
        }
        for bad in [
            "explorer.exe", "EXPLORER.EXE", "svchost.exe", "steam.exe", "chrome.exe", "powershell.exe", "cmd.exe", "a", "a.exe", ".exe", "game", "game.bat", "C:/x/game.exe", "..\\game.exe", "\"game.exe\"", "ga;me.exe", "ga$me.exe", "ga%me.exe",
            "ga`me.exe", "game.exe ", "ga\nme.exe", "",
        ] {
            assert_eq!(valid_watch(bad), None, "{bad:?}");
        }
        // Whatever sits in Windows' own folders, even under a name that is not on the list.
        if let Some(root) = std::env::var_os("SystemRoot") {
            let root = PathBuf::from(root);
            if let Some(found) = std::fs::read_dir(root.join("System32")).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| n.to_lowercase().ends_with(".exe") && n.len() >= 5) {
                assert_eq!(valid_watch(&found), None, "{found} is Windows'");
            }
        }
        // A game cannot be made to watch such a program.
        let install = Install { game_id: "manual:x".into(), title: "G".into(), source: "manual".into(), source_id: "x".into(), install_path: PathBuf::from("/nonexistent-carla-dir"), exe: None, args: String::new() };
        let mut watch = Watch::adopted(&install).watching(Some("explorer.exe"));
        assert!(watch.named.is_none());
        assert_eq!(watch.poll(), Phase::Running);
        assert_eq!(watch.poll(), Phase::Finished);
        // CarlaOS, its streaming parts, and the program running this very test.
        let me = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned();
        for bad in ["carla-companion.exe", "Carla-Companion.EXE", "carlaos.exe", "carla.exe", "Sunshine.exe", "sunshinesvc.exe", "moonlight.exe", "Moonlight.EXE", me.as_str(), &me.to_uppercase()] {
            assert_eq!(valid_watch(bad), None, "{bad}");
        }
        assert_eq!(Processes::default().end_named(&me), 0, "never the current process");
        assert!(Processes::default().any_named(&me));
        assert_eq!(Processes::default().end_named("explorer.exe"), 0);
        assert_eq!(Processes::default().end_named("svchost.exe"), 0);
    }

    /// Closing by name closes the programs of that name and no other.
    #[cfg(windows)]
    #[test]
    fn closing_by_name_ends_only_the_named_program() {
        let system32 = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32").join("ping.exe");
        let dir = std::env::temp_dir();
        let (mine, other) = (dir.join(format!("pingtest-{}.exe", std::process::id())), dir.join(format!("pingtest-other-{}.exe", std::process::id())));
        std::fs::copy(&system32, &mine).unwrap();
        std::fs::copy(&system32, &other).unwrap();
        let run = |exe: &Path| Command::new(exe).args(["-n", "60", "127.0.0.1"]).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        let (mut a, mut b) = (run(&mine), run(&other));
        std::thread::sleep(Duration::from_millis(500));
        let name = mine.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(Processes::default().end_named(&name), 1);
        let status = a.wait().unwrap();
        assert!(!status.success(), "it was ended");
        assert!(b.try_wait().unwrap().is_none(), "the other program is untouched");
        let _ = b.kill();
        let _ = b.wait();
        let _ = std::fs::remove_file(&mine);
        let _ = std::fs::remove_file(&other);
    }

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
    fn a_game_the_store_is_updating_is_waited_for() {
        let t0 = Instant::now();
        let mut t = Tracker::new(t0);
        let later = t0 + START_GRACE + Duration::from_secs(60);
        t.still_preparing(later - Duration::from_secs(30));
        assert_eq!(t.observe(false, later), Phase::Starting, "the update was still running half a minute ago");
        assert_eq!(t.observe(false, later + START_GRACE), Phase::NeverStarted, "the update ended and the game never came");
        // Not for ever.
        let mut t = Tracker::new(t0);
        let long = t0 + UPDATE_GRACE + Duration::from_secs(1);
        t.still_preparing(long);
        assert_eq!(t.observe(false, long), Phase::NeverStarted);
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

    #[test]
    fn a_cloud_game_only_opens_a_page_of_its_service() {
        let install = |args: &str, dir: &str| Install {
            game_id: "cloud:1".into(),
            title: "Forza".into(),
            source: "cloud".into(),
            source_id: "1".into(),
            install_path: PathBuf::from(dir),
            exe: Some(PathBuf::from("/bin/sh")),
            args: args.into(),
        };
        let profile = std::env::temp_dir().join("carla-cloud-profile").to_string_lossy().into_owned();
        assert!(start(&install("https://evil.example/play", &profile), None).is_err());
        assert!(start(&install("https://www.xbox.com/play --x", &profile), None).is_err());
        assert!(start(&install("https://www.xbox.com/play", "relative"), None).is_err(), "the profile must be a full path");
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
