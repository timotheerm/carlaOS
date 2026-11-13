//! Keeping a game's saves the same on this Ally and on the desktops, so a
//! game continues where it stopped whichever machine ran it last.
//!
//! Only for games whose save folder the administrator named on both
//! machines. Everything here is quiet: saves that could not be brought in
//! step must never get in the way of playing. What happened is remembered
//! and shown on the game's page.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use carla_core::library::saves::Backups;
use carla_core::library::sync::{self, Agreed, Folder, Way};
use carla_core::library::{HostRow, Synced, LOCAL};
use carla_core::link::client::{Error, Link};
use serde::Serialize;
use tauri::State;

use crate::library::{locked, now, Library};

/// One sync at a time: two at once could copy the same folder both ways.
static BUSY: Mutex<()> = Mutex::new(());

/// One desktop's line on the game's page.
#[derive(Debug, Clone, Serialize)]
pub struct SyncView {
    /// The desktop's name.
    host: String,
    at: i64,
    /// "same", "sent", "fetched" or "problem".
    outcome: String,
    both_changed: bool,
    files: u32,
    problem: Option<String>,
}

/// The game's save folder here, when the administrator named it. Steam's own
/// cloud folder is left to Steam.
fn folder(library: &Library, game_id: &str) -> Option<(PathBuf, String)> {
    let store = library.store();
    let folder = store.save_path(game_id).ok().flatten().filter(|f| f.is_dir())?;
    let title = store.install(game_id, LOCAL).ok()?.title;
    Some((folder, title))
}

pub(crate) fn views(library: &Library, game_id: &str) -> Vec<SyncView> {
    if folder(library, game_id).is_none() {
        return Vec::new();
    }
    let store = library.store();
    let hosts = store.hosts().unwrap_or_default();
    let rows = store.synced(game_id).unwrap_or_default();
    rows.into_iter()
        .filter_map(|row| {
            let host = hosts.iter().find(|h| h.id == row.host_id)?.name.clone();
            Some(SyncView { host, at: row.at, outcome: row.outcome, both_changed: row.both_changed, files: row.files, problem: row.problem })
        })
        .collect()
}

/// Whether to try again in a while: the desktop is off, or the game is still
/// open there.
fn later(text: &str) -> bool {
    text.contains("still running")
}

/// Bring this game's saves in step with one desktop that answered. Says
/// whether it is worth trying again soon.
fn with(library: &Library, game_id: &str, title: &str, folder: &PathBuf, host: &HostRow, link: &Link) -> bool {
    let record = |row: Synced| {
        let _ = library.store().set_synced(game_id, &row);
    };
    let failed = |text: String| Synced { host_id: host.id.clone(), digest: None, at: now(), outcome: "problem".into(), both_changed: false, files: 0, problem: Some(text) };
    let remote = match link.saves(game_id, title) {
        Ok(Some(remote)) => remote,
        Ok(None) => {
            // Its administrator has not named a folder there, or took it back.
            let _ = library.store().clear_synced(game_id, &host.id);
            return false;
        }
        Err(Error::Unreachable) => return true,
        Err(Error::Host(text)) if later(&text) => return true,
        Err(e) => {
            record(failed(e.to_string()));
            return false;
        }
    };
    let agreed = library.store().synced(game_id).ok().and_then(|rows| rows.into_iter().find(|r| r.host_id == host.id)).and_then(|r| r.digest);
    let agreed = agreed.map(|digest| Agreed { here: digest.clone(), there: digest });
    let here = Folder { folder: folder.clone(), backups: Backups::new(&library.0.data_dir.join("saves"), game_id), now: now() };
    match sync::bring_in_step(&here, &remote, agreed.as_ref()) {
        Ok(done) => {
            let outcome = match done.way {
                Way::Same => "same",
                Way::Send => "sent",
                Way::Fetch => "fetched",
            };
            record(Synced {
                host_id: host.id.clone(),
                digest: Some(done.agreed.here),
                at: now(),
                outcome: outcome.into(),
                both_changed: done.both_changed,
                files: (done.moved.files + done.moved.removed) as u32,
                problem: None,
            });
            false
        }
        Err(text) if later(&text) => true,
        Err(text) => {
            record(failed(text));
            false
        }
    }
}

/// With every desktop that answers, at once. `launching` is for the moment
/// just before the game starts here; at any other time nothing is touched
/// while a game runs.
pub(crate) fn all(library: &Library, game_id: &str, launching: bool) {
    let Some((folder, title)) = folder(library, game_id) else { return };
    let _one_at_a_time = locked(&BUSY);
    if !launching && locked(&library.0.active).is_some() {
        return;
    }
    let hosts = library.store().hosts().unwrap_or_default();
    // One after the other would be simpler, but a desktop that is off takes
    // seconds to not answer. The copies themselves still go one at a time.
    let reached: Vec<_> = hosts
        .into_iter()
        .map(|host| std::thread::spawn(move || crate::hosts::reach(&host).ok().map(|(link, ..)| (host, link))))
        .collect();
    for (host, link) in reached.into_iter().filter_map(|t| t.join().ok().flatten()) {
        with(library, game_id, &title, &folder, &host, &link);
    }
}

/// With the desktop a stream is about to start from, so it has what was
/// played on the Ally since.
pub(crate) fn before_stream(library: &Library, game_id: &str, host: &HostRow, link: &Link) {
    let Some((folder, title)) = folder(library, game_id) else { return };
    let _one_at_a_time = locked(&BUSY);
    with(library, game_id, &title, &folder, host, link);
}

/// After a stream: fetch what was played on the desktop. The game may take a
/// while to close there, so this tries a few times.
pub(crate) fn after_stream(library: &Library, game_id: &str, place: &str) {
    let Some((folder, title)) = folder(library, game_id) else { return };
    let Some(host) = library.store().hosts().ok().and_then(|hosts| hosts.into_iter().find(|h| h.id == place)) else { return };
    for wait in [3, 15, 30, 60] {
        std::thread::sleep(Duration::from_secs(wait));
        let _one_at_a_time = locked(&BUSY);
        // A game started here in the meantime: its own start took care of it.
        if locked(&library.0.active).is_some() {
            return;
        }
        let again = match crate::hosts::reach(&host) {
            Ok((link, ..)) => with(library, game_id, &title, &folder, &host, &link),
            Err(_) => false,
        };
        if !again {
            return;
        }
    }
}

/// Bring a game's saves in step now, and say how it went. Anyone may: it
/// does what starting the game would do.
#[tauri::command(async)]
pub fn saves_sync(library: State<'_, Library>, game_id: String) -> Vec<SyncView> {
    all(&library, &game_id, false);
    views(&library, &game_id)
}
