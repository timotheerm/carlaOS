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
