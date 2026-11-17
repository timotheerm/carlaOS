//! A bounded plain-text log file: the console of a program that has none.
//!
//! The Ally's shell and the desktop companion run without a console window.
//! When something goes wrong between them the only way to see what happened
//! is a log that can be read and sent. One line per event, with the time to the
//! millisecond, so that "the call took 38 seconds" shows.
//!
//! Lines are one line each, cut to a sensible length, and have IPv4 addresses
//! and the Windows account name hidden (see [`crate::crash::scrub`]). The file
//! is cut to its newest half when it grows past its limit, so it never fills a
//! disk. Callers write plain words and ids: never keys, PINs, tokens or things
//! typed by the player.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::crash::{scrub, utc};

/// One line is never longer than this.
const LINE_LEN: usize = 400;

static WRITING: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone)]
pub struct Log {
    path: PathBuf,
    max: usize,
}

/// "2026-11-17 12:04:09.123 UTC".
pub fn stamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let day = utc(now.as_secs() as i64);
    format!("{}.{:03} UTC", day.trim_end_matches(" UTC"), now.subsec_millis())
}

fn one_line(text: &str) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    scrub(&flat).chars().take(LINE_LEN).collect()
}

impl Log {
    /// `dir/name`, cut to its newest half when it passes `max_bytes`.
    pub fn new(dir: impl AsRef<Path>, name: &str, max_bytes: usize) -> Self {
        Log { path: dir.as_ref().join(name), max: max_bytes.max(4096) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Write one event. Never fails the caller.
    pub fn line(&self, text: &str) {
        let line = format!("{} {}\n", stamp(), one_line(text));
        let _guard = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(dir) = self.path.parent() {
            if fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = file.write_all(line.as_bytes());
        }
        if fs::metadata(&self.path).is_ok_and(|m| m.len() as usize > self.max) {
            if let Ok(text) = fs::read_to_string(&self.path) {
                let lines: Vec<&str> = text.lines().collect();
                let keep = lines[lines.len() / 2..].join("\n") + "\n";
                let _ = fs::write(&self.path, keep);
            }
        }
    }

    /// The newest `count` lines, oldest first.
    pub fn tail(&self, count: usize) -> Vec<String> {
        let text = fs::read_to_string(&self.path).unwrap_or_default();
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[lines.len().saturating_sub(count)..].to_vec()
    }

    /// The whole file, for copying into a message.
    pub fn text(&self) -> String {
        fs::read_to_string(&self.path).unwrap_or_default()
    }
}

/// `text` with each of `words` (a computer's name, say) replaced by `with`,
/// whatever the letter case. Words shorter than three letters are left alone:
/// they would be everywhere.
pub fn hide_words(text: &str, words: &[String], with: &str) -> String {
    let mut text = text.to_string();
    for word in words.iter().filter(|w| w.chars().count() >= 3) {
        let (lower, needle) = (text.to_ascii_lowercase(), word.to_ascii_lowercase());
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        while let Some(found) = lower[at..].find(&needle).map(|i| i + at) {
            out.push_str(&text[at..found]);
            out.push_str(with);
            at = found + needle.len();
        }
        out.push_str(&text[at..]);
        text = out;
    }
    text
}

/// What to do with an event that may happen over and over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Say {
    /// Leave it out: the same was written lately.
    Skip,
    /// Write it, saying how many were left out since the last time.
    Write { repeated: u32 },
}

/// Lets the same event through at most once in a while, counting the rest.
/// A dead address polled every few seconds then costs a line a minute.
#[derive(Debug, Default)]
pub struct Repeats {
    seen: HashMap<String, (Instant, u32)>,
}

impl Repeats {
    pub fn admit(&mut self, key: &str, now: Instant, every: Duration) -> Say {
        // Never grows without bound: keys are paths and outcomes, but be safe.
        if self.seen.len() > 256 {
            self.seen.clear();
        }
        match self.seen.get_mut(key) {
            Some((at, left_out)) if now.saturating_duration_since(*at) < every => {
                *left_out += 1;
                Say::Skip
            }
            Some((at, left_out)) => {
                let repeated = std::mem::take(left_out);
                *at = now;
                Say::Write { repeated }
            }
            None => {
                self.seen.insert(key.to_string(), (now, 0));
                Say::Write { repeated: 0 }
            }
        }
    }
}

/// Lines of several logs as one, by the time each starts with ("2026-11-17
/// 12:04:09", to the second: the crash trail has no more). Lines of the same
/// second keep the order of the logs given, and each log keeps its own order.
/// The newest `count` are kept.
pub fn merged(logs: Vec<Vec<String>>, count: usize) -> Vec<String> {
    let mut all: Vec<String> = logs.into_iter().flatten().collect();
    all.sort_by(|a, b| a.get(..19).unwrap_or("").cmp(b.get(..19).unwrap_or("")));
    let skip = all.len().saturating_sub(count);
    all.split_off(skip)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn names_are_hidden_whatever_their_case() {
        let words = vec!["HOME-PC".to_string(), "x".to_string()];
        assert_eq!(hide_words("home-pc did not answer; Home-Pc again", &words, "<desktop>"), "<desktop> did not answer; <desktop> again");
        assert_eq!(hide_words("x marks", &words, "<desktop>"), "x marks", "short words stay");
        assert_eq!(hide_words("nothing here", &[], "<desktop>"), "nothing here");
    }

    #[test]
    fn the_same_event_is_let_through_once_a_minute_and_counted() {
        let mut r = Repeats::default();
        let every = Duration::from_secs(60);
        let t0 = Instant::now();
        assert_eq!(r.admit("GET /v1/status unreachable", t0, every), Say::Write { repeated: 0 });
        for s in 1..=4 {
            assert_eq!(r.admit("GET /v1/status unreachable", t0 + Duration::from_secs(s * 10), every), Say::Skip);
        }
        assert_eq!(r.admit("GET /v1/games unreachable", t0 + Duration::from_secs(5), every), Say::Write { repeated: 0 }, "another event is its own");
        assert_eq!(r.admit("GET /v1/status unreachable", t0 + Duration::from_secs(61), every), Say::Write { repeated: 4 });
        assert_eq!(r.admit("GET /v1/status unreachable", t0 + Duration::from_secs(70), every), Say::Skip);
    }

    #[test]
    fn merged_logs_run_by_time_and_keep_the_newest() {
        let log = vec!["2026-11-17 12:00:02.500 UTC b1".to_string(), "2026-11-17 12:00:02.900 UTC b2".to_string(), "2026-11-17 12:00:05.100 UTC b3".to_string()];
        let trail = vec!["2026-11-17 12:00:01 UTC a1".to_string(), "2026-11-17 12:00:02 UTC a2".to_string(), "2026-11-17 12:00:06 UTC a3".to_string()];
        let all = merged(vec![log.clone(), trail.clone()], 100);
        let ends: Vec<&str> = all.iter().map(|l| l.rsplit(' ').next().unwrap()).collect();
        assert_eq!(ends, ["a1", "b1", "b2", "a2", "b3", "a3"], "same second: the first log's lines come first");
        assert_eq!(merged(vec![log, trail], 2).len(), 2);
        assert!(merged(Vec::new(), 5).is_empty());
        assert_eq!(merged(vec![vec!["odd".to_string(), "2026-11-17 12:00:01 UTC x".to_string()]], 5).len(), 2, "short lines are kept");
    }

    fn log(max: usize) -> Log {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("cfx applog {} {}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&dir);
        Log::new(dir, "x.log", max)
    }

    #[test]
    fn a_line_has_a_millisecond_stamp_and_is_one_plain_line() {
        let l = log(1 << 20);
        l.line("first\nsecond\u{7} line to 192.168.1.20");
        let lines = l.tail(10);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with("first second line to x.x.x.x"), "{}", lines[0]);
        let stamp = &lines[0][..27];
        assert_eq!(&stamp[10..11], " ");
        assert_eq!(&stamp[19..20], ".");
        assert!(stamp.ends_with(" UTC") || lines[0].contains(" UTC "), "{stamp}");
    }

    #[test]
    fn long_lines_are_cut() {
        let l = log(1 << 20);
        l.line(&"x".repeat(5000));
        assert!(l.tail(1)[0].len() < 40 + LINE_LEN);
    }

    #[test]
    fn the_file_keeps_only_its_newest_part() {
        let l = log(8192);
        for i in 0..400 {
            l.line(&format!("event number {i} with some padding to take room {}", "-".repeat(30)));
        }
        let size = fs::metadata(l.path()).unwrap().len() as usize;
        assert!(size <= 8192 + 200, "{size} bytes");
        assert!(l.tail(1)[0].contains("event number 399"));
        assert!(l.text().lines().count() > 10, "a good part is kept");
    }

    #[test]
    fn tail_returns_the_newest_lines_oldest_first() {
        let l = log(1 << 20);
        for i in 0..5 {
            l.line(&format!("n{i}"));
        }
        let t = l.tail(2);
        assert!(t[0].ends_with("n3") && t[1].ends_with("n4"));
        assert!(log(1 << 20).tail(3).is_empty(), "a missing file is an empty log");
    }

    #[test]
    fn writers_in_parallel_do_not_tear_lines() {
        let l = log(1 << 20);
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let l = l.clone();
                std::thread::spawn(move || (0..50).for_each(|i| l.line(&format!("thread {t} line {i} end"))))
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        let lines = l.tail(1000);
        assert_eq!(lines.len(), 400);
        assert!(lines.iter().all(|x| x.contains(" thread ") && x.ends_with(" end")));
    }
}
