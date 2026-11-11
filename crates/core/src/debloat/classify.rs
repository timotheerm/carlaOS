//! Turns what the scan found into a list of things an administrator can decide
//! on, each with the exact operations it stands for.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::catalogue::{self as cat, Tier};
use super::model::{Inventory, Op, StartMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An installed app.
    App,
    /// Something that starts with Windows.
    Startup,
    /// A background service.
    Service,
    /// A Windows setting.
    Setting,
}

/// One line in the clean-up list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    pub id: String,
    pub kind: Kind,
    pub tier: Tier,
    pub title: String,
    pub detail: String,
    /// Windows will ask for administrator approval to change this.
    pub needs_admin: bool,
}

/// An [`Item`] with the operations behind it. Items in the `Manual` and `Kept`
/// tiers never carry operations.
#[derive(Debug, Clone)]
pub struct Planned {
    pub item: Item,
    pub ops: Vec<Op>,
    pub store_id: Option<&'static str>,
}

const UNKNOWN_STARTUP: &str = "CarlaOS does not recognise this. Only turn it off if you know what it is.";
const UNKNOWN_APP: &str = "CarlaOS does not recognise this app, so it leaves it alone. If nobody needs it, uninstall it from Windows Settings.";
const BY_HAND: &str = " CarlaOS could not reinstall it, so uninstall it from Windows Settings yourself.";

/// Package names are letters, digits, dots, dashes and underscores. Anything
/// else is not something to hand to a command.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

pub fn classify(inv: &Inventory) -> Vec<Planned> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |p: Planned, out: &mut Vec<Planned>| {
        if seen.insert(p.item.id.clone()) {
            out.push(p);
        }
    };
    let plain = |id: String, kind, tier, title: &str, detail: &str| Planned {
        item: Item { id, kind, tier, title: title.to_string(), detail: detail.to_string(), needs_admin: false },
        ops: Vec::new(),
        store_id: None,
    };

    for p in &inv.packages {
        if p.framework || !plain_name(&p.name) {
            continue;
        }
        let id = format!("app:{}", p.name);
        if let Some(k) = cat::kept(cat::KEEP_APPS, &[&p.name]) {
            push(plain(id, Kind::App, Tier::Kept, &p.name, k.why), &mut out);
            continue;
        }
        // Parts of Windows itself are neither listed nor touched.
        if p.non_removable || p.signature.eq_ignore_ascii_case("System") {
            continue;
        }
        match cat::APPS.iter().find(|a| cat::matches(a.pattern, &p.name)) {
            Some(a) => match (a.tier, a.store_id) {
                (Tier::Recommended | Tier::Optional, Some(store_id)) => {
                    let mut planned = plain(id, Kind::App, a.tier, a.title, a.why);
                    planned.ops = vec![Op::AppxRemove { name: p.name.clone() }];
                    planned.store_id = Some(store_id);
                    push(planned, &mut out);
                }
                _ => {
                    let detail = format!("{}{}", a.why, BY_HAND);
                    push(plain(id, Kind::App, Tier::Manual, a.title, &detail), &mut out);
                }
            },
            // Unknown apps from other publishers are pointed out, never removed.
            None if !p.name.to_ascii_lowercase().starts_with("microsoft") => {
                push(plain(id, Kind::App, Tier::Manual, &p.name, UNKNOWN_APP), &mut out);
            }
            None => {}
        }
    }

    for p in &inv.programs {
        if let Some(rule) = cat::PROGRAMS.iter().find(|r| cat::matches(r.pattern, &p.name)) {
            let detail = format!("{}{}", rule.why, BY_HAND);
            push(plain(format!("prog:{}", p.name), Kind::App, Tier::Manual, &p.name, &detail), &mut out);
        }
    }

    for e in &inv.startup {
        let id = format!("run:{}:{}", serde_plain(&e.location), e.name);
        let texts = [e.name.as_str(), e.command.as_str()];
        if let Some(k) = cat::kept(cat::KEEP_STARTUP, &texts) {
            push(plain(id, Kind::Startup, Tier::Kept, &e.name, k.why), &mut out);
            continue;
        }
        if !e.enabled {
            continue;
        }
        let rule = cat::STARTUP.iter().find(|r| texts.iter().any(|t| cat::matches(r.pattern, t)));
        let (tier, title, why) = match rule {
            Some(r) => (r.tier, r.title, r.why),
            None => (Tier::Optional, e.name.as_str(), UNKNOWN_STARTUP),
        };
        let op = Op::StartupDisable {
            key: cat::startup_source(e.location).approved.to_string(),
            name: e.name.clone(),
        };
        let mut planned = plain(id, Kind::Startup, tier, title, why);
        planned.item.needs_admin = op.needs_admin();
        planned.ops = vec![op];
        push(planned, &mut out);
    }

    for s in &inv.services {
        let id = format!("svc:{}", s.name);
        if let Some(k) = cat::kept(cat::KEEP_SERVICES, &[&s.name]) {
            let title = if s.display_name.is_empty() { &s.name } else { &s.display_name };
            push(plain(id, Kind::Service, Tier::Kept, title, k.why), &mut out);
            continue;
        }
        let Some(rule) = cat::SERVICES.iter().find(|r| r.name.eq_ignore_ascii_case(&s.name)) else {
            continue;
        };
        if s.start == StartMode::Disabled {
            continue;
        }
        let mut planned = plain(id, Kind::Service, rule.tier, rule.title, rule.why);
        planned.item.needs_admin = true;
        planned.ops = vec![Op::ServiceSet {
            name: s.name.clone(),
            start: StartMode::Disabled,
            running: Some(false),
        }];
        push(planned, &mut out);
    }

    for setting in cat::SETTINGS {
        let ops: Vec<Op> = setting
            .values
            .iter()
            .filter(|rule| {
                let current = inv
                    .settings
                    .iter()
                    .find(|v| v.path.eq_ignore_ascii_case(rule.path) && v.name.eq_ignore_ascii_case(rule.name))
                    .and_then(|v| v.value.as_ref());
                current != Some(&rule.target())
            })
            .map(|rule| Op::RegSet {
                path: rule.path.to_string(),
                name: rule.name.to_string(),
                value: Some(rule.target()),
                drop_empty_key: false,
            })
            .collect();
        if ops.is_empty() {
            continue;
        }
        let mut planned = plain(format!("set:{}", setting.id), Kind::Setting, setting.tier, setting.title, setting.why);
        planned.item.needs_admin = ops.iter().any(Op::needs_admin);
        planned.ops = ops;
        push(planned, &mut out);
    }

    out.sort_by(|a, b| {
        (a.item.tier, a.item.kind, a.item.title.to_lowercase()).cmp(&(b.item.tier, b.item.kind, b.item.title.to_lowercase()))
    });
    out
}

fn serde_plain<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A plausible Ally, used by several tests.
    pub(crate) fn sample() -> Inventory {
        serde_json::from_value(json!({
            "packages": [
                {"name": "Microsoft.BingNews", "signature": "Store"},
                {"name": "Microsoft.ZuneMusic", "signature": "Store"},
                {"name": "MSTeams", "signature": "Store"},
                {"name": "B9ECED6F.ArmouryCrateSE", "signature": "Store"},
                {"name": "Microsoft.XboxGamingOverlay", "signature": "Store"},
                {"name": "Microsoft.GamingApp", "signature": "Store"},
                {"name": "Microsoft.SecHealthUI", "signature": "System", "non_removable": true},
                {"name": "Microsoft.VCLibs.140.00", "signature": "Store", "framework": true},
                {"name": "Microsoft.Windows.ShellExperienceHost", "signature": "System", "non_removable": true},
                {"name": "Microsoft.WindowsCalculator", "signature": "Store"},
                {"name": "SomeVendor.MysteryApp", "signature": "Store"},
                {"name": "Bad Name; rm", "signature": "Store"}
            ],
            "services": [
                {"name": "DiagTrack", "display_name": "Connected User Experiences and Telemetry", "start": "auto", "running": true},
                {"name": "SysMain", "display_name": "SysMain", "start": "auto", "running": true},
                {"name": "Fax", "display_name": "Fax", "start": "disabled", "running": false},
                {"name": "ArmouryCrateService", "display_name": "ARMOURY CRATE Service", "start": "auto", "running": true},
                {"name": "XboxGipSvc", "display_name": "Xbox Accessory Management Service", "start": "demand", "running": false},
                {"name": "WinDefend", "display_name": "Microsoft Defender Antivirus Service", "start": "auto", "running": true},
                {"name": "Themes", "display_name": "Themes", "start": "auto", "running": true}
            ],
            "startup": [
                {"location": "hkcu_run", "name": "OneDrive", "command": "\"C:\\Program Files\\Microsoft OneDrive\\OneDrive.exe\" /background", "enabled": true},
                {"location": "hkcu_run", "name": "Steam", "command": "\"C:\\Program Files (x86)\\Steam\\steam.exe\" -silent", "enabled": true},
                {"location": "hklm_run", "name": "SecurityHealth", "command": "%windir%\\system32\\SecurityHealthSystray.exe", "enabled": true},
                {"location": "hklm_run", "name": "RtkAudUService", "command": "\"C:\\Windows\\System32\\RtkAudUService64.exe\" -background", "enabled": true},
                {"location": "hklm_run32", "name": "Updater", "command": "\"C:\\Program Files (x86)\\Thing\\updater.exe\"", "enabled": true},
                {"location": "user_folder", "name": "Discord.lnk", "command": "", "enabled": false}
            ],
            "settings": [
                {"path": "HKLM\\SOFTWARE\\Policies\\Microsoft\\Dsh", "name": "AllowNewsAndInterests", "value": {"kind": "dword", "data": 0}},
                {"path": "HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge", "name": "StartupBoostEnabled", "value": {"kind": "dword", "data": 0}},
                {"path": "HKLM\\SOFTWARE\\Policies\\Microsoft\\Edge", "name": "BackgroundModeEnabled", "value": null}
            ],
            "programs": [
                {"name": "McAfee LiveSafe", "publisher": "McAfee, LLC"},
                {"name": "Steam", "publisher": "Valve"}
            ]
        }))
        .unwrap()
    }

    fn find<'a>(all: &'a [Planned], id: &str) -> Option<&'a Planned> {
        all.iter().find(|p| p.item.id == id)
    }

    #[test]
    fn known_clutter_is_offered_with_its_operations() {
        let all = classify(&sample());

        let news = find(&all, "app:Microsoft.BingNews").unwrap();
        assert_eq!(news.item.tier, Tier::Recommended);
        assert!(!news.item.needs_admin);
        assert_eq!(news.ops, vec![Op::AppxRemove { name: "Microsoft.BingNews".into() }]);
        assert_eq!(news.store_id, Some("9WZDNCRFHVFW"));

        let diag = find(&all, "svc:DiagTrack").unwrap();
        assert!(diag.item.needs_admin);
        assert_eq!(
            diag.ops,
            vec![Op::ServiceSet { name: "DiagTrack".into(), start: StartMode::Disabled, running: Some(false) }]
        );
        assert_eq!(find(&all, "svc:SysMain").unwrap().item.tier, Tier::Optional);

        let onedrive = find(&all, "run:hkcu_run:OneDrive").unwrap();
        assert_eq!(onedrive.item.tier, Tier::Recommended);
        assert!(!onedrive.item.needs_admin);
        assert_eq!(find(&all, "run:hkcu_run:Steam").unwrap().item.tier, Tier::Optional);

        // Unrecognised startup entries are offered, never preselected, and the
        // machine-wide ones need approval.
        let updater = find(&all, "run:hklm_run32:Updater").unwrap();
        assert_eq!(updater.item.tier, Tier::Optional);
        assert!(updater.item.needs_admin);
        assert!(matches!(&updater.ops[0], Op::StartupDisable { key, .. } if key.ends_with("StartupApproved\\Run32")));
    }

    #[test]
    fn protected_things_are_listed_but_carry_no_operations() {
        let all = classify(&sample());
        for id in [
            "app:B9ECED6F.ArmouryCrateSE",
            "app:Microsoft.XboxGamingOverlay",
            "app:Microsoft.GamingApp",
            "app:Microsoft.SecHealthUI",
            "svc:ArmouryCrateService",
            "svc:XboxGipSvc",
            "svc:WinDefend",
            "run:hklm_run:SecurityHealth",
            "run:hklm_run:RtkAudUService",
        ] {
            let p = find(&all, id).unwrap_or_else(|| panic!("{id} is not listed"));
            assert_eq!(p.item.tier, Tier::Kept, "{id}");
            assert!(p.ops.is_empty(), "{id}");
        }
        assert!(all.iter().all(|p| p.ops.is_empty() == matches!(p.item.tier, Tier::Kept | Tier::Manual)));
    }

    #[test]
    fn a_path_through_program_files_does_not_look_like_rog() {
        let all = classify(&sample());
        assert_ne!(find(&all, "run:hkcu_run:OneDrive").unwrap().item.tier, Tier::Kept);
    }

    #[test]
    fn unknown_and_unrestorable_things_are_left_to_a_person() {
        let all = classify(&sample());
        for id in ["app:MSTeams", "app:SomeVendor.MysteryApp", "prog:McAfee LiveSafe"] {
            assert_eq!(find(&all, id).unwrap().item.tier, Tier::Manual, "{id}");
        }
        // Windows' own parts, frameworks, unknown Microsoft apps, ordinary
        // programs and odd names are not listed at all.
        for id in [
            "app:Microsoft.VCLibs.140.00",
            "app:Microsoft.Windows.ShellExperienceHost",
            "app:Microsoft.WindowsCalculator",
            "app:Bad Name; rm",
            "prog:Steam",
            "svc:Themes",
        ] {
            assert!(find(&all, id).is_none(), "{id}");
        }
    }

    #[test]
    fn things_already_tidy_are_not_offered_again() {
        let all = classify(&sample());
        assert!(find(&all, "svc:Fax").is_none());
        assert!(find(&all, "run:user_folder:Discord.lnk").is_none());
        assert!(find(&all, "set:widgets").is_none());
        // Half-applied settings only carry what is still missing.
        let edge = find(&all, "set:edge-background").unwrap();
        assert_eq!(edge.ops.len(), 1);
        assert!(matches!(&edge.ops[0], Op::RegSet { name, .. } if name == "BackgroundModeEnabled"));
    }

    #[test]
    fn recommended_comes_first() {
        let all = classify(&sample());
        let tiers: Vec<Tier> = all.iter().map(|p| p.item.tier).collect();
        let mut sorted = tiers.clone();
        sorted.sort();
        assert_eq!(tiers, sorted);
        assert_eq!(tiers.first(), Some(&Tier::Recommended));
        assert_eq!(tiers.last(), Some(&Tier::Kept));
    }
}
