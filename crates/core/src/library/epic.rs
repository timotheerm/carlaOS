//! Finds games installed through the Epic Games Launcher by reading the
//! launcher's own manifest files. No sign-in and no network: every installed
//! item has one small JSON file under the launcher's `Manifests` folder.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{Scanned, Update, UpdateState};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Item {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    install_location: String,
    #[serde(default)]
    catalog_namespace: String,
    #[serde(default)]
    catalog_item_id: String,
    #[serde(default)]
    app_categories: Vec<String>,
    #[serde(default, rename = "bIsApplication")]
    is_application: bool,
    #[serde(default, rename = "bIsIncompleteInstall")]
    is_incomplete: bool,
}

/// Where the launcher keeps its manifests on this machine, if it is installed.
pub fn manifests() -> Option<PathBuf> {
    let data = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let dir = data.join("Epic").join("EpicGamesLauncher").join("Data").join("Manifests");
    dir.is_dir().then_some(dir)
}

/// Whether the launcher is installing a newer version of one game. The
/// launcher keeps what it is working on in a `Pending` folder beside its
/// manifests. It only learns of a new version while it runs and says nothing
/// about one it has not started on, so this is "being updated", never "an
/// update exists".
pub fn update_of(manifests: &Path, app_name: &str) -> Option<Update> {
    let pending = fs::read_dir(manifests.join("Pending")).ok()?;
    pending
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<Item>(&text).ok())
        .any(|item| item.app_name == app_name)
        .then_some(Update { state: UpdateState::Running, bytes: 0, done: 0 })
}

/// Only letters, digits and a few marks: these go into a web-style address.
fn plain(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}

/// The address that asks the launcher to start a game.
fn launch_address(item: &Item) -> String {
    let app = if plain(&item.catalog_namespace) && plain(&item.catalog_item_id) {
        format!("{}%3A{}%3A{}", item.catalog_namespace, item.catalog_item_id, item.app_name)
    } else {
        item.app_name.clone()
    };
    format!("com.epicgames.launcher://apps/{app}?action=launch&silent=true")
}

/// Every fully installed game the launcher knows about.
pub fn scan(manifests: &Path) -> Vec<Scanned> {
    let mut found: Vec<Scanned> = Vec::new();
    let Ok(entries) = fs::read_dir(manifests) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("item") {
            continue;
        }
        let Some(item) = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Item>(&text).ok()) else { continue };
        let install = PathBuf::from(&item.install_location);
        // Add-ons and the Unreal Engine itself have manifests too.
        let game = item.is_application && item.app_categories.iter().any(|c| c == "games");
        if !game || item.is_incomplete || item.display_name.trim().is_empty() || !plain(&item.app_name) || !install.is_dir() {
            continue;
        }
        if found.iter().any(|s| s.source_id == item.app_name) {
            continue;
        }
        found.push(Scanned {
            source_id: item.app_name.clone(),
            title: item.display_name.trim().to_string(),
            install_path: install,
            args: launch_address(&item),
            update: update_of(manifests, &item.app_name),
            ..Scanned::default()
        });
    }
    found.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(dir: &Path, file: &str, name: &str, app: &str, install: &Path, extra: &str) {
        let install = install.to_string_lossy().replace('\\', "\\\\");
        fs::write(
            dir.join(file),
            format!(r#"{{"FormatVersion":0,"DisplayName":"{name}","AppName":"{app}","InstallLocation":"{install}","CatalogNamespace":"ns1","CatalogItemId":"item1","AppCategories":["public","games","applications"],"bIsApplication":true{extra}}}"#),
        )
        .unwrap();
    }

    #[test]
    fn finds_installed_games_and_leaves_out_add_ons_and_half_installs() {
        let base = std::env::temp_dir().join(format!("carla-epic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (manifests, games) = (base.join("Manifests"), base.join("Games"));
        fs::create_dir_all(&manifests).unwrap();
        fs::create_dir_all(games.join("Velvet")).unwrap();
        fs::create_dir_all(games.join("Half")).unwrap();
        item(&manifests, "a.item", "Velvet Circuit", "Jade", &games.join("Velvet"), "");
        item(&manifests, "b.item", "Half There", "Opal", &games.join("Half"), r#","bIsIncompleteInstall":true"#);
        item(&manifests, "c.item", "Gone Away", "Ruby", &games.join("Missing"), "");
        item(&manifests, "d.item", "Odd Name", "bad name?x=1", &games.join("Velvet"), "");
        fs::write(manifests.join("e.item"), r#"{"DisplayName":"Extra Tracks","AppName":"JadeDlc","InstallLocation":"x","AppCategories":["addons"],"bIsApplication":false}"#).unwrap();
        fs::write(manifests.join("f.item"), "not json").unwrap();
        fs::write(manifests.join("notes.txt"), "ignored").unwrap();

        let found = scan(&manifests);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].source_id.as_str(), found[0].title.as_str()), ("Jade", "Velvet Circuit"));
        assert_eq!(found[0].install_path, games.join("Velvet"));
        assert_eq!(found[0].args, "com.epicgames.launcher://apps/ns1%3Aitem1%3AJade?action=launch&silent=true");
        assert_eq!(found[0].update, None);
        // The launcher is putting a newer version in place.
        fs::create_dir_all(manifests.join("Pending")).unwrap();
        item(&manifests.join("Pending"), "a.item", "Velvet Circuit", "Jade", &games.join("Velvet"), "");
        assert_eq!(scan(&manifests)[0].update.map(|u| u.state), Some(UpdateState::Running));
        assert_eq!(update_of(&manifests, "Opal"), None);
        let _ = fs::remove_dir_all(&base);
    }
}
