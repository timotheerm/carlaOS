//! Asking Steam to install a game of the library, and following what Steam is
//! fetching: first installs and updates alike.
//!
//! Steam does all the downloading. CarlaOS only asks, in one of two ways, and
//! then reads Steam's own record of each app (`appmanifest_<id>.acf`) to show
//! how far it is.
//!
//! The quiet way: Steam installs an app by itself when it finds a record for
//! it that says "update started". So CarlaOS writes that record, with the
//! folder Steam's own files name for the game, and has Steam start again so
//! it sees it. Nobody has to answer anything, which matters on a desktop
//! nobody is sitting at.
//!
//! The asking way, when the game's folder is not known or the record cannot
//! be written: Steam is handed its own `steam://install/<id>` address and
//! shows its install window, which someone has to answer.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::owned::App;
use super::steam::{self, Manifest, UPDATE_PAUSED, UPDATE_REQUIRED, UPDATE_RUNNING, UPDATE_STARTED};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Steam knows about it and has not begun.
    Queued,
    Downloading,
    Paused,
    /// Everything is fetched; Steam is putting the files in place.
    Finishing,
}

/// Something a store is fetching on one machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    /// "steam" for now.
    pub source: String,
    pub source_id: String,
    pub title: String,
    pub phase: Phase,
    /// Bytes fetched, and bytes to fetch. Both 0 until Steam has worked out
    /// how much there is.
    pub done: u64,
    pub total: u64,
    /// An installed game being brought up to date, not a first install.
    pub update: bool,
}

/// What one manifest says, with `fetched` giving the bytes waiting in
/// Steam's download folder for the app.
pub fn of(manifest: &Manifest, fetched: impl FnOnce() -> u64) -> Option<Download> {
    let flags = manifest.flags;
    if flags & (UPDATE_REQUIRED | UPDATE_RUNNING | UPDATE_PAUSED | UPDATE_STARTED) == 0 || manifest.tool() {
        return None;
    }
    let total = manifest.bytes_to_download;
    let mut done = manifest.bytes_downloaded.min(total);
    // Steam writes its record only now and then. The files it has fetched so
    // far say more, measured against how much there will be once unpacked.
    if total > 0 && manifest.bytes_to_stage > 0 && done < total {
        let part = fetched().min(manifest.bytes_to_stage) as u128 * total as u128 / manifest.bytes_to_stage as u128;
        done = done.max(part as u64);
    }
    let phase = if flags & UPDATE_PAUSED != 0 {
        Phase::Paused
    } else if flags & (UPDATE_RUNNING | UPDATE_STARTED) == 0 || total == 0 {
        Phase::Queued
    } else if done >= total {
        Phase::Finishing
    } else {
        Phase::Downloading
    };
    Some(Download {
        source: "steam".to_string(),
        source_id: manifest.appid.clone(),
        title: manifest.name.clone(),
        phase,
        done,
        total,
        update: manifest.last_updated > 0,
    })
}

/// The size of everything in a folder. Gives up counting in a folder with
/// an absurd number of files; the answer is then a little low.
fn folder_bytes(dir: &Path) -> u64 {
    let mut left = 200_000u32;
    let mut waiting = vec![dir.to_path_buf()];
    let mut bytes = 0;
    while let Some(dir) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if left == 0 {
                return bytes;
            }
            left -= 1;
            match entry.metadata() {
                Ok(m) if m.is_dir() => waiting.push(entry.path()),
                Ok(m) => bytes += m.len(),
                Err(_) => {}
            }
        }
    }
    bytes
}

/// Everything Steam is fetching or has been told to fetch on this machine.
pub fn steam(root: &Path) -> Vec<Download> {
    let mut list: Vec<Download> = steam::manifests(root).iter().filter_map(|m| of(m, || folder_bytes(&m.apps.join("downloading").join(&m.appid)))).collect();
    list.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    list
}

/// Free bytes on the disk a folder is on.
pub fn free_bytes(folder: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list().ok()?;
    let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
    let wanted = norm(folder);
    disks
        .iter()
        .filter(|d| wanted.starts_with(norm(d.mount_point()).trim_end_matches('/')))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// The library folder a new game goes to: the one with the most room.
pub fn roomiest(root: &Path) -> (PathBuf, Option<u64>) {
    let folders = steam::folders(root).into_iter().filter(|f| f.join("steamapps").is_dir()).map(|f| {
        let free = free_bytes(&f);
        (f, free)
    });
    // The first wins a tie, which is Steam's own folder.
    folders.rev().max_by_key(|(_, free)| *free).unwrap_or_else(|| (root.to_path_buf(), None))
}

/// How an install was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Started {
    /// Steam fetches it by itself.
    Quiet,
    /// Steam was already fetching it, or had been told to.
    Already,
    /// Steam is showing its install window and waits for an answer there.
    AskedSteam,
}

fn record(app: &App) -> String {
    let quoted = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "\"AppState\"\n{{\n\t\"appid\"\t\t\"{}\"\n\t\"Universe\"\t\t\"1\"\n\t\"name\"\t\t\"{}\"\n\t\"StateFlags\"\t\t\"{}\"\n\t\"installdir\"\t\t\"{}\"\n}}\n",
        app.appid,
        quoted(&app.title),
        UPDATE_REQUIRED | UPDATE_STARTED,
        quoted(&app.installdir),
    )
}

/// What starting an install needs from the machine, so it can be tried
/// without Steam.
pub struct Machine<'a> {
    /// Have Steam start again (or start), so it reads its records afresh.
    pub restart: &'a dyn Fn() -> io::Result<()>,
    /// Hand an address to Steam.
    pub open: &'a dyn Fn(&str) -> io::Result<()>,
}

/// Ask Steam to install a game of the library.
pub fn start(root: &Path, app: &App, machine: &Machine) -> Result<Started, String> {
    let problem = |e: io::Error| format!("Steam could not be asked to install {}: {e}.", app.title);
    if app.appid.is_empty() || app.appid.len() > 12 || !app.appid.bytes().all(|b| b.is_ascii_digit()) {
        return Err("That is not a game from the Steam library.".to_string());
    }
    if let Some(known) = steam::manifests(root).into_iter().find(|m| m.appid == app.appid) {
        if known.installed() && of(&known, || 0).is_none() {
            return Err(format!("{} is already installed.", app.title));
        }
        // Steam only fetches while it is running.
        (machine.restart)().map_err(problem)?;
        return Ok(Started::Already);
    }
    let ask = || (machine.open)(&format!("steam://install/{}", app.appid)).map(|()| Started::AskedSteam).map_err(problem);
    if app.installdir.is_empty() {
        return ask();
    }
    let (folder, _) = roomiest(root);
    let path = folder.join("steamapps").join(format!("appmanifest_{}.acf", app.appid));
    // Never over a record Steam wrote in the meantime.
    let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).and_then(|mut file| io::Write::write_all(&mut file, record(app).as_bytes()));
    if written.is_err() {
        let _ = std::fs::remove_file(&path);
        return ask();
    }
    (machine.restart)().map_err(problem)?;
    Ok(Started::Quiet)
}

fn steam_running() -> bool {
    let Ok(mut system) = sysinfo::System::new() else { return false };
    system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, sysinfo::ProcessRefreshKind::nothing());
    let client = if cfg!(windows) { "steam.exe" } else { "steam" };
    system.processes().values().any(|p| p.name().to_string_lossy().eq_ignore_ascii_case(client))
}

/// Whether having Steam read its records afresh means closing it first.
pub fn restart_closes_steam() -> bool {
    steam_running()
}

/// Close Steam if it is open, and start it again in the background.
pub fn restart_steam(root: &Path) -> io::Result<()> {
    let client = steam::client(root);
    if steam_running() {
        Command::new(&client).arg("-shutdown").spawn()?;
        let asked = Instant::now();
        while steam_running() && asked.elapsed() < Duration::from_secs(40) {
            std::thread::sleep(Duration::from_millis(500));
        }
        // Steam lets go of its files a moment after its window is gone.
        std::thread::sleep(Duration::from_secs(1));
    }
    Command::new(&client).arg("-silent").spawn().map(|_| ())
}

/// Start Steam in the background if it is not running, so it carries on
/// with what it was fetching.
pub fn keep_steam_running(root: &Path) -> io::Result<()> {
    if steam_running() {
        return Ok(());
    }
    Command::new(steam::client(root)).arg("-silent").spawn().map(|_| ())
}

/// The real machine: Steam is restarted, and addresses go to Steam itself.
pub fn start_here(root: &Path, app: &App) -> Result<Started, String> {
    let restart = || restart_steam(root);
    let keep = || keep_steam_running(root);
    let open = |address: &str| Command::new(steam::client(root)).arg(address).spawn().map(|_| ());
    // A game Steam already knows about needs no fresh start, only Steam.
    let known = steam::manifests(root).iter().any(|m| m.appid == app.appid);
    start(root, app, &Machine { restart: if known { &keep } else { &restart }, open: &open })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn manifest(flags: u32, to_download: u64, downloaded: u64, to_stage: u64) -> Manifest {
        Manifest { appid: "620".into(), name: "Portal 2".into(), installdir: "Portal 2".into(), flags, bytes_to_download: to_download, bytes_downloaded: downloaded, bytes_to_stage: to_stage, ..Manifest::default() }
    }

    #[test]
    fn steams_record_says_how_far_a_download_is() {
        assert_eq!(of(&manifest(4, 0, 0, 0), || 0), None, "installed and up to date");
        assert_eq!(of(&manifest(0, 0, 0, 0), || 0), None);
        let told = of(&manifest(1026, 0, 0, 0), || 0).unwrap();
        assert_eq!((told.phase, told.done, told.total, told.update), (Phase::Queued, 0, 0, false));
        // The record lags; the files fetched so far are counted against the unpacked size.
        let going = of(&manifest(1026, 1000, 100, 4000), || 2000).unwrap();
        assert_eq!((going.phase, going.done, going.total), (Phase::Downloading, 500, 1000));
        let recorded = of(&manifest(1026, 1000, 700, 4000), || 2000).unwrap();
        assert_eq!(recorded.done, 700, "whichever is further");
        assert_eq!(of(&manifest(1538, 1000, 300, 0), || 0).unwrap().phase, Phase::Paused);
        assert_eq!(of(&manifest(1026, 1000, 1000, 4000), || 0).unwrap().phase, Phase::Finishing);
        // An installed game with an update that has not begun.
        let update = of(&Manifest { last_updated: 1_700_000_000, ..manifest(6, 500, 0, 0) }, || 0).unwrap();
        assert_eq!((update.phase, update.update), (Phase::Queued, true));
        assert_eq!(of(&Manifest { name: "Steamworks Common Redistributables".into(), ..manifest(1026, 10, 0, 0) }, || 0), None);
    }

    #[test]
    fn an_install_is_started_quietly_when_the_folder_is_known_and_by_asking_steam_otherwise() {
        let root = std::env::temp_dir().join(format!("carla-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("steamapps").join("downloading").join("620")).unwrap();
        let calls = RefCell::new(Vec::new());
        let restart = || {
            calls.borrow_mut().push("restart".to_string());
            Ok(())
        };
        let open = |address: &str| {
            calls.borrow_mut().push(address.to_string());
            Ok(())
        };
        let machine = Machine { restart: &restart, open: &open };
        let portal = App { appid: "620".into(), title: "Portal \"2\"".into(), installdir: "Portal 2".into() };

        assert_eq!(start(&root, &portal, &machine), Ok(Started::Quiet));
        let written = steam::manifests(&root);
        assert_eq!((written[0].appid.as_str(), written[0].name.as_str(), written[0].installdir.as_str(), written[0].flags), ("620", "Portal \"2\"", "Portal 2", 1026));
        std::fs::write(root.join("steamapps").join("downloading").join("620").join("part"), vec![0u8; 10]).unwrap();
        assert_eq!(steam(&root).iter().map(|d| (d.source_id.as_str(), d.phase)).collect::<Vec<_>>(), [("620", Phase::Queued)]);

        // Asked twice: Steam's record is left as it is.
        assert_eq!(start(&root, &portal, &machine), Ok(Started::Already));
        // A game whose folder Steam's files do not name goes through Steam's own window.
        assert_eq!(start(&root, &App { appid: "400".into(), title: "Portal".into(), installdir: String::new() }, &machine), Ok(Started::AskedSteam));
        assert_eq!(steam::manifests(&root).len(), 1);
        assert_eq!(*calls.borrow(), ["restart", "restart", "steam://install/400"]);

        assert!(start(&root, &App { appid: "62 0".into(), ..portal.clone() }, &machine).is_err());
        std::fs::write(root.join("steamapps").join("appmanifest_620.acf"), "\"AppState\"\n{\n\"appid\" \"620\"\n\"name\" \"Portal 2\"\n\"StateFlags\" \"4\"\n\"installdir\" \"Portal 2\"\n}\n").unwrap();
        assert_eq!(start(&root, &portal, &machine), Err("Portal \"2\" is already installed.".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
