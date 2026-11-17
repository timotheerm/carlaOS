//! What a client leaves behind when it dies, so the player can send it for
//! diagnosis afterwards.
//!
//! Three things, all in one folder:
//!
//! - **Notes**: a short, bounded trail of what the client was doing
//!   (`breadcrumbs.log`): a session changing phase, a desktop no longer
//!   answering. Callers write plain words and ids only: never keys, PINs,
//!   tokens, paths of saves or anything typed by the player.
//! - **Panic reports**: when the program panics, the hook writes a report with
//!   the message, place, backtrace, the recent notes and a state line, then the
//!   program aborts (the release profile is `panic = "abort"`).
//! - **Unexpected-exit reports**: a marker file exists while the client runs
//!   and is removed on a normal exit. If it is still there at the next start,
//!   the last run ended some other way (a crash outside Rust, a forced close,
//!   Windows shutting down, a power cut) and a report is written from the notes.
//!
//! Reports are plain text and only stay on the device until the player or
//! administrator chooses to copy one. Nothing is sent anywhere from here.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The trail is cut to its newest half when it grows past this.
const NOTES_MAX: usize = 96 * 1024;
/// Notes shown in a report.
const NOTES_IN_REPORT: usize = 60;
/// One note is never longer than this, and never has more than one line.
const NOTE_LEN: usize = 300;
/// Reports kept; older ones are deleted.
const KEEP: usize = 10;

const NOTES: &str = "breadcrumbs.log";
const RUNNING: &str = "running.json";
const SEEN: &str = "seen.txt";

static WRITING: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportInfo {
    /// The file name, which is also what `read` takes.
    pub name: String,
    /// "panic", "unexpected" or "hang".
    pub kind: String,
    pub at: i64,
    pub size: u64,
    /// Not shown to the player yet.
    pub new: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Running {
    pid: u32,
    /// When the process started (seconds since 1970, from Windows), so a
    /// process that later got the same id is not mistaken for this one.
    #[serde(default)]
    start: u64,
    started: i64,
    version: String,
}

#[derive(Debug, Clone)]
pub struct Crash {
    dir: PathBuf,
    version: String,
    /// Whether the run a marker describes is still going. Replaced in tests.
    alive: fn(&Running) -> bool,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "2026-11-17 12:04:09 UTC" from seconds since 1970.
pub fn utc(at: i64) -> String {
    let days = at.div_euclid(86_400);
    let secs = at.rem_euclid(86_400);
    // Days since 1970-01-01 to a calendar date (Hinnant's civil_from_days).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// One line, no control characters, not too long.
fn flat(text: &str) -> String {
    let one: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(NOTE_LEN).collect()
}

/// The text of a report. `notes` is oldest first.
#[allow(clippy::too_many_arguments)]
pub fn render(kind: &str, version: &str, at: i64, summary: &str, detail: &str, state: &str, notes: &[String]) -> String {
    let (summary, detail, state) = (scrub(summary), scrub(detail), scrub(state));
    let (summary, detail, state) = (summary.as_str(), detail.as_str(), state.as_str());
    let mut out = String::new();
    out.push_str("CarlaOS crash report\n");
    out.push_str(&format!("kind: {kind}\nversion: {version}\ntime: {} ({at})\n", utc(at)));
    out.push_str(&format!("system: {} {}\n", std::env::consts::OS, std::env::consts::ARCH));
    out.push_str(&format!("summary: {summary}\n"));
    if !state.trim().is_empty() {
        out.push_str(&format!("\n--- state when it ended ---\n{}\n", state.trim_end()));
    }
    if !detail.trim().is_empty() {
        out.push_str(&format!("\n--- detail ---\n{}\n", detail.trim_end()));
    }
    out.push_str("\n--- what it was doing (oldest first) ---\n");
    if notes.is_empty() {
        out.push_str("(nothing was written down)\n");
    }
    for note in notes {
        out.push_str(&scrub(note));
        out.push('\n');
    }
    out
}

impl Crash {
    pub fn new(dir: impl Into<PathBuf>, version: impl Into<String>) -> Self {
        Crash { dir: dir.into(), version: version.into(), alive: still_running }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Write down what the client is doing. Never fails the caller.
    pub fn note(&self, text: &str) {
        let _guard = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        self.append(text);
    }

    /// Like `note`, but gives up when another thread is writing. For the panic
    /// hook, which must never wait: that thread may be the one that panicked.
    fn note_nowait(&self, text: &str) {
        let Ok(_guard) = WRITING.try_lock() else { return };
        self.append(text);
    }

    fn append(&self, text: &str) {
        let line = format!("{} {}\n", utc(now()), flat(text));
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.file(NOTES);
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(line.as_bytes());
        }
        if fs::metadata(&path).is_ok_and(|m| m.len() as usize > NOTES_MAX) {
            if let Ok(text) = fs::read_to_string(&path) {
                let lines: Vec<&str> = text.lines().collect();
                let keep = lines[lines.len() / 2..].join("\n") + "\n";
                let _ = fs::write(&path, keep);
            }
        }
    }

    /// The newest notes, oldest first. Reads without locking, so the panic
    /// hook can use it whatever the panicking thread held.
    fn notes(&self, count: usize) -> Vec<String> {
        let text = fs::read_to_string(self.file(NOTES)).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[lines.len().saturating_sub(count)..].to_vec()
    }

    /// Call once at start. Returns the name of a report when the last run did
    /// not end normally, then marks this run as running.
    pub fn begin(&self) -> Option<String> {
        let _ = fs::create_dir_all(&self.dir);
        let previous = self.marker();
        // Another copy of CarlaOS is still running: it keeps the marker, and
        // this one writes none, so neither hides the other's death.
        if previous.as_ref().is_some_and(|p| (self.alive)(p)) {
            self.note("started while another copy was running");
            return None;
        }
        let report = previous.and_then(|p| {
            let summary = "The previous run did not end normally (a crash outside the program's own checks, a forced close, Windows shutting down, or a power cut).";
            let detail = format!("previous run: version {}, started {}, process {}", p.version, utc(p.started), p.pid);
            self.write_report("unexpected", summary, &detail, "")
        });
        let pid = std::process::id();
        let running = Running { pid, start: start_time(pid).unwrap_or(0), started: now(), version: self.version.clone() };
        if let Ok(text) = serde_json::to_string(&running) {
            let _ = fs::write(self.file(RUNNING), text);
        }
        self.note(&format!("started, version {}", self.version));
        report
    }

    /// Call when the client ends normally, including before it asks Windows to
    /// restart or switch off. Leaves another copy's marker alone.
    pub fn end(&self) {
        if self.marker().is_some_and(|m| m.pid == std::process::id()) {
            self.note("ended normally");
            let _ = fs::remove_file(self.file(RUNNING));
        }
    }

    fn marker(&self) -> Option<Running> {
        fs::read_to_string(self.file(RUNNING)).ok().and_then(|t| serde_json::from_str(&t).ok())
    }

    /// Write a report file and return its name.
    pub fn write_report(&self, kind: &str, summary: &str, detail: &str, state: &str) -> Option<String> {
        let at = now();
        let text = render(kind, &self.version, at, summary, detail, state, &self.notes(NOTES_IN_REPORT));
        let _ = fs::create_dir_all(&self.dir);
        // Two reports in one second must not overwrite each other.
        let mut name = format!("{kind}-{at}.txt");
        let mut n = 1;
        while self.file(&name).exists() {
            name = format!("{kind}-{at}-{n}.txt");
            n += 1;
        }
        fs::write(self.file(&name), text).ok()?;
        self.prune();
        Some(name)
    }

    /// A panic, as the hook sees it. Also clears the running marker, so the
    /// next start does not report the same death twice.
    pub fn write_panic(&self, message: &str, place: &str, thread: &str, backtrace: &str, state: &str) -> Option<String> {
        let summary = format!("The program panicked on thread \"{thread}\": {}", flat(message));
        let detail = format!("at {place}\n\nbacktrace:\n{backtrace}");
        let name = self.write_report("panic", &summary, &detail, state);
        if self.marker().is_some_and(|m| m.pid == std::process::id()) {
            let _ = fs::remove_file(self.file(RUNNING));
        }
        name
    }

    /// The interface stopped answering while the program kept running.
    pub fn write_hang(&self, silent_for: i64, state: &str) -> Option<String> {
        let summary = format!("The interface stopped responding for {silent_for} seconds.");
        let detail = "The program was still running but its window did not answer. Whether it came back is in the notes below (\"responding again\").";
        self.write_report("hang", &summary, detail, state)
    }

    /// Report panics from now on. `state` says in a line or two what the
    /// client was doing; it runs inside the panic hook, so it must not wait
    /// for a lock (use `try_lock`) and must not allocate much.
    pub fn install_panic_hook(&self, state: impl Fn() -> String + Send + Sync + 'static) {
        let crash = self.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let message = match info.payload().downcast_ref::<&str>() {
                Some(s) => (*s).to_string(),
                None => info.payload().downcast_ref::<String>().cloned().unwrap_or_else(|| "(no message)".to_string()),
            };
            let place = info.location().map_or_else(|| "(unknown place)".to_string(), |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
            let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            crash.note_nowait(&format!("PANIC on {thread} at {place}: {message}"));
            crash.write_panic(&message, &place, &thread, &backtrace, &state());
            previous(info);
        }));
    }

    /// Reports, newest first.
    pub fn reports(&self) -> Vec<ReportInfo> {
        let seen: i64 = fs::read_to_string(self.file(SEEN)).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
        let mut found: Vec<ReportInfo> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let (kind, at) = parse_name(&name)?;
                Some(ReportInfo { size: entry.metadata().map_or(0, |m| m.len()), new: at > seen, kind, at, name })
            })
            .collect();
        found.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.name.cmp(&a.name)));
        found
    }

    /// The reports shown so far need not be announced again.
    pub fn mark_seen(&self) {
        if let Some(newest) = self.reports().first() {
            let _ = fs::write(self.file(SEEN), newest.at.to_string());
        }
    }

    /// The text of one report. Only names this module wrote are accepted.
    pub fn read(&self, name: &str) -> Result<String, String> {
        if parse_name(name).is_none() {
            return Err("That is not a crash report.".to_string());
        }
        fs::read_to_string(self.file(name)).map_err(|_| "That report is gone.".to_string())
    }

    fn prune(&self) {
        for old in self.reports().into_iter().skip(KEEP) {
            let _ = fs::remove_file(self.file(&old.name));
        }
    }
}

/// What a watchdog concludes from the interface's heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beat {
    Fine,
    /// No heartbeat yet (the interface is still loading).
    NotStarted,
    /// Silent for this many seconds: reported once per freeze.
    Froze(i64),
    StillFrozen,
}

/// The interface says it is alive every few seconds; a watchdog asks here
/// whether it has gone quiet for too long.
pub struct Heartbeat {
    last: AtomicI64,
    frozen: AtomicBool,
}

impl Heartbeat {
    pub const fn new() -> Self {
        Heartbeat { last: AtomicI64::new(0), frozen: AtomicBool::new(false) }
    }

    /// The interface answered. True when it had been judged frozen before.
    pub fn beat(&self, now: i64) -> bool {
        self.last.store(now, Ordering::SeqCst);
        self.frozen.swap(false, Ordering::SeqCst)
    }

    /// Count silence from now on: after the Ally slept, or when the window
    /// comes back from being hidden (a hidden page answers rarely).
    pub fn reset(&self, now: i64) {
        if self.last.load(Ordering::SeqCst) != 0 {
            self.last.store(now, Ordering::SeqCst);
        }
        self.frozen.store(false, Ordering::SeqCst);
    }

    pub fn check(&self, now: i64, limit: i64) -> Beat {
        let last = self.last.load(Ordering::SeqCst);
        if last == 0 {
            return Beat::NotStarted;
        }
        let silent = now - last;
        if silent <= limit {
            return Beat::Fine;
        }
        if self.frozen.swap(true, Ordering::SeqCst) {
            return Beat::StillFrozen;
        }
        // A beat may have landed since `last` was read: look again before judging.
        if now - self.last.load(Ordering::SeqCst) <= limit {
            self.frozen.store(false, Ordering::SeqCst);
            return Beat::Fine;
        }
        Beat::Froze(silent)
    }
}

/// `panic-1794913547.txt` or `unexpected-1794913547-2.txt`.
fn parse_name(name: &str) -> Option<(String, i64)> {
    let stem = name.strip_suffix(".txt")?;
    let (kind, rest) = stem.split_once('-')?;
    if !matches!(kind, "panic" | "unexpected" | "hang") {
        return None;
    }
    let at = rest.split('-').next()?;
    let tail_ok = rest.split('-').skip(1).all(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if at.is_empty() || !at.chars().all(|c| c.is_ascii_digit()) || !tail_ok || rest.split('-').count() > 2 {
        return None;
    }
    Some((kind.to_string(), at.parse().ok()?))
}

/// When a process started, if it is running now.
fn start_time(pid: u32) -> Option<u64> {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut system = System::new().ok()?;
    system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    system.process(Pid::from_u32(pid)).map(|p| p.start_time())
}

/// Whether the run a marker describes is still going: its process exists and
/// started when the marker says (a later process may have reused the id).
/// When the system cannot be asked, assume it is gone: a report is better than none.
fn still_running(marker: &Running) -> bool {
    if marker.pid == std::process::id() {
        // This process has not written its marker yet: it is an older one.
        return false;
    }
    start_time(marker.pid).is_some_and(|now| marker.start == 0 || now == marker.start)
}

/// Hide what a report should not carry out of the device: IPv4 addresses and
/// the Windows account name in a path. Best effort, applied to every report.
pub fn scrub(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        let parts: Vec<&str> = run.split('.').collect();
        let is_address = parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u16>().is_ok_and(|n| n <= 255));
        out.push_str(if is_address { "x.x.x.x" } else { run.as_str() });
        run.clear();
    };
    for c in text.chars() {
        if c.is_ascii_digit() || c == '.' {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    // `C:\Users\Name\...` and `C:/Users/Name/...` keep everything but the name.
    let lower = out.to_ascii_lowercase();
    let mut result = String::with_capacity(out.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find("users").map(|i| i + at) {
        let after = found + 5;
        let is_path = found > 0 && matches!(out.as_bytes()[found - 1], b'\\' | b'/') && matches!(out.as_bytes().get(after), Some(b'\\' | b'/'));
        result.push_str(&out[at..after]);
        at = after;
        if is_path {
            let name_start = after + 1;
            let name_end = out[name_start..].find(['\\', '/', '"', '\'', ' ', '\n']).map_or(out.len(), |i| i + name_start);
            result.push_str(&out[at..name_start]);
            result.push_str("<user>");
            at = name_end;
        }
    }
    result.push_str(&out[at..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn crash() -> Crash {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("carla crash test {} {}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&dir);
        Crash::new(dir, "9.9.9")
    }

    #[test]
    fn silence_is_reported_once_and_then_recovery_is_noticed() {
        let h = Heartbeat::new();
        assert_eq!(h.check(100, 25), Beat::NotStarted, "no heartbeat yet is not a freeze");
        assert!(!h.beat(100));
        assert_eq!(h.check(120, 25), Beat::Fine);
        assert_eq!(h.check(126, 25), Beat::Froze(26));
        assert_eq!(h.check(140, 25), Beat::StillFrozen, "one report per freeze");
        assert!(h.beat(141), "the interface answered again after being judged frozen");
        assert_eq!(h.check(150, 25), Beat::Fine);
        assert_eq!(h.check(170, 25), Beat::Froze(29), "a second freeze is a new report");
    }

    #[test]
    fn counting_silence_can_start_over() {
        let h = Heartbeat::new();
        h.reset(500);
        assert_eq!(h.check(900, 25), Beat::NotStarted, "reset does not start a heartbeat that never began");
        h.beat(100);
        h.reset(1000);
        assert_eq!(h.check(1020, 25), Beat::Fine, "after a sleep the silence counts from the wake-up");
    }

    #[test]
    fn a_freeze_is_a_report_of_its_own_kind() {
        let c = crash();
        c.note("session ended: Rocket League");
        let name = c.write_hang(31, "no game or stream was running").unwrap();
        assert!(name.starts_with("hang-"));
        let text = c.read(&name).unwrap();
        for part in ["kind: hang", "31 seconds", "no game or stream was running", "session ended: Rocket League"] {
            assert!(text.contains(part), "missing {part}");
        }
        assert_eq!(c.reports()[0].kind, "hang");
        let second = c.write_hang(40, "").unwrap();
        assert_ne!(second, name, "two freezes in one second do not overwrite each other");
        assert_eq!(c.reports().len(), 2);
    }

    #[test]
    fn dates_are_written_in_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(utc(1_794_913_547 - 1_794_913_547 % 86_400 + 3_661), "2026-11-17 01:01:01 UTC");
    }

    #[test]
    fn a_note_is_one_short_plain_line() {
        let c = crash();
        c.note("first\nsecond\u{7} line");
        c.note(&"x".repeat(1000));
        let notes = c.notes(10);
        assert_eq!(notes.len(), 2);
        assert!(notes[0].ends_with("first second line"));
        assert!(notes[1].len() < 30 + NOTE_LEN);
    }

    #[test]
    fn the_trail_keeps_only_its_newest_part() {
        let c = crash();
        for i in 0..2500 {
            c.note(&format!("event number {i} with some padding to take room {}", "-".repeat(20)));
        }
        let size = fs::metadata(c.file(NOTES)).unwrap().len() as usize;
        assert!(size <= NOTES_MAX + 200, "trail is {size} bytes");
        assert!(c.notes(1)[0].contains("event number 2499"));
    }

    #[test]
    fn a_run_that_did_not_end_is_reported_at_the_next_start() {
        let c = crash();
        assert_eq!(c.begin(), None, "the first start has nothing before it");
        c.note("session: Hades streaming from HOME PC");
        // No `end`: the process died. The next start finds the marker.
        let name = c.begin().expect("the previous run is reported");
        let text = c.read(&name).unwrap();
        assert!(text.contains("kind: unexpected") && text.contains("version: 9.9.9"));
        assert!(text.contains("session: Hades streaming from HOME PC"));
        c.end();
        assert_eq!(c.begin(), None, "a normal end is not reported");
    }

    #[test]
    fn a_panic_report_has_the_place_the_state_and_the_notes() {
        let c = crash();
        c.begin();
        c.note("desktop HOME PC stopped answering");
        let name = c.write_panic("index out of bounds", "src/hosts.rs:10:5", "worker", "frame 0\nframe 1", "streaming Hades, phase running").unwrap();
        let text = c.read(&name).unwrap();
        for part in ["kind: panic", "thread \"worker\"", "index out of bounds", "src/hosts.rs:10:5", "frame 1", "streaming Hades, phase running", "desktop HOME PC stopped answering"] {
            assert!(text.contains(part), "missing {part}");
        }
        assert_eq!(c.begin(), None, "a reported panic is not reported again as an unexpected exit");
    }

    #[test]
    fn addresses_and_the_account_name_are_hidden() {
        assert_eq!(scrub("connect to 192.168.1.20 and 100.90.1.2 failed"), "connect to x.x.x.x and x.x.x.x failed");
        assert_eq!(scrub("version 0.22.0, build 1.2.3.999, 10.0.19045.1"), "version 0.22.0, build 1.2.3.999, 10.0.19045.1");
        assert_eq!(scrub(r"C:\Users\Tim\AppData\Local\CarlaOS"), r"C:\Users\<user>\AppData\Local\CarlaOS");
        assert_eq!(scrub("c:/users/Tim/x and \"C:\\USERS\\Tim\""), "c:/users/<user>/x and \"C:\\USERS\\<user>\"");
        assert_eq!(scrub("many users answered"), "many users answered");
        let c = crash();
        let name = c.write_panic("could not reach 192.168.1.20", r"C:\Users\Tim\src\hosts.rs:1:1", "main", "", "").unwrap();
        let text = c.read(&name).unwrap();
        assert!(!text.contains("192.168") && !text.contains("Tim"), "{text}");
    }

    #[test]
    fn another_copy_that_is_still_running_keeps_its_marker() {
        let c = Crash { alive: |_| true, ..crash() };
        fs::create_dir_all(c.dir()).unwrap();
        let theirs = r#"{"pid":1,"start":5,"started":5,"version":"1.0.0"}"#;
        fs::write(c.file(RUNNING), theirs).unwrap();
        assert_eq!(c.begin(), None, "nothing is reported while the other copy runs");
        assert_eq!(fs::read_to_string(c.file(RUNNING)).unwrap(), theirs, "its marker is not overwritten");
        c.end();
        assert_eq!(fs::read_to_string(c.file(RUNNING)).unwrap(), theirs, "and not removed by this copy");
    }

    #[test]
    fn a_marker_of_a_process_that_is_gone_is_reported() {
        let c = Crash { alive: |_| false, ..crash() };
        fs::create_dir_all(c.dir()).unwrap();
        fs::write(c.file(RUNNING), r#"{"pid":1,"start":5,"started":5,"version":"1.0.0"}"#).unwrap();
        let name = c.begin().expect("reported");
        assert!(c.read(&name).unwrap().contains("version 1.0.0"));
    }

    /// The real hook, in a child process so no other test inherits it.
    #[test]
    fn the_panic_hook_writes_a_report_and_the_next_start_does_not_repeat_it() {
        const NAME: &str = "crash::tests::the_panic_hook_writes_a_report_and_the_next_start_does_not_repeat_it";
        if let Some(dir) = std::env::var_os("CARLA_CRASH_CHILD") {
            let c = Crash::new(dir, "9.9.9");
            c.begin();
            c.install_panic_hook(|| "state line".to_string());
            let _ = std::thread::Builder::new().name("worker".into()).spawn(|| panic!("boom at 10.1.2.3")).unwrap().join();
            std::process::exit(42);
        }
        let c = crash();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--test-threads=1"])
            .env("CARLA_CRASH_CHILD", c.dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(42), "the child ran the scenario");
        let reports = c.reports();
        assert_eq!(reports.len(), 1, "{reports:?}");
        let text = c.read(&reports[0].name).unwrap();
        for part in ["kind: panic", "thread \"worker\"", "boom at x.x.x.x", "state line", "backtrace:", "PANIC on worker"] {
            assert!(text.contains(part), "missing {part} in\n{text}");
        }
        assert!(!text.contains("10.1.2.3"));
        assert_eq!(c.begin(), None, "the panic cleared the marker, so it is not reported twice");
    }

    #[test]
    fn the_hook_never_waits_for_the_trail() {
        let c = crash();
        c.note("before");
        let busy = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        c.note_nowait("while busy");
        drop(busy);
        let notes = c.notes(10);
        assert_eq!(notes.len(), 1);
        assert!(notes[0].ends_with("before"));
    }

    #[test]
    fn only_reports_it_wrote_can_be_read() {
        let c = crash();
        for bad in ["..\\devices.json", "../x.txt", "panic-1.txt\\..\\x", "panic-.txt", "other-1.txt", "panic-1-2-3.txt", "running.json", "breadcrumbs.log"] {
            assert!(c.read(bad).is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn reports_are_listed_newest_first_and_announced_once() {
        let c = crash();
        fs::create_dir_all(c.dir()).unwrap();
        for name in ["panic-100.txt", "unexpected-300.txt", "panic-200.txt", "notes.txt"] {
            fs::write(c.file(name), "x").unwrap();
        }
        let list = c.reports();
        assert_eq!(list.iter().map(|r| r.at).collect::<Vec<_>>(), [300, 200, 100]);
        assert!(list.iter().all(|r| r.new));
        c.mark_seen();
        assert!(c.reports().iter().all(|r| !r.new));
        fs::write(c.file("panic-400.txt"), "x").unwrap();
        assert_eq!(c.reports().iter().filter(|r| r.new).count(), 1);
    }

    #[test]
    fn only_the_newest_reports_are_kept() {
        let c = crash();
        fs::create_dir_all(c.dir()).unwrap();
        for at in 1..=15 {
            fs::write(c.file(&format!("panic-{at}.txt")), "x").unwrap();
        }
        c.write_report("panic", "newest", "", "").unwrap();
        let kept = c.reports();
        assert_eq!(kept.len(), KEEP);
        assert!(kept.iter().all(|r| r.at > 6));
    }
}
