//! What the Steam accounts on this machine own, read from Steam's own files.
//! No sign-in and no network.
//!
//! `appcache/packageinfo.vdf` holds the packages those accounts have a
//! licence for, each with the apps in it. `appcache/appinfo.vdf` describes
//! those apps: name, kind, and the folder they install into. Together they
//! give the games of the library, including the ones that are not installed.
//!
//! Both files are Steam's private formats and have changed over the years.
//! Anything that does not look as expected is refused, and the library then
//! simply shows what is installed.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use super::bvdf::{self, Node};
use super::steam;

/// A game of the library.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct App {
    pub appid: String,
    pub title: String,
    /// The folder under `steamapps/common` it installs into, when known.
    pub installdir: String,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// The package Steam gives every account. It holds free tools and servers,
/// not what the account chose.
const EVERYONE: u32 = 0;
const NO_MORE_PACKAGES: u32 = 0xFFFF_FFFF;

/// The apps in every licensed package of `packageinfo.vdf`.
pub fn package_apps(bytes: &[u8]) -> Option<BTreeSet<u32>> {
    // Each entry: package id, a checksum, a change number, in newer files an
    // access token, then the package as a binary key-value map.
    let before_map = match u32_at(bytes, 0)? {
        0x0656_5527 => 4 + 20 + 4,
        0x0656_5528 => 4 + 20 + 4 + 8,
        _ => return None,
    };
    let mut apps = BTreeSet::new();
    let mut at = 8;
    loop {
        let id = u32_at(bytes, at)?;
        if id == NO_MORE_PACKAGES {
            return Some(apps);
        }
        at = at.checked_add(before_map)?;
        let (map, used) = bvdf::parse_prefix(bytes.get(at..)?, None)?;
        at += used;
        if id == EVERYONE {
            continue;
        }
        for (_, package) in map.children() {
            let listed = package.get("appids").into_iter().flat_map(Node::children);
            apps.extend(listed.filter_map(|(_, app)| app.number()).filter_map(|n| u32::try_from(n).ok()));
        }
    }
}

/// The table of names at the end of a newer `appinfo.vdf`.
fn names(bytes: &[u8], at: usize) -> Option<Vec<String>> {
    let count = u32_at(bytes, at)? as usize;
    let mut rest = bytes.get(at + 4..)?;
    // Every name takes at least its end byte.
    if count > rest.len() {
        return None;
    }
    let mut table = Vec::with_capacity(count);
    for _ in 0..count {
        let len = rest.iter().position(|b| *b == 0)?;
        table.push(String::from_utf8_lossy(&rest[..len]).into_owned());
        rest = &rest[len + 1..];
    }
    Some(table)
}

fn safe_folder(name: &str) -> bool {
    !name.is_empty() && name.len() <= 200 && name != "." && name != ".." && !name.contains(['/', '\\', ':', '"', '<', '>', '|', '?', '*']) && !name.chars().any(char::is_control)
}

fn game(appid: u32, map: &Node) -> Option<App> {
    let info = map.get("appinfo")?;
    let common = info.get("common")?;
    if !common.get("type").and_then(Node::text).is_some_and(|kind| kind.eq_ignore_ascii_case("game")) {
        return None;
    }
    // No list of systems means Windows only.
    let systems = common.get("oslist").and_then(Node::text).unwrap_or_default();
    if !systems.is_empty() && !systems.split(',').any(|s| s.trim().eq_ignore_ascii_case("windows")) {
        return None;
    }
    let title = common.get("name").and_then(Node::text)?.trim();
    if title.is_empty() {
        return None;
    }
    let installdir = info.get("config").and_then(|c| c.get("installdir")).and_then(Node::text).filter(|dir| safe_folder(dir)).unwrap_or_default();
    Some(App { appid: appid.to_string(), title: title.to_string(), installdir: installdir.to_string() })
}

/// The games among `wanted` that `appinfo.vdf` describes, by title.
pub fn apps(bytes: &[u8], wanted: &BTreeSet<u32>) -> Option<Vec<App>> {
    // After the header, each entry is: app id, the size of what follows,
    // some bookkeeping (longer in newer files), then the app as a binary
    // key-value map. The newest files keep the names of that map in a table
    // at the end.
    let (bookkeeping, table, mut at) = match u32_at(bytes, 0)? {
        0x0756_4427 => (40, None, 8),
        0x0756_4428 => (60, None, 8),
        0x0756_4429 => {
            let offset = i64::from_le_bytes(bytes.get(8..16)?.try_into().ok()?);
            (60, Some(names(bytes, usize::try_from(offset).ok()?)?), 16)
        }
        _ => return None,
    };
    let mut found = Vec::new();
    loop {
        let appid = u32_at(bytes, at)?;
        if appid == 0 {
            break;
        }
        let size = u32_at(bytes, at + 4)? as usize;
        let entry = bytes.get(at + 8..(at + 8).checked_add(size)?)?;
        at += 8 + size;
        if !wanted.contains(&appid) {
            continue;
        }
        // One app that cannot be read does not hide the others.
        let described = entry.get(bookkeeping..).and_then(|map| bvdf::parse_prefix(map, table.as_deref()));
        found.extend(described.and_then(|(map, _)| game(appid, &map)));
    }
    found.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    Some(found)
}

type Read = (PathBuf, Option<SystemTime>, Option<SystemTime>, Vec<App>);

/// Every game the accounts on this machine own, by title. The files are a
/// few megabytes and change rarely, so the answer is kept until they do.
pub fn library(root: &Path) -> Result<Vec<App>, String> {
    static LAST: Mutex<Option<Read>> = Mutex::new(None);
    let cache = root.join("appcache");
    let (packages, described) = (cache.join("packageinfo.vdf"), cache.join("appinfo.vdf"));
    let changed = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let (p, a) = (changed(&packages), changed(&described));
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, _, apps)) = last.as_ref().filter(|(r, lp, la, _)| r == root && *lp == p && *la == a && p.is_some()) {
        return Ok(apps.clone());
    }
    let never = || "Steam has not been opened on this machine yet, so its library is not known.".to_string();
    let newer = || "This version of Steam keeps its library in a way CarlaOS does not read yet.".to_string();
    let wanted = package_apps(&std::fs::read(&packages).map_err(|_| never())?).ok_or_else(newer)?;
    let found = apps(&std::fs::read(&described).map_err(|_| never())?, &wanted).ok_or_else(newer)?;
    *last = Some((root.to_path_buf(), p, a, found.clone()));
    Ok(found)
}

/// The games of the library that are not installed on this machine,
/// including the ones Steam is fetching now.
pub fn not_installed(root: &Path) -> Result<Vec<App>, String> {
    let known: BTreeSet<String> = steam::manifests(root).into_iter().filter(|m| m.installed()).map(|m| m.appid).collect();
    Ok(library(root)?.into_iter().filter(|app| !known.contains(&app.appid)).collect())
}

/// Builds the two files, for tests here and where installs are started.
#[cfg(test)]
pub(crate) mod write {
    use super::super::bvdf::write::{int, map, text};

    pub fn packages(list: &[(u32, &[u32])], newer: bool) -> Vec<u8> {
        let mut out = if newer { 0x0656_5528u32 } else { 0x0656_5527u32 }.to_le_bytes().to_vec();
        out.extend(1u32.to_le_bytes());
        for (id, apps) in list {
            out.extend(id.to_le_bytes());
            out.extend([0u8; 24]);
            if newer {
                out.extend([0u8; 8]);
            }
            let apps: Vec<_> = apps.iter().enumerate().map(|(i, app)| int(&i.to_string(), *app as i32)).collect();
            out.extend(map(&id.to_string(), &[int("packageid", *id as i32), map("appids", &apps)]));
            out.push(0x08);
        }
        out.extend(0xFFFF_FFFFu32.to_le_bytes());
        out
    }

    pub fn app(name: &str, kind: &str, systems: &str, installdir: &str) -> Vec<u8> {
        let mut out = map("appinfo", &[map("common", &[text("name", name), text("type", kind), text("oslist", systems)]), map("config", &[text("installdir", installdir)])]);
        out.push(0x08);
        out
    }

    /// The layout before names moved into a table.
    pub fn appinfo(list: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = 0x0756_4428u32.to_le_bytes().to_vec();
        out.extend(1u32.to_le_bytes());
        for (id, described) in list {
            out.extend(id.to_le_bytes());
            out.extend(((60 + described.len()) as u32).to_le_bytes());
            out.extend([0u8; 60]);
            out.extend(described);
        }
        out.extend(0u32.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(ids: &[u32]) -> BTreeSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn licensed_packages_give_their_apps_but_not_the_package_everyone_has() {
        for newer in [false, true] {
            let file = write::packages(&[(0, &[7, 480]), (1234, &[620, 621]), (99, &[400])], newer);
            assert_eq!(package_apps(&file), Some(wanted(&[400, 620, 621])), "newer layout: {newer}");
            assert_eq!(package_apps(&file[..file.len() - 6]), None, "a file cut short is refused");
        }
        assert_eq!(package_apps(b"\x01\x02\x03\x04\x05\x06\x07\x08rest"), None, "not this kind of file");
    }

    #[test]
    fn only_windows_games_of_the_library_are_listed() {
        let file = write::appinfo(&[
            (620, write::app("Portal 2", "Game", "windows,macos,linux", "Portal 2")),
            (621, write::app("Portal 2 Soundtrack", "Music", "", "Portal 2 OST")),
            (400, write::app("portal", "game", "", "Portal")),
            (70, write::app("Not Owned", "game", "", "Elsewhere")),
            (80, write::app("Mac Only", "game", "macos", "Mac Only")),
            (90, write::app("Odd Folder", "game", "windows", "..\\..\\Windows")),
            (95, vec![0x63, 0x63, 0x63]),
        ]);
        let games = apps(&file, &wanted(&[620, 621, 400, 80, 90, 95])).unwrap();
        let listed: Vec<_> = games.iter().map(|g| (g.appid.as_str(), g.title.as_str(), g.installdir.as_str())).collect();
        // A folder that points outside the library is not believed; the game is still listed.
        assert_eq!(listed, [("90", "Odd Folder", ""), ("400", "portal", "Portal"), ("620", "Portal 2", "Portal 2")]);
        assert_eq!(apps(&file[..file.len() - 9], &wanted(&[620])), None, "a file cut short is refused");
    }

    #[test]
    fn the_newest_layout_keeps_names_in_a_table() {
        // Names by number: 0 appinfo, 1 common, 2 name, 3 type, 4 config, 5 installdir.
        let key = |n: u32| n.to_le_bytes();
        let text = |n: u32, value: &str| [&[0x01][..], &key(n), value.as_bytes(), &[0]].concat();
        let map = |n: u32, inside: &[Vec<u8>]| [&[0x00][..], &key(n), &inside.concat(), &[0x08]].concat();
        let mut described = map(0, &[map(1, &[text(2, "Neon Harbor"), text(3, "game")]), map(4, &[text(5, "NeonHarbor")])]);
        described.push(0x08);

        let mut file = 0x0756_4429u32.to_le_bytes().to_vec();
        file.extend(1u32.to_le_bytes());
        file.extend([0u8; 8]); // where the table is; filled in below
        file.extend(10u32.to_le_bytes());
        file.extend(((60 + described.len()) as u32).to_le_bytes());
        file.extend([0u8; 60]);
        file.extend(&described);
        file.extend(0u32.to_le_bytes());
        let table = file.len() as i64;
        file[8..16].copy_from_slice(&table.to_le_bytes());
        file.extend(6u32.to_le_bytes());
        for name in ["appinfo", "common", "name", "type", "config", "installdir"] {
            file.extend(name.as_bytes());
            file.push(0);
        }

        let games = apps(&file, &wanted(&[10])).unwrap();
        assert_eq!(games, [App { appid: "10".into(), title: "Neon Harbor".into(), installdir: "NeonHarbor".into() }]);
        file.truncate(file.len() - 4);
        assert_eq!(apps(&file, &wanted(&[10])), None, "a table cut short is refused");
    }

    #[test]
    fn what_is_installed_is_not_offered_and_what_is_being_fetched_still_is() {
        let root = std::env::temp_dir().join(format!("carla-owned-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(library(&root).is_err(), "no Steam files, no library");
        std::fs::create_dir_all(root.join("appcache")).unwrap();
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(root.join("appcache").join("packageinfo.vdf"), write::packages(&[(5, &[10, 20, 30])], true)).unwrap();
        let described = write::appinfo(&[(10, write::app("Installed", "game", "", "Installed")), (20, write::app("Half Fetched", "game", "", "Half")), (30, write::app("Waiting", "game", "", "Waiting"))]);
        std::fs::write(root.join("appcache").join("appinfo.vdf"), described).unwrap();
        for (id, flags) in [("10", 4), ("20", 1026)] {
            std::fs::write(root.join("steamapps").join(format!("appmanifest_{id}.acf")), format!("\"AppState\"\n{{\n\"appid\" \"{id}\"\n\"name\" \"x\"\n\"StateFlags\" \"{flags}\"\n\"installdir\" \"x\"\n}}\n")).unwrap();
        }
        assert_eq!(library(&root).unwrap().len(), 3);
        assert_eq!(not_installed(&root).unwrap().iter().map(|g| g.title.as_str()).collect::<Vec<_>>(), ["Half Fetched", "Waiting"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
