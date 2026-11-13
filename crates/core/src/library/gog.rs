//! Finds games installed through GOG (GOG Galaxy or the plain installers).
//! Each one writes a registry key saying where it is and what to start, and
//! GOG games start by themselves, with no launcher in between.

use std::path::PathBuf;

use super::Scanned;

/// One game as its registry key describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub exe: String,
    pub working_dir: String,
    pub launch_param: String,
    /// Set for add-ons: the id of the game they belong to.
    pub depends_on: String,
}

#[cfg(windows)]
pub fn entries() -> Vec<Entry> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY};
    use winreg::RegKey;
    let Ok(games) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(r"SOFTWARE\GOG.com\Games", KEY_READ | KEY_WOW64_32KEY) else {
        return Vec::new();
    };
    games
        .enum_keys()
        .flatten()
        .filter_map(|id| {
            let key = games.open_subkey(&id).ok()?;
            let value = |name: &str| key.get_value::<String, _>(name).unwrap_or_default();
            Some(Entry {
                id,
                name: value("gameName"),
                path: value("path"),
                exe: value("exe"),
                working_dir: value("workingDir"),
                launch_param: value("launchParam"),
                depends_on: value("dependsOn"),
            })
        })
        .collect()
}

#[cfg(not(windows))]
pub fn entries() -> Vec<Entry> {
    Vec::new()
}

/// The games among the entries whose program is really there.
pub fn games(entries: Vec<Entry>) -> Vec<Scanned> {
    let mut found: Vec<Scanned> = Vec::new();
    for e in entries {
        let exe = PathBuf::from(&e.exe);
        let numeric = !e.id.is_empty() && e.id.bytes().all(|b| b.is_ascii_digit());
        if !numeric || !e.depends_on.trim().is_empty() || e.name.trim().is_empty() || !exe.is_file() || found.iter().any(|s| s.source_id == e.id) {
            continue;
        }
        let folder = [&e.working_dir, &e.path].into_iter().map(PathBuf::from).find(|p| p.is_dir());
        found.push(Scanned {
            source_id: e.id,
            title: e.name.trim().to_string(),
            install_path: folder.or_else(|| exe.parent().map(PathBuf::from)).unwrap_or_default(),
            exe: Some(exe),
            args: e.launch_param.trim().to_string(),
            ..Scanned::default()
        });
    }
    found.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    found
}

pub fn scan() -> Vec<Scanned> {
    games(entries())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn keeps_games_whose_program_exists_and_leaves_out_add_ons() {
        let base = std::env::temp_dir().join(format!("carla-gog-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("Tides")).unwrap();
        let exe = base.join("Tides").join("tides.exe");
        fs::write(&exe, b"program").unwrap();
        let text = |p: &std::path::Path| p.to_string_lossy().into_owned();
        let game = Entry { id: "1207658924".into(), name: "Hollow Tides".into(), path: text(&base.join("Tides")), exe: text(&exe), launch_param: "-skipintro".into(), ..Entry::default() };
        let found = games(vec![
            game.clone(),
            game.clone(),
            Entry { id: "1207658925".into(), name: "Hollow Tides Soundtrack".into(), depends_on: "1207658924".into(), ..game.clone() },
            Entry { id: "1207658926".into(), name: "Uninstalled".into(), exe: text(&base.join("gone.exe")), ..game.clone() },
            Entry { id: "../x".into(), name: "Odd Id".into(), ..game.clone() },
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].source_id.as_str(), found[0].title.as_str(), found[0].args.as_str()), ("1207658924", "Hollow Tides", "-skipintro"));
        assert_eq!((found[0].exe.as_deref(), found[0].install_path.as_path()), (Some(exe.as_path()), base.join("Tides").as_path()));
        let _ = fs::remove_dir_all(&base);
    }
}
