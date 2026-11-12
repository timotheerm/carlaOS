//! Save games: where they are, when the player last saved, and backups.
//!
//! CarlaOS never edits a save. It copies the whole save folder aside after a
//! play session that changed it, keeps the last few copies, and can put one
//! back. Putting one back first copies what is there now, so a restore can
//! itself be undone.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

/// Backups kept per game. Older ones are deleted.
pub const KEEP: usize = 5;
/// Save folders larger than this are not copied: that is not a save folder,
/// or it holds something (recordings, caches) that would fill the disk.
pub const LARGEST: u64 = 512 * 1024 * 1024;
const MOST_FILES: u64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Measure {
    pub files: u64,
    pub bytes: u64,
    /// Unix seconds of the newest file.
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Backup {
    /// Folder name of the backup; also the Unix time it was taken.
    pub id: String,
    pub at: i64,
    pub files: u64,
    pub bytes: u64,
}

/// What the interface shows about one game's saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Overview {
    pub folder: String,
    pub files: u64,
    pub bytes: u64,
    pub modified: Option<i64>,
    pub backups: Vec<Backup>,
}

fn walk(dir: &Path, visit: &mut dyn FnMut(&Path, &fs::Metadata) -> bool) -> bool {
    let Ok(entries) = fs::read_dir(dir) else { return true };
    for entry in entries.flatten() {
        // Links are not followed: a save folder must not lead elsewhere.
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        let go_on = if meta.is_dir() { walk(&path, visit) } else { visit(&path, &meta) };
        if !go_on {
            return false;
        }
    }
    true
}

/// Count and size a save folder. `None` when it is too big to be one.
pub fn measure(dir: &Path) -> Option<Measure> {
    let mut m = Measure::default();
    let complete = walk(dir, &mut |_, meta| {
        m.files += 1;
        m.bytes += meta.len();
        let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
        m.modified = m.modified.max(modified);
        m.files <= MOST_FILES && m.bytes <= LARGEST
    });
    complete.then_some(m)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            continue;
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn problem(what: &str, e: std::io::Error) -> String {
    format!("{what}: {e}.")
}

/// Where a Steam game keeps its Steam Cloud saves on this machine, for the
/// account that played last.
pub fn steam_folder(steam_root: &Path, app_id: &str) -> Option<PathBuf> {
    if app_id.is_empty() || !app_id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    fs::read_dir(steam_root.join("userdata"))
        .ok()?
        .flatten()
        .map(|user| user.path().join(app_id).join("remote"))
        .filter(|dir| dir.is_dir())
        .max_by_key(|dir| measure(dir).and_then(|m| m.modified))
}

/// The backups of one game, kept under `<store>/<game>`.
pub struct Backups {
    dir: PathBuf,
}

impl Backups {
    /// `store` is CarlaOS's own folder for all backups.
    pub fn new(store: &Path, game_id: &str) -> Self {
        // Game ids contain ':' and could contain anything; keep the folder
        // name plain.
        let safe: String = game_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
        Self { dir: store.join(safe) }
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Backup> {
        let mut list: Vec<Backup> = fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let id = entry.file_name().to_string_lossy().into_owned();
                let at: i64 = id.split('-').next()?.parse().ok()?;
                let m = measure(&entry.path())?;
                Some(Backup { id, at, files: m.files, bytes: m.bytes })
            })
            .collect();
        list.sort_by(|a, b| b.id.cmp(&a.id));
        list
    }

    pub fn overview(&self, folder: &Path) -> Option<Overview> {
        let m = measure(folder)?;
        Some(Overview { folder: folder.to_string_lossy().into_owned(), files: m.files, bytes: m.bytes, modified: m.modified, backups: self.list() })
    }

    /// Copy the save folder aside. With `only_if_changed`, does nothing when
    /// no file is newer than the latest backup. Returns the new backup.
    pub fn take(&self, folder: &Path, now: i64, only_if_changed: bool) -> Result<Option<Backup>, String> {
        self.take_keeping(folder, now, only_if_changed, None)
    }

    /// As [`take`](Self::take), but never deletes the backup named `keep`.
    fn take_keeping(&self, folder: &Path, now: i64, only_if_changed: bool, keep: Option<&str>) -> Result<Option<Backup>, String> {
        let Some(m) = measure(folder) else {
            return Err("The save folder is too large to back up.".to_string());
        };
        if m.files == 0 {
            return Ok(None);
        }
        let existing = self.list();
        if only_if_changed && existing.first().is_some_and(|latest| m.modified.unwrap_or(0) <= latest.at) {
            return Ok(None);
        }
        // Two backups in the same second get different names.
        let mut id = format!("{now:012}");
        let mut n = 1;
        while self.dir.join(&id).exists() {
            n += 1;
            id = format!("{now:012}-{n}");
        }
        let target = self.dir.join(&id);
        // Copy beside and rename, so a half-made backup is never listed.
        let partial = self.dir.join(format!("partial-{id}"));
        let _ = fs::remove_dir_all(&partial);
        let copied = copy_tree(folder, &partial).and_then(|()| fs::rename(&partial, &target));
        if let Err(e) = copied {
            let _ = fs::remove_dir_all(&partial);
            return Err(problem("The saves could not be backed up", e));
        }
        for old in self.list().into_iter().skip(KEEP).filter(|old| Some(old.id.as_str()) != keep) {
            let _ = fs::remove_dir_all(self.dir.join(old.id));
        }
        Ok(Some(Backup { id, at: now, files: m.files, bytes: m.bytes }))
    }

    /// Put a backup back. What is in the save folder now is backed up first.
    pub fn restore(&self, folder: &Path, backup_id: &str, now: i64) -> Result<(), String> {
        let wanted = self.list().into_iter().find(|b| b.id == backup_id).ok_or("That backup no longer exists.")?;
        let source = self.dir.join(&wanted.id);
        // Even when it is the oldest, the wanted backup survives this.
        self.take_keeping(folder, now, false, Some(&wanted.id))?;
        // Clear the folder's contents, not the folder: the game may hold it.
        for entry in fs::read_dir(folder).map_err(|e| problem("The save folder could not be read", e))?.flatten() {
            let path = entry.path();
            let removed = if entry.file_type().is_ok_and(|k| k.is_dir()) { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            removed.map_err(|e| problem("An existing save could not be replaced. Is the game still running?", e))?;
        }
        copy_tree(&source, folder).map_err(|e| problem("The backup could not be copied back", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("carla-saves-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("saves").join("slot1")).unwrap();
        dir
    }

    fn read(dir: &Path, file: &str) -> Option<String> {
        fs::read_to_string(dir.join(file)).ok()
    }

    #[test]
    fn a_backup_is_a_full_copy_and_a_restore_can_be_undone() {
        let root = scratch("restore");
        let saves = root.join("saves");
        fs::write(saves.join("profile.dat"), "level 3").unwrap();
        fs::write(saves.join("slot1").join("world.sav"), "castle").unwrap();
        let backups = Backups::new(&root.join("backups"), "steam:480");

        let m = measure(&saves).unwrap();
        assert_eq!((m.files, m.bytes), (2, 13));
        let first = backups.take(&saves, 5_000_000_000, false).unwrap().unwrap();
        assert_eq!((first.files, first.bytes), (2, 13));
        assert!(backups.take(&saves, 5_000_000_001, true).unwrap().is_none(), "nothing changed since");

        // The player carries on, then wants the old state back.
        fs::write(saves.join("profile.dat"), "level 9").unwrap();
        fs::write(saves.join("new.dat"), "extra").unwrap();
        fs::remove_file(saves.join("slot1").join("world.sav")).unwrap();
        backups.restore(&saves, &first.id, 5_000_000_100).unwrap();
        assert_eq!(read(&saves, "profile.dat").as_deref(), Some("level 3"));
        assert_eq!(read(&saves, "slot1/world.sav").as_deref(), Some("castle"));
        assert_eq!(read(&saves, "new.dat"), None, "files that were not in the backup are gone");

        // What was replaced was itself backed up, so the restore can be undone.
        let list = backups.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].at, 5_000_000_100);
        backups.restore(&saves, &list[0].id, 5_000_000_200).unwrap();
        assert_eq!(read(&saves, "profile.dat").as_deref(), Some("level 9"));
        assert_eq!(read(&saves, "new.dat").as_deref(), Some("extra"));

        assert!(backups.restore(&saves, "nope", 1).is_err());
        let overview = backups.overview(&saves).unwrap();
        assert_eq!((overview.files, overview.backups.len()), (2, 3));
    }

    #[test]
    fn only_the_newest_backups_are_kept() {
        let root = scratch("keep");
        let saves = root.join("saves");
        fs::write(saves.join("a"), "1").unwrap();
        let backups = Backups::new(&root.join("backups"), "manual:../../etc");
        assert!(backups.dir.starts_with(root.join("backups")), "a game id cannot point outside the store");
        for n in 0..(KEEP as i64 + 3) {
            backups.take(&saves, 6_000_000_000 + n, false).unwrap();
        }
        let list = backups.list();
        assert_eq!(list.len(), KEEP);
        assert_eq!(list[0].at, 6_000_000_000 + KEEP as i64 + 2);
        // Same second twice still makes two backups.
        backups.take(&saves, 7_000_000_000, false).unwrap();
        backups.take(&saves, 7_000_000_000, false).unwrap();
        assert_eq!(backups.list()[0].id, "007000000000-2");

        // Restoring the oldest kept backup works although a new one is made first.
        let oldest = backups.list().last().unwrap().id.clone();
        fs::write(saves.join("a"), "2").unwrap();
        backups.restore(&saves, &oldest, 8_000_000_000).unwrap();
        assert_eq!(read(&saves, "a").as_deref(), Some("1"));
    }

    #[test]
    fn an_empty_or_missing_folder_is_not_backed_up() {
        let root = scratch("empty");
        let backups = Backups::new(&root.join("backups"), "g");
        assert_eq!(backups.take(&root.join("saves"), 1, false).unwrap(), None);
        assert_eq!(backups.take(&root.join("missing"), 1, false).unwrap(), None);
        assert!(backups.list().is_empty());
    }

    #[test]
    fn steam_cloud_saves_are_found_for_the_account_that_played_last() {
        let root = scratch("steam");
        assert!(steam_folder(&root, "480").is_none());
        let old = root.join("userdata").join("111").join("480").join("remote");
        let new = root.join("userdata").join("222").join("480").join("remote");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("save.dat"), "x").unwrap();
        assert_eq!(steam_folder(&root, "480"), Some(new));
        assert!(steam_folder(&root, "..").is_none());
    }
}
