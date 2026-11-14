//! Being administered from a desktop. See `carla_core::remote`.
//!
//! For every desktop the administrator allowed, the Ally keeps asking that
//! desktop whether there is a request for it, and carries out what comes
//! back. The Ally never listens for connections itself. A desktop that was
//! not allowed is never asked, so nothing it says can reach this code.

use std::collections::HashSet;
use std::io::Write;
use std::sync::Mutex;
use std::time::Duration;

use carla_core::library::{HostRow, LOCAL};
use carla_core::link::client::Link;
use carla_core::remote::{self, Command, Done};
use carla_core::update::{self, Available, Part};
use carla_core::VERSION;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::admin::Admin;
use crate::library::{locked, now, Library};

/// Tells the interface that the catalogue changed behind its back.
const CHANGED_EVENT: &str = "library://changed";

/// The desktops that are being asked right now.
static ASKING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// What is left to do once the desktop has its answer.
enum After {
    Nothing,
    Restart,
    Update(Available),
}

fn playing(library: &Library) -> Option<String> {
    locked(&library.0.active).as_ref().map(|a| a.view.title.clone())
}

fn not_while_playing(library: &Library) -> Result<(), String> {
    match playing(library) {
        Some(title) => Err(format!("{title} is running on the Ally. Try again when it has been closed.")),
        None => Ok(()),
    }
}

fn run(app: &AppHandle, library: &Library, command: &Command) -> (Result<Value, String>, After) {
    let (op, args) = (command.op.as_str(), &command.args);
    let catalogue = remote::catalogue(&mut library.store(), op, args, now());
    if let Some(result) = catalogue {
        if !matches!(op, "games" | "desktops") {
            let _ = app.emit(CHANGED_EVENT, ());
        }
        return (result, After::Nothing);
    }
    let value = |v: Result<Value, serde_json::Error>| v.map_err(|e| e.to_string());
    let admin = app.state::<Admin>();
    let plain = |result| (result, After::Nothing);
    match op {
        "status" => plain((|| {
            let games = library.store().list()?;
            Ok(json!({
                "name": crate::hosts::this_device(),
                "version": VERSION,
                // Asking for updates needs the internet; without it the rest still answers.
                "update": update::check(Part::Ally).ok().flatten().map(|u| u.version),
                "playing": playing(library),
                "battery": crate::device::battery(),
                "games": games.len(),
                "on_ally": games.iter().filter(|g| g.places.iter().any(|p| p == LOCAL)).count(),
                "desktops": library.store().hosts()?.len(),
            }))
        })()),
        "rescan" => {
            let found = library.scan(app).map(|games| json!({ "games": games.len() }));
            let _ = app.emit(CHANGED_EVENT, ());
            plain(found)
        }
        "cleanup_scan" => plain(admin.cleanup_remote(|d| d.scan()).and_then(|r| value(serde_json::to_value(r)))),
        "cleanup_apply" => plain(admin.cleanup_remote(|d| d.apply(&remote::ids(args))).and_then(|r| value(serde_json::to_value(r)))),
        "cleanup_undo" => plain(admin.cleanup_remote(|d| d.undo(&remote::ids(args))).and_then(|r| value(serde_json::to_value(r)))),
        "update" => match not_while_playing(library).and_then(|()| update::check(Part::Ally)) {
            Ok(Some(found)) => (Ok(json!({ "version": found.version })), After::Update(found)),
            Ok(None) => plain(Ok(json!({ "version": null }))),
            Err(e) => plain(Err(e)),
        },
        "startup" => plain((|| {
            if let Some(enabled) = args["enabled"].as_bool() {
                carla_core::boot::set(&app.state::<crate::boot::Boot>().0, enabled)?;
            }
            Ok(json!({ "enabled": carla_core::boot::enabled() }))
        })()),
        "art_find" => plain((|| {
            let id = args["id"].as_str().ok_or("The request is missing \"id\".")?;
            let (title, found) = crate::art::find(library, id, args["term"].as_str().unwrap_or_default())?;
            let key_set = library.store().setting("steamgriddb_key")?.is_some();
            Ok(json!({ "id": id, "title": title, "candidates": found.candidates, "notes": found.notes, "key_set": key_set }))
        })()),
        "art_set" => {
            let done = (|| {
                let id = args["id"].as_str().ok_or("The request is missing \"id\".")?;
                let candidate = serde_json::from_value(args["candidate"].clone()).map_err(|_| "The request does not say which picture.".to_string())?;
                let shape = crate::art::apply(library, id, &candidate)?;
                let title = library.store().list()?.into_iter().find(|g| g.id == id).map(|g| g.title).unwrap_or_default();
                Ok(json!({ "id": id, "title": title, "shape": shape }))
            })();
            crate::library::allow_pictures(app, &library.0.art_dir);
            let _ = app.emit(CHANGED_EVENT, ());
            plain(done)
        }
        "art_clear" => {
            let done = (|| {
                let id = args["id"].as_str().ok_or("The request is missing \"id\".")?;
                let title = library.store().list()?.into_iter().find(|g| g.id == id).map(|g| g.title).ok_or("The Ally has no game with that id.")?;
                crate::art::clear(library, id)?;
                Ok(json!({ "id": id, "title": title }))
            })();
            let _ = app.emit(CHANGED_EVENT, ());
            plain(done)
        }
        "art_key" => plain(crate::art::set_key(library, args["key"].as_str().unwrap_or_default()).map(|kept| json!({ "key_set": kept }))),
        "check" => plain((|| {
            let wanted = args["desktop"].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("The request is missing \"desktop\".")?;
            let repair = args["repair"].as_bool() == Some(true);
            if repair {
                not_while_playing(library)?;
            }
            let hosts = library.store().hosts()?;
            let host = hosts.iter().find(|h| h.id == wanted || h.name.eq_ignore_ascii_case(wanted)).ok_or_else(|| format!("The Ally is not linked to a desktop called \"{wanted}\"."))?;
            let report = crate::hosts::check_host(library, host, repair);
            value(serde_json::to_value(report))
        })()),
        "restart" => match not_while_playing(library) {
            Ok(()) => (Ok(json!({ "restarting": true })), After::Restart),
            Err(e) => plain(Err(e)),
        },
        _ => plain(Err("This version of CarlaOS on the Ally does not know that request. Update it first.".to_string())),
    }
}

/// Keep a short record of what was asked from afar, for the administrator.
fn record(library: &Library, host: &HostRow, command: &Command, ok: bool) {
    let path = library.0.data_dir.join("remote.log");
    // Start afresh now and then, so the record never grows large.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 256 * 1024) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::File::options().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{}", json!({ "at": now(), "desktop": host.name, "asked": command.op, "ok": ok }));
    }
}

fn allowed(library: &Library, id: &str) -> Option<HostRow> {
    let store = library.store();
    store.host_admin(id).ok().filter(|allowed| *allowed)?;
    store.hosts().ok()?.into_iter().find(|h| h.id == id)
}

/// Ask one desktop for requests for as long as it answers and is allowed.
fn serve(app: &AppHandle, library: &Library, host: &HostRow, link: &Link) {
    while allowed(library, &host.id).is_some() {
        let command = match link.admin_next() {
            Ok(Some(command)) => command,
            Ok(None) => continue,
            Err(_) => return,
        };
        let (result, after) = run(app, library, &command);
        let done = Done::of(&command.id, result);
        record(library, host, &command, done.ok);
        // The answer matters to someone waiting at the desktop: try a few times.
        let delivered = (0..3).any(|attempt| {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(2));
            }
            link.admin_done(&done).is_ok()
        });
        match after {
            After::Nothing => {}
            After::Restart => app.restart(),
            After::Update(found) => {
                let installed = update::download(&found, &update::download_dir(), &|_| {}).and_then(|installer| update::install(Part::Ally, &installer));
                if installed.is_ok() {
                    app.exit(0);
                }
            }
        }
        if !delivered {
            return;
        }
    }
}

fn ask(app: AppHandle, library: Library, id: String) {
    std::thread::spawn(move || {
        while let Some(host) = allowed(&library, &id) {
            if let Ok((link, ..)) = crate::hosts::reach(&host) {
                serve(&app, &library, &host, &link);
            }
            // The desktop is off, or stopped answering: look again in a while.
            std::thread::sleep(Duration::from_secs(20));
        }
        if let Some(asking) = locked(&ASKING).as_mut() {
            asking.remove(&id);
        }
    });
}

/// Start asking every allowed desktop that is not being asked yet.
fn refresh(app: &AppHandle, library: &Library) {
    let hosts = library.store().hosts().unwrap_or_default();
    for host in hosts {
        if allowed(library, &host.id).is_none() {
            continue;
        }
        let new = locked(&ASKING).get_or_insert_with(HashSet::new).insert(host.id.clone());
        if new {
            ask(app.clone(), library.clone(), host.id);
        }
    }
}

/// Called once when CarlaOS starts.
pub fn watch(app: AppHandle, library: Library) {
    refresh(&app, &library);
}

/// Whether a desktop may administer this Ally.
#[tauri::command]
pub fn hosts_admin(library: State<'_, Library>, id: String) -> Result<bool, String> {
    library.store().host_admin(&id)
}

/// Allow a desktop to administer this Ally, or take that back. Taking it
/// back holds from the desktop's next question, within about twenty seconds.
#[tauri::command]
pub fn hosts_set_admin(app: AppHandle, admin: State<'_, Admin>, library: State<'_, Library>, id: String, allowed: bool) -> Result<(), String> {
    admin.require()?;
    library.store().set_host_admin(&id, allowed)?;
    refresh(&app, &library);
    Ok(())
}
