//! Runs the helper script with Windows PowerShell, which ships with Windows.
//!
//! Requests and answers travel as JSON files because an elevated process shares
//! neither pipes nor environment with the one that asked for it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

use super::{Error, Mode, Runner};

const SCRIPT: &str = include_str!("helper.ps1");

/// Exit code used when the approval prompt was dismissed (ERROR_CANCELLED).
const CANCELLED: i32 = 1223;

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct PowerShell {
    dir: PathBuf,
}

impl PowerShell {
    /// `dir` holds the script and the short-lived request and answer files.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
}

/// Always the PowerShell that belongs to Windows, never one found on PATH.
fn exe() -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
}

fn helper_args(script: &Path, mode: Mode, request: &Path, result: &Path) -> Vec<String> {
    let path = |p: &Path| p.to_string_lossy().into_owned();
    vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        path(script),
        "-Mode".into(),
        match mode {
            Mode::Scan => "scan".into(),
            Mode::Apply => "apply".into(),
        },
        "-Request".into(),
        path(request),
        "-Result".into(),
        path(result),
    ]
}

/// A PowerShell single-quoted string.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// The command that starts the helper elevated and passes its exit code on.
/// `Start-Process` joins its arguments with spaces, so any argument containing
/// one carries its own double quotes. Windows paths cannot contain a double
/// quote, and none of ours ends in a backslash.
fn elevate_command(exe: &Path, args: &[String]) -> String {
    let list: Vec<String> = args
        .iter()
        .map(|a| if a.contains(' ') { quoted(&format!("\"{a}\"")) } else { quoted(a) })
        .collect();
    format!(
        "try {{ $p = Start-Process -FilePath {} -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList @({}); exit $p.ExitCode }} catch {{ exit {CANCELLED} }}",
        quoted(&exe.to_string_lossy()),
        list.join(","),
    )
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_window(_: &mut Command) {}

impl Runner for PowerShell {
    fn run(&self, mode: Mode, request: &Value, elevate: bool) -> Result<Value, Error> {
        if !cfg!(windows) {
            return Err(Error::Unsupported);
        }
        let io = |what: &str, e: std::io::Error| Error::Helper(format!("{what}: {e}"));
        fs::create_dir_all(&self.dir).map_err(|e| io("creating the work folder", e))?;

        let script = self.dir.join("helper.ps1");
        fs::write(&script, SCRIPT).map_err(|e| io("writing the helper", e))?;

        let tag = format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        let request_file = self.dir.join(format!("request-{tag}.json"));
        let result_file = self.dir.join(format!("result-{tag}.json"));
        let _ = fs::remove_file(&result_file);
        fs::write(&request_file, request.to_string()).map_err(|e| io("writing the request", e))?;

        let exe = exe();
        let args = helper_args(&script, mode, &request_file, &result_file);
        let mut cmd = Command::new(&exe);
        if elevate {
            cmd.args(["-NoProfile", "-NonInteractive", "-Command", &elevate_command(&exe, &args)]);
        } else {
            cmd.args(&args);
        }
        hide_window(&mut cmd);
        let output = cmd.output();

        let answer = fs::read(&result_file);
        let _ = fs::remove_file(&request_file);
        let _ = fs::remove_file(&result_file);

        let output = output.map_err(|e| io("starting PowerShell", e))?;
        let bytes = match answer {
            Ok(bytes) => bytes,
            Err(_) if elevate && output.status.code() == Some(CANCELLED) => return Err(Error::Declined),
            Err(_) => {
                let said = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(Error::Helper(if said.is_empty() {
                    format!("no answer (exit code {})", output.status.code().unwrap_or(-1))
                } else {
                    said
                }));
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| Error::Helper(format!("unreadable answer ({e})")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_quotes_paths_with_spaces_and_apostrophes() {
        let args = helper_args(
            Path::new("C:\\Users\\Zoé O'Neil\\AppData\\Local\\CarlaOS\\helper.ps1"),
            Mode::Apply,
            Path::new("C:\\Temp\\request-1.json"),
            Path::new("C:\\Temp\\result-1.json"),
        );
        let cmd = elevate_command(Path::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"), &args);
        assert!(cmd.contains("-Verb RunAs"));
        // Spaces: wrapped in double quotes for the new process. Apostrophe: doubled for PowerShell.
        assert!(cmd.contains("'\"C:\\Users\\Zoé O''Neil\\AppData\\Local\\CarlaOS\\helper.ps1\"'"), "{cmd}");
        assert!(cmd.contains("'-File'"));
        assert!(cmd.contains("'C:\\Temp\\request-1.json'"));
        assert!(cmd.contains("'apply'"));
    }

    #[cfg(not(windows))]
    #[test]
    fn says_so_when_not_on_windows() {
        let ps = PowerShell::new(std::env::temp_dir().join("carla-ps-test"));
        assert_eq!(ps.run(Mode::Scan, &serde_json::json!({}), false), Err(Error::Unsupported));
    }
}
