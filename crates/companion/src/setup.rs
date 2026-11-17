//! Getting a desktop ready without questions: Sunshine installed, its web
//! API claimed for the companion, and the firewall open for the Ally.
//!
//! Each step first looks whether it is already done and then does the least
//! it can. Only two things can make Windows ask for permission, once each:
//! installing Sunshine and opening the firewall. Nothing here is needed again
//! after the first run.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use carla_core::link;

use crate::files::Config;
use crate::sunshine::{self, Sunshine};

/// The user name the companion gives itself in Sunshine.
pub const USER: &str = "carlaos";
/// The name of the firewall rules, also in the installer's uninstaller.
pub const RULE: &str = "CarlaOS Desktop Host";

/// Where Sunshine is installed, if it is.
pub fn sunshine_exe() -> Option<PathBuf> {
    ["ProgramFiles", "ProgramW6432"].iter().filter_map(std::env::var_os).map(|dir| PathBuf::from(dir).join("Sunshine").join("sunshine.exe")).find(|p| p.is_file())
}

/// Install Sunshine with Windows' own package manager. Windows asks once
/// to allow it.
pub fn install_sunshine() -> Result<(), String> {
    let by_hand = "Install it from https://github.com/LizardByte/Sunshine/releases instead, then open CarlaOS Desktop Host again.";
    let status = Command::new("winget")
        .args(["install", "--id", "LizardByte.Sunshine", "--exact", "--source", "winget", "--silent", "--accept-package-agreements", "--accept-source-agreements"])
        .status()
        .map_err(|e| format!("Windows' package manager (winget) could not be started: {e}. {by_hand}"))?;
    if !status.success() && sunshine_exe().is_none() {
        return Err(format!("Sunshine was not installed. {by_hand}"));
    }
    Ok(())
}

/// What starting Sunshine needs from the machine. Tests stand it in, so that
/// nothing is ever started by them.
pub trait Launch: Send + Sync {
    /// Sunshine's program is on this machine.
    fn installed(&self) -> bool;
    /// Ask Windows to start it (the service, else the program itself).
    fn start(&self) -> Result<(), String>;
}

/// Starts Sunshine for real.
pub struct Machine;

/// Run a program with no window and no console, with nothing to read or
/// write: a child of a process that has no console fails with "The handle is
/// invalid" unless its standard handles are nulled (see `quiet` in the
/// launcher of the library).
#[cfg(windows)]
fn unseen(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
}

impl Launch for Machine {
    fn installed(&self) -> bool {
        sunshine_exe().is_some()
    }

    #[cfg(windows)]
    fn start(&self) -> Result<(), String> {
        let exe = sunshine_exe().ok_or("Sunshine is not installed on this desktop.")?;
        // The service, when there is one and this program may start it. 1056
        // is "already running".
        if unseen(Command::new("sc").args(["start", "SunshineService"])).status().is_ok_and(|s| matches!(s.code(), Some(0) | Some(1056))) {
            return Ok(());
        }
        let mut command = Command::new(&exe);
        if let Some(folder) = exe.parent() {
            command.current_dir(folder);
        }
        unseen(&mut command).spawn().map(|_| ()).map_err(|e| format!("its program could not be started ({e})"))
    }

    #[cfg(not(windows))]
    fn start(&self) -> Result<(), String> {
        Err("Sunshine can only be started on Windows.".to_string())
    }
}

#[derive(Default)]
struct Attempt {
    /// An attempt is going on.
    running: bool,
    /// When the last one began.
    last: Option<Instant>,
    /// How the last one ended.
    outcome: Option<Result<(), String>>,
}

/// Makes sure Sunshine runs, starting it when it is installed and silent. One
/// attempt at a time (a second caller waits for the first one's outcome) and
/// at most one in `gap`, so that a Sunshine that cannot start is not restarted
/// over and over.
pub struct Starter {
    launch: Box<dyn Launch>,
    ready: Box<dyn Fn() -> bool + Send + Sync>,
    log: Box<dyn Fn(&str) + Send + Sync>,
    limit: Duration,
    poll: Duration,
    gap: Duration,
    state: std::sync::Mutex<Attempt>,
    done: std::sync::Condvar,
}

impl Starter {
    pub fn new(launch: Box<dyn Launch>, ready: Box<dyn Fn() -> bool + Send + Sync>, log: Box<dyn Fn(&str) + Send + Sync>) -> Self {
        Self { launch, ready, log, limit: Duration::from_secs(20), poll: Duration::from_secs(1), gap: Duration::from_secs(60), state: Default::default(), done: Default::default() }
    }

    /// For this desktop: Sunshine answering on `url`, told about in `host.log`.
    pub fn for_machine(url: &str, files: crate::files::Files) -> Self {
        let url = url.to_string();
        Self::new(Box::new(Machine), Box::new(move || sunshine::answers(&url)), Box::new(move |line| files.host(line)))
    }

    #[cfg(test)]
    fn timed(mut self, limit: Duration, poll: Duration, gap: Duration) -> Self {
        (self.limit, self.poll, self.gap) = (limit, poll, gap);
        self
    }

    /// Wait for Sunshine to answer, for at most the limit.
    fn wait_ready(&self) -> bool {
        let end = Instant::now() + self.limit;
        loop {
            if (self.ready)() {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(self.poll.min(end.saturating_duration_since(Instant::now())));
        }
    }

    /// Nothing when Sunshine already answers; otherwise start it and wait
    /// until it does. This waits for up to the limit.
    pub fn ensure(&self) -> Result<(), String> {
        if (self.ready)() {
            return Ok(());
        }
        if !self.launch.installed() {
            (self.log)("Sunshine is not running and is not installed, so it is not started.");
            return Err("Sunshine is not installed on this desktop.".to_string());
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.running {
            while state.running {
                state = self.done.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            return state.outcome.clone().unwrap_or(Ok(()));
        }
        if state.last.is_some_and(|at| at.elapsed() < self.gap) {
            return Err("Sunshine was started a moment ago and does not answer yet.".to_string());
        }
        state.running = true;
        state.last = Some(Instant::now());
        drop(state);

        (self.log)("Sunshine was not running: starting it.");
        let began = Instant::now();
        let result = self.launch.start().and_then(|()| {
            if self.wait_ready() {
                Ok(())
            } else {
                Err(format!("it did not answer within {} seconds", self.limit.as_secs()))
            }
        });
        match &result {
            Ok(()) => (self.log)(&format!("Sunshine is ready after {} s.", began.elapsed().as_secs())),
            Err(why) => (self.log)(&format!("Sunshine could not be started: {why}.")),
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.running = false;
        state.outcome = Some(result.clone());
        self.done.notify_all();
        result
    }
}

/// Start Sunshine if it is installed and not answering. Waits.
pub fn ensure_sunshine_running(starter: &Starter) -> Result<(), String> {
    starter.ensure()
}

/// The same on a thread of its own, for when nobody waits for it.
pub fn ensure_sunshine_running_later(starter: std::sync::Arc<Starter>) {
    std::thread::spawn(move || {
        let _ = starter.ensure();
    });
}

/// A password nobody has to know: the companion keeps it sealed and is the
/// only one that uses it.
pub fn new_password() -> String {
    link::random_hex(16)
}

/// Run a command line with administrator rights and wait for it. Windows
/// asks for permission. False when it was refused or failed.
fn elevated(program: &str, arguments: &str) -> bool {
    // Single quotes inside a PowerShell single-quoted string are doubled.
    let quoted = |text: &str| text.replace('\'', "''");
    let script = format!(
        "try {{ $p = Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode }} catch {{ exit 1223 }}",
        quoted(program),
        quoted(arguments)
    );
    Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).status().is_ok_and(|s| s.success())
}

/// The command lines that open the firewall for this program: its API and
/// the measuring packets, from the home network and from Tailscale only.
pub fn firewall_commands(exe: &Path) -> String {
    let rule = |protocol: &str| {
        format!(
            "netsh advfirewall firewall add rule name=\"{RULE}\" dir=in action=allow program=\"{}\" protocol={protocol} localport={} remoteip=localsubnet,100.64.0.0/10 profile=any enable=yes",
            exe.display(),
            link::PORT
        )
    };
    format!("netsh advfirewall firewall delete rule name=\"{RULE}\" & {} & {}", rule("TCP"), rule("UDP"))
}

/// Is the firewall already open for this program?
pub fn firewall_open(exe: &Path) -> bool {
    let Ok(out) = Command::new("netsh").args(["advfirewall", "firewall", "show", "rule", &format!("name={RULE}"), "verbose"]).output() else { return false };
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    out.status.success() && text.contains(&exe.display().to_string().to_lowercase())
}

/// Open the firewall for this program. Windows asks once to allow it.
pub fn open_firewall(exe: &Path) -> Result<(), String> {
    if elevated("cmd.exe", &format!("/c {}", firewall_commands(exe))) && firewall_open(exe) {
        Ok(())
    } else {
        Err("The firewall was not opened. Without it the Ally may not find this desktop; run carla-companion firewall to try again.".to_string())
    }
}

/// Give Sunshine a new user name and password with its own command, for
/// when the ones it has are not known. Needs administrator rights, and
/// Sunshine started again to take them up.
pub fn reset_credentials(exe: &Path, user: &str, password: &str) -> bool {
    let line = format!("/c \"\"{}\" --creds {user} {password} & net stop SunshineService & net start SunshineService\"", exe.display());
    elevated("cmd.exe", &line)
}

/// Wait until Sunshine's web API answers at all, after installing or
/// starting it again.
pub fn wait_for_sunshine(url: &str, limit: Duration) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        if sunshine::answers(url) {
            return true;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    false
}

/// How far the set-up of Sunshine got.
#[derive(Debug, PartialEq, Eq)]
pub enum Claimed {
    /// The companion already had a working user name and password.
    Already,
    /// Sunshine was new and the companion chose them.
    Fresh,
    /// Sunshine has a user name and password the companion does not know.
    Taken,
    /// Sunshine does not answer.
    Silent,
}

/// Get the companion a working user name and password for Sunshine without
/// asking anyone, when that is possible: keep what works, or choose them on
/// a Sunshine nobody has set up yet.
pub fn claim(config: &mut Config) -> Result<Claimed, String> {
    if let Some(password) = config.sunshine_password().filter(|_| !config.sunshine_user.is_empty()) {
        if Sunshine::new(&config.sunshine_url, &config.sunshine_user, &password).is_ok_and(|s| sunshine::Streamer::check(&s).is_ok()) {
            return Ok(Claimed::Already);
        }
    }
    if !sunshine::answers(&config.sunshine_url) {
        return Ok(Claimed::Silent);
    }
    let password = new_password();
    if sunshine::claim_fresh(&config.sunshine_url, USER, &password)? {
        config.set_sunshine(USER, &password)?;
        return Ok(Claimed::Fresh);
    }
    Ok(Claimed::Taken)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FakeLaunch {
        installed: bool,
        starts: Arc<AtomicUsize>,
        /// Becomes "answering" once started.
        up: Arc<AtomicBool>,
        works: bool,
        delay: Duration,
    }
    impl Launch for FakeLaunch {
        fn installed(&self) -> bool {
            self.installed
        }
        fn start(&self) -> Result<(), String> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(self.delay);
            if self.works {
                self.up.store(true, Ordering::SeqCst);
                Ok(())
            } else {
                Err("refused".to_string())
            }
        }
    }

    struct Rig {
        starter: Arc<Starter>,
        starts: Arc<AtomicUsize>,
        log: Arc<std::sync::Mutex<Vec<String>>>,
    }

    fn rig(installed: bool, running: bool, works: bool, delay: Duration, gap: Duration) -> Rig {
        let (starts, up) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(running)));
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (ready, sink) = (up.clone(), log.clone());
        let launch = FakeLaunch { installed, starts: starts.clone(), up, works, delay };
        let starter = Starter::new(Box::new(launch), Box::new(move || ready.load(Ordering::SeqCst)), Box::new(move |l| sink.lock().unwrap().push(l.to_string())))
            .timed(Duration::from_millis(150), Duration::from_millis(5), gap);
        Rig { starter: Arc::new(starter), starts, log }
    }

    #[test]
    fn nothing_is_started_that_is_not_installed_or_already_running() {
        let missing = rig(false, false, true, Duration::ZERO, Duration::ZERO);
        assert_eq!(ensure_sunshine_running(&missing.starter), Err("Sunshine is not installed on this desktop.".to_string()));
        assert_eq!(missing.starts.load(Ordering::SeqCst), 0);
        let running = rig(true, true, true, Duration::ZERO, Duration::ZERO);
        assert_eq!(running.starter.ensure(), Ok(()));
        assert_eq!(running.starts.load(Ordering::SeqCst), 0);
        assert!(running.log.lock().unwrap().is_empty());
    }

    #[test]
    fn a_silent_sunshine_is_started_and_waited_for() {
        let r = rig(true, false, true, Duration::ZERO, Duration::from_secs(60));
        assert_eq!(r.starter.ensure(), Ok(()));
        assert_eq!(r.starts.load(Ordering::SeqCst), 1);
        let log = r.log.lock().unwrap();
        assert!(log[0].contains("not running: starting it") && log[1].contains("is ready after"), "{log:?}");
    }

    #[test]
    fn a_failed_start_is_not_repeated_within_the_gap_but_is_after_it() {
        let r = rig(true, false, false, Duration::ZERO, Duration::from_secs(60));
        let first = r.starter.ensure().unwrap_err();
        assert!(first.contains("refused"), "{first}");
        assert!(r.log.lock().unwrap().iter().any(|l| l.contains("could not be started: refused")));
        assert!(r.starter.ensure().is_err());
        assert_eq!(r.starts.load(Ordering::SeqCst), 1, "rate limited");
        let open = rig(true, false, false, Duration::ZERO, Duration::ZERO);
        assert!(open.starter.ensure().is_err() && open.starter.ensure().is_err());
        assert_eq!(open.starts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn two_callers_at_once_start_it_once() {
        let r = rig(true, false, true, Duration::from_millis(60), Duration::from_secs(60));
        let (a, b) = (r.starter.clone(), r.starter.clone());
        let first = std::thread::spawn(move || a.ensure());
        std::thread::sleep(Duration::from_millis(20));
        let second = std::thread::spawn(move || b.ensure());
        assert_eq!((first.join().unwrap(), second.join().unwrap()), (Ok(()), Ok(())));
        assert_eq!(r.starts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_started_sunshine_that_stays_silent_ends_with_the_time_limit() {
        struct Silent(Arc<AtomicUsize>);
        impl Launch for Silent {
            fn installed(&self) -> bool {
                true
            }
            fn start(&self) -> Result<(), String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
        let (starts, log) = (Arc::new(AtomicUsize::new(0)), Arc::new(std::sync::Mutex::new(Vec::new())));
        let sink = log.clone();
        let starter = Starter::new(Box::new(Silent(starts.clone())), Box::new(|| false), Box::new(move |l| sink.lock().unwrap().push(l.to_string())))
            .timed(Duration::from_millis(60), Duration::from_millis(5), Duration::from_secs(60));
        assert_eq!(starter.ensure(), Err("it did not answer within 0 seconds".to_string()));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert!(log.lock().unwrap().iter().any(|l| l.contains("could not be started")));
    }

    #[test]
    fn the_firewall_is_opened_for_this_program_from_home_and_tailscale_only() {
        let commands = firewall_commands(Path::new(r"C:\Users\t\AppData\Local\Programs\CarlaOS Desktop Host\carla-companion.exe"));
        assert_eq!(commands.matches("add rule").count(), 2);
        assert!(commands.starts_with("netsh advfirewall firewall delete rule name=\"CarlaOS Desktop Host\""), "never twice");
        for part in ["protocol=TCP localport=47600", "protocol=UDP localport=47600", "remoteip=localsubnet,100.64.0.0/10", r#"program="C:\Users\t\AppData\Local\Programs\CarlaOS Desktop Host\carla-companion.exe""#] {
            assert!(commands.contains(part), "{part}");
        }
    }

    #[test]
    fn the_password_nobody_types_is_long_and_plain() {
        let password = new_password();
        assert!(password.len() == 32 && password.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(password, new_password());
    }

    #[test]
    fn a_sunshine_that_does_not_answer_is_said_to_be_silent() {
        // Nothing listens on this port of this machine.
        let mut config = Config::new("HOME PC");
        config.sunshine_url = "https://localhost:9".to_string();
        assert_eq!(claim(&mut config), Ok(Claimed::Silent));
        assert!(config.sunshine_user.is_empty());
    }
}
