//! Runs the real helper script against the machine the tests run on. Only
//! built on Windows; the build pipeline runs it on a throwaway Windows machine.
//!
//! Everything changed here is put back by the same undo path the shell uses,
//! and the tests check that the machine ends up exactly as it started.
#![cfg(windows)]

use std::path::PathBuf;

use carla_core::debloat::{
    catalogue, ApplyResponse, Before, Debloat, Inventory, Kind, Mode, Op, PowerShell, RegValue, Runner, StartMode,
    StartupLocation, Step, Tier,
};
use serde_json::{json, Value};

fn work(name: &str) -> PathBuf {
    // A space and an apostrophe in the path, like a real user name might have.
    let dir = std::env::temp_dir().join(format!("carla live {name} o'{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn scan(ps: &PowerShell, settings: Value) -> Inventory {
    let startup: Vec<Value> = catalogue::STARTUP_SOURCES
        .iter()
        .map(|s| json!({ "location": s.location, "run": s.run, "folder": s.folder, "approved": s.approved }))
        .collect();
    let answer = ps.run(Mode::Scan, &json!({ "settings": settings, "startup": startup }), false).unwrap();
    serde_json::from_value(answer).unwrap()
}

fn value(ps: &PowerShell, path: &str, name: &str) -> Option<RegValue> {
    scan(ps, json!([{ "path": path, "name": name }])).settings.remove(0).value
}

/// Run one operation and return what was there before.
fn run(ps: &PowerShell, op: &Op, elevate: bool) -> Before {
    let answer = ps.run(Mode::Apply, &json!({ "ops": [Step { id: 7, op: op.clone() }] }), elevate).unwrap();
    let mut response: ApplyResponse = serde_json::from_value(answer).unwrap();
    let result = response.results.remove(0);
    assert_eq!(result.id, 7);
    assert!(result.ok, "{op:?} failed: {:?}", result.error);
    result.before.unwrap_or_default()
}

#[test]
fn scan_reads_this_machine() {
    let dir = work("scan");
    let debloat = Debloat::new(PowerShell::new(dir.clone()), dir.join("journal.json"));
    let report = debloat.scan().unwrap();
    println!("{} items, {} warnings", report.items.len(), report.warnings.len());
    for item in &report.items {
        println!("  {:?} {:?} {} ({})", item.tier, item.kind, item.title, item.id);
    }
    assert_eq!(report.warnings, Vec::<String>::new());
    assert!(!report.items.is_empty());
    assert!(report.changes.is_empty());

    let inventory = scan(&PowerShell::new(dir), json!([]));
    assert!(inventory.services.len() > 50, "only {} services", inventory.services.len());
}

#[test]
fn registry_values_round_trip_exactly() {
    let ps = PowerShell::new(work("registry"));
    let path = format!("HKCU\\Software\\CarlaOS Test {}", std::process::id());

    for wanted in [RegValue::Dword(u32::MAX), RegValue::Dword(0), RegValue::String("héllo 'world'".into())] {
        assert_eq!(value(&ps, &path, "Probe"), None);
        let set = Op::RegSet { path: path.clone(), name: "Probe".into(), value: Some(wanted.clone()), drop_empty_key: false };
        let before = run(&ps, &set, false);
        assert_eq!(before, Before { key_existed: Some(false), ..Default::default() });
        assert_eq!(value(&ps, &path, "Probe"), Some(wanted.clone()));

        // Overwriting reports the value that was there, and undo brings it back.
        let over = Op::RegSet { path: path.clone(), name: "Probe".into(), value: Some(RegValue::Dword(7)), drop_empty_key: false };
        let seen = run(&ps, &over, false);
        assert_eq!(seen, Before { key_existed: Some(true), value: Some(wanted.clone()), ..Default::default() });
        run(&ps, &over.inverse(&seen, None).unwrap(), false);
        assert_eq!(value(&ps, &path, "Probe"), Some(wanted));

        // Undoing the first write removes the value and the key it created.
        run(&ps, &set.inverse(&before, None).unwrap(), false);
        assert_eq!(value(&ps, &path, "Probe"), None);
        let again = run(&ps, &Op::RegSet { path: path.clone(), name: "Probe".into(), value: None, drop_empty_key: false }, false);
        assert_eq!(again.key_existed, Some(false), "the key was not cleaned up");
    }
}

#[test]
fn a_startup_entry_is_switched_off_and_back_on() {
    let ps = PowerShell::new(work("startup"));
    let source = catalogue::startup_source(StartupLocation::HkcuRun);
    let name = format!("CarlaOS Probe {}", std::process::id());
    let entry = Op::RegSet {
        path: source.run.unwrap().to_string(),
        name: name.clone(),
        value: Some(RegValue::String("cmd.exe /c rem".into())),
        drop_empty_key: false,
    };
    let entry_before = run(&ps, &entry, false);
    let enabled = |ps: &PowerShell| {
        scan(ps, json!([]))
            .startup
            .into_iter()
            .find(|e| e.location == StartupLocation::HkcuRun && e.name == name)
            .expect("the probe entry is listed")
            .enabled
    };
    assert!(enabled(&ps));

    let off = Op::StartupDisable { key: source.approved.to_string(), name: name.clone() };
    let before = run(&ps, &off, false);
    assert_eq!(before.data, None);
    assert!(!enabled(&ps));

    run(&ps, &off.inverse(&before, None).unwrap(), false);
    assert!(enabled(&ps));

    // Switching off twice keeps the first record, so undo is still exact.
    let first = run(&ps, &off, false);
    let second = run(&ps, &off, false);
    assert!(second.data.is_some());
    run(&ps, &off.inverse(&second, None).unwrap(), false);
    assert!(!enabled(&ps));
    run(&ps, &off.inverse(&first, None).unwrap(), false);
    assert!(enabled(&ps));

    run(&ps, &entry.inverse(&entry_before, None).unwrap(), false);
}

#[test]
fn a_machine_wide_value_goes_through_the_elevated_helper() {
    let ps = PowerShell::new(work("elevated"));
    let path = format!("HKLM\\SOFTWARE\\CarlaOS Test {}", std::process::id());
    let set = Op::RegSet { path: path.clone(), name: "Probe".into(), value: Some(RegValue::Dword(1)), drop_empty_key: false };
    assert!(set.needs_admin());
    let before = run(&ps, &set, true);
    assert_eq!(value(&ps, &path, "Probe"), Some(RegValue::Dword(1)));
    run(&ps, &set.inverse(&before, None).unwrap(), true);
    assert_eq!(value(&ps, &path, "Probe"), None);
}

#[test]
fn a_service_is_disabled_and_restored() {
    let ps = PowerShell::new(work("service"));
    let find = |ps: &PowerShell, name: &str| scan(ps, json!([])).services.into_iter().find(|s| s.name.eq_ignore_ascii_case(name));
    // The least important catalogue service this machine has.
    let candidates = ["Fax", "MapsBroker", "WMPNetworkSvc", "RetailDemo", "WerSvc", "Spooler", "SysMain", "DiagTrack"];
    let Some(original) = candidates.iter().find_map(|n| find(&ps, n).filter(|s| s.start != StartMode::Disabled)) else {
        println!("no catalogue service to try on this machine");
        return;
    };
    println!("trying {} ({:?}, running: {})", original.name, original.start, original.running);

    let off = Op::ServiceSet { name: original.name.clone(), start: StartMode::Disabled, running: Some(false) };
    let before = run(&ps, &off, true);
    assert_eq!(before.start, Some(original.start));
    assert_eq!(before.running, Some(original.running));
    assert_eq!(find(&ps, &original.name).unwrap().start, StartMode::Disabled);

    run(&ps, &off.inverse(&before, None).unwrap(), true);
    assert_eq!(find(&ps, &original.name).unwrap().start, original.start);
}

#[test]
fn the_engine_applies_and_undoes_a_real_setting() {
    let dir = work("engine");
    let debloat = Debloat::new(PowerShell::new(dir.clone()), dir.join("journal.json"));
    let ps = PowerShell::new(dir);
    let report = debloat.scan().unwrap();
    let Some(item) = report.items.iter().find(|i| i.kind == Kind::Setting && i.tier != Tier::Kept) else {
        println!("every setting is already applied on this machine");
        return;
    };
    let setting = catalogue::SETTINGS.iter().find(|s| format!("set:{}", s.id) == item.id).unwrap();
    let read = |ps: &PowerShell| -> Vec<Option<RegValue>> {
        setting.values.iter().map(|v| value(ps, v.path, v.name)).collect()
    };
    let original = read(&ps);
    println!("applying {} (was {original:?})", item.id);

    let outcome = debloat.apply(std::slice::from_ref(&item.id)).unwrap();
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(outcome.done, vec![item.id.clone()]);
    assert!(read(&ps).iter().zip(setting.values).all(|(now, rule)| now.as_ref() == Some(&rule.target())));

    let after = debloat.scan().unwrap();
    assert!(after.items.iter().all(|i| i.id != item.id));
    assert_eq!(after.changes.len(), 1);
    assert_eq!(after.changes[0].item_id, item.id);

    let undone = debloat.undo(std::slice::from_ref(&item.id)).unwrap();
    assert_eq!(undone.failed, vec![]);
    assert_eq!(read(&ps), original);
    assert!(debloat.scan().unwrap().changes.is_empty());
}
