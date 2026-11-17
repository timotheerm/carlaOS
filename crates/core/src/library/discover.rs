//! Looking for games, ports and emulators nobody has told CarlaOS about.
//!
//! Store games appear by themselves. Everything else used to need the
//! administrator to point at each program in turn. This looks where such
//! things usually are, so they can be added in one go:
//!
//! - folders that are kept for games (`C:\Games`, `D:\Ports`...): every
//!   folder inside is taken for one game, and its main program is picked;
//! - shortcuts on the desktop and in the Start menu, when they lead to a
//!   port or an emulator CarlaOS knows, or into one of those game folders;
//! - the places programs install themselves to, for the ports and emulators
//!   CarlaOS knows by name. Anything else there is not a game.
//!
//! Nothing is added, started or changed here: it is a list to choose from.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::emulation::{self, Suggestion};
use super::ports::{self, recognise};
use super::{pe, shortcut, Kind};

/// Folders this deep below a game's own folder are looked through.
pub(super) const DEPTH: usize = 3;
/// More files than this in one folder and the rest is left alone.
pub(super) const MOST_FILES: usize = 4000;
/// More than this is more than anyone wants to look through.
pub(super) const MOST_FOUND: usize = 200;

/// A program that looks like a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundGame {
    pub exe: PathBuf,
    /// The options a shortcut passes.
    pub args: String,
    pub title: String,
    pub kind: Kind,
    /// The console a port first came out on.
    pub origin: String,
    /// What a port CarlaOS knows calls itself.
    pub port: String,
    /// Where it was found, in a few words: "Desktop", "D:\Games".
    pub from: String,
}

/// An emulator CarlaOS knows how to start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FoundEmulator {
    pub exe: PathBuf,
    /// What the program is called: "Dolphin".
    pub program: String,
    /// The console it plays: "GameCube and Wii".
    pub name: String,
    pub options: String,
    pub endings: Vec<String>,
    pub from: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Found {
    pub games: Vec<FoundGame>,
    pub emulators: Vec<FoundEmulator>,
}

/// Where to look.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    /// Folders kept for games: each folder inside is one game.
    pub games: Vec<PathBuf>,
    /// Folders of shortcuts, with what to call them.
    pub shortcuts: Vec<(PathBuf, &'static str)>,
    /// Where programs install themselves: only what CarlaOS knows by name.
    pub programs: Vec<PathBuf>,
}

pub fn lower(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

pub(super) fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn is_program(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
}

/// Programs that come with a game and are not the game.
pub(super) fn helper(path: &Path) -> bool {
    let name = stem(path);
    let starts = ["unins", "setup", "install", "vc_redist", "vcredist", "dxsetup", "dxwebsetup", "dotnet", "crashpad", "crashreport", "unitycrashhandler", "ue4prereq", "ueprereq", "qtwebengine", "cefsharp", "python", "7z", "oalinst", "physx", "directx", "notification_helper", "createdump", "updater", "update", "patcher"];
    let has = ["crashhandler", "crashreporter", "errorreport", "redist", "uninstall"];
    // Whole words only: "Observer" is a game, "server" is not.
    let words = ["helper", "benchmark", "config", "settings", "server", "editor", "diagnostics"];
    starts.iter().any(|s| name.starts_with(s)) || has.iter().any(|s| name.contains(s)) || name.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| words.contains(&w))
}

/// The emulator a program is, leaving out the tools that come with one.
fn emulator(path: &Path) -> Option<Suggestion> {
    let name = stem(path);
    if ["tool", "room", "-cmd", "updater", "setup", "install", "crash", "server", "sdl-headless"].iter().any(|w| name.contains(w)) {
        return None;
    }
    // "Eden.exe" is an emulator; "Eden Rising.exe" is a game. After the
    // emulator's name comes nothing, or something that is not a word.
    let after = emulation::known_start(path).and_then(|start| name.strip_prefix(start).map(str::to_string))?;
    if after.starts_with(|c: char| c.is_ascii_alphabetic() || c == ' ') && !after.starts_with("windows") {
        return None;
    }
    Some(emulation::suggest(path)).filter(|s| !s.name.is_empty())
}

fn programs(dir: &Path, depth: usize, left: &mut usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if *left == 0 {
            return;
        }
        *left -= 1;
        let path = entry.path();
        // Links are not followed: a folder must not lead elsewhere.
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && depth > 0 {
            programs(&path, depth - 1, left, out);
        } else if kind.is_file() && is_program(&path) && !helper(&path) {
            out.push(path);
        }
    }
}

pub(super) fn squash(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// The program that is the game, out of all the programs in a game's folder.
fn main_program(folder: &Path, found: &[PathBuf]) -> Option<PathBuf> {
    let name = squash(&folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let score = |path: &PathBuf| -> (u8, u64) {
        let stem = squash(&stem(path));
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let rank = if recognise(path).is_some() {
            5
        } else if !stem.is_empty() && (stem == name || name.starts_with(&stem) || stem.starts_with(&name)) {
            4
        } else if stem.ends_with("shipping") {
            3
        } else if stem.contains("launcher") {
            1
        } else {
            2
        };
        (rank, size)
    };
    found.iter().max_by_key(|p| score(p)).cloned()
}

fn game(exe: PathBuf, args: String, named: Option<&str>, from: &str) -> FoundGame {
    let product = pe::product_name(&exe);
    let known = recognise(&exe)
        .or_else(|| named.and_then(|n| recognise(Path::new(&format!("{n}.lnk")))))
        .or_else(|| product.as_deref().and_then(|n| recognise(Path::new(&format!("{n}.exe")))));
    FoundGame {
        title: known.map(|p| p.title.to_string()).or(named.map(str::to_string)).or(product).unwrap_or_else(|| ports::suggest_title(&exe)),
        kind: if known.is_some() { Kind::Port } else { Kind::Game },
        origin: known.map(|p| p.console.to_string()).unwrap_or_default(),
        port: known.map(|p| p.port.to_string()).unwrap_or_default(),
        from: from.to_string(),
        exe,
        args,
    }
}

fn found_emulator(exe: PathBuf, suggestion: Suggestion, from: &str) -> FoundEmulator {
    FoundEmulator {
        program: pe::product_name(&exe).unwrap_or_else(|| ports::suggest_title(&exe)),
        name: suggestion.name.to_string(),
        options: suggestion.options.to_string(),
        endings: suggestion.endings.iter().map(|e| e.to_string()).collect(),
        from: from.to_string(),
        exe,
    }
}

fn folders_in(dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|k| k.is_dir())).map(|e| e.path()).collect();
    list.sort();
    list
}

fn shortcuts_in(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && depth > 0 {
            shortcuts_in(&path, depth - 1, out);
        } else if kind.is_file() && shortcut::is_shortcut(&path) && out.len() < MOST_FILES {
            out.push(path);
        }
    }
}

/// Look through the places for games, ports and emulators. `known` holds the
/// programs that are in the library already, lower case, which are left out.
pub fn look(places: &Places, known: &HashSet<String>) -> Found {
    let mut found = Found::default();
    let mut seen: HashSet<String> = known.clone();
    let from_of = |dir: &Path| dir.to_string_lossy().into_owned();
    let inside_games = |path: &Path| {
        let path = lower(path);
        places.games.iter().any(|g| path.starts_with(&format!("{}\\", lower(g).trim_end_matches('\\'))))
    };
    let mut add = |found: &mut Found, exe: PathBuf, args: String, named: Option<&str>, from: &str, only_known: bool| {
        if !exe.is_file() || !is_program(&exe) || helper(&exe) || seen.contains(&lower(&exe)) {
            return;
        }
        if let Some(suggestion) = emulator(&exe) {
            seen.insert(lower(&exe));
            found.emulators.push(found_emulator(exe, suggestion, from));
            return;
        }
        let candidate = game(exe, args, named, from);
        if only_known && candidate.kind != Kind::Port {
            return;
        }
        seen.insert(lower(&candidate.exe));
        found.games.push(candidate);
    };

    // Folders kept for games: one game per folder, and loose programs that
    // are known by name.
    for root in &places.games {
        let from = from_of(root);
        for folder in folders_in(root) {
            let mut left = MOST_FILES;
            let mut list = Vec::new();
            programs(&folder, DEPTH, &mut left, &mut list);
            // A folder of emulators or ports holds several things worth a card.
            let special: Vec<PathBuf> = list.iter().filter(|p| emulator(p).is_some() || recognise(p).is_some()).cloned().collect();
            if special.is_empty() {
                if let Some(main) = main_program(&folder, &list) {
                    add(&mut found, main, String::new(), None, &from, false);
                }
            }
            // One card for each folder: a port's folder often holds two
            // builds of the same program.
            let mut folders = HashSet::new();
            for exe in special {
                if folders.insert(exe.parent().map(lower)) {
                    add(&mut found, exe, String::new(), None, &from, false);
                }
            }
        }
        let mut left = MOST_FILES;
        let mut loose = Vec::new();
        programs(root, 0, &mut left, &mut loose);
        loose.sort();
        for exe in loose {
            add(&mut found, exe, String::new(), None, &from, true);
        }
    }

    // Shortcuts: what they lead to counts when CarlaOS knows it by name, or
    // when it is in a folder kept for games.
    for (dir, from) in &places.shortcuts {
        let mut list = Vec::new();
        shortcuts_in(dir, 2, &mut list);
        list.sort();
        for link in list {
            let Ok(read) = shortcut::read(&link) else { continue };
            let named = link.file_stem().map(|s| s.to_string_lossy().into_owned());
            let only_known = !inside_games(&read.target);
            add(&mut found, read.target, read.args, named.as_deref(), from, only_known);
        }
    }

    // Where programs install themselves: only ports and emulators known by name.
    for root in &places.programs {
        let from = from_of(root);
        for folder in folders_in(root) {
            let mut left = 600;
            let mut list = Vec::new();
            programs(&folder, 2, &mut left, &mut list);
            list.sort();
            let mut folders = HashSet::new();
            for exe in list.into_iter().filter(|p| emulator(p).is_some() || recognise(p).is_some()) {
                if folders.insert(exe.parent().map(lower)) {
                    add(&mut found, exe, String::new(), None, &from, true);
                }
            }
        }
    }

    found.games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    found.emulators.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    found.games.truncate(MOST_FOUND);
    found.emulators.truncate(MOST_FOUND);
    found
}

/// The usual places on this machine.
pub fn places() -> Places {
    let mut places = Places::default();
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_dir());
    let mut games: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        for letter in 'C'..='Z' {
            let drive = PathBuf::from(format!("{letter}:\\"));
            if !drive.is_dir() {
                continue;
            }
            for name in ["Games", "Game", "Ports", "PC Ports", "Emulators", "Emulation", "Emu"] {
                games.push(drive.join(name));
            }
        }
    }
    if let Some(home) = env("USERPROFILE").or_else(|| env("HOME")) {
        for name in ["Games", "Ports", "Emulators", "Emulation"] {
            games.push(home.join(name));
            games.push(home.join("Documents").join(name));
        }
        places.shortcuts.push((home.join("Desktop"), "Desktop"));
        places.programs.push(home.join("scoop").join("apps"));
    }
    if let Some(public) = env("PUBLIC") {
        places.shortcuts.push((public.join("Desktop"), "Desktop"));
    }
    for name in ["APPDATA", "PROGRAMDATA"] {
        if let Some(dir) = env(name) {
            places.shortcuts.push((dir.join("Microsoft").join("Windows").join("Start Menu").join("Programs"), "Start menu"));
        }
    }
    if let Some(local) = env("LOCALAPPDATA") {
        places.programs.push(local.join("Programs"));
        places.programs.push(local);
    }
    for name in ["APPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        places.programs.extend(env(name));
    }
    places.games = games.into_iter().filter(|p| p.is_dir()).collect();
    places.shortcuts.retain(|(p, _)| p.is_dir());
    places.programs.retain(|p| p.is_dir());
    places
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program(path: &Path, details: &[(&str, &str)], padding: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = pe::sample(details);
        bytes.extend(vec![0u8; padding]);
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn games_ports_and_emulators_are_found_where_they_usually_are() {
        let base = std::env::temp_dir().join(format!("carla-discover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let games = base.join("Games");
        // An ordinary game: the big program named like its folder, not its helpers.
        program(&games.join("Paper Lanterns").join("PaperLanterns.exe"), &[("ProductName", "Paper Lanterns")], 64);
        program(&games.join("Paper Lanterns").join("UnityCrashHandler64.exe"), &[], 4096);
        program(&games.join("Paper Lanterns").join("unins000.exe"), &[], 4096);
        // An Unreal game: a small starter, and the real program deep inside.
        program(&games.join("Tin Knight").join("TinKnight.exe"), &[("ProductName", "BootstrapPackagedGame")], 0);
        program(&games.join("Tin Knight").join("TinKnight").join("Binaries").join("Win64").join("TinKnight-Win64-Shipping.exe"), &[], 9000);
        // A port, known by its program's name, two builds in one folder.
        program(&games.join("Zelda").join("soh.exe"), &[], 0);
        program(&games.join("Zelda").join("soh-debug.exe"), &[], 0);
        // A folder of emulators.
        program(&games.join("Emus").join("Dolphin-x64").join("Dolphin.exe"), &[("ProductName", "Dolphin")], 0);
        program(&games.join("Emus").join("Dolphin-x64").join("DolphinTool.exe"), &[], 0);
        program(&games.join("Emus").join("PCSX2").join("pcsx2-qt.exe"), &[], 0);
        // A game that is in the library already.
        program(&games.join("Old").join("old.exe"), &[], 0);
        fs::create_dir_all(games.join("Empty")).unwrap();

        // Shortcuts: one into the games folder, one to a known port elsewhere,
        // one to an ordinary program, one to something that is gone.
        let desktop = base.join("Desktop");
        fs::create_dir_all(&desktop).unwrap();
        let elsewhere = base.join("Elsewhere");
        program(&elsewhere.join("sm64").join("sm64.us.f3dex2e.exe"), &[], 0);
        program(&elsewhere.join("Office").join("writer.exe"), &[("ProductName", "Writer")], 0);
        let link = |name: &str, target: &Path, args: &str| fs::write(desktop.join(name), shortcut::sample(&target.to_string_lossy(), args, "", true)).unwrap();
        link("Mario.lnk", &elsewhere.join("sm64").join("sm64.us.f3dex2e.exe"), "--fullscreen");
        link("Writer.lnk", &elsewhere.join("Office").join("writer.exe"), "");
        link("Gone.lnk", &elsewhere.join("gone.exe"), "");
        link("Lanterns.lnk", &games.join("Paper Lanterns").join("PaperLanterns.exe"), "");

        // Where programs install themselves: only what is known by name.
        let installed = base.join("Programs");
        program(&installed.join("Ryujinx").join("Ryujinx.exe"), &[], 0);
        program(&installed.join("Some Editor").join("editor.exe"), &[], 0);
        program(&installed.join("Mail").join("mail.exe"), &[], 0);

        let places = Places { games: vec![games.clone()], shortcuts: vec![(desktop, "Desktop")], programs: vec![installed] };
        let known: HashSet<String> = [lower(&games.join("Old").join("old.exe"))].into();
        let found = look(&places, &known);

        let titles: Vec<(&str, Kind, &str)> = found.games.iter().map(|g| (g.title.as_str(), g.kind, g.from.as_str())).collect();
        let from = games.to_string_lossy().into_owned();
        assert_eq!(
            titles,
            [("Paper Lanterns", Kind::Game, from.as_str()), ("Super Mario 64", Kind::Port, "Desktop"), ("The Legend of Zelda: Ocarina of Time", Kind::Port, from.as_str()), ("Tin Knight", Kind::Game, from.as_str())]
        );
        let knight = &found.games[3];
        assert!(knight.exe.ends_with("TinKnight-Win64-Shipping.exe"), "the real program, not the starter: {:?}", knight.exe);
        assert_eq!(found.games[1].args, "--fullscreen");
        assert_eq!((found.games[2].origin.as_str(), found.games[2].port.as_str()), ("Nintendo 64", "Ship of Harkinian"));

        let emulators: Vec<(&str, &str)> = found.emulators.iter().map(|e| (e.program.as_str(), e.name.as_str())).collect();
        assert_eq!(emulators, [("Dolphin", "GameCube and Wii"), ("pcsx2 qt", "PlayStation 2"), ("Ryujinx", "Switch")]);
        assert!(found.emulators[0].options.contains("{game}") && found.emulators[0].endings.contains(&"rvz".to_string()));

        // Looking again once everything was added finds nothing new.
        let all: HashSet<String> = known.into_iter().chain(found.games.iter().map(|g| lower(&g.exe))).chain(found.emulators.iter().map(|e| lower(&e.exe))).collect();
        assert_eq!(look(&places, &all), Found::default());
        let _ = fs::remove_dir_all(&base);
    }
}
