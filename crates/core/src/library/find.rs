//! Helping the administrator add a game that no store lists: a native port,
//! an unofficial Windows game, a game started through a launcher.
//!
//! [`find`] looks through the folders games are usually kept in, and through
//! any folder the administrator names, and returns the programs most likely
//! to be the game, best first. The real game program is preferred over a
//! launcher in the same folder. When a launcher is offered as well, it says
//! which program the real game runs as (`watch_hint`), because a launcher
//! that hands over to the game and exits would otherwise end the session.
//!
//! Nothing is started here and nothing is read but file names, folder names
//! and the details a program carries about itself. No network.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::discover::{helper, lower, squash, stem, DEPTH, MOST_FILES, MOST_FOUND};
use super::{launch, pe, ports, shortcut};

/// What a candidate is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FoundKind {
    /// The program of the game itself.
    Game,
    /// A launcher, bootstrapper or script that starts the game.
    Launcher,
    /// A shortcut that leads to the game.
    Shortcut,
}

/// A program that may be a game, with why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub title: String,
    /// What to start: a program, a script or a shortcut.
    pub exe: PathBuf,
    /// The game's folder.
    pub folder: PathBuf,
    pub kind: FoundKind,
    /// Higher is more likely to be what the administrator wants.
    pub score: i32,
    /// Why it was picked, in a few words.
    pub note: String,
    /// The real game's program name, when this starts it through something
    /// that may exit first. Pass it as `--watch`.
    pub watch_hint: Option<String>,
}

/// A candidate under a "Program Files" folder must be at least this sure:
/// that folder is full of programs that are not games.
const STRICT: i32 = 90;
const MOST_FOLDERS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Unreal's `*-Win64-Shipping.exe` or Unity's player.
    Strong,
    Plain,
    Launcher,
    Script,
    Link,
}

struct Entry {
    path: PathBuf,
    depth: usize,
    class: Class,
    note: &'static str,
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn skipped_folder(name: &str) -> bool {
    let name = name.to_lowercase();
    ["redist", "directx", "dotnet", "installer", "crashpad"].iter().any(|w| name.contains(w))
}

fn walk(dir: &Path, depth: usize, left: &mut usize, out: &mut Vec<(PathBuf, usize)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if *left == 0 {
            return;
        }
        *left -= 1;
        let path = entry.path();
        // Links are not followed: a folder must not lead elsewhere.
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if depth < DEPTH && !skipped_folder(&entry.file_name().to_string_lossy()) {
                walk(&path, depth + 1, left, out);
            }
        } else if kind.is_file() && matches!(extension(&path).as_str(), "exe" | "bat" | "cmd" | "lnk") {
            out.push((path, depth));
        }
    }
}

/// What a name says about being a launcher.
fn launcher_name(path: &Path) -> bool {
    let name = stem(path);
    let words = ["launch", "start", "play", "run", "loader", "starter", "autorun"];
    ["launcher", "bootstrap", "prelaunch"].iter().any(|w| name.contains(w)) || name.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| words.contains(&w))
}

fn strong_note(path: &Path) -> Option<&'static str> {
    let name = stem(path);
    if ["-win64-shipping", "-win32-shipping", "-wingdk-shipping"].iter().any(|s| name.ends_with(s)) {
        return Some("The game's own program (Unreal Engine)");
    }
    let plain = path.file_stem()?.to_string_lossy().into_owned();
    if path.parent()?.join(format!("{plain}_Data")).is_dir() {
        return Some("The game's own program (Unity)");
    }
    None
}

fn classify(path: PathBuf, depth: usize) -> Option<Entry> {
    let (class, note) = match extension(&path).as_str() {
        "exe" if helper(&path) => return None,
        "exe" => match strong_note(&path) {
            Some(note) => (Class::Strong, note),
            None if launcher_name(&path) => (Class::Launcher, "A launcher"),
            None => (Class::Plain, "The main program in the folder"),
        },
        "bat" | "cmd" if helper(&path) => return None,
        "bat" | "cmd" => (Class::Script, "A script that starts the game"),
        "lnk" if helper(&path) => return None,
        "lnk" => {
            // Only a shortcut that still leads somewhere, and not to a helper.
            let target = shortcut::read(&path).ok()?.target;
            if !target.is_file() || helper(&target) || shortcut::is_shortcut(&target) {
                return None;
            }
            (Class::Link, "A shortcut to the game")
        }
        _ => return None,
    };
    Some(Entry { path, depth, class, note })
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn folder_title(folder: &Path) -> String {
    folder.file_name().map(|n| n.to_string_lossy().replace(['_', '-'], " ").split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default()
}

fn title_of(folder: &Path, entry: &Entry) -> String {
    match entry.class {
        Class::Link | Class::Script => {
            let own = entry.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if matches!(entry.class, Class::Link) {
                own
            } else {
                Some(folder_title(folder)).filter(|t| !t.is_empty()).unwrap_or(own)
            }
        }
        _ => {
            // A bootstrapper often calls itself by the engine's name.
            let product = pe::product_name(&entry.path).map(|p| p.trim().to_string()).filter(|p| !p.is_empty() && !p.to_lowercase().contains("bootstrap"));
            match (product, entry.class) {
                (Some(product), _) => product,
                (None, Class::Strong | Class::Launcher) => Some(folder_title(folder)).filter(|t| !t.is_empty()).unwrap_or_else(|| ports::suggest_title(&entry.path)),
                _ => ports::suggest_title(&entry.path),
            }
        }
    }
}

/// The candidates of one game folder: the best, and a launcher that starts it.
fn rank(folder: &Path) -> Vec<Candidate> {
    let mut left = MOST_FILES;
    let mut files = Vec::new();
    walk(folder, 0, &mut left, &mut files);
    let mut entries: Vec<Entry> = files.into_iter().filter_map(|(path, depth)| classify(path, depth)).collect();
    // Next to the real program, the other programs are only there to start it.
    if entries.iter().any(|e| e.class == Class::Strong) {
        for e in entries.iter_mut().filter(|e| e.class == Class::Plain) {
            e.class = Class::Launcher;
            e.note = "Starts the game";
        }
    }
    let name = squash(&folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let score = |e: &Entry| -> i32 {
        let depth = e.depth as i32;
        match e.class {
            Class::Strong => 100 - depth * 2,
            Class::Plain => {
                let same = squash(&stem(&e.path));
                let named = !same.is_empty() && !name.is_empty() && (same == name || name.starts_with(&same) || same.starts_with(&name));
                50 - depth * 3 + if named { 15 } else { 0 } + (size(&e.path) / (8 << 20)).min(10) as i32
            }
            Class::Link => 40 - depth * 3,
            Class::Script => 35 - depth * 3,
            Class::Launcher => 30 - depth * 2,
        }
    };
    let mut scored: Vec<(i32, Entry)> = entries.into_iter().map(|e| (score(&e), e)).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| size(&b.1.path).cmp(&size(&a.1.path))).then_with(|| a.1.path.cmp(&b.1.path)));
    // The real game, if the folder has one.
    let real = scored.iter().find(|(_, e)| matches!(e.class, Class::Strong | Class::Plain)).and_then(|(_, e)| launch::valid_watch(&file_name(&e.path)));
    let candidate = |score: i32, e: &Entry, hint: Option<String>| Candidate {
        title: title_of(folder, e),
        exe: e.path.clone(),
        folder: folder.to_path_buf(),
        kind: match e.class {
            Class::Strong | Class::Plain => FoundKind::Game,
            Class::Launcher | Class::Script => FoundKind::Launcher,
            Class::Link => FoundKind::Shortcut,
        },
        score,
        note: match &hint {
            Some(real) => format!("{}; the game itself runs as {real}", e.note),
            None => e.note.to_string(),
        },
        watch_hint: hint,
    };
    let mut out = Vec::new();
    if let Some((best, e)) = scored.first() {
        let hint = |e: &Entry| if matches!(e.class, Class::Launcher | Class::Script) { real.clone().filter(|r| *r != file_name(&e.path)) } else { None };
        out.push(candidate(*best, e, hint(e)));
        // The launcher too, when the game is the best: some games only work
        // when started through it. It is watched by the game's program name.
        if let Some((s, l)) = scored.iter().skip(1).find(|(_, l)| matches!(l.class, Class::Launcher | Class::Script) && hint(l).is_some()) {
            if matches!(e.class, Class::Strong | Class::Plain) {
                out.push(candidate(*s, l, hint(l)));
            }
        }
    }
    out
}

fn subfolders(dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|k| k.is_dir())).map(|e| e.path()).collect();
    list.sort();
    list.truncate(MOST_FOLDERS);
    list
}

/// Whether a program, script or shortcut lies directly in the folder.
fn holds_program(dir: &Path) -> bool {
    fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.file_type().is_ok_and(|k| k.is_file()) && classify(e.path(), 0).is_some())
}

/// Look for games. Each folder inside a folder of `places` is taken for one
/// game. Each of `given` is a game's own folder when a program lies directly
/// in it, and a folder of games otherwise.
pub fn find(places: &[PathBuf], given: &[PathBuf]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut done: HashSet<String> = HashSet::new();
    let mut take = |folder: &Path, strict: bool, out: &mut Vec<Candidate>| {
        if !store_managed(folder) && done.insert(lower(folder)) {
            out.extend(rank(folder).into_iter().filter(|c| !strict || c.score >= STRICT));
        }
    };
    for root in given {
        if holds_program(root) {
            take(root, false, &mut out);
        } else {
            for folder in subfolders(root) {
                take(&folder, false, &mut out);
            }
        }
    }
    for root in places {
        let strict = lower(root).contains("program files");
        for folder in subfolders(root) {
            take(&folder, strict, &mut out);
        }
    }
    let mut seen = HashSet::new();
    out.retain(|c| seen.insert(lower(&c.exe)));
    out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase())));
    out.truncate(MOST_FOUND);
    out
}

/// The folders games are usually kept in on this machine.
pub fn default_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from).filter(|p| p.is_dir());
    if cfg!(windows) {
        for letter in 'C'..='Z' {
            let drive = PathBuf::from(format!("{letter}:\\"));
            if drive.is_dir() {
                for name in ["Games", "Game", "Jeux", "Ports", "PC Ports", "itch"] {
                    roots.push(drive.join(name));
                }
            }
        }
    }
    if let Some(home) = env("USERPROFILE").or_else(|| env("HOME")) {
        for name in ["Games", "Ports"] {
            roots.push(home.join(name));
            roots.push(home.join("Documents").join(name));
        }
    }
    for name in ["ProgramFiles", "ProgramFiles(x86)"] {
        roots.extend(env(name));
    }
    let mut seen = HashSet::new();
    roots.retain(|p| p.is_dir() && seen.insert(lower(p)));
    roots
}

/// Whether a text can sit inside quotes in PowerShell and in cmd without any
/// part of it being read as something to run or expand.
fn safe_to_paste(text: &str, path: bool) -> bool {
    !text.is_empty() && text.chars().all(|c| !hidden(c) && (c.is_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-' | '(' | ')' | '+' | ',') || (path && matches!(c, '\\' | ':' | '/'))))
}

/// Characters that show as nothing, or change the way the text around them
/// reads: control characters, soft hyphen, zero-width and bidirectional marks.
fn hidden(c: char) -> bool {
    c.is_control() || matches!(c, '\u{AD}' | '\u{61C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}' | '\u{FEFF}')
}

/// A text as it may be shown in a terminal: without those characters.
pub fn visible(text: &str) -> String {
    text.chars().filter(|c| !hidden(*c)).collect()
}

/// A line to paste into a terminal that adds a candidate. `None` when the
/// title, the path or the watched name holds anything but letters, digits,
/// spaces and `. _ - ( ) + ,` (a path may also hold `\ : /`): such a line is
/// never offered, because a terminal could read the rest as a command
/// (`$(...)`, a backtick, `%VAR%`, a quote), or it could look like something
/// it is not (right-to-left marks, zero-width characters).
pub fn add_line(c: &Candidate) -> Option<String> {
    let exe = c.exe.to_string_lossy();
    if !safe_to_paste(&c.title, false) || !safe_to_paste(&exe, true) || !c.watch_hint.as_deref().is_none_or(|w| safe_to_paste(w, false)) {
        return None;
    }
    let mut line = format!("carla-companion add \"{}\" \"{exe}\"", c.title);
    if let Some(watch) = &c.watch_hint {
        line.push_str(&format!(" --watch \"{watch}\""));
    }
    Some(line)
}

/// Where a store keeps the games it installed: they are listed already.
fn store_managed(path: &Path) -> bool {
    let path = format!("{}\\", lower(path).trim_end_matches('\\'));
    ["\\steamapps\\", "\\epic games\\", "\\gog galaxy\\"].iter().any(|w| path.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &Path, padding: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; padding]).unwrap();
    }

    fn base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("carla-find-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_real_game_beats_its_launcher_and_the_launcher_says_what_to_watch() {
        let games = base("rank").join("Games");
        // Unreal: a stub in the root, the real program deep inside.
        file(&games.join("Tin Knight").join("TinKnight.exe"), 10);
        file(&games.join("Tin Knight").join("TinKnight").join("Binaries").join("Win64").join("TinKnight-Win64-Shipping.exe"), 5000);
        file(&games.join("Tin Knight").join("Engine").join("Extras").join("Redist").join("en-us").join("UEPrereqSetup_x64.exe"), 5000);
        // Unity: the player next to its data folder, and its crash handler.
        file(&games.join("Moon Garden").join("MoonGarden.exe"), 100);
        fs::create_dir_all(games.join("Moon Garden").join("MoonGarden_Data")).unwrap();
        file(&games.join("Moon Garden").join("UnityCrashHandler64.exe"), 9000);
        // A launcher in the root and the game in a folder of its own.
        file(&games.join("Pixel Quest").join("Launcher.exe"), 50);
        file(&games.join("Pixel Quest").join("bin").join("Quest.exe"), 4000);
        file(&games.join("Pixel Quest").join("vc_redist.x64.exe"), 90000);
        file(&games.join("Pixel Quest").join("unins000.exe"), 90000);
        // Only a script.
        file(&games.join("Old Port").join("run.bat"), 5);
        // Nothing worth adding.
        file(&games.join("Tools").join("setup.exe"), 5);

        let found = find(&[games.clone()], &[]);
        let names: Vec<String> = found.iter().map(|c| file_name(&c.exe)).collect();
        for gone in ["vc_redist.x64.exe", "unins000.exe", "UnityCrashHandler64.exe", "setup.exe", "UEPrereqSetup_x64.exe"] {
            assert!(!names.contains(&gone.to_string()), "{gone} is not a game: {names:?}");
        }
        let get = |n: &str| found.iter().find(|c| file_name(&c.exe) == n).unwrap_or_else(|| panic!("{n} in {names:?}")).clone();

        let shipping = get("TinKnight-Win64-Shipping.exe");
        assert_eq!((shipping.kind, shipping.title.as_str(), shipping.watch_hint.clone()), (FoundKind::Game, "Tin Knight", None));
        let stub = get("TinKnight.exe");
        assert_eq!((stub.kind, stub.watch_hint.as_deref()), (FoundKind::Launcher, Some("TinKnight-Win64-Shipping.exe")));
        assert!(shipping.score > stub.score);

        let unity = get("MoonGarden.exe");
        assert_eq!((unity.kind, unity.watch_hint.clone(), unity.title.as_str()), (FoundKind::Game, None, "Moon Garden"));
        assert!(!names.iter().any(|n| n.starts_with("Unity")));

        let quest = get("Quest.exe");
        let launcher = get("Launcher.exe");
        assert_eq!((quest.kind, launcher.kind), (FoundKind::Game, FoundKind::Launcher));
        assert_eq!(launcher.watch_hint.as_deref(), Some("Quest.exe"));
        assert!(quest.score > launcher.score);

        let script = get("run.bat");
        assert_eq!((script.kind, script.watch_hint.clone()), (FoundKind::Launcher, None));
        // Best first.
        assert!(found.windows(2).all(|w| w[0].score >= w[1].score));
        assert!(found[0].exe.ends_with("MoonGarden.exe"));
        let line = add_line(&launcher).unwrap();
        assert!(line.starts_with("carla-companion add \"Pixel Quest\" \"") && line.ends_with("--watch \"Quest.exe\""), "{line}");
        assert!(!add_line(&quest).unwrap().contains("--watch"));
        // A name a terminal would read as a command gets no line to paste.
        for bad in ["Foo $(calc)", "Foo `calc`", "100% %PATH%", "Say \"hi\"", "It's", "a;b", "a&b", "a|b", "a<b", "a^b", "a\nb", "a\u{202E}b", "a\u{200B}b", "\u{FEFF}a", "a\u{AD}b", "a\u{2066}b", "a\u{61C}b", "\u{2018}a\u{2019}", "x \u{201C}y\u{201D}", "a\tb", "a!b{c}", "a@b#c", "a=b"] {
            assert!(add_line(&Candidate { title: bad.into(), ..quest.clone() }).is_none(), "title {bad:?}");
            assert!(add_line(&Candidate { exe: PathBuf::from(format!("D:/Games/{bad}/q.exe")), ..quest.clone() }).is_none(), "path {bad:?}");
            assert!(add_line(&Candidate { watch_hint: Some(bad.into()), ..quest.clone() }).is_none(), "watch {bad:?}");
        }
        // A backslash ends a quoted word in some terminals: not in a title or a watched name.
        for bad in ["Title\\", "a\\b"] {
            assert!(add_line(&Candidate { title: bad.into(), ..quest.clone() }).is_none(), "title {bad:?}");
            assert!(add_line(&Candidate { watch_hint: Some(bad.into()), ..quest.clone() }).is_none(), "watch {bad:?}");
        }
        assert!(add_line(&Candidate { title: String::new(), ..quest.clone() }).is_none());
        // Names in other scripts and with accents are fine.
        for good in ["Caf\u{e9} \u{d1}and\u{fa} (2)", "\u{30b2}\u{30fc}\u{30e0} 1", "\u{418}\u{433}\u{440}\u{430}, part-2 +", "Pixel Quest 2.0_x"] {
            let line = add_line(&Candidate { title: good.into(), exe: PathBuf::from(format!("D:\\Games\\{good}\\q.exe")), ..quest.clone() });
            assert!(line.is_some_and(|l| l.contains(good)), "{good}");
        }
        assert_eq!(visible("a\u{202E}b\u{200B}c\u{FEFF}d\te\u{e9}\u{30b2}"), "abcde\u{e9}\u{30b2}");
        // Games a store installed are not offered, and a hint that is not a safe name is dropped.
        file(&games.join("steamapps").join("common").join("Stored").join("stored.exe"), 100);
        file(&games.join("Epic Games").join("Fortnite").join("f.exe"), 100);
        file(&games.join("Sys Game").join("Game.exe"), 100);
        file(&games.join("Sys Game").join("Start.exe"), 100);
        file(&games.join("Sys Game").join("explorer.exe"), 9000);
        let again = find(&[games.clone()], &[]);
        assert!(!again.iter().any(|c| matches!(file_name(&c.exe).as_str(), "stored.exe" | "f.exe")));
        assert!(again.iter().all(|c| c.watch_hint.as_deref() != Some("explorer.exe")));
        let _ = fs::remove_dir_all(games.parent().unwrap());
    }

    #[test]
    fn a_named_folder_is_a_game_or_a_folder_of_games_and_shortcuts_count() {
        let root = base("given");
        let game = root.join("One Game");
        file(&game.join("one.exe"), 100);
        file(&game.join("Play One.lnk"), 0); // not a valid shortcut: ignored
        let found = find(&[], &[game.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].kind, found[0].folder.clone()), (FoundKind::Game, game.clone()));
        // The same folder of games, named one level up.
        file(&root.join("Two").join("two.exe"), 100);
        let both = find(&[], &[root.clone()]);
        assert_eq!(both.len(), 2);
        // A folder that is not there finds nothing and nothing is run.
        assert!(find(&[root.join("missing")], &[root.join("missing")]).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn program_files_only_offers_what_is_surely_a_game() {
        let pf = base("pf").join("Program Files");
        file(&pf.join("Office").join("word.exe"), 9000);
        file(&pf.join("Some Game").join("SomeGame.exe"), 100);
        fs::create_dir_all(pf.join("Some Game").join("SomeGame_Data")).unwrap();
        let found = find(&[pf.clone()], &[]);
        assert_eq!(found.iter().map(|c| file_name(&c.exe)).collect::<Vec<_>>(), ["SomeGame.exe"]);
        let _ = fs::remove_dir_all(pf.parent().unwrap());
    }
}
