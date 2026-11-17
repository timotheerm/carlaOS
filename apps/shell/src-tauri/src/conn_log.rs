//! The connection log: what the Ally's side of the link to a desktop was
//! doing, with the time of each event to the millisecond. It answers "did the
//! desktop stop answering, for how long, and which step of a stream waited".
//!
//! It lives in `logs` next to the `crash` folder, as `connection.log`, and is
//! cut to its newest half at 256 KB. Settings, Activity log shows it together
//! with the crash trail, and copies it for sending.
//!
//! Written: failed requests to a desktop and slow ones (with the path, the
//! status or kind of failure, the time and whether it went over the home
//! network or Tailscale), desktops becoming reachable or unreachable, how long
//! each look at the desktops took, and the steps of starting and ending a
//! stream. Never written: addresses, request or answer bodies, headers, keys,
//! PINs, tokens or anything the player typed. The log file also hides IPv4
//! addresses and the Windows account name (see `carla_core::applog`).

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use carla_core::applog::{self, Log, Repeats, Say};
use carla_core::link::client::{self, Call};

const NAME: &str = "connection.log";
const MAX_BYTES: usize = 256 * 1024;
/// A request slower than this is written down even when it worked.
const SLOW_MS: u64 = 1500;
/// Most lines the whole log can be asked for: more than the files hold.
const MOST: usize = 5000;

/// The same failure of the same path is written at most this often.
const REPEAT_EVERY: Duration = Duration::from_secs(60);

static LOG: OnceLock<Log> = OnceLock::new();
/// Names of computers (the desktops, and this Ally): never written as they are.
static NAMES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static REPEATS: Mutex<Option<Repeats>> = Mutex::new(None);

/// Open the log and have every request to a desktop watched. Worked out by
/// hand, like the crash folder, because this runs before the interface exists.
pub fn open(identifier: &str, version: &str) {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let _ = LOG.set(Log::new(base.join(identifier).join("logs"), NAME, MAX_BYTES));
    if let Some(own) = std::env::var_os("COMPUTERNAME").and_then(|n| n.into_string().ok()) {
        know_name(&own);
    }
    client::set_observer(observe);
    line(&format!("--- CarlaOS {version} started"));
}

/// A computer's name, from now on written as `<pc>`. A desktop's own words
/// (an error it sent, "HOME-PC did not answer") can carry its name.
pub fn know_name(name: &str) {
    let name = name.trim();
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if name.chars().count() >= 3 && !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
        names.push(name.to_string());
    }
}

/// Names and addresses taken out of a line.
fn plain(text: &str) -> String {
    let names = NAMES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    applog::hide_words(text, &names, "<pc>")
}

/// One event. Does nothing before `open`.
pub fn line(text: &str) {
    if let Some(log) = LOG.get() {
        log.line(&plain(text));
    }
}

/// Whether a request is worth a line: it failed, or it was slow. A picture the
/// desktop has none of (404 on a cover) is how it says so, not a fault.
fn worth_writing(call: &Call) -> bool {
    if call.status == Some(404) && call.path.starts_with("/v1/cover/") {
        return false;
    }
    call.error.is_some() || call.status.is_some_and(|s| s >= 400) || call.elapsed_ms >= SLOW_MS
}

/// The words for one request. Path, outcome, time and route only.
fn describe(call: &Call) -> String {
    let outcome = match (call.status, call.error) {
        (Some(status), _) => format!("answered {status}"),
        (None, Some(kind)) => format!("failed ({kind})"),
        (None, None) => "ended".to_string(),
    };
    format!("link {} {} {} {outcome} in {} ms", call.route, call.method, call.path, call.elapsed_ms)
}

/// What makes two calls "the same" for not writing them again: route, method,
/// path and how it went (not how long it took).
fn same_kind(call: &Call) -> String {
    let how = match (call.status, call.error) {
        (Some(status), _) if call.elapsed_ms >= SLOW_MS && status < 400 => "slow".to_string(),
        (Some(status), _) => status.to_string(),
        (None, kind) => kind.unwrap_or("ended").to_string(),
    };
    format!("{} {} {} {how}", call.route, call.method, call.path)
}

fn observe(call: &Call) {
    if !worth_writing(call) {
        return;
    }
    let said = REPEATS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Repeats::default).admit(&same_kind(call), Instant::now(), REPEAT_EVERY);
    match said {
        Say::Skip => {}
        Say::Write { repeated: 0 } => line(&describe(call)),
        Say::Write { repeated } => line(&format!("{} (repeated {repeated} times in the last minute or so)", describe(call))),
    }
}

/// The newest `count` lines of the connection log and the crash trail, by time.
pub fn tail(count: usize) -> Vec<String> {
    let mine = LOG.get().map(|log| log.tail(count)).unwrap_or_default();
    // The trail is scrubbed when written, but names and IPv6 addresses may
    // have come later; the screen and the copy are never less careful.
    let trail = crate::crash::trail(count).into_iter().map(|l| plain(&carla_core::crash::scrub(&l))).collect();
    applog::merged(vec![mine, trail], count)
}

/// Everything in both, to copy into a message.
pub fn text() -> String {
    let lines = tail(MOST);
    if lines.is_empty() {
        return String::new();
    }
    lines.join("\n") + "\n"
}

/// The folder the log is in.
fn folder() -> Option<PathBuf> {
    LOG.get().and_then(|log| log.path().parent().map(PathBuf::from))
}

/// The newest lines for the Activity log screen.
#[tauri::command(async)]
pub fn log_tail(count: usize) -> Vec<String> {
    tail(count.clamp(1, MOST))
}

/// The whole activity log, for "Copy log".
#[tauri::command(async)]
pub fn log_text() -> String {
    text()
}

#[tauri::command(async)]
pub fn log_open_folder() -> Result<(), String> {
    let dir = folder().ok_or("The folder is not known yet.")?;
    std::fs::create_dir_all(&dir).map_err(|_| "The folder could not be made.".to_string())?;
    std::process::Command::new("explorer.exe").arg(&dir).spawn().map(|_| ()).map_err(|_| "The folder could not be opened.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(status: Option<u16>, error: Option<&'static str>, ms: u64) -> Call<'static> {
        Call { method: "GET", path: "/v1/status", status, error, elapsed_ms: ms, route: "local" }
    }

    #[test]
    fn failed_and_slow_calls_are_written_and_quick_good_ones_are_not() {
        assert!(!worth_writing(&call(Some(200), None, 40)));
        assert!(worth_writing(&call(Some(200), None, 1500)));
        assert!(worth_writing(&call(Some(409), None, 10)));
        assert!(worth_writing(&call(None, Some("unreachable"), 3001)));
    }

    #[test]
    fn a_missing_cover_is_not_news_but_other_missing_things_are() {
        let cover = Call { path: "/v1/cover/steam:10", ..call(Some(404), None, 20) };
        assert!(!worth_writing(&cover));
        assert!(worth_writing(&Call { path: "/v1/games", ..call(Some(404), None, 20) }));
        assert!(worth_writing(&Call { path: "/v1/cover/steam:10", ..call(None, Some("unreachable"), 20) }));
    }

    #[test]
    fn the_same_failure_and_the_same_slowness_count_as_the_same_event() {
        assert_eq!(same_kind(&call(None, Some("unreachable"), 3000)), same_kind(&call(None, Some("unreachable"), 3100)));
        assert_ne!(same_kind(&call(None, Some("unreachable"), 3000)), same_kind(&call(None, Some("untrusted"), 3000)));
        assert_ne!(same_kind(&call(Some(200), None, 2000)), same_kind(&call(Some(200), None, 40)));
    }

    #[test]
    fn names_of_computers_are_taken_out_of_lines() {
        know_name("DESKTOP-LIVINGROOM");
        assert_eq!(plain("desktop-livingroom did not answer"), "<pc> did not answer");
    }

    #[test]
    fn a_line_has_the_path_and_timing_but_nothing_that_identifies_the_desktop() {
        let text = describe(&call(None, Some("unreachable"), 3001));
        assert_eq!(text, "link local GET /v1/status failed (unreachable) in 3001 ms");
        assert_eq!(describe(&call(Some(200), None, 2000)), "link local GET /v1/status answered 200 in 2000 ms");
    }
}
