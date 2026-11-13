//! Games played through an emulator.
//!
//! CarlaOS brings no emulator, no firmware, no keys and no games. The
//! administrator installs an emulator, puts their own game files in a folder,
//! and tells CarlaOS about both: that pair is a "console" here. Every game
//! file in the folder then gets a card that starts the emulator with it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{manual_key, Scanned};

/// Where the game file goes in an emulator's options.
pub const GAME: &str = "{game}";

/// Folders this deep below the chosen one are still looked through.
const DEPTH: usize = 3;
/// A folder with more files than this is not a folder of games.
const MOST_FILES: usize = 5000;

/// One emulator and the folder of game files it plays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Console {
    /// Empty when new; the catalogue gives it one.
    #[serde(default)]
    pub id: String,
    /// Shown on the badge of its games: "GameCube", "PlayStation 2".
    pub name: String,
    pub emulator: PathBuf,
    /// The emulator's options, with `{game}` where the game file goes.
    pub options: String,
    pub folder: PathBuf,
    /// File endings that are games, without the dot, lower case.
    pub endings: Vec<String>,
}

/// What CarlaOS suggests for an emulator it recognises by its file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub name: &'static str,
    pub options: &'static str,
    pub endings: &'static [&'static str],
}

/// (start of the program's file name, suggestion). Options are the ones each
/// emulator documents for "start this game full screen, without my window".
const KNOWN: &[(&str, Suggestion)] = &[
    ("dolphin", Suggestion { name: "GameCube and Wii", options: "-b -e \"{game}\"", endings: &["iso", "gcm", "rvz", "wbfs", "ciso", "gcz", "wad"] }),
    ("pcsx2", Suggestion { name: "PlayStation 2", options: "-batch -fullscreen -- \"{game}\"", endings: &["iso", "chd", "cso", "zso", "gz"] }),
    ("duckstation", Suggestion { name: "PlayStation", options: "-batch -fullscreen -- \"{game}\"", endings: &["chd", "cue", "pbp", "m3u", "iso"] }),
    ("ppsspp", Suggestion { name: "PSP", options: "--fullscreen \"{game}\"", endings: &["iso", "cso", "pbp", "chd"] }),
    ("mgba", Suggestion { name: "Game Boy Advance", options: "-f \"{game}\"", endings: &["gba", "gbc", "gb", "zip", "7z"] }),
    ("melonds", Suggestion { name: "Nintendo DS", options: "-f \"{game}\"", endings: &["nds", "dsi"] }),
    ("snes9x", Suggestion { name: "Super Nintendo", options: "-fullscreen \"{game}\"", endings: &["sfc", "smc", "zip"] }),
    ("mesen", Suggestion { name: "NES", options: "--fullscreen \"{game}\"", endings: &["nes", "fds", "zip"] }),
    ("flycast", Suggestion { name: "Dreamcast", options: "\"{game}\"", endings: &["chd", "gdi", "cdi", "cue"] }),
    ("xemu", Suggestion { name: "Xbox", options: "-full-screen -dvd_path \"{game}\"", endings: &["iso", "xiso"] }),
    ("cemu", Suggestion { name: "Wii U", options: "-f -g \"{game}\"", endings: &["wua", "wud", "wux", "rpx"] }),
    ("rpcs3", Suggestion { name: "PlayStation 3", options: "--no-gui --fullscreen \"{game}\"", endings: &["iso"] }),
    ("lime3ds", Suggestion { name: "Nintendo 3DS", options: "\"{game}\"", endings: &["3ds", "cci", "cxi"] }),
    ("azahar", Suggestion { name: "Nintendo 3DS", options: "\"{game}\"", endings: &["3ds", "cci", "cxi"] }),
];

/// What to fill in for an emulator, going by its program's name. A program
/// CarlaOS does not know gets the plainest options there are.
pub fn suggest(emulator: &Path) -> Suggestion {
    // By hand, so a Windows path is read the same way on any machine.
    let path = emulator.to_string_lossy().to_lowercase();
    let file = path.rsplit(['/', '\\']).next().unwrap_or_default();
    KNOWN
        .iter()
        .find(|(start, _)| file.starts_with(start))
        .map(|(_, s)| s.clone())
        .unwrap_or(Suggestion { name: "", options: "\"{game}\"", endings: &[] })
}

/// Tidy what the administrator typed, or say what is missing.
pub fn checked(mut console: Console) -> Result<Console, String> {
    console.name = console.name.trim().to_string();
    console.options = console.options.trim().to_string();
    console.endings = console
        .endings
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_lowercase())
        .filter(|e| !e.is_empty() && e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .collect();
    console.endings.sort();
    console.endings.dedup();
    if console.name.is_empty() || console.name.len() > 40 {
        return Err("The console needs a short name.".to_string());
    }
    if !console.emulator.is_file() {
        return Err("The emulator's program could not be found.".to_string());
    }
    if !console.folder.is_dir() {
        return Err("The folder of games could not be found.".to_string());
    }
    if console.endings.is_empty() {
        return Err("Say which file endings are games, for example iso or chd.".to_string());
    }
    // Programs are not games, whatever folder they are in.
    if console.endings.iter().any(|e| ["exe", "bat", "cmd", "com", "lnk", "msi", "ps1", "dll"].contains(&e.as_str())) {
        return Err("Programs cannot be listed as game files.".to_string());
    }
    if !console.options.contains(GAME) {
        console.options = format!("{} \"{GAME}\"", console.options).trim().to_string();
    }
    Ok(console)
}

/// "Tin Knight (USA) (Rev 1) [!].iso" is shown as "Tin Knight".
pub fn title(file: &Path) -> String {
    let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = String::new();
    let mut depth = 0u32;
    for c in stem.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            '_' if depth == 0 => out.push(' '),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let tidy = out.split_whitespace().collect::<Vec<_>>().join(" ");
    // "Legend, The" is how collections sort names; show it the natural way.
    let tidy = match tidy.rsplit_once(", ") {
        Some((rest, article)) if ["The", "A", "An"].contains(&article) => format!("{article} {rest}"),
        _ => tidy,
    };
    if tidy.is_empty() { stem } else { tidy }
}

fn files(dir: &Path, endings: &[String], depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if out.len() >= MOST_FILES {
            return;
        }
        let path = entry.path();
        // Links are not followed: the folder must not lead elsewhere.
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() && depth > 0 {
            files(&path, endings, depth - 1, out);
        } else if kind.is_file() {
            let ending = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if endings.contains(&ending) {
                out.push(path);
            }
        }
    }
}

/// Every game file of one console, as a game that starts its emulator.
pub fn scan(console: &Console) -> Vec<Scanned> {
    if !console.emulator.is_file() {
        return Vec::new();
    }
    let mut found = Vec::new();
    files(&console.folder, &console.endings, DEPTH, &mut found);
    // A disc image and its track list are one game: the list is what starts it.
    let lists: Vec<PathBuf> = found.iter().filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue") || e.eq_ignore_ascii_case("m3u"))).map(|f| f.with_extension("")).collect();
    found.retain(|f| {
        let listed = lists.iter().any(|l| *l == f.with_extension(""));
        let is_list = f.extension().is_some_and(|e| e.eq_ignore_ascii_case("cue") || e.eq_ignore_ascii_case("m3u"));
        is_list || !listed
    });
    let mut games: Vec<Scanned> = found
        .into_iter()
        // A quote cannot be in a Windows file name; elsewhere it would break the options.
        .filter(|file| !file.to_string_lossy().contains('"'))
        .map(|file| {
            let path = file.to_string_lossy();
            Scanned {
                source_id: format!("{}-{}", console.id, manual_key(&path)),
                title: title(&file),
                // The emulator is what runs, so its folder is what is watched.
                install_path: console.emulator.parent().map(PathBuf::from).unwrap_or_default(),
                exe: Some(console.emulator.clone()),
                args: console.options.replace(GAME, &path),
                platform: console.name.clone(),
                emulated: true,
                ..Scanned::default()
            }
        })
        .collect();
    games.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    games.dedup_by(|a, b| a.source_id == b.source_id);
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::launch::split_args;

    #[test]
    fn names_are_tidied_for_the_card() {
        assert_eq!(title(Path::new("Tin Knight (USA) (Rev 1) [!].iso")), "Tin Knight");
        assert_eq!(title(Path::new("Legend_of_the_Lake, The (Europe).chd")), "The Legend of the Lake");
        assert_eq!(title(Path::new("(Demo).iso")), "(Demo)");
        assert_eq!(title(Path::new("Lowpoly Rally 2.cso")), "Lowpoly Rally 2");
    }

    #[test]
    fn known_emulators_get_their_options_and_unknown_ones_the_plainest() {
        let dolphin = suggest(Path::new(r"C:\Emulators\Dolphin-x64\Dolphin.exe"));
        assert_eq!((dolphin.name, dolphin.options), ("GameCube and Wii", "-b -e \"{game}\""));
        assert_eq!(suggest(Path::new("/opt/pcsx2-qt")).name, "PlayStation 2");
        let other = suggest(Path::new("/opt/something-else"));
        assert_eq!((other.name, other.options, other.endings.len()), ("", "\"{game}\"", 0));
    }

    #[test]
    fn a_console_lists_its_game_files_and_starts_the_emulator_with_each() {
        let base = std::env::temp_dir().join(format!("carla-emu-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let games = base.join("My Games");
        fs::create_dir_all(games.join("More").join("Deeper")).unwrap();
        let emulator = base.join("Emu").join("dolphin.exe");
        fs::create_dir_all(emulator.parent().unwrap()).unwrap();
        fs::write(&emulator, b"program").unwrap();
        for file in ["Tin Knight (USA).iso", "More/Comet Crew.RVZ", "More/Deeper/Third.iso", "notes.txt", "setup.exe", "Disc Game.cue", "Disc Game.bin"] {
            fs::write(games.join(file), b"x").unwrap();
        }

        let typed = Console { id: String::new(), name: " GameCube ".into(), emulator: emulator.clone(), options: "-b -e".into(), folder: games.clone(), endings: vec![".ISO".into(), "rvz".into(), "cue".into(), "bin".into(), "rvz".into(), "bad ending".into()] };
        let console = Console { id: "c1".into(), ..checked(typed.clone()).unwrap() };
        assert_eq!((console.name.as_str(), console.options.as_str()), ("GameCube", "-b -e \"{game}\""));
        assert_eq!(console.endings, ["bin", "cue", "iso", "rvz"]);

        let found = scan(&console);
        assert_eq!(found.iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), ["Comet Crew", "Disc Game", "Third", "Tin Knight"]);
        let knight = &found[3];
        assert!(knight.emulated && knight.platform == "GameCube" && knight.source_id.starts_with("c1-"));
        assert_eq!((knight.exe.as_deref(), knight.install_path.as_path()), (Some(emulator.as_path()), base.join("Emu").as_path()));
        // The path has a space in it and still arrives as one option.
        let file = games.join("Tin Knight (USA).iso").to_string_lossy().into_owned();
        assert_eq!(split_args(&knight.args), ["-b", "-e", file.as_str()]);
        // The track list starts a disc game, not the raw image next to it.
        assert!(found[1].args.ends_with("Disc Game.cue\""));

        assert!(checked(Console { endings: vec!["exe".into()], ..typed.clone() }).is_err());
        assert!(checked(Console { endings: Vec::new(), ..typed.clone() }).is_err());
        assert!(checked(Console { name: " ".into(), ..typed.clone() }).is_err());
        assert!(checked(Console { folder: base.join("nowhere"), ..typed.clone() }).is_err());
        // An emulator that has been removed leaves no cards behind.
        fs::remove_file(&emulator).unwrap();
        assert!(scan(&console).is_empty());
        let _ = fs::remove_dir_all(&base);
    }
}
