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
use carla_core::library::sync::{self, Agreed, Attempt, Folder, Way};
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

/// How long a sync waits for the one before it. A sync that talks to a desktop
/// that went away can take minutes; a game about to start must not wait for it.
const BUSY_WAIT: Duration = Duration::from_secs(20);

/// Said to the player when a game cannot start because the saves of the last
/// one are still being fetched into the folder it would use.
const STILL_SYNCING: &str = "Saves are still being brought in step. Try again in a moment.";

/// Take the turn on `lock`, waiting at most `wait`. `None` when another sync still has it.
fn take_turn<'a>(lock: &'a Mutex<()>, what: &str, wait: Duration) -> Option<std::sync::MutexGuard<'a, ()>> {
    let started = std::time::Instant::now();
    let turn = carla_core::link::deadline::lock_within(lock, wait);
    let ms = started.elapsed().as_millis();
    match &turn {
        Some(_) if ms >= 1000 => crate::conn_log::line(&format!("save sync ({what}) waited {ms} ms for the one before it")),
        None => crate::conn_log::line(&format!("save sync ({what}) gave up after {ms} ms: the one before it is still busy")),
        _ => {}
    }
    turn
}

fn one_at_a_time(what: &str, wait: Duration) -> Option<std::sync::MutexGuard<'static, ()>> {
    take_turn(&BUSY, what, wait)
}

/// Whether a game here may start now: only when its saves were brought in step
/// (or there was nothing to do). A fetch from a desktop still running in the
/// same folder would write the files the game is using.
pub(crate) fn launch_may_start(synced: bool) -> Result<(), String> {
    if synced {
        Ok(())
    } else {
        Err(STILL_SYNCING.to_string())
    }
}

/// Said back to a copy that must stop because a game has started here.
const GAME_STARTED: &str = "A game is still running here, so the saves are left alone.";

/// This Ally's save folder, handed to a copy that a game may overtake. A game
/// that has started here is looked for ONLY before the copy begins (at
/// `protect`, which comes before anything changes): then nothing is touched.
/// Once a copy has begun it always finishes, because a half-copied folder looks
/// like a change on both sides and would be pushed over the desktop's real saves.
/// The game's own start waits for this copy (or is refused), so it need not stop.
struct Yielding<'a> {
    inner: &'a dyn sync::Side,
    game_started: &'a dyn Fn() -> bool,
}

impl sync::Side for Yielding<'_> {
    fn manifest(&self) -> Result<sync::Manifest, String> {
        self.inner.manifest()
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.inner.read(path)
    }

    fn write(&self, path: &str, contents: &[u8], modified: i64) -> Result<(), String> {
        self.inner.write(path, contents, modified)
    }

    fn remove(&self, path: &str) -> Result<(), String> {
        self.inner.remove(path)
    }

    fn protect(&self) -> Result<(), String> {
        if (self.game_started)() {
            return Err(GAME_STARTED.to_string());
        }
        self.inner.protect()
    }
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
///
/// With `yielding`, nothing is written to this Ally's folder once a game has
/// started here (see `Yielding`): for the syncs that run after a game, which a
/// new game may overtake.
fn with(library: &Library, game_id: &str, title: &str, folder: &PathBuf, host: &HostRow, link: &Link, after_stream: bool, yielding: bool) -> bool {
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
    let game_started = || locked(&library.0.active).is_some();
    let yield_to_game = Yielding { inner: &here, game_started: &game_started };
    let here_side: &dyn sync::Side = if yielding { &yield_to_game } else { &here };
    match sync::bring_in_step_for(remote.keep_clean, here_side, &remote, agreed.as_ref()) {
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
///
/// Says whether it is safe to go on: `true` when the saves were brought in step
/// or there was nothing to do, `false` when another sync still had the turn
/// after the wait and this one did not run. A game about to start here must
/// not start then (see `launch_may_start`); after a game, nobody is told.
pub(crate) fn all(library: &Library, game_id: &str, launching: bool) -> bool {
    let Some((folder, title)) = folder(library, game_id) else { return true };
    let Some(_one_at_a_time) = one_at_a_time("all", BUSY_WAIT) else { return false };
    if !launching && locked(&library.0.active).is_some() {
        return true;
    }
    let hosts = library.store().hosts().unwrap_or_default();
    // One after the other would be simpler, but a desktop that is off takes
    // seconds to not answer. The copies themselves still go one at a time.
    let reached: Vec<_> = hosts
        .into_iter()
        .map(|host| std::thread::spawn(move || crate::hosts::reach(&host).ok().map(|(link, ..)| (host, link))))
        .collect();
    for (host, link) in reached.into_iter().filter_map(|t| t.join().ok().flatten()) {
        with(library, game_id, &title, &folder, &host, &link, false, !launching);
    }
    true
}

/// With the desktop a stream is about to start from, so it has what was
/// played on the Ally since.
///
/// Says no when the desktop clears its saves after streams, the Ally has
/// saves, and they could not be shown to be on the desktop now: the stream
/// would start from nothing and its new saves would replace the real ones.
pub(crate) fn before_stream(library: &Library, game_id: &str, host: &HostRow, link: &Link) -> Result<(), String> {
    let Some((folder, title)) = folder(library, game_id) else { return Ok(()) };
    // Waits for the sync that followed the last game, but not for ever: with
    // a desktop that went away that one can take minutes.
    let Some(_one_at_a_time) = one_at_a_time("before_stream", BUSY_WAIT) else {
        return launch_may_start(false);
    };
    with(library, game_id, &title, &folder, host, link, false, false);
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
    let waits = sync::AFTER_STREAM_WAITS;
    let mut wait = Duration::from_secs(waits[0]);
    let mut tries = 0;
    loop {
        std::thread::sleep(wait);
        let outcome = {
            // The turn is let go before the next wait.
            let Some(_one_at_a_time) = one_at_a_time("after_stream", BUSY_WAIT) else { return };
            if locked(&library.0.active).is_some() {
                // A game started here in the meantime: its own start took care of it.
                Attempt::GameHere
            } else {
                match crate::hosts::reach(&host) {
                    Ok((link, ..)) if with(library, game_id, &title, &folder, &host, &link, true, true) => Attempt::TryAgain,
                    Ok(_) => Attempt::InStep,
                    // Off, waking or reconnecting: the schedule goes on.
                    Err(Error::Unreachable) => Attempt::Unreachable,
                    Err(Error::Host(text)) if later(&text) => Attempt::TryAgain,
                    // Refused, not the linked desktop, or a problem of its own.
                    Err(_) => Attempt::Fatal,
                }
            }
        };
        tries += 1;
        match sync::retry_after(outcome, tries, &waits) {
            Some(next) => wait = next,
            None => return,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_does_not_start_while_the_last_games_saves_are_still_being_fetched() {
        assert_eq!(launch_may_start(true), Ok(()));
        let refusal = launch_may_start(false).unwrap_err();
        assert_eq!(refusal, "Saves are still being brought in step. Try again in a moment.");
    }

    #[test]
    fn a_turn_that_stays_taken_is_given_up_on_and_one_let_go_is_had() {
        let lock = Mutex::new(());
        let held = lock.lock().unwrap();
        let started = std::time::Instant::now();
        assert!(take_turn(&lock, "test", Duration::from_millis(120)).is_none(), "gives up: after_stream then returns quietly, a launch is refused");
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(held);
        assert!(take_turn(&lock, "test", Duration::from_millis(120)).is_some());
    }

    #[test]
    fn two_syncs_never_run_at_once() {
        let lock = std::sync::Arc::new(Mutex::new(()));
        let inside = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let overlapped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let runs: Vec<_> = (0..4)
            .map(|_| {
                let (lock, inside, overlapped) = (lock.clone(), inside.clone(), overlapped.clone());
                std::thread::spawn(move || {
                    let Some(_turn) = take_turn(&lock, "test", Duration::from_secs(10)) else { return };
                    if inside.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0 {
                        overlapped.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    std::thread::sleep(Duration::from_millis(30));
                    inside.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                })
            })
            .collect();
        runs.into_iter().for_each(|r| r.join().unwrap());
        assert!(!overlapped.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// A folder that records what was done to it.
    #[derive(Default)]
    struct Fake {
        done: std::cell::RefCell<Vec<String>>,
    }

    impl sync::Side for Fake {
        fn manifest(&self) -> Result<sync::Manifest, String> {
            Ok(sync::Manifest::default())
        }
        fn read(&self, _: &str) -> Result<Vec<u8>, String> {
            Ok(Vec::new())
        }
        fn write(&self, path: &str, _: &[u8], _: i64) -> Result<(), String> {
            self.done.borrow_mut().push(format!("write {path}"));
            Ok(())
        }
        fn remove(&self, path: &str) -> Result<(), String> {
            self.done.borrow_mut().push(format!("remove {path}"));
            Ok(())
        }
        fn protect(&self) -> Result<(), String> {
            self.done.borrow_mut().push("protect".to_string());
            Ok(())
        }
    }

    #[test]
    fn a_copy_that_has_begun_always_finishes_even_if_a_game_starts_meanwhile() {
        use sync::Side;
        let fake = Fake::default();
        let started = std::cell::Cell::new(false);
        let check = || started.get();
        let side = Yielding { inner: &fake, game_started: &check };
        side.protect().unwrap();
        side.write("a", b"", 1).unwrap();
        started.set(true);
        side.write("b", b"", 1).unwrap();
        side.remove("c").unwrap();
        assert_eq!(*fake.done.borrow(), ["protect", "write a", "write b", "remove c"], "never half a copy");
    }

    #[test]
    fn a_copy_that_sees_a_game_before_it_begins_changes_nothing() {
        use sync::Side;
        let fake = Fake::default();
        let check = || true;
        let side = Yielding { inner: &fake, game_started: &check };
        let err = side.protect().unwrap_err();
        assert!(later(&err));
        assert!(fake.done.borrow().is_empty());
    }

    #[test]
    fn a_copy_stopped_by_a_starting_game_is_told_to_try_later() {
        assert!(later(GAME_STARTED), "the fetch counts as to be retried, and after_stream then sees the game and goes");
    }
}
