//! Helping the administrator find where a game keeps its saves.
//!
//! This only suggests. Nothing here names a folder for the sync: the
//! administrator confirms a suggestion at the desktop with
//! `carla-companion saves set`. No network, no writing; folders are read, and
//! links are never followed.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use super::saves;

/// How far and how long a walk of one root may go.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub depth: usize,
    pub files: usize,
    pub time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { depth: 6, files: 200_000, time: Duration::from_secs(20) }
    }
}

/// How many suggestions are returned.
const TOP: usize = 5;
/// How many folders, and how long, `guess` may look at.
const GUESS_FOLDERS: usize = 20_000;
const GUESS_TIME: Duration = Duration::from_secs(5);

/// The places games usually keep saves, that exist. `env` reads an
/// environment variable; `game_folder` is the game's own install folder.
/// The game's folder and Saved Games come first, so they are walked first.
pub fn known_roots(env: impl Fn(&str) -> Option<String>, game_folder: Option<&Path>) -> Vec<PathBuf> {
    let var = |name: &str| env(name).filter(|v| !v.trim().is_empty()).map(PathBuf::from);
    let mut all: Vec<PathBuf> = Vec::new();
    all.extend(game_folder.map(Path::to_path_buf));
    let home = var("USERPROFILE");
    if let Some(home) = &home {
        all.push(home.join("Saved Games"));
        all.push(home.join("Documents").join("My Games"));
        all.push(home.join("Documents"));
        all.push(home.join("AppData").join("LocalLow"));
    }
    if let Some(local) = var("LOCALAPPDATA") {
        if let Some(parent) = local.parent() {
            all.push(parent.join("LocalLow"));
        }
        all.push(local);
    }
    all.extend(var("APPDATA"));
    all.extend(var("PROGRAMDATA"));
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in all {
        if root.is_dir() && !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

/// Folder names that are never worth looking into. (This also hides Steam's
/// own `userdata` folder.)
fn noisy(name: &str) -> bool {
    let n = name.to_lowercase();
    const EXACT: &[&str] = &[
        "microsoft", "packages", "temp", "tmp", "crashdumps", "crash reports", "crashes", "user data", "nvidia", "nvidia corporation", "amd", "steam", "epic", "epicgameslauncher",
        "unity", "discord", "node_modules", "google", "mozilla", "chrome", "edge", "d3dscache", "windows", "program files", ".git", "claude", "carlaos",
    ];
    EXACT.contains(&n.as_str()) || n.contains("cache") || n.contains("crashpad") || n.contains("browser")
}

/// A link or junction: not followed.
fn is_link(entry: &fs::DirEntry) -> bool {
    let Ok(kind) = entry.file_type() else { return true };
    if kind.is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT: junctions and mount points too.
        if let Ok(meta) = entry.metadata() {
            return meta.file_attributes() & 0x400 != 0;
        }
    }
    false
}

/// Every file under some folders: its change time and size.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub files: HashMap<PathBuf, (Option<SystemTime>, u64)>,
    /// The roots that were walked, in order.
    pub roots: Vec<PathBuf>,
    /// The roots whose walk finished before a limit was reached.
    pub complete: Vec<PathBuf>,
    /// A limit stopped at least one walk before it was done.
    pub cut_short: bool,
}

struct Walk {
    limits: Limits,
    started: Instant,
    count: usize,
}

impl Snapshot {
    /// Each root has its own file and time budget.
    pub fn take(roots: &[PathBuf], limits: Limits) -> Self {
        let mut snap = Snapshot::default();
        for root in roots {
            let mut walk = Walk { limits, started: Instant::now(), count: 0 };
            snap.roots.push(root.clone());
            if snap.visit(&mut walk, root, 0) {
                snap.complete.push(root.clone());
            } else {
                snap.cut_short = true;
            }
        }
        snap
    }

    /// `false` when a limit was reached.
    fn visit(&mut self, walk: &mut Walk, dir: &Path, depth: usize) -> bool {
        let Ok(entries) = fs::read_dir(dir) else { return true };
        for entry in entries.flatten() {
            if walk.count >= walk.limits.files || walk.started.elapsed() >= walk.limits.time {
                return false;
            }
            if is_link(&entry) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let path = entry.path();
            if meta.is_dir() {
                if depth + 1 > walk.limits.depth || noisy(&entry.file_name().to_string_lossy()) {
                    continue;
                }
                if !self.visit(walk, &path, depth + 1) {
                    return false;
                }
            } else {
                walk.count += 1;
                self.files.insert(path, (meta.modified().ok(), meta.len()));
            }
        }
        true
    }
}

/// One suggested save folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub folder: PathBuf,
    /// Files created or changed (0 for a guess from names alone).
    pub changed: usize,
    pub bytes: u64,
    pub score: i32,
    pub reasons: Vec<String>,
}

fn save_like_dir(name: &str) -> bool {
    let n = name.to_lowercase();
    ["save", "slot", "profile"].iter().any(|w| n.contains(w))
}

fn slotty(name: &str) -> bool {
    let n = name.to_lowercase();
    (n.contains("slot") || n.contains("profile")) && !n.starts_with("save")
}

/// The lower-case parts of a path.
fn comps(path: &Path) -> Vec<String> {
    path.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect()
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Whether a file, given by its path below the root, is logs, cache or crash data.
fn is_log_or_cache(rel: &Path) -> bool {
    matches!(extension(rel).as_str(), "log" | "tmp" | "dmp" | "etl" | "txt" | "cache") || comps(rel).iter().any(|c| noise_word(c))
}

fn noise_word(c: &str) -> bool {
    ["cache", "shader", "crash", "telemetry"].iter().any(|w| c.contains(w)) || c == "logs" || c == "log"
}

fn is_config_only(path: &Path) -> bool {
    matches!(extension(path).as_str(), "ini" | "cfg" | "conf" | "xml")
}

/// The words of a title that can tell it apart: lower case, without
/// punctuation, "the", years and roman numerals.
pub fn title_words(title: &str) -> Vec<String> {
    title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && *w != "the")
        .filter(|w| !(w.len() == 4 && w.chars().all(|c| c.is_ascii_digit()) && (w.starts_with("19") || w.starts_with("20"))))
        .map(|w| roman(w).unwrap_or(w).to_string())
        .collect()
}

fn roman(word: &str) -> Option<&'static str> {
    Some(match word {
        "i" => "1",
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "v" => "5",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        "ix" => "9",
        "x" => "10",
        _ => return None,
    })
}

/// A folder or title name reduced so that spelling variants compare equal.
fn normal(name: &str) -> String {
    title_words(name).concat()
}

/// Points for where the folder is, from the root's own name and the parts
/// below it. Nothing above the root counts: a user may be called anything.
fn place(root: &Path, rel: &Path, reasons: &mut Vec<String>) -> i32 {
    let mut parts = root.file_name().map(|n| vec![n.to_string_lossy().to_lowercase()]).unwrap_or_default();
    parts.extend(comps(rel));
    for (name, points, what) in [("saved games", 15, "in Saved Games"), ("my games", 15, "in Documents\\My Games"), ("locallow", 10, "in AppData\\LocalLow"), ("roaming", 5, "in AppData\\Roaming")] {
        if parts.iter().any(|p| p == name) {
            reasons.push(what.to_string());
            return points;
        }
    }
    0
}

/// Words too common to tell a game's folder from any other.
const COMMON: &[&str] = &["game", "games", "data", "local", "save", "saves", "user", "users", "edition", "remastered", "collection", "online", "world", "windows", "steam", "program", "files"];

/// Points when the game's name is in the path below the root.
fn title_score(rel: &Path, title: Option<&str>, reasons: &mut Vec<String>) -> i32 {
    let words: Vec<String> = title.map(title_words).unwrap_or_default().into_iter().filter(|w| w.len() >= 4 && !COMMON.contains(&w.as_str())).collect();
    let parts = comps(rel);
    let hit = words.iter().filter(|w| parts.iter().any(|p| p.contains(w.as_str()))).count();
    if hit > 0 {
        reasons.push("the game's name is in the folder's path".to_string());
        20 + (10 * hit.min(2) as i32)
    } else {
        0
    }
}

fn below(root: &Path, path: &Path) -> bool {
    path != root && path.starts_with(root)
}

/// The folders that changed between two snapshots, best guess first. Only
/// roots that both walks finished are compared: files elsewhere would look
/// new just because a walk stopped early.
pub fn diff(before: &Snapshot, after: &Snapshot, title: Option<&str>) -> Vec<Candidate> {
    let valid: Vec<&PathBuf> = after.roots.iter().filter(|r| after.complete.contains(r) && before.complete.contains(r)).collect();
    // folder -> (its root, files below it that are new or changed)
    let mut groups: HashMap<PathBuf, (PathBuf, Vec<(PathBuf, u64)>)> = HashMap::new();
    for (path, stamp) in &after.files {
        if before.files.get(path) == Some(stamp) {
            continue;
        }
        let Some(root) = valid.iter().filter(|r| path.starts_with(r)).max_by_key(|r| r.components().count()) else { continue };
        let Some(mut folder) = path.parent() else { continue };
        // A file in a slot or profile folder belongs to the folder above it,
        // but never to the root or above.
        while folder.file_name().is_some_and(|n| slotty(&n.to_string_lossy())) && folder.parent().is_some_and(|p| below(root, p)) {
            folder = folder.parent().unwrap();
        }
        // A file directly in a root says nothing about a game's folder.
        if !below(root, folder) {
            continue;
        }
        groups.entry(folder.to_path_buf()).or_insert_with(|| ((*root).clone(), Vec::new())).1.push((path.clone(), stamp.1));
    }
    // A folder inside another changed folder is part of it, when that outer
    // folder is below the same root and could be a save folder itself.
    let mut folders: Vec<PathBuf> = groups.keys().cloned().collect();
    folders.sort_by_key(|f| std::cmp::Reverse(f.components().count()));
    for inner in &folders {
        let Some((root, _)) = groups.get(inner) else { continue };
        let root = root.clone();
        let outer = folders
            .iter()
            .filter(|o| *o != inner && inner.starts_with(o) && below(&root, o) && groups.contains_key(*o))
            .min_by_key(|o| o.components().count())
            .filter(|o| saves::sensible(o) && saves::measure(o).is_some());
        if let Some(outer) = outer {
            if let Some((_, moved)) = groups.remove(inner) {
                groups.get_mut(outer).unwrap().1.extend(moved);
            }
        }
    }
    let mut found: Vec<Candidate> = groups
        .into_iter()
        .filter(|(folder, _)| saves::sensible(folder) && saves::measure(folder).is_some())
        .map(|(folder, (root, files))| {
            let mut reasons = Vec::new();
            let rel = folder.strip_prefix(&root).unwrap_or(&folder).to_path_buf();
            let rel_files: Vec<PathBuf> = files.iter().map(|(p, _)| p.strip_prefix(&root).unwrap_or(p).to_path_buf()).collect();
            let name_of = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let mut score = 0;
            let saveish = rel_files.iter().filter(|p| matches!(extension(p).as_str(), "sav" | "save" | "sl2" | "dat" | "srm" | "slot" | "sgd") || name_of(p).contains("save")).count();
            if saveish > 0 {
                score += 20 + 5 * saveish.min(4) as i32;
                reasons.push(format!("{saveish} file(s) named like saves"));
            }
            let dir_hint = comps(&rel).iter().any(|c| save_like_dir(c)) || rel_files.iter().any(|p| p.parent().is_some_and(|d| comps(d).iter().any(|c| save_like_dir(c))));
            if dir_hint {
                score += 15;
                reasons.push("save, slot or profile in the path".to_string());
            }
            let bytes: u64 = files.iter().map(|(_, b)| *b).sum();
            if bytes / files.len().max(1) as u64 <= 1024 * 1024 {
                score += 5;
                reasons.push("small files, as saves are".to_string());
            }
            score += place(&root, &rel, &mut reasons);
            score += title_score(&rel, title, &mut reasons);
            let logs = rel_files.iter().filter(|p| is_log_or_cache(p)).count();
            if logs * 2 >= files.len() || comps(&rel).iter().any(|c| noise_word(c)) {
                score -= 40;
                reasons.push("looks like logs, cache or crash data".to_string());
            }
            if files.iter().all(|(p, _)| is_config_only(p)) {
                score -= 20;
                reasons.push("only settings files changed".to_string());
            }
            Candidate { folder, changed: files.len(), bytes, score, reasons }
        })
        .collect();
    found.sort_by(|a, b| b.score.cmp(&a.score).then(b.changed.cmp(&a.changed)).then(a.folder.cmp(&b.folder)));
    found.truncate(TOP);
    found
}

/// What `guess` has used up.
struct Budget {
    started: Instant,
    folders: usize,
}

impl Budget {
    fn spent(&mut self) -> bool {
        self.folders += 1;
        self.folders > GUESS_FOLDERS || self.started.elapsed() >= GUESS_TIME
    }
}

/// Folders whose name matches the title, without any playing. Looks at most
/// three folders deep under each root, and for a bounded time. A name that
/// only starts like the title ("Hades II" for "Hades") is offered only when
/// nothing matches exactly.
pub fn guess(title: &str, roots: &[PathBuf]) -> Vec<Candidate> {
    let wanted = normal(title);
    if wanted.is_empty() {
        return Vec::new();
    }
    let (mut exact, mut near) = (Vec::new(), Vec::new());
    let mut budget = Budget { started: Instant::now(), folders: 0 };
    for root in roots {
        // The root itself may be the game's folder: only its save folders count.
        let root_is_game = root.file_name().is_some_and(|n| normal(&n.to_string_lossy()) == wanted);
        let mut ctx = Look { root, wanted: &wanted, exact: &mut exact, near: &mut near, budget: &mut budget };
        ctx.look(root, 0, if root_is_game { Kind::Exact } else { Kind::None });
    }
    let mut found = if exact.is_empty() { near } else { exact };
    found.sort_by(|a, b| b.score.cmp(&a.score).then(a.folder.cmp(&b.folder)));
    found.dedup_by(|a, b| a.folder == b.folder);
    found.truncate(TOP);
    found
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    None,
    Exact,
    Near,
}

struct Look<'a> {
    root: &'a Path,
    wanted: &'a str,
    exact: &'a mut Vec<Candidate>,
    near: &'a mut Vec<Candidate>,
    budget: &'a mut Budget,
}

impl Look<'_> {
    fn look(&mut self, dir: &Path, depth: usize, inside: Kind) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            if self.budget.spent() {
                return;
            }
            if is_link(&entry) || !entry.metadata().is_ok_and(|m| m.is_dir()) {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if noisy(&name) {
                continue;
            }
            let n = normal(&name);
            let kind = if n == self.wanted {
                Kind::Exact
            } else if n.len() >= 5 && self.wanted.len() >= 5 && (n.starts_with(self.wanted) || self.wanted.starts_with(&n)) {
                Kind::Near
            } else {
                Kind::None
            };
            let mut here = inside;
            if kind != Kind::None {
                here = kind;
                let has_save_child = fs::read_dir(&path).into_iter().flatten().flatten().any(|c| c.metadata().is_ok_and(|m| m.is_dir()) && save_like_dir(&c.file_name().to_string_lossy()));
                if !has_save_child {
                    let (base, why) = if kind == Kind::Exact { (40, "its name is the game's name") } else { (25, "its name is close to the game's name") };
                    self.add(&path, kind, base, why);
                }
            } else if inside != Kind::None && save_like_dir(&name) {
                self.add(&path, inside, 55, "a save folder inside the game's folder");
            }
            if depth + 1 < 3 {
                self.look(&path, depth + 1, here);
            }
        }
    }

    fn add(&mut self, folder: &Path, kind: Kind, base: i32, why: &str) {
        if !saves::sensible(folder) {
            return;
        }
        let Some(m) = saves::measure(folder) else { return };
        let mut reasons = vec![why.to_string()];
        let rel = folder.strip_prefix(self.root).unwrap_or(folder);
        let score = base + place(self.root, rel, &mut reasons);
        let list = if kind == Kind::Exact { &mut *self.exact } else { &mut *self.near };
        list.push(Candidate { folder: folder.to_path_buf(), changed: 0, bytes: m.bytes, score, reasons });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        // No word like "save" in the name: scoring must not see the test's own path.
        let dir = std::env::temp_dir().join(format!("cfx-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn env_of(home: &Path) -> impl Fn(&str) -> Option<String> {
        let home = home.to_string_lossy().to_string();
        move |name| match name {
            "USERPROFILE" => Some(home.clone()),
            "LOCALAPPDATA" => Some(format!("{home}\\AppData\\Local")),
            "APPDATA" => Some(format!("{home}\\AppData\\Roaming")),
            _ => None,
        }
    }

    #[test]
    fn roots_are_the_existing_known_places() {
        let home = scratch("roots");
        fs::create_dir_all(home.join("Saved Games")).unwrap();
        fs::create_dir_all(home.join("AppData").join("Local")).unwrap();
        fs::create_dir_all(home.join("AppData").join("LocalLow")).unwrap();
        let game = home.join("game");
        fs::create_dir_all(&game).unwrap();
        let roots = known_roots(env_of(&home), Some(&game));
        assert_eq!(roots[0], game);
        assert_eq!(roots[1], home.join("Saved Games"));
        assert!(roots.contains(&home.join("AppData").join("LocalLow")));
        assert!(!roots.contains(&home.join("Documents")));
        assert_eq!(roots.iter().filter(|r| **r == home.join("AppData").join("LocalLow")).count(), 1);
        assert!(known_roots(|_| None, None).is_empty());
    }

    #[test]
    fn the_save_folder_beats_logs_and_cache() {
        let home = scratch("diff");
        let root = home.join("Saved Games");
        put(&root.join("Other").join("old.txt"), "x");
        let roots = [root.clone(), home.clone()];
        let before = Snapshot::take(&roots, Limits::default());
        put(&root.join("Hades").join("Profile1.sav"), "save");
        put(&root.join("Hades").join("Profile2.sav"), "save");
        put(&root.join("Hades").join("output.log"), "log");
        put(&home.join("Logs").join("game.log"), "log");
        put(&home.join("ShaderStuff").join("a.dat"), "x");
        put(&home.join("Settings").join("game.ini"), "x");
        let after = Snapshot::take(&roots, Limits::default());
        let found = diff(&before, &after, Some("Hades"));
        assert_eq!(found[0].folder, root.join("Hades"));
        assert_eq!(found[0].changed, 3);
        assert!(found[0].score > found[1].score);
        assert!(found.iter().all(|c| !c.reasons.is_empty()));
        assert!(found.iter().all(|c| c.folder != root.join("Other")));
    }

    #[test]
    fn slot_folders_belong_to_their_parent() {
        let home = scratch("slots");
        let roots = [home.clone()];
        let before = Snapshot::take(&roots, Limits::default());
        put(&home.join("Game").join("slot1").join("a.dat"), "x");
        put(&home.join("Game").join("slot2").join("b.dat"), "x");
        let after = Snapshot::take(&roots, Limits::default());
        let found = diff(&before, &after, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].folder, home.join("Game"));
        assert_eq!(found[0].changed, 2);
    }

    #[test]
    fn a_changed_file_in_the_root_does_not_swallow_the_save_folder() {
        // The root is a game's install folder: a log there, and the saves below.
        let game = scratch("install-root").join("Nova");
        let roots = [game.clone()];
        put(&game.join("nova.exe"), "x");
        let before = Snapshot::take(&roots, Limits::default());
        put(&game.join("output.log"), "x");
        put(&game.join("settings.ini"), "x");
        put(&game.join("Saves").join("a.sav"), "x");
        let after = Snapshot::take(&roots, Limits::default());
        let found = diff(&before, &after, Some("Nova"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].folder, game.join("Saves"));
        // A changed folder above another one (but below the root) still takes it in.
        let home = scratch("outer");
        let roots = [home.clone()];
        let before = Snapshot::take(&roots, Limits::default());
        put(&home.join("Game").join("a.ini"), "x");
        put(&home.join("Game").join("Saves").join("b.sav"), "x");
        let found = diff(&before, &Snapshot::take(&roots, Limits::default()), None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].folder, home.join("Game"));
    }

    #[test]
    fn the_path_above_the_root_never_scores() {
        // The root's own path says "saves", "slot", "profile" and "hades".
        let root = scratch("leak").join("saves slot profile hades");
        let roots = [root.clone()];
        put(&root.join("keep").join("x.bin"), "x");
        let before = Snapshot::take(&roots, Limits::default());
        put(&root.join("Plain").join("a.bin"), "x");
        let found = diff(&before, &Snapshot::take(&roots, Limits::default()), Some("Hades"));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].score, 5, "{:?}", found[0].reasons);
        assert_eq!(found[0].reasons, vec!["small files, as saves are".to_string()]);
    }

    #[test]
    fn a_title_word_that_is_common_does_not_match() {
        let root = scratch("common");
        let roots = [root.clone()];
        let before = Snapshot::take(&roots, Limits::default());
        put(&root.join("Data").join("a.bin"), "x");
        let found = diff(&before, &Snapshot::take(&roots, Limits::default()), Some("The Game Data"));
        assert!(found[0].reasons.iter().all(|r| !r.contains("name")));
    }

    #[test]
    fn a_root_that_was_cut_short_is_not_compared() {
        let home = scratch("partial");
        for i in 0..5 {
            put(&home.join(format!("f{i}.sav")), "x");
        }
        let roots = [home.clone()];
        let before = Snapshot::take(&roots, Limits { files: 2, ..Limits::default() });
        assert!(before.cut_short);
        put(&home.join("New").join("a.sav"), "x");
        let after = Snapshot::take(&roots, Limits::default());
        assert!(diff(&before, &after, None).is_empty());
    }

    #[test]
    fn guess_is_tolerant_about_names() {
        let home = scratch("guess");
        let docs = home.join("Documents").join("My Games");
        put(&docs.join("Alpha Quest II").join("a.sav"), "x");
        put(&docs.join("Unrelated").join("b.sav"), "x");
        put(&home.join("Saved Games").join("The Beta-Game (2019)").join("c.sav"), "x");
        let roots = known_roots(env_of(&home), None);
        let a = guess("alpha quest 2", &roots);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].folder, docs.join("Alpha Quest II"));
        let b = guess("Beta Game", &roots);
        assert_eq!(b[0].folder, home.join("Saved Games").join("The Beta-Game (2019)"));
        assert!(guess("Gamma", &roots).is_empty());
        assert!(guess("   ", &roots).is_empty());
    }

    #[test]
    fn guess_offers_a_longer_name_only_when_nothing_else_matches() {
        let home = scratch("near");
        let docs = home.join("Documents");
        put(&docs.join("Hades II").join("a.sav"), "x");
        let roots = known_roots(env_of(&home), None);
        assert_eq!(guess("Hades", &roots).len(), 1);
        put(&docs.join("Hades").join("a.sav"), "x");
        let found = guess("Hades", &roots);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].folder, docs.join("Hades"));
    }

    #[test]
    fn guess_prefers_a_save_folder_in_the_game_folder() {
        let home = scratch("install");
        let game = home.join("Games").join("Nova");
        put(&game.join("nova.exe"), "x");
        put(&game.join("Saves").join("a.sav"), "x");
        let found = guess("Nova", &[game.clone()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].folder, game.join("Saves"));
    }

    #[test]
    fn noisy_trees_are_skipped() {
        let home = scratch("noisy");
        put(&home.join("Microsoft").join("a.sav"), "x");
        put(&home.join("node_modules").join("b.sav"), "x");
        put(&home.join("GPUCache").join("c.sav"), "x");
        put(&home.join("Fine").join("d.sav"), "x");
        let snap = Snapshot::take(&[home.clone()], Limits::default());
        assert_eq!(snap.files.len(), 1);
        assert!(snap.files.contains_key(&home.join("Fine").join("d.sav")));
    }

    #[test]
    fn limits_are_respected() {
        let home = scratch("limits");
        for i in 0..10 {
            put(&home.join(format!("f{i}.sav")), "x");
        }
        put(&home.join("a").join("b").join("c").join("deep.sav"), "x");
        let few = Snapshot::take(&[home.clone()], Limits { files: 3, ..Limits::default() });
        assert_eq!(few.files.len(), 3);
        assert!(few.cut_short);
        assert!(few.complete.is_empty());
        let shallow = Snapshot::take(&[home.clone()], Limits { depth: 2, ..Limits::default() });
        assert_eq!(shallow.files.len(), 10);
        assert!(!shallow.cut_short);
        let late = Snapshot::take(&[home.clone()], Limits { time: Duration::ZERO, ..Limits::default() });
        assert!(late.cut_short);
        assert!(late.files.is_empty());
        // Each root has its own budget.
        let other = scratch("limits2");
        put(&other.join("x.sav"), "x");
        let two = Snapshot::take(&[home.clone(), other.clone()], Limits { files: 3, ..Limits::default() });
        assert!(two.files.contains_key(&other.join("x.sav")));
        assert_eq!(two.complete, vec![other]);
    }

    #[cfg(windows)]
    #[test]
    fn junctions_are_not_followed() {
        let home = scratch("links");
        put(&home.join("real").join("a.sav"), "x");
        let link = home.join("link");
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(home.join("real")).output().map(|o| o.status.success()).unwrap_or(false);
        assert!(made, "mklink /J failed, so the junction test could not run");
        let snap = Snapshot::take(&[home.clone()], Limits::default());
        assert_eq!(snap.files.len(), 1);
        assert!(!snap.files.contains_key(&link.join("a.sav")));
    }

    #[test]
    fn a_folder_that_is_not_sensible_is_dropped() {
        let root = PathBuf::from("C:\\Windows");
        let mut after = Snapshot { roots: vec![root.clone()], complete: vec![root], ..Snapshot::default() };
        after.files.insert(PathBuf::from("C:\\Windows\\System32\\x.sav"), (None, 1));
        let before = Snapshot { roots: after.roots.clone(), complete: after.complete.clone(), ..Snapshot::default() };
        assert!(diff(&before, &after, None).is_empty());
    }

    #[test]
    fn title_words_drop_noise() {
        assert_eq!(title_words("The Witcher III: Wild Hunt (2015)"), vec!["witcher", "3", "wild", "hunt"]);
    }
}
