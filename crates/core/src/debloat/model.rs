//! Plain data exchanged with the Windows helper script.

use serde::{Deserialize, Deserializer, Serialize};

/// Accept `null` where a list is expected. Windows PowerShell 5.1 writes an
/// empty pipeline as `null`.
fn list<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

/// Everything the scan reads from the machine. Nothing here is a judgement:
/// deciding what may be changed happens in [`super::classify`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Inventory {
    #[serde(default, deserialize_with = "list")]
    pub packages: Vec<Package>,
    #[serde(default, deserialize_with = "list")]
    pub services: Vec<Service>,
    #[serde(default, deserialize_with = "list")]
    pub startup: Vec<StartupEntry>,
    #[serde(default, deserialize_with = "list")]
    pub settings: Vec<SettingValue>,
    #[serde(default, deserialize_with = "list")]
    pub programs: Vec<Program>,
    /// Parts of the scan that failed, in the script's words.
    #[serde(default, deserialize_with = "list")]
    pub errors: Vec<String>,
}

/// A Store (AppX/MSIX) package installed for the current user.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Package {
    pub name: String,
    #[serde(default)]
    pub framework: bool,
    #[serde(default)]
    pub non_removable: bool,
    /// `Store`, `System`, `Developer`...
    #[serde(default)]
    pub signature: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Service {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    pub start: StartMode,
    #[serde(default)]
    pub running: bool,
}

/// Service start types, spelled the way `sc.exe config start=` takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StartMode {
    Auto,
    DelayedAuto,
    Demand,
    Disabled,
}

/// Where a program that starts with Windows is registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupLocation {
    HkcuRun,
    HklmRun,
    HklmRun32,
    UserFolder,
    CommonFolder,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartupEntry {
    pub location: StartupLocation,
    pub name: String,
    #[serde(default)]
    pub command: String,
    pub enabled: bool,
}

/// A registry value, limited to the types the helper can restore exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum RegValue {
    Dword(u32),
    String(String),
    ExpandString(String),
    /// Any other type. The helper refuses to overwrite these.
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SettingValue {
    pub path: String,
    pub name: String,
    /// `None` when the value does not exist.
    #[serde(default)]
    pub value: Option<RegValue>,
}

/// A classic installed program, from the Windows uninstall list.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Program {
    pub name: String,
    #[serde(default)]
    pub publisher: String,
}

/// One change the helper script can make. Each has an exact opposite, built by
/// [`Op::inverse`] from the state the helper found just before changing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Remove a Store package for the current user.
    AppxRemove { name: String },
    /// Bring a Store package back: from the copy Windows kept, else the Store.
    AppxRestore {
        name: String,
        manifest: Option<String>,
        store_id: Option<String>,
    },
    /// Set a service's start type, and optionally stop or start it.
    ServiceSet {
        name: String,
        start: StartMode,
        running: Option<bool>,
    },
    /// Turn a startup entry off the way Task Manager does.
    StartupDisable { key: String, name: String },
    /// Put a startup entry's approval record back (`None` removes the record).
    StartupRestore {
        key: String,
        name: String,
        data: Option<String>,
    },
    /// Write a registry value (`None` deletes it).
    RegSet {
        path: String,
        name: String,
        value: Option<RegValue>,
        #[serde(default)]
        drop_empty_key: bool,
    },
}

/// What the helper found before it changed something. Only the fields that
/// belong to the operation are filled in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Before {
    #[serde(default)]
    pub manifest: Option<String>,
    #[serde(default)]
    pub start: Option<StartMode>,
    #[serde(default)]
    pub running: Option<bool>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub value: Option<RegValue>,
    #[serde(default)]
    pub key_existed: Option<bool>,
}

impl Op {
    /// Machine-wide changes need administrator approval; per-user ones do not.
    pub fn needs_admin(&self) -> bool {
        match self {
            Op::AppxRemove { .. } | Op::AppxRestore { .. } => false,
            Op::ServiceSet { .. } => true,
            Op::StartupDisable { key, .. } | Op::StartupRestore { key, .. } => is_machine(key),
            Op::RegSet { path, .. } => is_machine(path),
        }
    }

    /// The operation that puts things back exactly as `before` describes.
    /// `store_id` is only used for packages. Returns `None` when the previous
    /// state cannot be restored, in which case the change must not be made.
    pub fn inverse(&self, before: &Before, store_id: Option<&str>) -> Option<Op> {
        match self {
            Op::AppxRemove { name } => Some(Op::AppxRestore {
                name: name.clone(),
                manifest: before.manifest.clone(),
                store_id: store_id.map(str::to_string),
            }),
            Op::ServiceSet { name, .. } => Some(Op::ServiceSet {
                name: name.clone(),
                start: before.start?,
                running: before.running,
            }),
            Op::StartupDisable { key, name } => Some(Op::StartupRestore {
                key: key.clone(),
                name: name.clone(),
                data: before.data.clone(),
            }),
            Op::RegSet { path, name, .. } => {
                if before.value == Some(RegValue::Other) {
                    return None;
                }
                Some(Op::RegSet {
                    path: path.clone(),
                    name: name.clone(),
                    drop_empty_key: before.value.is_none() && before.key_existed == Some(false),
                    value: before.value.clone(),
                })
            }
            // Restores are only ever built as inverses; they are not undone.
            Op::AppxRestore { .. } | Op::StartupRestore { .. } => None,
        }
    }
}

fn is_machine(path: &str) -> bool {
    path.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("HKLM\\"))
}

/// One numbered operation in a request to the helper.
#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub id: usize,
    #[serde(flatten)]
    pub op: Op,
}

/// The helper's answer for one [`Step`].
#[derive(Debug, Clone, Deserialize)]
pub struct StepResult {
    pub id: usize,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub before: Option<Before>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ApplyResponse {
    #[serde(default, deserialize_with = "list")]
    pub results: Vec<StepResult>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn steps_serialise_flat_for_the_helper() {
        let step = Step {
            id: 3,
            op: Op::RegSet {
                path: "HKCU\\Software\\X".into(),
                name: "Y".into(),
                value: Some(RegValue::Dword(0)),
                drop_empty_key: false,
            },
        };
        assert_eq!(
            serde_json::to_value(&step).unwrap(),
            json!({"id": 3, "op": "reg_set", "path": "HKCU\\Software\\X", "name": "Y",
                   "value": {"kind": "dword", "data": 0}, "drop_empty_key": false})
        );
    }

    #[test]
    fn inventory_tolerates_nulls_and_unknown_value_types() {
        let inv: Inventory = serde_json::from_value(json!({
            "packages": null,
            "services": [{"name": "DiagTrack", "display_name": "x", "start": "delayed-auto", "running": true}],
            "settings": [
                {"path": "HKCU\\A", "name": "a", "value": null},
                {"path": "HKCU\\A", "name": "b", "value": {"kind": "other"}},
                {"path": "HKCU\\A", "name": "c", "value": {"kind": "dword", "data": 4294967295u32}}
            ]
        }))
        .unwrap();
        assert!(inv.packages.is_empty());
        assert_eq!(inv.services[0].start, StartMode::DelayedAuto);
        assert_eq!(inv.settings[0].value, None);
        assert_eq!(inv.settings[1].value, Some(RegValue::Other));
        assert_eq!(inv.settings[2].value, Some(RegValue::Dword(u32::MAX)));
    }

    #[test]
    fn only_machine_wide_changes_need_admin() {
        let reg = |path: &str| Op::RegSet { path: path.into(), name: "n".into(), value: None, drop_empty_key: false };
        assert!(reg("HKLM\\SOFTWARE\\Policies\\X").needs_admin());
        assert!(!reg("HKCU\\Software\\X").needs_admin());
        assert!(Op::ServiceSet { name: "DiagTrack".into(), start: StartMode::Disabled, running: None }.needs_admin());
        assert!(!Op::AppxRemove { name: "Microsoft.BingNews".into() }.needs_admin());
    }

    #[test]
    fn inverse_restores_what_was_there() {
        let set = Op::RegSet {
            path: "HKLM\\SOFTWARE\\Policies\\Microsoft\\Dsh".into(),
            name: "AllowNewsAndInterests".into(),
            value: Some(RegValue::Dword(0)),
            drop_empty_key: false,
        };
        // The key did not exist: undo removes the value and the key we created.
        let fresh = Before { key_existed: Some(false), ..Default::default() };
        assert_eq!(
            set.inverse(&fresh, None),
            Some(Op::RegSet {
                path: "HKLM\\SOFTWARE\\Policies\\Microsoft\\Dsh".into(),
                name: "AllowNewsAndInterests".into(),
                value: None,
                drop_empty_key: true,
            })
        );
        // A value was there: undo writes it back and never deletes the key.
        let had = Before { key_existed: Some(true), value: Some(RegValue::Dword(1)), ..Default::default() };
        assert!(matches!(
            set.inverse(&had, None),
            Some(Op::RegSet { value: Some(RegValue::Dword(1)), drop_empty_key: false, .. })
        ));
        // A value of a type we cannot write back: no inverse, so no change.
        let odd = Before { key_existed: Some(true), value: Some(RegValue::Other), ..Default::default() };
        assert_eq!(set.inverse(&odd, None), None);

        let svc = Op::ServiceSet { name: "SysMain".into(), start: StartMode::Disabled, running: Some(false) };
        let before = Before { start: Some(StartMode::Auto), running: Some(true), ..Default::default() };
        assert_eq!(
            svc.inverse(&before, None),
            Some(Op::ServiceSet { name: "SysMain".into(), start: StartMode::Auto, running: Some(true) })
        );
        assert_eq!(svc.inverse(&Before::default(), None), None);
    }
}
