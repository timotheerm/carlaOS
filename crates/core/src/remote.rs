//! Administering the Ally from a desktop.
//!
//! The administrator types a command at a linked desktop. The Ally, which
//! already talks to that desktop, asks it "anything for me?" and gets the
//! command in the signed reply, carries it out, and sends the result back.
//! The Ally never listens for anyone: nothing has to be opened in its
//! firewall, and it works wherever the Ally can reach the desktop.
//!
//! The Ally only asks desktops its administrator allowed, on the Ally, with
//! the PIN.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::library::{saves, Kind, ManualGame, Store, LOCAL};

/// One thing the administrator asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Command {
    /// Chosen by the desktop, to match the result to the question.
    pub id: String,
    pub op: String,
    #[serde(default)]
    pub args: Value,
}

/// How it went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Done {
    pub id: String,
    pub ok: bool,
    /// What was asked for, or `{"error": "..."}`.
    pub output: Value,
}

impl Done {
    pub fn of(id: &str, result: Result<Value, String>) -> Self {
        match result {
            Ok(output) => Self { id: id.to_string(), ok: true, output },
            Err(error) => Self { id: id.to_string(), ok: false, output: json!({ "error": error }) },
        }
    }

    pub fn error(&self) -> Option<&str> {
        if self.ok {
            None
        } else {
            Some(self.output["error"].as_str().unwrap_or("The Ally could not do that."))
        }
    }
}

/// Everything that can be asked, for the help text and to refuse the rest.
pub const OPS: &[&str] = &["status", "games", "rescan", "add_game", "remove_game", "save_folder", "desktops", "cleanup_scan", "cleanup_apply", "cleanup_undo", "update", "restart"];

fn text(args: &Value, name: &str) -> Result<String, String> {
    args[name].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).ok_or_else(|| format!("The request is missing \"{name}\"."))
}

/// The ids in `args.ids`.
pub fn ids(args: &Value) -> Vec<String> {
    args["ids"].as_array().map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// The requests that only read or change the catalogue. `None` for a request
/// that needs more than the catalogue; the shell answers those itself.
pub fn catalogue(store: &mut Store, op: &str, args: &Value, now: i64) -> Option<Result<Value, String>> {
    Some(match op {
        "games" => store.list().and_then(|games| {
            let mut out = Vec::new();
            for g in games {
                let save_folder = store.save_path(&g.id)?.map(|p| p.to_string_lossy().into_owned());
                out.push(json!({
                    "id": g.id, "title": g.title, "platform": g.platform, "kind": g.kind,
                    "on_ally": g.places.iter().any(|p| p == LOCAL),
                    "desktops": g.places.iter().filter(|p| *p != LOCAL).count(),
                    "minutes": g.playtime_minutes, "added_by_hand": g.manual, "save_folder": save_folder,
                }));
            }
            Ok(Value::Array(out))
        }),
        "add_game" => (|| {
            let exe = PathBuf::from(text(args, "exe")?);
            if !exe.is_absolute() || !exe.is_file() {
                return Err(format!("{} is not a program on the Ally. Give its full path there.", exe.display()));
            }
            let kind = match args["kind"].as_str().unwrap_or("game") {
                "game" => Kind::Game,
                "port" => Kind::Port,
                "homebrew" => Kind::Homebrew,
                other => return Err(format!("\"{other}\" is not a kind of game. Use game, port or homebrew.")),
            };
            let game = ManualGame {
                title: text(args, "title")?,
                exe,
                args: args["args"].as_str().unwrap_or_default().to_string(),
                kind,
                cover: None,
                description: args["description"].as_str().unwrap_or_default().to_string(),
            };
            let id = store.add_manual(&game, now)?;
            Ok(json!({ "id": id, "title": game.title }))
        })(),
        "remove_game" => (|| {
            let id = text(args, "id")?;
            let title = store.list()?.into_iter().find(|g| g.id == id && g.manual).map(|g| g.title);
            let title = title.ok_or("No game added by hand on the Ally has that id. Games from a store leave when they are uninstalled.")?;
            store.remove_manual(&id)?;
            Ok(json!({ "id": id, "title": title }))
        })(),
        "save_folder" => (|| {
            let id = text(args, "id")?;
            let title = store.list()?.into_iter().find(|g| g.id == id).map(|g| g.title).ok_or("The Ally has no game with that id.")?;
            let folder = match args["folder"].as_str().map(str::trim).filter(|f| !f.is_empty()) {
                None => None,
                Some(folder) => {
                    let folder = Path::new(folder);
                    if !folder.is_absolute() || !folder.is_dir() {
                        return Err(format!("{} is not a folder on the Ally. Give its full path there.", folder.display()));
                    }
                    if !saves::sensible(folder) || saves::measure(folder).is_none() {
                        return Err("That folder is too general or too large. Give the folder that holds only this game's saves.".to_string());
                    }
                    Some(folder)
                }
            };
            store.set_save_path(&id, folder)?;
            Ok(json!({ "id": id, "title": title, "folder": folder.map(|f| f.to_string_lossy().into_owned()) }))
        })(),
        "desktops" => store.hosts().and_then(|hosts| {
            let mut out = Vec::new();
            for h in hosts {
                out.push(json!({ "id": h.id, "name": h.name, "address": h.address, "last_seen": h.last_seen, "may_administer": store.host_admin(&h.id)? }));
            }
            Ok(Value::Array(out))
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_can_be_looked_after_from_a_desktop() {
        let dir = std::env::temp_dir().join(format!("carla-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let saves = dir.join("Saved Games").join("Lanterns");
        std::fs::create_dir_all(&saves).unwrap();
        let exe = dir.join("lanterns.exe");
        std::fs::write(&exe, b"program").unwrap();
        let mut store = Store::in_memory().unwrap();
        let mut ask = |op: &str, args: Value| catalogue(&mut store, op, &args, 100).expect("a catalogue request");

        assert_eq!(ask("games", Value::Null).unwrap(), json!([]));
        // A program that is not on the Ally cannot be added from afar.
        assert!(ask("add_game", json!({ "title": "Ghost", "exe": dir.join("missing.exe") })).is_err());
        assert!(ask("add_game", json!({ "title": "Ghost", "exe": "relative.exe" })).is_err());
        assert!(ask("add_game", json!({ "exe": exe })).is_err());
        assert!(ask("add_game", json!({ "title": "Paper Lanterns", "exe": exe, "kind": "emulator" })).is_err());
        let added = ask("add_game", json!({ "title": " Paper Lanterns ", "exe": exe, "args": "-fullscreen", "kind": "homebrew" })).unwrap();
        let id = added["id"].as_str().unwrap().to_string();
        let games = ask("games", Value::Null).unwrap();
        assert_eq!((games[0]["title"].as_str(), games[0]["kind"].as_str(), games[0]["on_ally"].as_bool(), games[0]["added_by_hand"].as_bool()), (Some("Paper Lanterns"), Some("homebrew"), Some(true), Some(true)));

        // Its save folder, which must be a real and sensible one.
        assert!(ask("save_folder", json!({ "id": id, "folder": "/" })).is_err());
        assert!(ask("save_folder", json!({ "id": id, "folder": dir.join("nowhere") })).is_err());
        assert!(ask("save_folder", json!({ "id": "manual:other", "folder": saves })).is_err());
        ask("save_folder", json!({ "id": id, "folder": saves })).unwrap();
        assert_eq!(ask("games", Value::Null).unwrap()[0]["save_folder"].as_str(), Some(saves.to_string_lossy().as_ref()));
        assert_eq!(ask("save_folder", json!({ "id": id })).unwrap()["folder"], Value::Null);

        assert!(ask("remove_game", json!({ "id": "steam:10" })).is_err());
        assert_eq!(ask("remove_game", json!({ "id": id })).unwrap()["title"].as_str(), Some("Paper Lanterns"));
        assert_eq!(ask("games", Value::Null).unwrap(), json!([]));
        assert_eq!(ask("desktops", Value::Null).unwrap(), json!([]));

        // What needs more than the catalogue is left to the shell.
        assert!(catalogue(&mut store, "restart", &Value::Null, 100).is_none());
        assert!(OPS.iter().filter(|op| catalogue(&mut store, op, &Value::Null, 100).is_some()).count() == 5);
        let failed = Done::of("7", Err("No.".into()));
        assert_eq!((failed.ok, failed.error()), (false, Some("No.")));
        assert_eq!(Done::of("7", Ok(json!(1))).error(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
