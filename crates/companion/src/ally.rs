//! `carla-companion ally ...`: administering a linked Ally from this desktop.
//!
//! The request is left for the Ally, which fetches it through the companion
//! that runs in the background, carries it out and sends back the result.
//! See `carla_core::remote`.

use std::time::{Duration, Instant};

use carla_core::link;
use carla_core::link::quality::{Choice, Setting};
use carla_core::remote::Command;
use serde_json::{json, Value};

use crate::files::{now, Files, ASK_LIFETIME};

pub const HELP: &str = "Administering a linked Ally from this desktop. On the Ally this desktop must
be allowed first: Settings, Desktops, this desktop, \"Administer this Ally\".

  carla-companion ally                  how the Ally is doing
  carla-companion ally games            every game in its library, with ids
  carla-companion ally rescan           look for installed games again
  carla-companion ally add NAME PROGRAM [--args OPTIONS] [--kind game|port|homebrew] [--about TEXT] [--console NAME]
                                        add a program that is on the Ally as a game; a shortcut
                                        is followed, and --console says what a port is from
  carla-companion ally remove ID        take a game added by hand off its library
  carla-companion ally saves ID FOLDER  say where a game keeps its saves on the Ally
  carla-companion ally saves ID --forget
  carla-companion ally desktops         the desktops the Ally is linked to
  carla-companion ally cleanup          what the Windows clean-up found there
  carla-companion ally cleanup apply ID [ID ...]
  carla-companion ally cleanup undo ID [ID ...]
  carla-companion ally update           install the newest CarlaOS on the Ally
  carla-companion ally restart          close CarlaOS on the Ally and start it again
  carla-companion ally startup [on|off] whether CarlaOS opens when the Ally is turned on
  carla-companion ally picture DESKTOP [auto|auto120|sharp|balanced|light|lightest|smooth]
                                        the picture of streams from a desktop to the Ally
  carla-companion ally check DESKTOP [--repair]
                                        check streaming from a desktop step by step, as the
                                        Ally sees it; --repair wakes it, pairs streaming again
                                        or updates it when that is what is wrong
  carla-companion ally art ID [NAME]    look for pictures of a game and show them here,
                                        numbered, in the browser; NAME searches for
                                        something other than the game's own name
  carla-companion ally art use NUMBER   give the game the picture with that number
  carla-companion ally art clear ID     go back to the picture the game had by itself
  carla-companion ally art key KEY      a free SteamGridDB key, for many more pictures
                                        (carla-companion ally art key none forgets it)

With more than one Ally linked, say which first: carla-companion ally --device ID games
(see carla-companion devices).";

/// What to ask and how long the Ally may take over it.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub op: &'static str,
    pub args: Value,
    pub patience: Duration,
}

fn usage<T>() -> Result<T, String> {
    Err(HELP.to_string())
}

/// Turn the words after `ally` into a request.
pub fn request(words: &[String]) -> Result<Request, String> {
    let quick = Duration::from_secs(45);
    let slow = Duration::from_secs(300);
    let plain = |op, patience| Ok(Request { op, args: Value::Null, patience });
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] | ["status"] => plain("status", quick),
        ["games"] => plain("games", quick),
        ["desktops"] => plain("desktops", quick),
        ["rescan"] => plain("rescan", Duration::from_secs(120)),
        ["restart"] => plain("restart", quick),
        ["update"] => plain("update", Duration::from_secs(900)),
        ["startup"] => plain("startup", quick),
        ["startup", way @ ("on" | "off")] => Ok(Request { op: "startup", args: json!({ "enabled": *way == "on" }), patience: quick }),
        ["picture", desktop] => Ok(Request { op: "picture", args: json!({ "desktop": desktop }), patience: quick }),
        ["picture", desktop, picture] if Setting::parse(picture).is_some() => Ok(Request { op: "picture", args: json!({ "desktop": desktop, "picture": picture }), patience: quick }),
        ["check", desktop] => Ok(Request { op: "check", args: json!({ "desktop": desktop }), patience: Duration::from_secs(120) }),
        ["check", desktop, "--repair"] | ["check", "--repair", desktop] => {
            Ok(Request { op: "check", args: json!({ "desktop": desktop, "repair": true }), patience: Duration::from_secs(240) })
        }
        ["cleanup"] => plain("cleanup_scan", slow),
        ["cleanup", way @ ("apply" | "undo"), ids @ ..] if !ids.is_empty() => {
            Ok(Request { op: if *way == "apply" { "cleanup_apply" } else { "cleanup_undo" }, args: json!({ "ids": ids }), patience: slow })
        }
        ["remove", id] => Ok(Request { op: "remove_game", args: json!({ "id": id }), patience: quick }),
        ["saves", id, "--forget"] => Ok(Request { op: "save_folder", args: json!({ "id": id }), patience: quick }),
        ["saves", id, folder] => Ok(Request { op: "save_folder", args: json!({ "id": id, "folder": folder }), patience: quick }),
        ["art", "use", _] | ["art"] => Err("Look for pictures first: carla-companion ally art ID (ids are in carla-companion ally games).".to_string()),
        ["art", "clear", id] => Ok(Request { op: "art_clear", args: json!({ "id": id }), patience: quick }),
        ["art", "key", key] => Ok(Request { op: "art_key", args: json!({ "key": if *key == "none" { "" } else { key } }), patience: quick }),
        ["art", id, name @ ..] => Ok(Request { op: "art_find", args: json!({ "id": id, "term": name.join(" ") }), patience: Duration::from_secs(90) }),
        ["add", title, exe, rest @ ..] if rest.len() % 2 == 0 => {
            let mut args = json!({ "title": title, "exe": exe });
            for pair in rest.chunks(2) {
                let name = match pair[0] {
                    "--args" => "args",
                    "--kind" => "kind",
                    "--about" => "description",
                    "--console" => "origin",
                    _ => return usage(),
                };
                args[name] = json!(pair[1]);
            }
            Ok(Request { op: "add_game", args, patience: quick })
        }
        _ => usage(),
    }
}

fn word(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

fn count(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn outcome(out: &Value, verb: &str) -> String {
    let mut text = format!("{} {verb}.", count(&out["done"]));
    for failed in out["failed"].as_array().into_iter().flatten() {
        text.push_str(&format!("\nNot {verb}: {} ({})", word(&failed["title"]), word(&failed["error"])));
    }
    if out["restart"].as_bool() == Some(true) {
        text.push_str("\nSome of it only takes full effect after the Ally restarts.");
    }
    text
}

/// The Ally's answer, in words.
pub fn show(op: &str, out: &Value) -> String {
    match op {
        "status" => {
            let battery = match out["battery"]["percent"].as_u64() {
                Some(percent) => format!("{percent}%{}", if out["battery"]["charging"].as_bool() == Some(true) { ", charging" } else { "" }),
                None => "no battery".to_string(),
            };
            let update = match out["update"].as_str() {
                Some(version) => format!("{version} is available (carla-companion ally update)"),
                None => "up to date".to_string(),
            };
            [
                format!("Ally:      {}", word(&out["name"])),
                format!("CarlaOS:   {}, {update}", word(&out["version"])),
                format!("Playing:   {}", out["playing"].as_str().unwrap_or("nothing")),
                format!("Battery:   {battery}"),
                format!("Games:     {} ({} installed on the Ally)", out["games"], out["on_ally"]),
                format!("Desktops:  {}", out["desktops"]),
            ]
            .join("\n")
        }
        "games" => {
            let lines: Vec<String> = out
                .as_array()
                .into_iter()
                .flatten()
                .map(|g| {
                    let place = match (g["on_ally"].as_bool() == Some(true), g["desktops"].as_u64().unwrap_or(0)) {
                        (true, 0) => "on the Ally",
                        (true, _) => "on the Ally and streamed",
                        (false, _) => "streamed",
                    };
                    let saves = g["save_folder"].as_str().map(|f| format!("\n    saves: {f}")).unwrap_or_default();
                    format!("{}\n    {} · {} · {place} · {} min played{saves}", word(&g["title"]), word(&g["id"]), word(&g["platform"]), g["minutes"])
                })
                .collect();
            if lines.is_empty() {
                "The Ally's library is empty.".to_string()
            } else {
                lines.join("\n")
            }
        }
        "desktops" => {
            let lines: Vec<String> = out
                .as_array()
                .into_iter()
                .flatten()
                .map(|d| format!("{}  {}  {}  picture: {}{}", word(&d["id"]), word(&d["name"]), word(&d["address"]), word(&d["picture"]), if d["may_administer"].as_bool() == Some(true) { "  (may administer the Ally)" } else { "" }))
                .collect();
            lines.join("\n")
        }
        "picture" => {
            let setting = out["picture"].as_str().and_then(Setting::parse).unwrap_or_default();
            let mut text = format!("Streams from {} to the Ally: {}.", word(&out["name"]), setting.words());
            if let Ok(choice) = serde_json::from_value::<Choice>(out["streamed"]["choice"].clone()) {
                text.push_str(&format!("\nThe last one got {}", choice.level.words()));
                let measured = choice.measured();
                text.push_str(&if measured.is_empty() { ".".to_string() } else { format!(" (measured {measured}).") });
            }
            text
        }
        "check" => match serde_json::from_value::<link::check::Report>(out.clone()) {
            Ok(report) => report.words(),
            Err(_) => "The Ally answered in a way this companion does not understand. Update one of them.".to_string(),
        },
        "rescan" => format!("The Ally looked again: {} games.", out["games"]),
        "add_game" => {
            let mut text = format!("{} was added to the Ally. Its id is {}.", word(&out["title"]), word(&out["id"]));
            if !word(&out["port"]).is_empty() {
                text.push_str(&format!("\nRecognised as {}, a native port of a {} game.", word(&out["port"]), word(&out["origin"])));
            }
            text.push_str(&format!("\nFor a picture: carla-companion ally art {}", word(&out["id"])));
            text
        }
        "art_find" => {
            let mut lines: Vec<String> = out["candidates"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, c)| format!("{:>3}  {}  {}  {}", i + 1, if c["shape"] == "wide" { "wide " } else { "cover" }, word(&c["source"]), word(&c["label"])))
                .collect();
            if lines.is_empty() {
                lines.push(format!("No pictures were found for {}. Try another name: carla-companion ally art {} \"NAME\"", word(&out["title"]), word(&out["id"])));
            } else {
                lines.insert(0, format!("Pictures for {}:", word(&out["title"])));
                lines.push("Choose one: carla-companion ally art use NUMBER".to_string());
            }
            lines.extend(out["notes"].as_array().into_iter().flatten().map(|n| format!("Note: {}", word(n))));
            if out["key_set"].as_bool() == Some(false) {
                lines.push("For many more: get a free key at steamgriddb.com (Preferences, API), then carla-companion ally art key KEY".to_string());
            }
            lines.join("\n")
        }
        "art_set" => format!("{} has its new {}.", word(&out["title"]), if out["shape"] == "wide" { "wide picture" } else { "cover" }),
        "art_clear" => format!("{} is back to the picture it had by itself.", word(&out["title"])),
        "art_key" => if out["key_set"].as_bool() == Some(true) {
            "The Ally keeps the SteamGridDB key and asks it for pictures from now on.".to_string()
        } else {
            "The Ally forgot the SteamGridDB key.".to_string()
        },
        "remove_game" => format!("{} was taken off the Ally's library. Nothing was deleted from its disk.", word(&out["title"])),
        "save_folder" => match out["folder"].as_str() {
            Some(folder) => format!("Saves of {} are in {folder} on the Ally.", word(&out["title"])),
            None => format!("The Ally forgot where {} keeps its saves. Safe copies are kept.", word(&out["title"])),
        },
        "cleanup_scan" => {
            let mut lines = Vec::new();
            for item in out["items"].as_array().into_iter().flatten() {
                let approval = if item["needs_admin"].as_bool() == Some(true) { " · Windows asks for approval on the Ally's screen" } else { "" };
                lines.push(format!("{}\n    {} · {}{approval}\n    {}", word(&item["title"]), word(&item["id"]), word(&item["tier"]), word(&item["detail"])));
            }
            if lines.is_empty() {
                lines.push("Nothing to clean up was found.".to_string());
            }
            let changes = out["changes"].as_array().map(Vec::as_slice).unwrap_or_default();
            if !changes.is_empty() {
                lines.push("\nChanged by CarlaOS earlier (carla-companion ally cleanup undo ID):".to_string());
                lines.extend(changes.iter().map(|c| format!("    {}  {}", word(&c["item_id"]), word(&c["title"]))));
            }
            lines.extend(out["warnings"].as_array().into_iter().flatten().map(|w| format!("Note: {}", word(w))));
            lines.join("\n")
        }
        "cleanup_apply" => outcome(out, "applied"),
        "cleanup_undo" => outcome(out, "undone"),
        "update" => match out["version"].as_str() {
            Some(version) => format!("The Ally is installing CarlaOS {version}. It closes for a moment and comes back by itself."),
            None => "The Ally already has the newest CarlaOS.".to_string(),
        },
        "restart" => "CarlaOS on the Ally is starting again.".to_string(),
        "startup" => if out["enabled"].as_bool() == Some(true) {
            "The Ally opens CarlaOS by itself when it is turned on.".to_string()
        } else {
            "The Ally opens the Windows desktop when it is turned on.".to_string()
        },
        _ => serde_json::to_string_pretty(out).unwrap_or_default(),
    }
}

/// Which Ally is meant: the one named, or the only one.
fn device(files: &Files, named: Option<&str>) -> Result<String, String> {
    let devices = files.devices();
    match named {
        Some(id) => devices.iter().find(|d| d.id == id).map(|d| d.id.clone()).ok_or_else(|| "No linked device has that id (see carla-companion devices).".to_string()),
        None => match devices.as_slice() {
            [] => Err("No Ally is linked to this desktop yet.".to_string()),
            [only] => Ok(only.id.clone()),
            many => Err(format!("Say which Ally: carla-companion ally --device ID ...\n{}", many.iter().map(|d| format!("  {}  {}", d.id, d.name)).collect::<Vec<_>>().join("\n"))),
        },
    }
}

const NOT_LISTENING: &str = "The Ally did not fetch the request. It must be switched on with CarlaOS running, able to reach this desktop, \
and this desktop must be allowed on it: Settings, Desktops, this desktop, \"Administer this Ally\".";

/// Leave the request for the Ally and wait for its answer.
pub fn ask(files: &Files, device: &str, request: &Request) -> Result<Value, String> {
    let command = Command { id: link::random_hex(8), op: request.op.to_string(), args: request.args.clone() };
    files.ask_ally(device, &command)?;
    let asked = Instant::now();
    loop {
        if let Some(done) = files.take_ally_answer(device, &command.id) {
            return match done.error() {
                Some(error) => Err(error.to_string()),
                None => Ok(done.output),
            };
        }
        let fetched = !files.ally_asked(device);
        if !fetched && asked.elapsed() > Duration::from_secs(ASK_LIFETIME as u64) {
            files.take_ally_ask(device, now());
            return Err(NOT_LISTENING.to_string());
        }
        if fetched && asked.elapsed() > request.patience + Duration::from_secs(ASK_LIFETIME as u64) {
            return Err("The Ally took the request but has not answered. It may still be working on it; ask for its status in a while.".to_string());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// The pictures last looked for, kept so that one can be chosen by number.
const CHOICES: &str = "art-choices.json";

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A page of the pictures found, each under its number, for the browser.
pub fn sheet(out: &Value) -> String {
    let mut page = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Pictures for {title}</title>\
<style>body{{background:#1e1220;color:#f8ecef;font:16px system-ui,sans-serif;margin:2rem}}h1{{color:#f2a7bb}}\
.all{{display:flex;flex-wrap:wrap;gap:1.25rem}}figure{{margin:0;width:220px}}figure.wide{{width:460px}}\
img{{width:100%;border-radius:14px;background:#2c1b2f;display:block}}b{{color:#f2a7bb;font-size:1.4rem}}\
figcaption{{padding:.4rem .2rem;color:#b9a6b3}}code{{color:#f8ecef}}</style>\
<h1>Pictures for {title}</h1><p>Choose one with <code>carla-companion ally art use NUMBER</code>. A picture that does not load does not exist.</p><div class=\"all\">",
        title = escape(word(&out["title"]))
    );
    for (i, c) in out["candidates"].as_array().into_iter().flatten().enumerate() {
        // Only pictures from where the Ally looked, whatever the answer says.
        if !carla_core::library::meta::trusted_picture(word(&c["thumb"])) {
            continue;
        }
        page.push_str(&format!(
            "<figure class=\"{}\"><img src=\"{}\" loading=\"lazy\" alt=\"\"><figcaption><b>{}</b> {} · {}</figcaption></figure>",
            if c["shape"] == "wide" { "wide" } else { "tall" },
            escape(word(&c["thumb"])),
            i + 1,
            escape(word(&c["source"])),
            escape(word(&c["label"]))
        ));
    }
    page + "</div>"
}

/// Show a page in the administrator's browser. Nothing is lost if it does
/// not open: the list is printed as well.
fn open_page(path: &std::path::Path) {
    let opened = if cfg!(windows) {
        std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path).spawn()
    };
    if opened.is_err() {
        println!("Open {} to see them.", path.display());
    }
}

/// "art use 3": the third of the pictures last looked for.
pub fn chosen(files: &Files, number: &str) -> Result<Request, String> {
    let none = "Look for pictures first: carla-companion ally art ID";
    let last: Value = std::fs::read(files.dir().join(CHOICES)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).ok_or(none)?;
    let candidates = last["candidates"].as_array().ok_or(none)?;
    let candidate = number.parse::<usize>().ok().and_then(|n| candidates.get(n.checked_sub(1)?)).ok_or_else(|| format!("Say a number from 1 to {}.", candidates.len()))?;
    Ok(Request { op: "art_set", args: json!({ "id": last["id"], "candidate": candidate }), patience: Duration::from_secs(90) })
}

pub fn run(files: &Files, words: &[String], serving: bool) -> Result<i32, String> {
    let (named, words) = match words {
        [flag, id, rest @ ..] if flag == "--device" => (Some(id.as_str()), rest),
        _ => (None, words),
    };
    if words.first().is_some_and(|w| w == "help" || w == "--help") {
        println!("{HELP}");
        return Ok(0);
    }
    let request = match words {
        [art, use_, number] if art == "art" && use_ == "use" => chosen(files, number)?,
        _ => request(words)?,
    };
    let device = device(files, named)?;
    if !serving {
        return Err("The companion is not running on this desktop, so the Ally cannot fetch anything. Start it (carla-companion) and try again.".to_string());
    }
    if files.ally_listening(&device).is_none_or(|at| now() - at > 90) {
        println!("Waiting for the Ally to ask for requests…");
    }
    let out = ask(files, &device, &request)?;
    if request.op == "art_find" && out["candidates"].as_array().is_some_and(|c| !c.is_empty()) {
        // Remembered for "art use NUMBER", and shown as pictures.
        let _ = std::fs::write(files.dir().join(CHOICES), serde_json::to_vec(&out).unwrap_or_default());
        let page = files.dir().join("art-choices.html");
        if std::fs::write(&page, sheet(&out)).is_ok() {
            open_page(&page);
        }
    }
    println!("{}", show(request.op, &out));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        carla_core::library::launch::split_args(line)
    }

    #[test]
    fn what_the_administrator_types_becomes_a_request() {
        assert_eq!(request(&words("")).unwrap().op, "status");
        assert_eq!(request(&words("games")).unwrap().op, "games");
        let add = request(&words(r#"add "Paper Lanterns" "C:\Games\Lanterns\lanterns.exe" --args "-fullscreen -vsync" --kind homebrew"#)).unwrap();
        assert_eq!(add.op, "add_game");
        assert_eq!(add.args, json!({ "title": "Paper Lanterns", "exe": r"C:\Games\Lanterns\lanterns.exe", "args": "-fullscreen -vsync", "kind": "homebrew" }));
        assert_eq!(request(&words("saves manual:1 --forget")).unwrap().args, json!({ "id": "manual:1" }));
        assert_eq!(request(&words(r#"saves manual:1 "D:\Saves\One""#)).unwrap().args, json!({ "id": "manual:1", "folder": r"D:\Saves\One" }));
        let apply = request(&words("cleanup apply app.news app.weather")).unwrap();
        assert_eq!((apply.op, apply.args), ("cleanup_apply", json!({ "ids": ["app.news", "app.weather"] })));
        assert_eq!(request(&words(r#"art manual:1 ocarina of time"#)).unwrap().args, json!({ "id": "manual:1", "term": "ocarina of time" }));
        assert_eq!(request(&words("art steam:10")).unwrap().args, json!({ "id": "steam:10", "term": "" }));
        assert_eq!(request(&words("art key none")).unwrap().args, json!({ "key": "" }));
        for wrong in ["cleanup apply", "add OnlyAName", "add A B --colour pink", "remove", "format c:", "saves", "startup maybe", "picture", "picture HOME ultra", "art", "art use 2", "check", "check A B"] {
            assert!(request(&words(wrong)).is_err(), "{wrong:?} is not a request");
        }
        // Everything that can be typed is something the Ally knows.
        for line in ["status", "games", "rescan", "add A B", "remove X", "saves X Y", "desktops", "cleanup", "cleanup apply X", "cleanup undo X", "update", "restart", "startup", "startup on", "picture HOME", "picture HOME auto120", "picture HOME light", "art X", "art X zelda", "art clear X", "art key abcdef0123456789", "check HOME", "check HOME --repair", "check --repair HOME"] {
            assert!(carla_core::remote::OPS.contains(&request(&words(line)).unwrap().op), "{line}");
        }
    }

    #[test]
    fn answers_are_shown_in_words() {
        let status = show("status", &json!({ "name": "ALLY", "version": "0.4.0", "update": "0.5.0", "playing": null, "battery": { "percent": 64, "charging": true }, "games": 12, "on_ally": 5, "desktops": 2 }));
        assert!(status.contains("0.4.0, 0.5.0 is available") && status.contains("64%, charging") && status.contains("Playing:   nothing") && status.contains("12 (5 installed on the Ally)"));
        let games = show("games", &json!([{ "id": "manual:1", "title": "Paper Lanterns", "platform": "Standalone", "on_ally": true, "desktops": 1, "minutes": 30, "save_folder": "D:\\Saves" }]));
        assert_eq!(games, "Paper Lanterns\n    manual:1 · Standalone · on the Ally and streamed · 30 min played\n    saves: D:\\Saves");
        assert_eq!(show("games", &json!([])), "The Ally's library is empty.");
        let applied = show("cleanup_apply", &json!({ "done": ["a", "b"], "failed": [{ "id": "c", "title": "News", "error": "It is in use." }], "restart": true }));
        assert_eq!(applied, "2 applied.\nNot applied: News (It is in use.)\nSome of it only takes full effect after the Ally restarts.");
        assert!(show("update", &json!({ "version": null })).contains("already"));
        assert!(show("add_game", &json!({ "id": "manual:1", "title": "Ocarina of Time", "kind": "port", "origin": "Nintendo 64", "port": "Hookshot" })).contains("Recognised as Hookshot, a native port of a Nintendo 64 game."));
    }

    #[test]
    fn pictures_are_listed_shown_and_chosen_by_number() {
        let dir = std::env::temp_dir().join(format!("carla-ally-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = Files::new(dir.clone());
        assert!(chosen(&files, "1").is_err(), "nothing was looked for yet");
        let found = json!({ "id": "manual:1", "title": "Zelda <Ocarina>", "key_set": false, "notes": [], "candidates": [
            { "source": "Wikipedia", "label": "The Legend of Zelda: Ocarina of Time", "shape": "wide", "thumb": "https://upload.wikimedia.org/a.jpg", "full": "https://upload.wikimedia.org/a.jpg" },
            { "source": "Box art", "label": "Ocarina \"USA\"", "shape": "tall", "thumb": "https://thumbnails.libretro.com/b.png", "full": "https://thumbnails.libretro.com/b.png" },
            { "source": "Odd", "label": "x", "shape": "tall", "thumb": "https://evil.example/c.png\" onerror=\"x", "full": "https://evil.example/c.png" }] });
        let listed = show("art_find", &found);
        assert!(listed.contains("  1  wide   Wikipedia  The Legend of Zelda: Ocarina of Time") && listed.contains("  2  cover  Box art") && listed.contains("steamgriddb.com"), "{listed}");
        let page = sheet(&found);
        assert!(page.contains("Pictures for Zelda &lt;Ocarina&gt;") && page.contains("<b>2</b> Box art · Ocarina &quot;USA&quot;"));
        assert!(!page.contains("evil.example"), "only pictures from where the Ally looked are shown");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(CHOICES), serde_json::to_vec(&found).unwrap()).unwrap();
        let pick = chosen(&files, "2").unwrap();
        assert_eq!((pick.op, &pick.args["id"], &pick.args["candidate"]["source"]), ("art_set", &json!("manual:1"), &json!("Box art")));
        assert!(chosen(&files, "0").is_err() && chosen(&files, "9").is_err() && chosen(&files, "two").is_err());
        assert!(show("art_find", &json!({ "id": "x", "title": "Nothing", "candidates": [] })).contains("No pictures were found for Nothing"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_request_nobody_fetches_is_given_up_on_and_an_answer_is_returned() {
        let dir = std::env::temp_dir().join(format!("carla-ally-cli-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let files = Files::new(dir.clone());
        assert!(device(&files, None).is_err(), "no Ally is linked");
        // An Ally that answers: here a thread playing its part through the files.
        let ally = files.clone();
        std::thread::spawn(move || loop {
            if let Some(command) = ally.take_ally_ask("dev1", now()) {
                let result = if command.op == "games" { Ok(json!([])) } else { Err("The Ally has no game with that id.".to_string()) };
                ally.set_ally_answer("dev1", &carla_core::remote::Done::of(&command.id, result)).unwrap();
            }
            std::thread::sleep(Duration::from_millis(50));
        });
        assert_eq!(ask(&files, "dev1", &request(&words("games")).unwrap()).unwrap(), json!([]));
        assert_eq!(ask(&files, "dev1", &request(&words("remove nothing")).unwrap()).err().as_deref(), Some("The Ally has no game with that id."));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
