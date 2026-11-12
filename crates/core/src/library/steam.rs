//! Finds installed Steam games by reading Steam's own files. No sign-in and no
//! network: `libraryfolders.vdf` lists the library folders, and each
//! `appmanifest_<id>.acf` in them describes one installed app.

use std::fs;
use std::path::{Path, PathBuf};

use super::vdf;
use super::Scanned;

/// Things Steam installs alongside games that are not games.
const NOT_GAMES: &[&str] = &["228980"]; // Steamworks Common Redistributables
const NOT_GAME_NAMES: &[&str] = &["Proton ", "Steam Linux Runtime", "Steamworks Common", "SteamVR"];

/// `StateFlags` bit for "fully installed".
const FULLY_INSTALLED: u32 = 4;

/// Where Steam is installed on this machine, if it is.
pub fn find_root() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.join("steamapps").is_dir())
}

#[cfg(windows)]
fn candidates() -> Vec<PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let mut out = Vec::new();
    let read = |hive, key: &str, value: &str| -> Option<PathBuf> {
        RegKey::predef(hive).open_subkey(key).ok()?.get_value::<String, _>(value).ok().map(PathBuf::from)
    };
    out.extend(read(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"));
    out.extend(read(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"));
    out.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    out
}

#[cfg(not(windows))]
fn candidates() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    vec![home.join(".local/share/Steam"), home.join(".steam/steam")]
}

/// The Steam program itself, used to start a game.
pub fn client(root: &Path) -> PathBuf {
    root.join(if cfg!(windows) { "steam.exe" } else { "steam.sh" })
}

fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![root.to_path_buf()];
    let listed = fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf"))
        .ok()
        .and_then(|text| vdf::parse(&text));
    if let Some(list) = listed.as_ref().and_then(|v| v.get("libraryfolders")) {
        for (_, entry) in list.children() {
            if let Some(path) = entry.text("path") {
                let path = PathBuf::from(path);
                if !folders.iter().any(|f| same_path(f, &path)) {
                    folders.push(path);
                }
            }
        }
    }
    folders
}

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

/// Steam keeps cover art it has already downloaded. Its layout has changed
/// over the years, so look in each place it has used.
fn art(root: &Path, appid: &str, name: &str) -> Option<PathBuf> {
    let cache = root.join("appcache").join("librarycache");
    let flat = cache.join(format!("{appid}_{name}"));
    if flat.is_file() {
        return Some(flat);
    }
    let dir = cache.join(appid);
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path().join(name))
        .find(|p| p.is_file())
}

/// Every fully installed game in every library folder of this Steam install.
pub fn scan(root: &Path) -> Vec<Scanned> {
    let mut found: Vec<Scanned> = Vec::new();
    for folder in library_folders(root) {
        let apps = folder.join("steamapps");
        let Ok(entries) = fs::read_dir(&apps) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Some(manifest) = fs::read_to_string(entry.path()).ok().and_then(|t| vdf::parse(&t)) else { continue };
            let Some(state) = manifest.get("AppState") else { continue };
            let (Some(appid), Some(title), Some(dir)) = (state.text("appid"), state.text("name"), state.text("installdir"))
            else {
                continue;
            };
            let flags: u32 = state.text("StateFlags").and_then(|f| f.parse().ok()).unwrap_or(0);
            if flags & FULLY_INSTALLED == 0
                || NOT_GAMES.contains(&appid)
                || NOT_GAME_NAMES.iter().any(|n| title.starts_with(n))
                || found.iter().any(|s| s.source_id == appid)
            {
                continue;
            }
            found.push(Scanned {
                source_id: appid.to_string(),
                title: title.to_string(),
                install_path: apps.join("common").join(dir),
                cover: art(root, appid, "library_600x900.jpg"),
                hero: art(root, appid, "library_hero.jpg"),
            });
        }
    }
    found.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(dir: &Path, appid: &str, name: &str, flags: u32) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join(format!("appmanifest_{appid}.acf")),
            format!("\"AppState\"\n{{\n\t\"appid\"\t\"{appid}\"\n\t\"name\"\t\"{name}\"\n\t\"StateFlags\"\t\"{flags}\"\n\t\"installdir\"\t\"{name}\"\n}}\n"),
        )
        .unwrap();
    }

    #[test]
    fn finds_installed_games_across_library_folders() {
        let base = std::env::temp_dir().join(format!("carla-steam-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("Steam");
        let second = base.join("SteamLibrary");
        manifest(&root.join("steamapps"), "620", "Portal 2", 4);
        manifest(&root.join("steamapps"), "228980", "Steamworks Common Redistributables", 4);
        manifest(&root.join("steamapps"), "730", "Half Downloaded", 1026);
        manifest(&second.join("steamapps"), "400", "Portal", 4);
        fs::write(
            root.join("steamapps").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\"0\" {{ \"path\" \"{}\" }}\n\"1\" {{ \"path\" \"{}\" }}\n}}",
                root.to_string_lossy().replace('\\', "\\\\"),
                second.to_string_lossy().replace('\\', "\\\\"),
            ),
        )
        .unwrap();
        // Newer cache layout: art sits in a hashed folder under the app id.
        let hashed = root.join("appcache").join("librarycache").join("620").join("abc123");
        fs::create_dir_all(&hashed).unwrap();
        fs::write(hashed.join("library_600x900.jpg"), b"jpg").unwrap();

        let games = scan(&root);
        let titles: Vec<_> = games.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["Portal", "Portal 2"]);
        assert_eq!(games[1].install_path, root.join("steamapps").join("common").join("Portal 2"));
        assert_eq!(games[1].cover.as_deref(), Some(hashed.join("library_600x900.jpg").as_path()));
        assert_eq!(games[0].cover, None);
        let _ = fs::remove_dir_all(&base);
    }
}
