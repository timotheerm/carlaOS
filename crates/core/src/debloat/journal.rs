//! The record of every change CarlaOS made to Windows, with how to undo it.
//!
//! It is a plain JSON file so an administrator can read it, and so a remote
//! administrator can fetch it. A damaged journal is reported, never replaced:
//! it is the only place the previous values live.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::classify::Kind;
use super::model::Op;
use super::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    pub item_id: String,
    pub title: String,
    pub kind: Kind,
    /// Seconds since 1970, UTC.
    pub applied_at: u64,
    /// What was done.
    pub done: Op,
    /// What puts it back.
    pub undo: Op,
    #[serde(default)]
    pub undone_at: Option<u64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    version: u32,
    entries: Vec<Entry>,
}

#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    file: File,
}

impl Journal {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let file = match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                Error::Journal(format!("{} cannot be read ({e}). It was left untouched.", path.display()))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => File { version: 1, entries: Vec::new() },
            Err(e) => return Err(Error::Journal(format!("{}: {e}", path.display()))),
        };
        Ok(Self { path: path.to_path_buf(), file })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.file.entries
    }

    /// Changes that are in effect right now, oldest first.
    pub fn active(&self) -> impl Iterator<Item = &Entry> {
        self.file.entries.iter().filter(|e| e.undone_at.is_none())
    }

    pub fn record(&mut self, item_id: &str, title: &str, kind: Kind, at: u64, done: Op, undo: Op) {
        let seq = self.file.entries.last().map_or(1, |e| e.seq + 1);
        self.file.entries.push(Entry {
            seq,
            item_id: item_id.to_string(),
            title: title.to_string(),
            kind,
            applied_at: at,
            done,
            undo,
            undone_at: None,
        });
    }

    pub fn mark_undone(&mut self, seq: u64, at: u64) {
        if let Some(e) = self.file.entries.iter_mut().find(|e| e.seq == seq) {
            e.undone_at = Some(at);
        }
    }

    /// Write to a temporary file first, so a crash never leaves half a journal.
    pub fn save(&self) -> Result<(), Error> {
        let io = |e: std::io::Error| Error::Journal(format!("{}: {e}", self.path.display()));
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(io)?;
        }
        let text = serde_json::to_string_pretty(&self.file).map_err(|e| Error::Journal(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, text).map_err(io)?;
        fs::rename(&tmp, &self.path).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debloat::model::StartMode;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("carla-journal-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("journal.json")
    }

    fn svc(start: StartMode) -> Op {
        Op::ServiceSet { name: "DiagTrack".into(), start, running: None }
    }

    #[test]
    fn records_survive_a_reload() {
        let path = temp("reload");
        let mut j = Journal::load(&path).unwrap();
        assert_eq!(j.entries().len(), 0);
        j.record("svc:DiagTrack", "Usage data collection", Kind::Service, 100, svc(StartMode::Disabled), svc(StartMode::Auto));
        j.record("svc:DiagTrack", "Usage data collection", Kind::Service, 101, svc(StartMode::Disabled), svc(StartMode::Auto));
        j.save().unwrap();

        let mut again = Journal::load(&path).unwrap();
        assert_eq!(again.entries(), j.entries());
        assert_eq!(again.entries()[1].seq, 2);
        again.mark_undone(1, 200);
        again.save().unwrap();
        let last = Journal::load(&path).unwrap();
        assert_eq!(last.active().map(|e| e.seq).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn a_damaged_journal_is_an_error_and_is_not_overwritten() {
        let path = temp("damaged");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert!(matches!(Journal::load(&path), Err(Error::Journal(_))));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
    }
}
