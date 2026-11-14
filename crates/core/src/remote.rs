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

use crate::library::{cloud, ports, saves, Kind, ManualGame, Store, LOCAL};
use crate::link::quality::Setting;
use crate::power;

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
pub const OPS: &[&str] = &["status", "games", "rescan", "add_game", "remove_game", "save_folder", "desktops", "cleanup_scan", "cleanup_apply", "cleanup_undo", "update", "restart", "startup", "picture", "art_find", "art_set", "art_clear", "art_key", "add_cloud", "power", "check"];

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
                    "desktops": g.places.iter().filter(|p| *p != LOCAL && *p != cloud::CLOUD).count(),
                    "cloud": g.places.iter().any(|p| p == cloud::CLOUD),
                    "minutes": g.playtime_minutes, "added_by_hand": g.manual, "save_folder": save_folder,
                    "origin": g.origin, "picture": if g.own_art { "chosen" } else if g.cover.is_some() || g.hero.is_some() { "found" } else { "none" },
                }));
            }
            Ok(Value::Array(out))
        }),
        "add_game" => (|| {
            let chosen = PathBuf::from(text(args, "exe")?);
            if !chosen.is_absolute() || !chosen.is_file() {
                return Err(format!("{} is not a program on the Ally. Give its full path there.", chosen.display()));
            }
            // A shortcut is followed to its program, and a port CarlaOS knows
            // gets the console it is from.
            let seen = ports::inspect(&chosen)?;
            let kind = match args["kind"].as_str() {
                None => seen.kind,
                Some("game") => Kind::Game,
                Some("port") => Kind::Port,
                Some("homebrew") => Kind::Homebrew,
                Some(other) => return Err(format!("\"{other}\" is not a kind of game. Use game, port or homebrew.")),
            };
            let game = ManualGame {
                title: text(args, "title")?,
                exe: seen.exe,
                args: args["args"].as_str().map(str::to_string).unwrap_or(seen.args),
                kind,
                cover: None,
                description: args["description"].as_str().unwrap_or_default().to_string(),
                origin: args["origin"].as_str().map(str::to_string).unwrap_or(if kind == Kind::Port { seen.origin } else { String::new() }),
            };
            let id = store.add_manual(&game, now)?;
            Ok(json!({ "id": id, "title": game.title, "kind": kind, "origin": game.origin.trim(), "port": seen.port }))
        })(),
        "add_cloud" => (|| {
            let link = text(args, "link")?;
            let typed = args["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
            let title = typed.unwrap_or_else(|| cloud::suggest_title(&link));
            if title.is_empty() {
                return Err("The link does not say the game's name. Give it too: carla-companion ally cloud LINK \"NAME\"".to_string());
            }
            let id = store.add_cloud(&link, &title, now)?;
            let (service, _) = cloud::checked(&link)?;
            Ok(json!({ "id": id, "title": title, "service": service.name }))
        })(),
        "remove_game" => (|| {
            let id = text(args, "id")?;
            let title = store.list()?.into_iter().find(|g| g.id == id && g.manual).map(|g| g.title);
            let title = title.ok_or("No game added by hand or cloud game on the Ally has that id. Games from a store leave when they are uninstalled.")?;
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
                let (setting, streamed) = store.host_quality(&h.id)?;
                out.push(json!({
                    "id": h.id, "name": h.name, "address": h.address, "last_seen": h.last_seen, "may_administer": store.host_admin(&h.id)?,
                    "picture": setting.name(), "streamed": streamed,
                }));
            }
            Ok(Value::Array(out))
        }),
        "picture" => (|| {
            let wanted = text(args, "desktop")?;
            let hosts = store.hosts()?;
            let host = hosts.iter().find(|h| h.id == wanted || h.name.eq_ignore_ascii_case(&wanted)).ok_or_else(|| format!("The Ally is not linked to a desktop called \"{wanted}\"."))?;
            if let Some(name) = args["picture"].as_str() {
                let setting = Setting::parse(name).ok_or_else(|| format!("\"{name}\" is not a picture. Use one of: {}.", Setting::all().iter().map(|s| s.name()).collect::<Vec<_>>().join(", ")))?;
                store.set_host_quality(&host.id, setting)?;
            }
            let (setting, streamed) = store.host_quality(&host.id)?;
            Ok(json!({ "id": host.id, "name": host.name, "picture": setting.name(), "streamed": streamed }))
        })(),
        "power" => (|| {
            let id = text(args, "id")?;
            let title = store.list()?.into_iter().find(|g| g.id == id).map(|g| g.title).ok_or("The Ally has no game with that id.")?;
            let kind = match args["kind"].as_str() {
                None | Some("local") => power::Kind::Local,
                Some("stream") => power::Kind::Stream,
                Some("cloud") => power::Kind::Cloud,
                Some(other) => return Err(format!("\"{other}\" is not a way to play. Use local, stream or cloud.")),
            };
            if args["forget"].as_bool() == Some(true) {
                power::choose(store, &id, kind, None)?;
            } else if args["power"].is_string() || args["screen"].is_string() {
                let mut profile = power::profile(store, &id, kind)?;
                if let Some(name) = args["power"].as_str() {
                    profile.power = power::Power::parse(name).ok_or_else(|| format!("\"{name}\" is not a power level. Use quiet, balanced or turbo."))?;
                }
                if let Some(name) = args["screen"].as_str() {
                    profile.screen = power::Screen::parse(name).ok_or_else(|| format!("\"{name}\" is not a screen speed. Use smooth or saver."))?;
                }
                power::choose(store, &id, kind, Some(profile))?;
            }
            let profile = power::profile(store, &id, kind)?;
            let chosen = power::chosen(store, &id, kind)?.is_some();
            Ok(json!({ "id": id, "title": title, "power": profile.power, "screen": profile.screen, "chosen": chosen }))
        })(),
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

        assert_eq!(games[0]["picture"].as_str(), Some("none"));
        // A port CarlaOS knows by its program is filed as one, with its console.
        let soh = dir.join("soh.exe");
        std::fs::write(&soh, b"program").unwrap();
        let port = ask("add_game", json!({ "title": "Ocarina of Time", "exe": soh })).unwrap();
        assert_eq!((port["kind"].as_str(), port["origin"].as_str(), port["port"].as_str()), (Some("port"), Some("Nintendo 64"), Some("Ship of Harkinian")));
        ask("remove_game", json!({ "id": port["id"] })).unwrap();

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
        // A cloud game, named by its link or by hand.
        assert!(ask("add_cloud", json!({ "link": "https://play.geforcenow.com/mall/#/deeplink?game-id=1" })).is_err(), "no name in that link");
        let forza = ask("add_cloud", json!({ "link": "https://www.xbox.com/en-US/play/games/forza-horizon-5/9NKX70BBCDRN" })).unwrap();
        assert_eq!((forza["title"].as_str(), forza["service"].as_str()), (Some("Forza Horizon 5"), Some("Xbox Cloud Gaming")));
        assert_eq!(ask("games", Value::Null).unwrap()[0]["cloud"].as_bool(), Some(true));
        // Its power level, chosen from the desktop.
        let set = ask("power", json!({ "id": forza["id"], "kind": "cloud", "power": "turbo" })).unwrap();
        assert_eq!((set["power"].as_str(), set["screen"].as_str(), set["chosen"].as_bool()), (Some("turbo"), Some("smooth"), Some(true)));
        assert!(ask("power", json!({ "id": forza["id"], "power": "max" })).is_err());
        assert_eq!(ask("power", json!({ "id": forza["id"], "kind": "cloud", "forget": true })).unwrap()["power"].as_str(), Some("quiet"));
        assert!(ask("power", json!({ "id": "steam:404" })).is_err());
        assert!(ask("remove_game", json!({ "id": forza["id"] })).is_ok());
        assert!(ask("picture", json!({ "desktop": "HOME PC" })).is_err(), "not linked to it");

        // What needs more than the catalogue is left to the shell.
        assert!(catalogue(&mut store, "restart", &Value::Null, 100).is_none());
        assert!(OPS.iter().filter(|op| catalogue(&mut store, op, &Value::Null, 100).is_some()).count() == 8);
        let failed = Done::of("7", Err("No.".into()));
        assert_eq!((failed.ok, failed.error()), (false, Some("No.")));
        assert_eq!(Done::of("7", Ok(json!(1))).error(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
