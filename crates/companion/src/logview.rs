//! `carla-companion log`: the console of the companion that runs without one.
//!
//! The companion runs in the background with no window and writes what it does
//! to `host.log` (and the launcher to `launcher.log`). This prints the newest
//! lines of either, and with `--follow` keeps printing new ones as they come,
//! so it can sit in a terminal next to the running companion.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::files::Files;

const DEFAULT_LINES: usize = 100;
const MOST_LINES: usize = 10_000;
/// How often `--follow` looks for new lines.
const POLL: Duration = Duration::from_millis(500);

const USAGE: &str = "Use: carla-companion log [--lines N] [--follow] [--launcher]\n     carla-companion log path [--launcher]";

#[derive(Debug, PartialEq, Eq)]
pub struct Show {
    pub lines: usize,
    pub follow: bool,
    /// The launcher's own log instead of the companion's.
    pub launcher: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Ask {
    Show(Show),
    Path { launcher: bool },
}

/// Read what follows `log` on the command line.
pub fn parse(args: &[String]) -> Result<Ask, String> {
    let (path, rest) = match args.first().map(String::as_str) {
        Some("path") => (true, &args[1..]),
        _ => (false, args),
    };
    let mut show = Show { lines: DEFAULT_LINES, follow: false, launcher: false };
    let mut rest = rest.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--launcher" => show.launcher = true,
            "--follow" | "-f" if !path => show.follow = true,
            "--lines" | "-n" if !path => {
                let wanted = rest.next().and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=MOST_LINES).contains(n));
                show.lines = wanted.ok_or_else(|| format!("--lines needs a number from 1 to {MOST_LINES}.\n{USAGE}"))?;
            }
            _ => return Err(USAGE.to_string()),
        }
    }
    Ok(if path { Ask::Path { launcher: show.launcher } } else { Ask::Show(show) })
}

/// What was printed last, so that only what came after is printed next. The
/// file is cut to its newest half now and then, so a position in it would
/// not hold; the last line does (each starts with the time to the millisecond,
/// so no two are alike).
#[derive(Default)]
pub struct Tail {
    last: Option<String>,
}

impl Tail {
    /// The lines of `text` that came after the last one seen. A line still
    /// being written, without its end, waits for the next look.
    pub fn news<'a>(&mut self, text: &'a str) -> Vec<&'a str> {
        let whole = &text[..text.rfind('\n').map_or(0, |end| end + 1)];
        let lines: Vec<&str> = whole.lines().collect();
        let from = match &self.last {
            // Not found: the file was cut or replaced, so all of it is new.
            Some(last) => lines.iter().rposition(|line| line == last).map_or(0, |at| at + 1),
            None => 0,
        };
        if let Some(line) = lines.last() {
            self.last = Some(line.to_string());
        }
        lines[from..].to_vec()
    }
}

/// Print the newest lines of a log and, when asked, the new ones for as long
/// as `wait` says to go on. `read` gives the file's text, or `None` when
/// there is no file; both are given so that tests need neither a file nor time.
pub fn show(opts: &Show, path: &Path, read: &mut dyn FnMut() -> Option<String>, out: &mut dyn Write, wait: &mut dyn FnMut() -> bool) -> std::io::Result<()> {
    let mut tail = Tail::default();
    let (mut first, mut told) = (true, false);
    loop {
        match read() {
            Some(text) => {
                let news = tail.news(&text);
                let shown = if first { &news[news.len().saturating_sub(opts.lines)..] } else { &news[..] };
                if first && news.is_empty() {
                    writeln!(out, "The log is empty so far: {}", path.display())?;
                }
                for line in shown {
                    writeln!(out, "{line}")?;
                }
                first = false;
            }
            None if !told => {
                writeln!(out, "There is no log yet: {}\nThe companion writes it once it runs.", path.display())?;
                told = true;
            }
            None => {}
        }
        out.flush()?;
        if !opts.follow || !wait() {
            return Ok(());
        }
    }
}

/// `carla-companion log ...`
pub fn run(files: &Files, args: &[String]) -> Result<i32, String> {
    let named = |launcher: bool| files.dir().join(if launcher { crate::files::LAUNCHER_LOG } else { crate::files::HOST_LOG });
    match parse(args)? {
        Ask::Path { launcher } => {
            println!("{}", named(launcher).display());
            Ok(0)
        }
        Ask::Show(opts) => {
            let path = named(opts.launcher);
            if opts.follow {
                // On the error stream, so that what is printed is only the log.
                eprintln!("Following {} (press Ctrl+C to stop).", path.display());
            }
            let mut read = || std::fs::read(&path).ok().map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
            let mut wait = || {
                std::thread::sleep(POLL);
                true
            };
            match show(&opts, &path, &mut read, &mut std::io::stdout().lock(), &mut wait) {
                // Whoever reads the output closed it (`| more`): not a failure.
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(0),
                Err(e) => Err(e.to_string()),
                Ok(()) => Ok(0),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &[&str]) -> Vec<String> {
        line.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn log_reads_its_options_and_refuses_what_it_does_not_know() {
        assert_eq!(parse(&words(&[])), Ok(Ask::Show(Show { lines: 100, follow: false, launcher: false })));
        assert_eq!(parse(&words(&["--lines", "20", "--follow", "--launcher"])), Ok(Ask::Show(Show { lines: 20, follow: true, launcher: true })));
        assert_eq!(parse(&words(&["-f", "-n", "5"])), Ok(Ask::Show(Show { lines: 5, follow: true, launcher: false })));
        assert_eq!(parse(&words(&["path"])), Ok(Ask::Path { launcher: false }));
        assert_eq!(parse(&words(&["path", "--launcher"])), Ok(Ask::Path { launcher: true }));
        for bad in [&["--lines"][..], &["--lines", "x"], &["--lines", "0"], &["--lines", "10001"], &["--lines", "-3"], &["--bogus"], &["path", "--follow"], &["path", "--lines", "3"], &["tail"]] {
            assert!(parse(&words(bad)).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn only_lines_after_the_last_one_seen_are_new() {
        let mut tail = Tail::default();
        assert_eq!(tail.news("a\nb\n"), ["a", "b"]);
        assert!(tail.news("a\nb\n").is_empty(), "nothing came");
        assert_eq!(tail.news("a\nb\nc\nd\n"), ["c", "d"]);
        // The file was cut to its newest part: what was seen is not printed again.
        assert_eq!(tail.news("d\ne\n"), ["e"]);
        // Replaced by something else: all of it is new.
        assert_eq!(tail.news("x\ny\n"), ["x", "y"]);
        // A line that is still being written waits until it is whole.
        assert!(tail.news("x\ny\nhalf a li").is_empty());
        assert_eq!(tail.news("x\ny\nhalf a line\n"), ["half a line"]);
    }

    #[test]
    fn the_newest_lines_are_printed_and_a_missing_file_is_said_plainly() {
        let text = (1..=250).map(|n| format!("line {n}\n")).collect::<String>();
        let mut out = Vec::new();
        show(&Show { lines: 3, follow: false, launcher: false }, Path::new("host.log"), &mut || Some(text.clone()), &mut out, &mut || panic!("not following")).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "line 248\nline 249\nline 250\n");

        let mut out = Vec::new();
        show(&Show { lines: 3, follow: false, launcher: false }, Path::new("host.log"), &mut || None, &mut out, &mut || panic!("not following")).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "There is no log yet: host.log\nThe companion writes it once it runs.\n");

        let mut out = Vec::new();
        show(&Show { lines: 3, follow: false, launcher: false }, Path::new("host.log"), &mut || Some(String::new()), &mut out, &mut || panic!("not following")).unwrap();
        assert!(String::from_utf8(out).unwrap().starts_with("The log is empty so far"));
    }

    #[test]
    fn following_prints_what_comes_and_stops_when_told() {
        let states = std::cell::RefCell::new(vec![None, None, Some("old\n"), Some("old\nnew 1\n"), Some("old\nnew 1\n"), Some("new 1\nnew 2\nnew 3\n")].into_iter());
        let looks = std::cell::Cell::new(0);
        let mut out = Vec::new();
        let mut read = || {
            looks.set(looks.get() + 1);
            states.borrow_mut().next().flatten().map(str::to_string)
        };
        // Goes on until the file states are used up: no sleeping, no hanging.
        let mut waits = 0;
        let mut wait = || {
            waits += 1;
            waits < 6
        };
        show(&Show { lines: 100, follow: true, launcher: false }, Path::new("host.log"), &mut read, &mut out, &mut wait).unwrap();
        let printed = String::from_utf8(out).unwrap();
        // Said once that there is no log, though it was looked for twice.
        assert_eq!(printed.matches("There is no log yet").count(), 1, "{printed}");
        assert!(printed.ends_with("old\nnew 1\nnew 2\nnew 3\n"), "{printed}");
        assert_eq!(printed.matches("new 1").count(), 1, "a line is printed once");
        assert_eq!(looks.get(), 6);
    }
}
