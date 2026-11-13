//! Updating an installed CarlaOS from its GitHub releases.
//!
//! Both programs use this: the Ally interface for itself, the companion for
//! the desktop host. An update is the ordinary installer of a newer release,
//! downloaded, checked against the release's checksum file and run quietly.
//! See docs/RELEASES.md for how releases are numbered and named.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The one place updates come from.
pub const REPOSITORY: &str = "timotheerm/carlaOS";
const SUMS: &str = "SHA256SUMS.txt";
/// No installer is anywhere near this; a larger download is not ours.
const LARGEST_INSTALLER: u64 = 400 * 1024 * 1024;

/// Which half of CarlaOS is installed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    Ally,
    DesktopHost,
}

impl Part {
    /// The installer's file name in a release.
    pub fn installer(self, version: &str) -> String {
        match self {
            Part::Ally => format!("CarlaOS-Ally-{version}-setup.exe"),
            Part::DesktopHost => format!("CarlaOS-Desktop-Host-{version}-setup.exe"),
        }
    }
}

/// A newer release that has an installer for this half.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Available {
    pub version: String,
    /// The release's page, for reading what is new.
    pub page: String,
    pub installer: String,
    pub installer_url: String,
    pub sums_url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// "0.5.0" or "v0.5.0" as numbers. Anything else is not a release version.
pub fn parse(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let found = (next()?, next()?, next()?);
    parts.next().is_none().then_some(found)
}

/// Files are only ever fetched from this repository's releases.
fn ours(url: &str) -> bool {
    url.starts_with(&format!("https://github.com/{REPOSITORY}/releases/download/"))
}

/// The newest release above `current` in GitHub's list of releases, if any.
/// Betas count: until 1.0 every release is one. A version CarlaOS cannot
/// read (a local build) counts as older than everything.
pub fn newest(releases: &[u8], part: Part, current: &str) -> Result<Option<Available>, String> {
    let releases: Vec<Release> = serde_json::from_slice(releases).map_err(|_| "GitHub gave an answer CarlaOS did not understand.".to_string())?;
    let current = parse(current).unwrap_or((0, 0, 0));
    let mut best: Option<((u64, u64, u64), Available)> = None;
    for release in releases.iter().filter(|r| !r.draft) {
        let Some(number) = parse(&release.tag_name) else { continue };
        if number <= current || best.as_ref().is_some_and(|(have, _)| *have >= number) {
            continue;
        }
        let version = format!("{}.{}.{}", number.0, number.1, number.2);
        let name = part.installer(&version);
        let asset = |wanted: &str| release.assets.iter().find(|a| a.name == wanted && ours(&a.browser_download_url));
        // A release from before this half existed, or one still uploading, has nothing for us.
        let (Some(installer), Some(sums)) = (asset(&name), asset(SUMS)) else { continue };
        best = Some((
            number,
            Available {
                version,
                page: release.html_url.clone(),
                installer: name,
                installer_url: installer.browser_download_url.clone(),
                sums_url: sums.browser_download_url.clone(),
                size: installer.size,
            },
        ));
    }
    Ok(best.map(|(_, update)| update))
}

/// The checksum a release's checksum file gives for one of its files.
pub fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (sum, file) = line.split_once(char::is_whitespace)?;
        let sum = sum.trim().to_lowercase();
        (file.trim().trim_start_matches('*') == name && sum.len() == 64 && sum.bytes().all(|b| b.is_ascii_hexdigit())).then_some(sum)
    })
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(timeout)).user_agent(format!("CarlaOS/{}", crate::VERSION)).build().into()
}

fn offline<E>(_: E) -> String {
    "GitHub could not be reached. Check the internet connection and try again.".to_string()
}

/// Ask GitHub whether there is something newer than what is running.
pub fn check(part: Part) -> Result<Option<Available>, String> {
    check_from(part, crate::VERSION)
}

/// Ask GitHub whether there is something newer than `current`.
pub fn check_from(part: Part, current: &str) -> Result<Option<Available>, String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=30");
    let mut reply = agent(Duration::from_secs(20)).get(&url).header("accept", "application/vnd.github+json").call().map_err(|e| match e {
        ureq::Error::StatusCode(403 | 429) => "GitHub is asking to slow down. Try again in an hour.".to_string(),
        other => offline(other),
    })?;
    let body = reply.body_mut().with_config().limit(8 * 1024 * 1024).read_to_vec().map_err(offline)?;
    newest(&body, part, current)
}

/// Download an update's installer into `dir` and make sure it is the file
/// the release says it is. `progress` hears how many bytes have arrived.
pub fn download(update: &Available, dir: &Path, progress: &dyn Fn(u64)) -> Result<PathBuf, String> {
    if !ours(&update.installer_url) || !ours(&update.sums_url) || update.installer.contains(['/', '\\']) {
        return Err("That update does not come from CarlaOS's releases.".to_string());
    }
    let agent = agent(Duration::from_secs(900));
    let sums = agent.get(&update.sums_url).call().map_err(offline)?.body_mut().with_config().limit(64 * 1024).read_to_string().map_err(offline)?;
    let expected = expected_sum(&sums, &update.installer).ok_or("The release has no checksum for its installer, so it was not installed.")?;

    let problem = |e: std::io::Error| format!("The update could not be saved: {e}.");
    std::fs::create_dir_all(dir).map_err(problem)?;
    let partial = dir.join(format!("{}.partial", update.installer));
    let mut file = std::fs::File::create(&partial).map_err(problem)?;
    let mut reply = agent.get(&update.installer_url).call().map_err(offline)?;
    let mut reader = reply.body_mut().with_config().limit(LARGEST_INSTALLER).reader();
    let (mut hash, mut got, mut buffer) = (Sha256::new(), 0u64, vec![0u8; 256 * 1024]);
    loop {
        let n = reader.read(&mut buffer).map_err(offline)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        file.write_all(&buffer[..n]).map_err(problem)?;
        got += n as u64;
        progress(got);
    }
    file.sync_all().map_err(problem)?;
    drop(file);
    let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if actual != expected {
        let _ = std::fs::remove_file(&partial);
        return Err("The downloaded update is not the file the release describes, so it was not installed. Try again.".to_string());
    }
    let done = dir.join(&update.installer);
    let _ = std::fs::remove_file(&done);
    std::fs::rename(&partial, &done).map_err(problem)?;
    Ok(done)
}

/// Where downloads wait until they are installed.
pub fn download_dir() -> PathBuf {
    std::env::temp_dir().join("CarlaOS-update")
}

/// Start a downloaded installer and return at once: it replaces the program
/// that calls this, which should stop right after.
///
/// The Ally's installer shows a small progress window and opens CarlaOS again
/// when it is done. The desktop host's works unseen and restarts the companion
/// in the background itself.
pub fn install(part: Part, installer: &Path) -> Result<(), String> {
    let mut command = Command::new(installer);
    match part {
        Part::Ally => command.args(["/P", "/R"]),
        Part::DesktopHost => command.arg("/S"),
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // On its own, so that stopping the program that started it (which the
        // host installer does) cannot take the installer down too.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(|_| ()).map_err(|e| format!("The update could not be started: {e}."))
}

/// The desktop host installed on this machine by its installer, if there is
/// one: its version and its program. This is how the Ally interface notices a
/// machine that has both halves.
#[cfg(windows)]
pub fn desktop_host_here() -> Option<(String, PathBuf)> {
    use winreg::enums::HKEY_CURRENT_USER;
    let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CarlaOS Desktop Host").ok()?;
    let version: String = key.get_value("DisplayVersion").ok()?;
    let folder: String = key.get_value("InstallLocation").ok()?;
    let program = PathBuf::from(folder).join("carla-companion.exe");
    program.is_file().then_some((version, program))
}

#[cfg(not(windows))]
pub fn desktop_host_here() -> Option<(String, PathBuf)> {
    None
}

/// Check, download and start installing in one go. `Ok(None)` when this is
/// already the newest version.
pub fn update_now(part: Part, progress: &dyn Fn(u64)) -> Result<Option<Available>, String> {
    let Some(update) = check(part)? else { return Ok(None) };
    let installer = download(&update, &download_dir(), progress)?;
    install(part, &installer)?;
    Ok(Some(update))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, draft: bool, files: &[&str]) -> serde_json::Value {
        let assets: Vec<_> = files
            .iter()
            .map(|name| serde_json::json!({ "name": name, "size": 5, "browser_download_url": format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{name}") }))
            .collect();
        serde_json::json!({ "tag_name": tag, "draft": draft, "prerelease": true, "html_url": format!("https://github.com/{REPOSITORY}/releases/tag/{tag}"), "assets": assets })
    }

    fn list(releases: &[serde_json::Value]) -> Vec<u8> {
        serde_json::to_vec(releases).unwrap()
    }

    const FULL: [&str; 4] = ["CarlaOS-Ally-X-setup.exe", "CarlaOS-Desktop-Host-X-setup.exe", "CarlaOS-Full-X-setup.exe", "SHA256SUMS.txt"];

    fn files(version: &str) -> Vec<String> {
        FULL.iter().map(|f| f.replace('X', version)).collect()
    }

    fn with(tag: &str, draft: bool) -> serde_json::Value {
        let names = files(tag.trim_start_matches('v'));
        release(tag, draft, &names.iter().map(String::as_str).collect::<Vec<_>>())
    }

    #[test]
    fn versions_are_three_numbers() {
        assert_eq!(parse("0.5.0"), Some((0, 5, 0)));
        assert_eq!(parse("v1.20.3"), Some((1, 20, 3)));
        for bad in ["dev", "0.5", "0.5.0.1", "0.5.x", "", "0.5.0-beta"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn picks_the_highest_number_not_the_first_listed() {
        let releases = list(&[with("v0.9.0", false), with("v0.10.0", false), with("v0.5.0", false)]);
        let update = newest(&releases, Part::Ally, "0.5.0").unwrap().unwrap();
        assert_eq!(update.version, "0.10.0");
        assert_eq!(update.installer, "CarlaOS-Ally-0.10.0-setup.exe");
        assert!(update.installer_url.ends_with("/v0.10.0/CarlaOS-Ally-0.10.0-setup.exe"));
        assert!(update.sums_url.ends_with("/v0.10.0/SHA256SUMS.txt"));
        let host = newest(&releases, Part::DesktopHost, "0.9.0").unwrap().unwrap();
        assert_eq!(host.installer, "CarlaOS-Desktop-Host-0.10.0-setup.exe");
    }

    #[test]
    fn nothing_when_up_to_date_or_ahead() {
        let releases = list(&[with("v0.5.0", false), with("v0.4.0", false)]);
        assert_eq!(newest(&releases, Part::Ally, "0.5.0").unwrap(), None);
        assert_eq!(newest(&releases, Part::Ally, "0.6.0").unwrap(), None);
        assert_eq!(newest(&[], Part::Ally, "0.5.0").ok(), None, "not a list");
        assert_eq!(newest(b"[]", Part::Ally, "0.5.0").unwrap(), None);
    }

    #[test]
    fn a_local_build_is_older_than_any_release() {
        let releases = list(&[with("v0.2.0", false)]);
        assert_eq!(newest(&releases, Part::Ally, "dev").unwrap().unwrap().version, "0.2.0");
    }

    #[test]
    fn skips_drafts_other_tags_and_releases_without_our_installer() {
        let ally_only = release("v0.8.0", false, &["CarlaOS-Ally-0.8.0-setup.exe", "SHA256SUMS.txt"]);
        let no_sums = release("v0.9.0", false, &["CarlaOS-Ally-0.9.0-setup.exe"]);
        let releases = list(&[with("v0.7.0", true), with("nightly", false), ally_only, no_sums, with("v0.6.0", false)]);
        assert_eq!(newest(&releases, Part::Ally, "0.5.0").unwrap().unwrap().version, "0.8.0");
        assert_eq!(newest(&releases, Part::DesktopHost, "0.5.0").unwrap().unwrap().version, "0.6.0");
    }

    #[test]
    fn only_files_from_our_releases_count() {
        let elsewhere = serde_json::json!({ "tag_name": "v0.9.0", "assets": [
            { "name": "CarlaOS-Ally-0.9.0-setup.exe", "browser_download_url": "https://example.com/CarlaOS-Ally-0.9.0-setup.exe" },
            { "name": "SHA256SUMS.txt", "browser_download_url": format!("https://github.com/{REPOSITORY}/releases/download/v0.9.0/SHA256SUMS.txt") },
        ]});
        assert_eq!(newest(&list(&[elsewhere]), Part::Ally, "0.5.0").unwrap(), None);
        let update = Available {
            version: "0.9.0".into(),
            page: String::new(),
            installer: "CarlaOS-Ally-0.9.0-setup.exe".into(),
            installer_url: "https://github.com/someone-else/carlaOS/releases/download/v0.9.0/CarlaOS-Ally-0.9.0-setup.exe".into(),
            sums_url: format!("https://github.com/{REPOSITORY}/releases/download/v0.9.0/SHA256SUMS.txt"),
            size: 0,
        };
        let refused = download(&update, &std::env::temp_dir(), &|_| {}).unwrap_err();
        assert!(refused.contains("does not come from"), "{refused}");
    }

    #[test]
    fn reads_the_checksum_file() {
        let sum = "3660f4b4349d29af3d42fdb16eed0c436fc7eb1e9b5e2458c6b441fa5bf65c6b";
        let sums = format!("{sum}  CarlaOS-Ally-0.5.0-setup.exe\n{}  *CarlaOS-Full-0.5.0-setup.exe\nshort  CarlaOS-Desktop-Host-0.5.0-setup.exe\n", sum.to_uppercase());
        assert_eq!(expected_sum(&sums, "CarlaOS-Ally-0.5.0-setup.exe").as_deref(), Some(sum));
        assert_eq!(expected_sum(&sums, "CarlaOS-Full-0.5.0-setup.exe").as_deref(), Some(sum), "binary marker and capitals");
        assert_eq!(expected_sum(&sums, "CarlaOS-Desktop-Host-0.5.0-setup.exe"), None, "not a checksum");
        assert_eq!(expected_sum(&sums, "CarlaOS-Ally-0.5.1-setup.exe"), None);
    }
}
