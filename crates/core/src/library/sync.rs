//! Keeping one game's saves the same on two machines.
//!
//! Both machines describe their save folder as a list of files with a
//! fingerprint each. Whichever side changed since the two last agreed is
//! copied onto the other; when both changed, the one saved last wins. Before
//! anything is overwritten the receiving side keeps a safe copy, so the losing
//! saves can always be put back.
//!
//! This module knows nothing about networks: a [`Side`] is a folder here or a
//! desktop at the other end of a link.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::saves::Backups;

/// A save folder with more files than this is not synchronised.
pub const MOST_FILES: usize = 2000;
/// One file is sent in one piece, so it has to stay reasonable.
pub const LARGEST_FILE: u64 = 32 * 1024 * 1024;
/// All the files of one game together.
pub const LARGEST: u64 = 512 * 1024 * 1024;

/// Ending of a file that is still being written.
const PART: &str = ".carla-part";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn fingerprint(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// One file of a save folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Relative to the save folder, with `/` between folders on every system.
    pub path: String,
    pub size: u64,
    /// Unix seconds.
    pub modified: i64,
    /// SHA-256 of the contents, hex.
    pub hash: String,
}

/// Everything in a save folder, sorted by path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub files: Vec<Entry>,
}

impl Manifest {
    /// One fingerprint for the whole folder: equal exactly when the same
    /// files hold the same contents. Times do not count.
    pub fn digest(&self) -> String {
        let mut all = Sha256::new();
        for f in &self.files {
            all.update(f.path.as_bytes());
            all.update([0]);
            all.update(f.hash.as_bytes());
            all.update([0]);
        }
        hex(&all.finalize())
    }

    /// When the newest file was written.
    pub fn newest(&self) -> Option<i64> {
        self.files.iter().map(|f| f.modified).max()
    }

    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Whether this could be the description of a real save folder. What a
    /// desktop sends is checked with this before it is believed.
    pub fn sound(&self) -> bool {
        self.files.len() <= MOST_FILES
            && self.bytes() <= LARGEST
            && self.files.iter().all(|f| f.size <= LARGEST_FILE && parts(&f.path).is_some() && f.hash.len() == 64 && f.hash.bytes().all(|b| b.is_ascii_hexdigit()))
            && self.files.windows(2).all(|pair| pair[0].path < pair[1].path)
    }
}

/// The folders and file name of a relative path, when it is a plain one:
/// nothing that climbs out, starts at a root or names a drive.
fn parts(relative: &str) -> Option<Vec<&str>> {
    if relative.is_empty() || relative.len() > 400 || relative.contains(['\\', ':', '\0']) {
        return None;
    }
    let parts: Vec<&str> = relative.split('/').collect();
    let plain = |p: &&str| !p.is_empty() && *p != "." && *p != ".." && !p.ends_with([' ', '.']) && !p.chars().any(|c| c.is_control() || "<>\"|?*".contains(c));
    parts.iter().all(plain).then_some(parts)
}

/// Where a relative path lands inside a save folder. `None` for anything that
/// would land outside it, or that passes through a link.
pub fn inside(folder: &Path, relative: &str) -> Option<PathBuf> {
    let mut path = folder.to_path_buf();
    for part in parts(relative)? {
        // A link inside the folder could lead anywhere.
        if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
            return None;
        }
        path.push(part);
    }
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return None;
    }
    // Belt and braces: the result is still below the folder.
    path.strip_prefix(folder).ok().filter(|rest| rest.components().all(|c| matches!(c, Component::Normal(_))))?;
    Some(path)
}

fn too_big() -> String {
    "The save folder is too large to keep in step between machines.".to_string()
}

fn list(folder: &Path, dir: &Path, out: &mut Vec<Entry>, bytes: &mut u64) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(dir) else { return Ok(()) };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        } else if kind.is_dir() {
            list(folder, &path, out, bytes)?;
        } else if kind.is_file() {
            let Ok(meta) = entry.metadata() else { continue };
            let Ok(relative) = path.strip_prefix(folder) else { continue };
            let relative = relative.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            // A name the other system could not hold is left where it is, and
            // so is a file this module was interrupted while writing.
            if parts(&relative).is_none() || relative.ends_with(PART) {
                continue;
            }
            *bytes += meta.len();
            if out.len() >= MOST_FILES || meta.len() > LARGEST_FILE || *bytes > LARGEST {
                return Err(too_big());
            }
            let mut contents = Vec::new();
            fs::File::open(&path).and_then(|mut f| f.read_to_end(&mut contents)).map_err(|e| format!("A save could not be read: {e}."))?;
            let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            out.push(Entry { path: relative, size: contents.len() as u64, modified, hash: fingerprint(&contents) });
        }
    }
    Ok(())
}

/// Describe a save folder. A folder that does not exist yet is empty.
pub fn manifest(folder: &Path) -> Result<Manifest, String> {
    let mut files = Vec::new();
    list(folder, folder, &mut files, &mut 0)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Manifest { files })
}

/// One of the two machines.
pub trait Side {
    fn manifest(&self) -> Result<Manifest, String>;
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
    /// Write one file, with the time it was saved on the other machine.
    fn write(&self, path: &str, contents: &[u8], modified: i64) -> Result<(), String>;
    fn remove(&self, path: &str) -> Result<(), String>;
    /// Keep a safe copy of what is there now. Called before anything changes.
    fn protect(&self) -> Result<(), String>;
}

/// A save folder on this machine, with its safe copies.
pub struct Folder {
    pub folder: PathBuf,
    pub backups: Backups,
    pub now: i64,
}

fn outside() -> String {
    "That is not a file in the save folder.".to_string()
}

impl Side for Folder {
    fn manifest(&self) -> Result<Manifest, String> {
        manifest(&self.folder)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let file = inside(&self.folder, path).ok_or_else(outside)?;
        if fs::metadata(&file).map_err(|_| outside())?.len() > LARGEST_FILE {
            return Err(too_big());
        }
        fs::read(&file).map_err(|e| format!("A save could not be read: {e}."))
    }

    fn write(&self, path: &str, contents: &[u8], modified: i64) -> Result<(), String> {
        if contents.len() as u64 > LARGEST_FILE {
            return Err(too_big());
        }
        let file = inside(&self.folder, path).ok_or_else(outside)?;
        let problem = |e: std::io::Error| format!("A save could not be written. Is the game still running? ({e})");
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(problem)?;
        }
        // Written beside and renamed, so the game never finds half a save.
        let beside = file.with_file_name(format!("{}{PART}", file.file_name().unwrap_or_default().to_string_lossy()));
        let written = fs::write(&beside, contents).and_then(|()| {
            let when = UNIX_EPOCH + Duration::from_secs(modified.max(0) as u64);
            fs::File::options().write(true).open(&beside)?.set_modified(when)?;
            fs::rename(&beside, &file)
        });
        if written.is_err() {
            let _ = fs::remove_file(&beside);
        }
        written.map_err(problem)
    }

    fn remove(&self, path: &str) -> Result<(), String> {
        let file = inside(&self.folder, path).ok_or_else(outside)?;
        match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("An old save could not be removed. Is the game still running? ({e})")),
            _ => {
                // Folders the removal left empty go too, up to the save folder.
                let mut dir = file.parent();
                while let Some(d) = dir.filter(|d| *d != self.folder && d.starts_with(&self.folder)) {
                    if fs::remove_dir(d).is_err() {
                        break;
                    }
                    dir = d.parent();
                }
                Ok(())
            }
        }
    }

    fn protect(&self) -> Result<(), String> {
        self.backups.take(&self.folder, self.now, false).map(|_| ())
    }
}

/// Which way the saves should go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Way {
    /// Both machines already hold the same saves.
    Same,
    /// This machine's saves go to the other one.
    Send,
    /// The other machine's saves come here.
    Fetch,
}

/// What the two folders looked like when they last agreed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agreed {
    pub here: String,
    pub there: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub way: Way,
    /// Both sides had changed, so the one saved last was taken. The other
    /// side's saves are in its safe copies.
    pub both_changed: bool,
}

/// Decide which way to copy. `agreed` is what both looked like after the last
/// time they were brought in step, when that ever happened.
pub fn decide(here: &Manifest, there: &Manifest, agreed: Option<&Agreed>) -> Decision {
    let (h, t) = (here.digest(), there.digest());
    let plain = |way| Decision { way, both_changed: false };
    if h == t {
        return plain(Way::Same);
    }
    // A machine with no saves at all never wipes the other's.
    match (here.files.is_empty(), there.files.is_empty()) {
        (true, false) => return plain(Way::Fetch),
        (false, true) => return plain(Way::Send),
        _ => {}
    }
    if let Some(agreed) = agreed {
        match (agreed.here == h, agreed.there == t) {
            (true, false) => return plain(Way::Fetch),
            (false, true) => return plain(Way::Send),
            _ => {}
        }
    }
    // Both changed, or they never agreed: the saves written last are the ones
    // the player means. A tie goes to this machine, which is in their hands.
    let way = if there.newest() > here.newest() { Way::Fetch } else { Way::Send };
    Decision { way, both_changed: true }
}

/// What a copy moved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Moved {
    pub files: usize,
    pub bytes: u64,
    pub removed: usize,
}

/// Make `to` hold exactly what `from` holds. Only files that differ travel.
/// `to` keeps a safe copy first. A file that arrives damaged stops everything
/// before any old save is removed.
pub fn mirror(from: &dyn Side, wanted: &Manifest, to: &dyn Side, existing: &Manifest) -> Result<Moved, String> {
    if !wanted.sound() {
        return Err("The other machine described its saves in a way CarlaOS does not trust.".to_string());
    }
    let has = |e: &Entry| existing.files.iter().any(|x| x.path == e.path && x.hash == e.hash);
    let changed: Vec<&Entry> = wanted.files.iter().filter(|e| !has(e)).collect();
    let gone: Vec<&Entry> = existing.files.iter().filter(|x| !wanted.files.iter().any(|e| e.path == x.path)).collect();
    if changed.is_empty() && gone.is_empty() {
        return Ok(Moved::default());
    }
    to.protect()?;
    let mut moved = Moved::default();
    for entry in changed {
        let contents = from.read(&entry.path)?;
        if contents.len() as u64 != entry.size || fingerprint(&contents) != entry.hash {
            return Err("A save changed or was damaged on the way. Nothing was removed; try again when the game is closed.".to_string());
        }
        to.write(&entry.path, &contents, entry.modified)?;
        moved.files += 1;
        moved.bytes += entry.size;
    }
    for entry in gone {
        to.remove(&entry.path)?;
        moved.removed += 1;
    }
    Ok(moved)
}

/// What bringing two machines in step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub way: Way,
    pub both_changed: bool,
    pub moved: Moved,
    /// What both hold now, to remember for next time.
    pub agreed: Agreed,
}

/// Compare this machine's saves with the other's and copy whichever way is
/// right. `agreed` is what the last call returned for the same two machines.
pub fn bring_in_step(here: &dyn Side, there: &dyn Side, agreed: Option<&Agreed>) -> Result<Outcome, String> {
    let (h, t) = (here.manifest()?, there.manifest()?);
    if !t.sound() {
        return Err("The other machine described its saves in a way CarlaOS does not trust.".to_string());
    }
    let decision = decide(&h, &t, agreed);
    let (moved, kept) = match decision.way {
        Way::Same => (Moved::default(), &h),
        Way::Send => (mirror(here, &h, there, &t)?, &h),
        Way::Fetch => (mirror(there, &t, here, &h)?, &t),
    };
    let digest = kept.digest();
    Ok(Outcome { way: decision.way, both_changed: decision.both_changed, moved, agreed: Agreed { here: digest.clone(), there: digest } })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Pair {
        base: PathBuf,
        ally: Folder,
        desk: Folder,
    }

    fn pair(name: &str) -> Pair {
        let base = std::env::temp_dir().join(format!("carla-sync-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let side = |who: &str| {
            let folder = base.join(who).join("saves");
            fs::create_dir_all(&folder).unwrap();
            Folder { folder, backups: Backups::new(&base.join(who).join("copies"), "game:1"), now: 5_000 }
        };
        Pair { ally: side("ally"), desk: side("desk"), base }
    }

    fn put(side: &Folder, path: &str, contents: &str, modified: i64) {
        side.write(path, contents.as_bytes(), modified).unwrap();
    }

    fn sync(here: &Folder, there: &Folder, agreed: Option<&Agreed>) -> (Decision, Moved, Agreed) {
        let done = bring_in_step(here, there, agreed).unwrap();
        let now = Agreed { here: here.manifest().unwrap().digest(), there: there.manifest().unwrap().digest() };
        assert_eq!(now.here, now.there, "after a sync both sides hold the same");
        assert_eq!(done.agreed, now, "and that is what is remembered");
        (Decision { way: done.way, both_changed: done.both_changed }, done.moved, done.agreed)
    }

    #[test]
    fn saves_follow_the_player_from_one_machine_to_the_other_and_back() {
        let p = pair("follow");
        put(&p.ally, "slot1.sav", "ally day 1", 1_000);
        put(&p.ally, "profile/settings.ini", "volume=7", 1_000);

        // First time: the desktop has nothing, so it receives.
        let (d, moved, agreed) = sync(&p.ally, &p.desk, None);
        assert_eq!((d.way, d.both_changed, moved.files), (Way::Send, false, 2));
        assert_eq!(fs::read_to_string(p.desk.folder.join("profile").join("settings.ini")).unwrap(), "volume=7");
        assert_eq!(p.desk.manifest().unwrap().files[1].modified, 1_000, "the time it was saved travels with it");

        // Nothing changed: nothing moves, and no safe copy is made for nothing.
        let (d, moved, agreed) = sync(&p.ally, &p.desk, Some(&agreed));
        assert_eq!((d.way, moved), (Way::Same, Moved::default()));
        assert!(p.desk.backups.list().is_empty());

        // Played on the desktop, even with a clock that runs behind: it only
        // matters that the desktop changed and the Ally did not.
        put(&p.desk, "slot1.sav", "desk day 2", 900);
        fs::remove_file(p.desk.folder.join("profile").join("settings.ini")).unwrap();
        put(&p.desk, "slot2.sav", "new slot", 900);
        let (d, moved, agreed) = sync(&p.ally, &p.desk, Some(&agreed));
        assert_eq!((d.way, d.both_changed), (Way::Fetch, false));
        assert_eq!((moved.files, moved.removed), (2, 1));
        assert_eq!(fs::read_to_string(p.ally.folder.join("slot1.sav")).unwrap(), "desk day 2");
        assert!(!p.ally.folder.join("profile").exists(), "an emptied folder is tidied away");
        // What the Ally had before is in its safe copies.
        let copy = &p.ally.backups.list()[0];
        assert_eq!(copy.files, 2);

        // Played on the Ally next: it goes back the other way.
        put(&p.ally, "slot1.sav", "ally day 3", 2_000);
        let (d, ..) = sync(&p.ally, &p.desk, Some(&agreed));
        assert_eq!(d.way, Way::Send);
        assert_eq!(fs::read_to_string(p.desk.folder.join("slot1.sav")).unwrap(), "ally day 3");
        let _ = fs::remove_dir_all(&p.base);
    }

    #[test]
    fn when_both_changed_the_last_save_wins_and_the_other_is_kept() {
        let p = pair("both");
        put(&p.ally, "slot1.sav", "start", 1_000);
        let (.., agreed) = sync(&p.ally, &p.desk, None);
        put(&p.ally, "slot1.sav", "played offline on the ally", 2_000);
        put(&p.desk, "slot1.sav", "played later on the desktop", 3_000);
        let (d, _, _) = sync(&p.ally, &p.desk, Some(&agreed));
        assert_eq!((d.way, d.both_changed), (Way::Fetch, true));
        assert_eq!(fs::read_to_string(p.ally.folder.join("slot1.sav")).unwrap(), "played later on the desktop");
        // The Ally's own evening is not lost.
        let copy = p.ally.backups.list().remove(0);
        let kept = p.base.join("ally").join("copies").join("game_1").join(copy.id).join("slot1.sav");
        assert_eq!(fs::read_to_string(kept).unwrap(), "played offline on the ally");

        // Two machines that never agreed and both have saves: same rule.
        let q = pair("never");
        put(&q.ally, "a.sav", "ally", 9_000);
        put(&q.desk, "a.sav", "desk", 8_000);
        let d = decide(&q.ally.manifest().unwrap(), &q.desk.manifest().unwrap(), None);
        assert_eq!((d.way, d.both_changed), (Way::Send, true));
        let _ = fs::remove_dir_all(&p.base);
        let _ = fs::remove_dir_all(&q.base);
    }

    #[test]
    fn nothing_reaches_outside_the_save_folder() {
        let p = pair("outside");
        let secret = p.base.join("ally").join("secret.txt");
        fs::write(&secret, "not a save").unwrap();
        for path in ["../secret.txt", "/etc/passwd", "a/../../secret.txt", "C:/Windows/x", "a\\b", "", ".", "x/./y", "con ", "bad?.sav"] {
            assert!(inside(&p.ally.folder, path).is_none(), "{path:?} must be refused");
            assert!(p.ally.read(path).is_err());
            assert!(p.ally.write(path, b"x", 1).is_err());
            assert!(p.ally.remove(path).is_err());
        }
        assert_eq!(fs::read_to_string(&secret).unwrap(), "not a save");
        #[cfg(unix)]
        {
            // A link planted in the save folder is neither listed nor followed.
            std::os::unix::fs::symlink(p.base.join("ally"), p.ally.folder.join("escape")).unwrap();
            assert!(p.ally.manifest().unwrap().files.is_empty());
            assert!(inside(&p.ally.folder, "escape/secret.txt").is_none());
            assert!(p.ally.write("escape/secret.txt", b"overwritten", 1).is_err());
            assert_eq!(fs::read_to_string(&secret).unwrap(), "not a save");
        }
        // A description with a path that climbs out is not believed at all.
        let lying = Manifest { files: vec![Entry { path: "../secret.txt".into(), size: 1, modified: 1, hash: fingerprint(b"x") }] };
        assert!(!lying.sound());
        assert!(mirror(&p.desk, &lying, &p.ally, &Manifest::default()).is_err());
        let _ = fs::remove_dir_all(&p.base);
    }

    #[test]
    fn a_save_damaged_on_the_way_stops_the_copy_before_anything_is_removed() {
        let p = pair("damaged");
        put(&p.ally, "old.sav", "keep me", 1_000);
        put(&p.desk, "new.sav", "fresh", 2_000);
        let mut wanted = p.desk.manifest().unwrap();
        wanted.files[0].hash = fingerprint(b"something else");
        let failed = mirror(&p.desk, &wanted, &p.ally, &p.ally.manifest().unwrap());
        assert!(failed.is_err());
        assert_eq!(fs::read_to_string(p.ally.folder.join("old.sav")).unwrap(), "keep me");
        assert!(!p.ally.folder.join("new.sav").exists());
        let _ = fs::remove_dir_all(&p.base);
    }

    #[test]
    fn a_folder_that_is_no_save_folder_is_refused() {
        let p = pair("big");
        for i in 0..=MOST_FILES {
            fs::write(p.ally.folder.join(format!("{i}.sav")), b"x").unwrap();
        }
        assert!(p.ally.manifest().is_err());
        let _ = fs::remove_dir_all(&p.base);
    }
}
