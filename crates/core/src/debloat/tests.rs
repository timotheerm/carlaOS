//! End-to-end tests of the engine against a pretend Windows machine that
//! follows the same rules as the helper script.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;

#[derive(Debug, Clone, PartialEq, Default)]
struct Machine {
    /// Installed package names.
    packages: BTreeSet<String>,
    /// Service name to (start type, running).
    services: BTreeMap<String, (StartMode, bool)>,
    /// (approval key, entry name) to approval record. Missing means enabled.
    approved: BTreeMap<(String, String), String>,
    /// (key path, value name) to value.
    registry: BTreeMap<(String, String), RegValue>,
    /// Registry keys that exist even when empty.
    keys: Vec<String>,
}

struct Fake {
    machine: RefCell<Machine>,
    /// Say no to the approval prompt.
    decline: bool,
    /// `elevate` flag of every apply call, in order.
    calls: RefCell<Vec<bool>>,
}

impl Fake {
    fn new() -> Self {
        let mut m = Machine {
            packages: ["Microsoft.BingNews", "Microsoft.ZuneMusic", "B9ECED6F.ArmouryCrateSE", "Microsoft.GamingApp"]
                .map(String::from)
                .into_iter()
                .collect(),
            ..Default::default()
        };
        m.services.insert("DiagTrack".into(), (StartMode::Auto, true));
        m.services.insert("SysMain".into(), (StartMode::DelayedAuto, false));
        m.services.insert("ArmouryCrateService".into(), (StartMode::Auto, true));
        m.keys.push("HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge".into());
        m.registry.insert(
            ("HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge".into(), "StartupBoostEnabled".into()),
            RegValue::Dword(1),
        );
        Self { machine: RefCell::new(m), decline: false, calls: RefCell::new(Vec::new()) }
    }

    fn scan(&self) -> Value {
        let m = self.machine.borrow();
        let enabled = |key: &str, name: &str| !m.approved.contains_key(&(key.to_string(), name.to_string()));
        let hkcu = catalogue::startup_source(StartupLocation::HkcuRun).approved;
        let hklm = catalogue::startup_source(StartupLocation::HklmRun).approved;
        json!({
            "packages": m.packages.iter().map(|n| json!({"name": n, "signature": "Store"})).collect::<Vec<_>>(),
            "services": m.services.iter().map(|(n, (start, running))| json!({
                "name": n, "display_name": n, "start": start, "running": running
            })).collect::<Vec<_>>(),
            "startup": [
                {"location": "hkcu_run", "name": "OneDrive", "command": "OneDrive.exe", "enabled": enabled(hkcu, "OneDrive")},
                {"location": "hklm_run", "name": "Updater", "command": "updater.exe", "enabled": enabled(hklm, "Updater")},
                {"location": "hklm_run", "name": "SecurityHealth", "command": "SecurityHealthSystray.exe", "enabled": true}
            ],
            "settings": catalogue::SETTINGS.iter().flat_map(|s| s.values).map(|v| json!({
                "path": v.path, "name": v.name,
                "value": m.registry.get(&(v.path.to_string(), v.name.to_string()))
            })).collect::<Vec<_>>(),
            "programs": []
        })
    }

    fn apply(m: &mut Machine, op: &Value) -> Result<Value, String> {
        let s = |k: &str| op[k].as_str().unwrap_or_default().to_string();
        match op["op"].as_str().unwrap() {
            "appx_remove" => {
                let name = s("name");
                if !m.packages.remove(&name) {
                    return Err("It is not installed.".into());
                }
                Ok(json!({"manifest": format!("C:\\WindowsApps\\{name}\\AppxManifest.xml")}))
            }
            "appx_restore" => {
                if op["store_id"].is_null() && op["manifest"].is_null() {
                    return Err("Windows could not bring it back.".into());
                }
                m.packages.insert(s("name"));
                Ok(json!({}))
            }
            "service_set" => {
                let entry = m.services.get_mut(&s("name")).ok_or("This service does not exist here.")?;
                let before = json!({"start": entry.0, "running": entry.1});
                entry.0 = serde_json::from_value(op["start"].clone()).unwrap();
                if let Some(run) = op["running"].as_bool() {
                    entry.1 = run;
                }
                Ok(before)
            }
            "startup_disable" => {
                let before = m.approved.insert((s("key"), s("name")), "AwAAAA==".into());
                Ok(json!({"data": before}))
            }
            "startup_restore" => {
                match op["data"].as_str() {
                    Some(data) => m.approved.insert((s("key"), s("name")), data.to_string()),
                    None => m.approved.remove(&(s("key"), s("name"))),
                };
                Ok(json!({}))
            }
            "reg_set" => {
                let slot = (s("path"), s("name"));
                let key_existed = m.keys.contains(&slot.0);
                let before = m.registry.get(&slot).cloned();
                if op["value"].is_null() {
                    m.registry.remove(&slot);
                    let empty = !m.registry.keys().any(|(p, _)| *p == slot.0);
                    if op["drop_empty_key"] == json!(true) && empty {
                        m.keys.retain(|k| *k != slot.0);
                    }
                } else {
                    if !key_existed {
                        m.keys.push(slot.0.clone());
                    }
                    m.registry.insert(slot, serde_json::from_value(op["value"].clone()).unwrap());
                }
                Ok(json!({"value": before, "key_existed": key_existed}))
            }
            other => Err(format!("unknown op {other}")),
        }
    }
}

impl Runner for &Fake {
    fn run(&self, mode: Mode, request: &Value, elevate: bool) -> Result<Value, Error> {
        if mode == Mode::Scan {
            assert!(!elevate, "scanning never asks for approval");
            return Ok(self.scan());
        }
        self.calls.borrow_mut().push(elevate);
        if elevate && self.decline {
            return Err(Error::Declined);
        }
        let mut m = self.machine.borrow_mut();
        let results: Vec<Value> = request["ops"]
            .as_array()
            .unwrap()
            .iter()
            .map(|op| {
                // What reaches an unelevated helper must not need elevation.
                let parsed: Op = serde_json::from_value(op.clone()).unwrap();
                assert_eq!(parsed.needs_admin(), elevate, "{op}");
                match Fake::apply(&mut m, op) {
                    Ok(before) => json!({"id": op["id"], "ok": true, "before": before}),
                    Err(e) => json!({"id": op["id"], "ok": false, "error": e}),
                }
            })
            .collect();
        Ok(json!({ "results": results }))
    }
}

fn journal_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("carla-debloat-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("journal.json")
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn applying_everything_then_undoing_restores_the_machine_exactly() {
    let fake = Fake::new();
    let original = fake.machine.borrow().clone();
    let debloat = Debloat::new(&fake, journal_path("roundtrip"));

    let report = debloat.scan().unwrap();
    let wanted: Vec<String> = report
        .items
        .iter()
        .filter(|i| matches!(i.tier, Tier::Recommended | Tier::Optional))
        .map(|i| i.id.clone())
        .collect();
    assert!(wanted.len() >= 10, "{wanted:?}");
    assert!(report.changes.is_empty());

    let outcome = debloat.apply(&wanted).unwrap();
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(outcome.done.len(), wanted.len());
    assert!(outcome.restart);
    assert_ne!(*fake.machine.borrow(), original);
    {
        let m = fake.machine.borrow();
        assert!(!m.packages.contains("Microsoft.BingNews"));
        assert_eq!(m.services["DiagTrack"], (StartMode::Disabled, false));
        // The protected neighbours were not touched.
        assert!(m.packages.contains("B9ECED6F.ArmouryCrateSE"));
        assert!(m.packages.contains("Microsoft.GamingApp"));
        assert_eq!(m.services["ArmouryCrateService"], (StartMode::Auto, true));
    }

    // Nothing left to offer, and every change is listed for undo.
    let after = debloat.scan().unwrap();
    assert!(after.items.iter().all(|i| matches!(i.tier, Tier::Kept | Tier::Manual)), "{:?}", after.items);
    let mut changed: Vec<String> = after.changes.iter().map(|c| c.item_id.clone()).collect();
    let mut expected = wanted.clone();
    changed.sort();
    expected.sort();
    assert_eq!(changed, expected);

    let undone = debloat.undo(&wanted).unwrap();
    assert_eq!(undone.failed, vec![]);
    assert_eq!(*fake.machine.borrow(), original);
    assert!(debloat.scan().unwrap().changes.is_empty());
}

#[test]
fn per_user_and_machine_changes_go_through_separate_helpers() {
    let fake = Fake::new();
    let debloat = Debloat::new(&fake, journal_path("split"));
    debloat.apply(&ids(&["app:Microsoft.BingNews", "svc:DiagTrack"])).unwrap();
    assert_eq!(*fake.calls.borrow(), vec![false, true]);

    // Nothing machine-wide selected: no approval prompt at all.
    fake.calls.borrow_mut().clear();
    debloat.apply(&ids(&["run:hkcu_run:OneDrive"])).unwrap();
    assert_eq!(*fake.calls.borrow(), vec![false]);
}

#[test]
fn protected_unknown_and_manual_ids_are_refused() {
    let fake = Fake::new();
    let original = fake.machine.borrow().clone();
    let debloat = Debloat::new(&fake, journal_path("refuse"));
    let outcome = debloat
        .apply(&ids(&[
            "app:B9ECED6F.ArmouryCrateSE",
            "app:Microsoft.GamingApp",
            "svc:ArmouryCrateService",
            "run:hklm_run:SecurityHealth",
            "svc:WinDefend",
            "app:Whatever.NotInstalled",
        ]))
        .unwrap();
    assert!(outcome.done.is_empty());
    assert_eq!(outcome.failed.len(), 6);
    assert_eq!(*fake.machine.borrow(), original);
    assert!(fake.calls.borrow().is_empty(), "the helper was never asked to change anything");
}

#[test]
fn declining_approval_keeps_machine_changes_out_and_the_rest_in() {
    let mut fake = Fake::new();
    fake.decline = true;
    let debloat = Debloat::new(&fake, journal_path("decline"));
    let outcome = debloat.apply(&ids(&["app:Microsoft.BingNews", "svc:DiagTrack", "set:widgets"])).unwrap();
    assert_eq!(outcome.done, ids(&["app:Microsoft.BingNews"]));
    assert_eq!(outcome.failed.len(), 2);
    assert!(outcome.failed.iter().all(|f| f.error == Error::Declined.to_string()));
    assert_eq!(fake.machine.borrow().services["DiagTrack"], (StartMode::Auto, true));
    // Only what really happened is in the journal.
    let changes = debloat.scan().unwrap().changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].item_id, "app:Microsoft.BingNews");
}

#[test]
fn a_setting_made_of_several_values_is_undone_as_one() {
    let fake = Fake::new();
    let original = fake.machine.borrow().clone();
    let debloat = Debloat::new(&fake, journal_path("multi"));
    debloat.apply(&ids(&["set:edge-background"])).unwrap();
    {
        let m = fake.machine.borrow();
        let edge = "HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge".to_string();
        assert_eq!(m.registry[&(edge.clone(), "StartupBoostEnabled".to_string())], RegValue::Dword(0));
        assert_eq!(m.registry[&(edge, "BackgroundModeEnabled".to_string())], RegValue::Dword(0));
    }
    let report = debloat.scan().unwrap();
    assert_eq!(report.changes.len(), 1);
    assert!(report.changes[0].needs_admin);

    debloat.undo(&ids(&["set:edge-background"])).unwrap();
    // The value that was 1 is 1 again, and the one that did not exist is gone.
    assert_eq!(*fake.machine.borrow(), original);
}

#[test]
fn a_key_created_for_a_setting_is_removed_again_on_undo() {
    let fake = Fake::new();
    let debloat = Debloat::new(&fake, journal_path("key"));
    let dsh = "HKLM\\SOFTWARE\\Policies\\Microsoft\\Dsh".to_string();
    debloat.apply(&ids(&["set:widgets"])).unwrap();
    assert!(fake.machine.borrow().keys.contains(&dsh));
    debloat.undo(&ids(&["set:widgets"])).unwrap();
    assert!(!fake.machine.borrow().keys.contains(&dsh));
}

#[test]
fn undoing_something_never_applied_fails_cleanly() {
    let fake = Fake::new();
    let debloat = Debloat::new(&fake, journal_path("nothing"));
    let outcome = debloat.undo(&ids(&["svc:DiagTrack"])).unwrap();
    assert!(outcome.done.is_empty());
    assert_eq!(outcome.failed.len(), 1);
    assert!(fake.calls.borrow().is_empty());
}

#[test]
fn a_damaged_journal_stops_everything_before_any_change() {
    let fake = Fake::new();
    let original = fake.machine.borrow().clone();
    let path = journal_path("damaged");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "garbage").unwrap();
    let debloat = Debloat::new(&fake, path);
    assert!(matches!(debloat.apply(&ids(&["svc:DiagTrack"])), Err(Error::Journal(_))));
    assert_eq!(*fake.machine.borrow(), original);
}
