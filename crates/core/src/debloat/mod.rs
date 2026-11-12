//! Clean-up of Windows on the handheld: find what can be removed or switched
//! off, apply what the administrator picks, and undo any of it later.
//!
//! Three promises shape this module:
//!
//! 1. **Default-deny.** Only things named in [`catalogue`] can be changed, and
//!    ASUS, Xbox, security, driver and controller parts are protected by
//!    pattern on top of that.
//! 2. **Every change has an exact opposite.** The helper reports the state it
//!    found just before changing something; the opposite operation is written
//!    to the [`journal`] before anything else happens. A change whose previous
//!    state cannot be restored is not made.
//! 3. **Nothing is decided by the UI.** The UI sends item ids. What an id
//!    stands for is worked out here, from a fresh scan.

pub mod catalogue;
mod classify;
mod journal;
mod model;
mod powershell;

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

pub use catalogue::Tier;
pub use classify::{classify, Item, Kind, Planned};
pub use journal::{Entry, Journal};
pub use model::{ApplyResponse, Before, Inventory, Op, RegValue, StartMode, StartupLocation, Step, StepResult};
pub use powershell::PowerShell;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Not running on Windows.
    Unsupported,
    /// The administrator said no to the Windows approval prompt.
    Declined,
    /// The helper script failed or answered with something unexpected.
    Helper(String),
    /// The journal could not be read or written.
    Journal(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported => write!(f, "Cleaning up Windows only works on Windows."),
            Error::Declined => write!(f, "Windows did not get administrator approval."),
            Error::Helper(e) => write!(f, "The Windows helper failed: {e}"),
            Error::Journal(e) => write!(f, "The change journal has a problem: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Scan,
    Apply,
}

/// Runs the helper. [`PowerShell`] is the real one; tests use a fake.
pub trait Runner {
    /// `elevate` asks Windows for administrator approval first.
    fn run(&self, mode: Mode, request: &Value, elevate: bool) -> Result<Value, Error>;
}

/// A change CarlaOS made earlier that is still in effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub item_id: String,
    pub title: String,
    pub kind: Kind,
    pub applied_at: u64,
    pub needs_admin: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub items: Vec<Item>,
    pub changes: Vec<Change>,
    /// Parts of the scan that could not be read.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub id: String,
    pub title: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Outcome {
    /// Ids of the items that were fully applied or fully undone.
    pub done: Vec<String>,
    pub failed: Vec<Failure>,
    /// Some of it only takes full effect after a restart.
    pub restart: bool,
}

pub struct Debloat<R> {
    runner: R,
    journal: PathBuf,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl<R: Runner> Debloat<R> {
    pub fn new(runner: R, journal: PathBuf) -> Self {
        Self { runner, journal }
    }

    fn inventory(&self) -> Result<Inventory, Error> {
        let settings: Vec<Value> = catalogue::SETTINGS
            .iter()
            .flat_map(|s| s.values)
            .map(|v| json!({ "path": v.path, "name": v.name }))
            .collect();
        let startup: Vec<Value> = catalogue::STARTUP_SOURCES
            .iter()
            .map(|s| json!({ "location": s.location, "run": s.run, "folder": s.folder, "approved": s.approved }))
            .collect();
        let answer = self.runner.run(Mode::Scan, &json!({ "settings": settings, "startup": startup }), false)?;
        serde_json::from_value(answer).map_err(|e| Error::Helper(format!("unexpected scan result ({e})")))
    }

    /// Look at the machine. Changes nothing.
    pub fn scan(&self) -> Result<Report, Error> {
        let journal = Journal::load(&self.journal)?;
        let inventory = self.inventory()?;
        let items = classify(&inventory).into_iter().map(|p| p.item).collect();
        Ok(Report { items, changes: changes(&journal), warnings: inventory.errors })
    }

    /// Run one batch and answer for every step, whatever went wrong.
    fn run_batch(&self, steps: &[Step], elevate: bool) -> HashMap<usize, Result<Before, String>> {
        let mut out = HashMap::new();
        if steps.is_empty() {
            return out;
        }
        let answer = self
            .runner
            .run(Mode::Apply, &json!({ "ops": steps }), elevate)
            .and_then(|v| {
                serde_json::from_value::<ApplyResponse>(v)
                    .map_err(|e| Error::Helper(format!("unexpected result ({e})")))
            });
        match answer {
            Ok(response) => {
                for r in response.results {
                    let result = if r.ok {
                        Ok(r.before.unwrap_or_default())
                    } else {
                        Err(r.error.unwrap_or_else(|| "Windows refused the change.".to_string()))
                    };
                    out.insert(r.id, result);
                }
                for s in steps {
                    out.entry(s.id).or_insert_with(|| Err("The helper did not answer for this change.".to_string()));
                }
            }
            Err(e) => {
                for s in steps {
                    out.insert(s.id, Err(e.to_string()));
                }
            }
        }
        out
    }

    /// Apply the items with these ids. Ids that are unknown, protected or
    /// already tidy fail individually; the rest go ahead.
    pub fn apply(&self, ids: &[String]) -> Result<Outcome, Error> {
        // Refuse to change anything unless the undo record can be written.
        let mut journal = Journal::load(&self.journal)?;
        journal.save()?;

        let planned = classify(&self.inventory()?);
        let mut outcome = Outcome::default();
        let mut chosen: Vec<&Planned> = Vec::new();
        for id in ids {
            if chosen.iter().any(|p| &p.item.id == id) {
                continue;
            }
            match planned.iter().find(|p| &p.item.id == id) {
                Some(p) if !p.ops.is_empty() && matches!(p.item.tier, Tier::Recommended | Tier::Optional) => {
                    chosen.push(p)
                }
                Some(p) => outcome.failed.push(Failure {
                    id: id.clone(),
                    title: p.item.title.clone(),
                    error: "CarlaOS does not change this.".to_string(),
                }),
                None => outcome.failed.push(Failure {
                    id: id.clone(),
                    title: id.clone(),
                    error: "Not found on this device any more.".to_string(),
                }),
            }
        }

        // Number every operation, remembering which item it belongs to.
        let mut owner: Vec<&Planned> = Vec::new();
        let mut steps: Vec<Step> = Vec::new();
        for p in &chosen {
            for op in &p.ops {
                steps.push(Step { id: owner.len(), op: op.clone() });
                owner.push(p);
            }
        }

        let mut errors: HashMap<&str, String> = HashMap::new();
        for elevate in [false, true] {
            let batch: Vec<Step> = steps.iter().filter(|s| s.op.needs_admin() == elevate).cloned().collect();
            let results = self.run_batch(&batch, elevate);
            for step in &batch {
                let p = owner[step.id];
                let recorded = match &results[&step.id] {
                    Ok(before) => match step.op.inverse(before, p.store_id) {
                        Some(undo) => {
                            journal.record(&p.item.id, &p.item.title, p.item.kind, now(), step.op.clone(), undo);
                            Ok(())
                        }
                        None => Err("Changed, but the previous state was not reported.".to_string()),
                    },
                    Err(e) => Err(e.clone()),
                };
                if let Err(e) = recorded {
                    errors.entry(p.item.id.as_str()).or_insert(e);
                }
            }
            // Saved per batch: the approval prompt can sit there for a while.
            journal.save()?;
        }

        for p in chosen {
            match errors.remove(p.item.id.as_str()) {
                Some(error) => outcome.failed.push(Failure { id: p.item.id.clone(), title: p.item.title.clone(), error }),
                None => {
                    outcome.restart |= p.item.kind != Kind::App;
                    outcome.done.push(p.item.id.clone());
                }
            }
        }
        Ok(outcome)
    }

    /// Undo every change still in effect for the items with these ids, newest
    /// first.
    pub fn undo(&self, ids: &[String]) -> Result<Outcome, Error> {
        let mut journal = Journal::load(&self.journal)?;
        journal.save()?;

        let mut entries: Vec<Entry> = journal.active().filter(|e| ids.contains(&e.item_id)).cloned().collect();
        entries.reverse();
        let steps: Vec<Step> = entries.iter().map(|e| Step { id: e.seq as usize, op: e.undo.clone() }).collect();

        let mut errors: HashMap<&str, String> = HashMap::new();
        for elevate in [false, true] {
            let batch: Vec<Step> = steps.iter().filter(|s| s.op.needs_admin() == elevate).cloned().collect();
            let results = self.run_batch(&batch, elevate);
            for entry in entries.iter().filter(|e| e.undo.needs_admin() == elevate) {
                match &results[&(entry.seq as usize)] {
                    Ok(_) => journal.mark_undone(entry.seq, now()),
                    Err(e) => {
                        errors.entry(entry.item_id.as_str()).or_insert_with(|| e.clone());
                    }
                }
            }
            journal.save()?;
        }

        let mut outcome = Outcome::default();
        for id in ids {
            if outcome.done.contains(id) || outcome.failed.iter().any(|f| &f.id == id) {
                continue;
            }
            let Some(entry) = entries.iter().find(|e| &e.item_id == id) else {
                outcome.failed.push(Failure {
                    id: id.clone(),
                    title: id.clone(),
                    error: "There is nothing to undo for this.".to_string(),
                });
                continue;
            };
            match errors.remove(id.as_str()) {
                Some(error) => outcome.failed.push(Failure { id: id.clone(), title: entry.title.clone(), error }),
                None => {
                    outcome.restart |= entry.kind != Kind::App;
                    outcome.done.push(id.clone());
                }
            }
        }
        Ok(outcome)
    }
}

/// Active journal entries, one line per item, newest first.
fn changes(journal: &Journal) -> Vec<Change> {
    let mut out: Vec<Change> = Vec::new();
    for e in journal.active() {
        match out.iter_mut().find(|c| c.item_id == e.item_id) {
            Some(c) => {
                c.applied_at = c.applied_at.max(e.applied_at);
                c.needs_admin |= e.undo.needs_admin();
            }
            None => out.push(Change {
                item_id: e.item_id.clone(),
                title: e.title.clone(),
                kind: e.kind,
                applied_at: e.applied_at,
                needs_admin: e.undo.needs_admin(),
            }),
        }
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests;
