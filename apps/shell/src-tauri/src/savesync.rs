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
    let folder = store.save_folder_of(game_id).ok().flatten().filter(|f| f.is_dir())?;
    // A console's saves are named by the console, which a desktop names the same way.
    let title = match store.save_title_of(game_id).ok().flatten() {
        Some(console) => console,
        None => store.install(game_id, LOCAL).ok()?.title,
    };
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

/// The sync just verified that both sides hold the same saves (`agreed` is
/// their shared digest). Describe this Ally's own folder afresh and offer it
/// to the desktop that was just streamed from, which removes its live saves
/// only when it was told to and its files match exactly. A desktop that keeps
/// them is not an error, and nothing is ever retried. Returns a line for the
/// game's page, or nothing when there is nothing to say.
fn clear_desktop(library: &Library, game_id: &str, host: &HostRow, folder: &PathBuf, remote: &carla_core::link::client::RemoteSaves<'_>, agreed: &str) -> Option<String> {
    use carla_core::link::client::RetireAnswer;
    // A game started here meanwhile: not a moment to judge anything.
    if locked(&library.0.active).is_some() {
        return None;
    }
    let mine = sync::manifest(folder).ok().filter(|m| !m.files.is_empty() && m.digest() == agreed)?;
    // What happened goes to the crash trail; only a real failure is shown
    // on the game's page (a success there would look like an error).
    match remote.retire(&mine) {
        Ok(RetireAnswer::Done(done)) => {
            remember(library, &host.id, game_id, true);
            crate::crash::note(&format!("saves of {game_id} cleared from desktop {}: {} files, {} left in place, safe copy {}", host.id, done.files, done.skipped, done.backup));
            None
        }
        // The usual case: the administrator never asked for it.
        Ok(RetireAnswer::NotFlagged | RetireAnswer::Unsupported) => None,
        Ok(RetireAnswer::Kept(why)) => {
            crate::crash::note(&format!("saves of {game_id} kept on desktop {}: {why}", host.id));
            None
        }
        Err(e) => Some(format!("Kept on the desktop: {e}")),
    }
}

/// Keep what a desktop last said about clearing this game's live saves, in
/// the database: it must survive a restart, so that a stream can still be held
/// back when the desktop cannot be asked.
fn remember(library: &Library, host: &str, game_id: &str, cleans: bool) {
    let _ = library.store().set_keep_clean(game_id, host, cleans);
}

/// What the desktop last said (`false` when it never did).
fn cleans(library: &Library, host: &str, game_id: &str) -> bool {
    library.store().keep_clean(game_id, host).ok().flatten().unwrap_or(false)
}

/// Bring this game's saves in step with one desktop that answered. Says
/// whether it is worth trying again soon.
///
/// With `after_stream`, once the saves are verified to be in step the desktop
/// is also offered proof so it can clear its own copy, if its administrator
/// asked it to (see `clear_desktop`).
fn with(library: &Library, game_id: &str, title: &str, folder: &PathBuf, host: &HostRow, link: &Link, after_stream: bool) -> bool {
    let record = |row: Synced| {
        let _ = library.store().set_synced(game_id, &row);
    };
    let failed = |text: String| Synced { host_id: host.id.clone(), digest: None, at: now(), outcome: "problem".into(), both_changed: false, files: 0, problem: Some(text) };
    // Where the saves are known to be, so a desktop with no folder yet can
    // suggest its own. Only a hint: it syncs nothing by itself.
    let known_place = library.store().save_place(game_id).ok().flatten();
    let remote = match link.saves_placed(game_id, title, known_place.as_ref()) {
        Ok(Some(remote)) => {
            // Learnt only from a desktop that answered a signed request. The
            // first desktop to say it is believed; only it may change it.
            if let Some(place) = &remote.place {
                let _ = library.store().learn_save_place(game_id, &host.id, place);
            }
            remember(library, &host.id, game_id, remote.keep_clean);
            remote
        }
        Ok(None) => {
            // Its administrator has not named a folder there, or took it back.
            let _ = library.store().clear_synced(game_id, &host.id);
            remember(library, &host.id, game_id, false);
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
    match sync::bring_in_step_for(remote.keep_clean, &here, &remote, agreed.as_ref()) {
        Ok(done) => {
            let outcome = match done.way {
                Way::Same => "same",
                Way::Send => "sent",
                Way::Fetch => "fetched",
            };
            let row = Synced {
                host_id: host.id.clone(),
                digest: Some(done.agreed.here.clone()),
                at: now(),
                outcome: outcome.into(),
                both_changed: done.both_changed,
                files: (done.moved.files + done.moved.removed) as u32,
                problem: None,
            };
            let note = if after_stream { clear_desktop(library, game_id, host, folder, &remote, &done.agreed.here) } else { None };
            record(Synced { problem: note, ..row });
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
        with(library, game_id, &title, &folder, &host, &link, false);
    }
}

/// With the desktop a stream is about to start from, so it has what was
/// played on the Ally since.
///
/// Says no when the desktop clears its saves after streams, the Ally has
/// saves, and they could not be shown to be on the desktop now: the stream
/// would start from nothing and its new saves would replace the real ones.
pub(crate) fn before_stream(library: &Library, game_id: &str, host: &HostRow, link: &Link) -> Result<(), String> {
    let Some((folder, title)) = folder(library, game_id) else { return Ok(()) };
    let _one_at_a_time = locked(&BUSY);
    with(library, game_id, &title, &folder, host, link, false);
    // Asked again afterwards: what the desktop holds now, not before the copy.
    let known_place = library.store().save_place(game_id).ok().flatten();
    let (cleans_now, theirs) = match link.saves_placed(game_id, &title, known_place.as_ref()) {
        Ok(Some(now)) => {
            remember(library, &host.id, game_id, now.keep_clean);
            (now.keep_clean, sync::Side::manifest(&now).ok())
        }
        Ok(None) => {
            remember(library, &host.id, game_id, false);
            return Ok(());
        }
        // Could not ask (off, refused, timed out): go by what it last said,
        // which is kept in the database and so survives a restart.
        Err(_) => {
            let ally_has_saves = sync::manifest(&folder).map(|m| !m.files.is_empty()).unwrap_or(true);
            if !sync::gate_on_failure(cleans(library, &host.id, game_id), ally_has_saves) {
                return Ok(());
            }
            (true, None)
        }
    };
    if !cleans_now {
        return Ok(());
    }
    let verdict = match sync::manifest(&folder) {
        Ok(ours) => sync::may_stream(true, &ours, theirs.as_ref()),
        // Cannot even read the Ally's folder: nothing is known to be on the desktop.
        Err(_) => Err(sync::NOT_ON_DESKTOP.to_string()),
    };
    if verdict.is_err() {
        crate::crash::note(&format!("stream from {} held back: saves of {game_id} are not verified on the desktop that clears its saves", host.id));
    }
    verdict
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
            Ok((link, ..)) => with(library, game_id, &title, &folder, &host, &link, true),
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
