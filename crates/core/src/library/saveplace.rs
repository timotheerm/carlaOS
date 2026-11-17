//! Where a save folder is, in words that mean the same on every machine.
//!
//! `C:\Users\Anna\Saved Games\Wolf` on one desktop is `C:\Users\Bob\Saved
//! Games\Wolf` on another. A `SavePlace` says "Saved Games, then Wolf", so a
//! second desktop can suggest its own folder. It is only ever a hint: nothing
//! is read or written until the administrator confirms the folder at that
//! desktop.
//!
//! The environment is passed in, so tests never read the real profile.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::saves::sensible;

const MOST_PARTS: usize = 12;
const LONGEST_PART: usize = 100;

/// A well-known place on a Windows machine that a save folder can be below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveRoot {
    SavedGames,
    Documents,
    /// `Documents\My Games`.
    MyGames,
    LocalAppData,
    LocalAppDataLow,
    RoamingAppData,
    ProgramData,
    /// The folder the game is installed in.
    GameFolder,
    /// A root a newer CarlaOS knows and this one does not: the hint is dropped.
    #[serde(other)]
    Unknown,
}

const ROOTS: [SaveRoot; 8] = [
    SaveRoot::SavedGames,
    SaveRoot::Documents,
    SaveRoot::MyGames,
    SaveRoot::LocalAppData,
    SaveRoot::LocalAppDataLow,
    SaveRoot::RoamingAppData,
    SaveRoot::ProgramData,
    SaveRoot::GameFolder,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavePlace {
    pub root: SaveRoot,
    /// Below the root, with `/` between the parts.
    pub rel: String,
}

/// The well-known folders of one machine. Missing ones are `None`.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub saved_games: Option<PathBuf>,
    pub documents: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub local_app_data_low: Option<PathBuf>,
    pub roaming_app_data: Option<PathBuf>,
    pub program_data: Option<PathBuf>,
}

impl Env {
    /// Derived from environment variables, read through `var`.
    pub fn from_vars(var: &dyn Fn(&str) -> Option<String>) -> Self {
        let get = |name: &str| var(name).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
        let home = get("USERPROFILE");
        let local = get("LOCALAPPDATA").or_else(|| home.as_ref().map(|h| h.join("AppData").join("Local")));
        Self {
            saved_games: home.as_ref().map(|h| h.join("Saved Games")),
            documents: home.as_ref().map(|h| h.join("Documents")),
            local_app_data_low: local.as_ref().and_then(|l| l.parent().map(|p| p.join("LocalLow"))),
            local_app_data: local,
            roaming_app_data: get("APPDATA").or_else(|| home.as_ref().map(|h| h.join("AppData").join("Roaming"))),
            program_data: get("PROGRAMDATA"),
        }
    }

    /// This machine's.
    pub fn real() -> Self {
        #[allow(unused_mut)]
        let mut env = Self::from_vars(&|name| std::env::var(name).ok());
        #[cfg(windows)]
        {
            // Windows knows where these really are, also when redirected
            // (OneDrive); the variables are the fallback.
            use windows_sys::Win32::UI::Shell::*;
            let set = |slot: &mut Option<PathBuf>, id| {
                if let Some(path) = known_folder(id).filter(|p| p.is_absolute()) {
                    *slot = Some(path);
                }
            };
            set(&mut env.documents, &FOLDERID_Documents);
            set(&mut env.saved_games, &FOLDERID_SavedGames);
            set(&mut env.local_app_data, &FOLDERID_LocalAppData);
            set(&mut env.local_app_data_low, &FOLDERID_LocalAppDataLow);
            set(&mut env.roaming_app_data, &FOLDERID_RoamingAppData);
            set(&mut env.program_data, &FOLDERID_ProgramData);
        }
        env
    }

    fn root(&self, root: SaveRoot, game_folder: Option<&Path>) -> Option<PathBuf> {
        match root {
            SaveRoot::SavedGames => self.saved_games.clone(),
            SaveRoot::Documents => self.documents.clone(),
            SaveRoot::MyGames => self.documents.as_ref().map(|d| d.join("My Games")),
            SaveRoot::LocalAppData => self.local_app_data.clone(),
            SaveRoot::LocalAppDataLow => self.local_app_data_low.clone(),
            SaveRoot::RoamingAppData => self.roaming_app_data.clone(),
            SaveRoot::ProgramData => self.program_data.clone(),
            SaveRoot::GameFolder => game_folder.map(Path::to_path_buf),
            SaveRoot::Unknown => None,
        }
        .filter(|p| p.is_absolute())
    }
}

#[cfg(windows)]
fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    let mut out: *mut u16 = std::ptr::null_mut();
    // SAFETY: on success `out` is a NUL-terminated string that is ours to free.
    unsafe {
        let hr = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut out);
        if out.is_null() {
            return None;
        }
        let path = (hr == 0).then(|| {
            let mut len = 0;
            while *out.add(len) != 0 {
                len += 1;
            }
            PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(out, len)))
        });
        CoTaskMemFree(out as *const _);
        path
    }
}

fn parts(path: &Path) -> Vec<String> {
    path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()
}

/// The parts of `path` below `root`, when it is below it (letter case does
/// not matter, as on Windows).
fn below(path: &Path, root: &Path) -> Option<Vec<String>> {
    let (path, root) = (parts(path), parts(root));
    let inside = path.len() > root.len() && root.iter().zip(&path).all(|(r, p)| r.to_lowercase() == p.to_lowercase());
    inside.then(|| path[root.len()..].to_vec())
}

/// Names Windows reserves for devices, with or without an extension.
fn reserved(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

fn plain(part: &str) -> bool {
    !part.is_empty()
        && !reserved(part)
        && part.len() <= LONGEST_PART
        && part != "."
        && part != ".."
        && !part.ends_with(' ')
        && !part.ends_with('.')
        && !part.chars().any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
}

/// The place of an absolute folder: below the longest well-known root that
/// holds it. `None` when it is below none of them.
pub fn portable(abs_folder: &Path, env: &Env, game_folder: Option<&Path>) -> Option<SavePlace> {
    if !abs_folder.is_absolute() {
        return None;
    }
    ROOTS
        .iter()
        .filter_map(|&root| {
            let base = env.root(root, game_folder)?;
            let rel = below(abs_folder, &base)?;
            Some((parts(&base).len(), root, rel))
        })
        .max_by_key(|(depth, ..)| *depth)
        .filter(|(_, _, rel)| rel.len() <= MOST_PARTS && rel.iter().all(|p| plain(p)))
        .map(|(_, root, rel)| SavePlace { root, rel: rel.join("/") })
}

fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

/// The folder a place names on this machine. Refuses anything that could
/// lead out of the root. The folder need not exist.
pub fn resolve(place: &SavePlace, env: &Env, game_folder: Option<&Path>) -> Option<PathBuf> {
    let base = env.root(place.root, game_folder)?;
    if place.rel.is_empty() || place.rel.len() > MOST_PARTS * (LONGEST_PART + 1) {
        return None;
    }
    let pieces: Vec<&str> = place.rel.split('/').collect();
    if pieces.len() > MOST_PARTS || !pieces.iter().all(|p| plain(p)) {
        return None;
    }
    let mut path = base.clone();
    for piece in pieces {
        path.push(piece);
        // Whatever already exists on the way must be a plain folder.
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if is_link(&meta) => return None,
            _ => {}
        }
    }
    let normal = path.strip_prefix(&base).ok()?.components().all(|c| matches!(c, Component::Normal(_)));
    (normal && path.starts_with(&base) && sensible(&path)).then_some(path)
}

/// Whether a folder is one where Windows or a game loads programs from, so
/// that files put there could run. Such a folder is never suggested.
pub fn loads_code(path: &Path, place: Option<&SavePlace>) -> bool {
    let low: Vec<String> = parts(path).iter().map(|p| p.to_lowercase()).collect();
    let after = |name: &str, then: &str| low.iter().position(|p| p == name).is_some_and(|i| low[i + 1..].iter().any(|p| p == then));
    let by_name = low.iter().any(|p| matches!(p.as_str(), "startup" | "start menu" | "system32" | "syswow64" | "winsxs"))
        || after("windowspowershell", "modules")
        || after("powershell", "modules");
    let bin = |p: &str| matches!(p, "bin" | "binaries" | "win64" | "win32" | "x64" | "x86");
    let in_game_bin = place.is_some_and(|pl| pl.root == SaveRoot::GameFolder && (pl.rel.is_empty() || pl.rel.split('/').next().is_some_and(|p| bin(&p.to_lowercase()))));
    by_name || in_game_bin
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        let vars = |name: &str| match name {
            "USERPROFILE" => Some(d("/u/Bob")),
            "LOCALAPPDATA" => Some(d("/u/Bob/AppData/Local")),
            "APPDATA" => Some(d("/u/Bob/AppData/Roaming")),
            "PROGRAMDATA" => Some(d("/pd")),
            _ => None,
        };
        let mut env = Env::from_vars(&vars);
        env.program_data = Some(abs("/pd"));
        env
    }

    fn abs(s: &str) -> PathBuf {
        PathBuf::from(d(s))
    }

    /// Drive paths on Windows, where `/u` alone is not absolute.
    fn d(s: &str) -> String {
        if cfg!(windows) { format!("C:{s}") } else { s.to_string() }
    }

    #[test]
    fn the_longest_root_wins_and_the_path_is_portable() {
        let env = env();
        let place = portable(&abs("/u/Bob/Documents/My Games/Wolf/saves"), &env, None).unwrap();
        assert_eq!((place.root, place.rel.as_str()), (SaveRoot::MyGames, "Wolf/saves"));
        let place = portable(&abs("/u/Bob/AppData/LocalLow/Studio/Game"), &env, None).unwrap();
        assert_eq!((place.root, place.rel.as_str()), (SaveRoot::LocalAppDataLow, "Studio/Game"));
        let place = portable(&abs("/u/bob/saved games/Doom"), &env, None).unwrap();
        assert_eq!((place.root, place.rel.as_str()), (SaveRoot::SavedGames, "Doom"));
        let game = abs("/games/Wolf");
        let place = portable(&abs("/games/Wolf/save"), &env, Some(&game)).unwrap();
        assert_eq!((place.root, place.rel.as_str()), (SaveRoot::GameFolder, "save"));
        // Outside every root, or the root itself: no hint.
        assert!(portable(&abs("/elsewhere/saves"), &env, None).is_none());
        assert!(portable(&abs("/u/Bob/Saved Games"), &env, None).is_none());
        assert!(portable(&abs("relative/x"), &env, None).is_none());
    }

    #[test]
    fn a_place_resolves_on_another_machine() {
        let other = Env::from_vars(&|name| match name {
            "USERPROFILE" => Some(d("/u/Anna")),
            _ => None,
        });
        let place = SavePlace { root: SaveRoot::MyGames, rel: "Wolf/saves".into() };
        assert_eq!(resolve(&place, &other, None), Some(abs("/u/Anna/Documents/My Games/Wolf/saves")));
        // No such root there.
        let place = SavePlace { root: SaveRoot::ProgramData, rel: "Wolf".into() };
        assert_eq!(resolve(&place, &other, None), None);
        let place = SavePlace { root: SaveRoot::GameFolder, rel: "Wolf".into() };
        assert_eq!(resolve(&place, &other, None), None);
        assert_eq!(resolve(&place, &other, Some(&abs("/g/Wolf"))), Some(abs("/g/Wolf/Wolf")));
    }

    #[test]
    fn hostile_places_are_refused() {
        let env = env();
        let long = "a".repeat(5000);
        let many = vec!["a"; 40].join("/");
        let toolong = "b".repeat(101);
        for rel in ["", "..", "../x", "a/../b", "/abs", "C:/Windows", "C:", "a\\b", "\\\\host\\share", "a//b", ".", "a/.", "x:y", "a/", &long, &many, &toolong, "con?", "a /b", "a./b"] {
            let place = SavePlace { root: SaveRoot::SavedGames, rel: rel.to_string() };
            assert_eq!(resolve(&place, &env, None), None, "{rel:?}");
        }
        assert!(resolve(&SavePlace { root: SaveRoot::SavedGames, rel: "Fine Game".into() }, &env, None).is_some());
    }

    #[test]
    fn device_names_are_refused_in_every_part() {
        let env = env();
        for name in ["NUL", "nul.txt", "Con", "PRN", "aux", "COM1", "com9.log", "LPT1", "lpt9", "CONIN$", "conout$", "NUL .txt"] {
            for rel in [name.to_string(), format!("Game/{name}"), format!("{name}/Game")] {
                let place = SavePlace { root: SaveRoot::SavedGames, rel: rel.clone() };
                assert_eq!(resolve(&place, &env, None), None, "{rel}");
            }
        }
        // Near misses are fine.
        for ok in ["COM0", "COM10", "LPT", "Console", "Nullsoft", "auxiliary"] {
            let place = SavePlace { root: SaveRoot::SavedGames, rel: ok.to_string() };
            assert!(resolve(&place, &env, None).is_some(), "{ok}");
        }
        // And a folder that is one never gets a place.
        assert!(portable(&abs("/u/Bob/Saved Games/NUL"), &env, None).is_none());
    }

    #[test]
    fn folders_that_load_code_are_known() {
        let game = |rel: &str| SavePlace { root: SaveRoot::GameFolder, rel: rel.to_string() };
        for bad in ["/u/Bob/Documents/WindowsPowerShell/Modules/X", "/u/Bob/Documents/PowerShell/Modules", "/u/Bob/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup/G", "/x/Windows/System32/G", "/x/Start Menu/G"] {
            assert!(loads_code(&abs(bad), None), "{bad}");
        }
        assert!(!loads_code(&abs("/u/Bob/Saved Games/Wolf"), None));
        assert!(!loads_code(&abs("/u/Bob/Documents/PowerShell/Profile"), None));
        for rel in ["", "bin/x", "Binaries/Win64", "WIN64"] {
            assert!(loads_code(&abs("/g/Wolf/x"), Some(&game(rel))), "{rel}");
        }
        assert!(!loads_code(&abs("/g/Wolf/saves"), Some(&game("saves"))));
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_below_the_root_is_refused_but_the_root_may_be_one() {
        let dir = std::env::temp_dir().join(format!("carla-junction-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("Saved Games");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(dir.join("elsewhere").join("x")).unwrap();
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(root.join("jn")).arg(dir.join("elsewhere")).output().unwrap();
        assert!(made.status.success(), "mklink failed: {}", String::from_utf8_lossy(&made.stdout));
        let env = Env { saved_games: Some(root.clone()), ..Env::default() };
        let place = SavePlace { root: SaveRoot::SavedGames, rel: "jn/x".into() };
        assert_eq!(resolve(&place, &env, None), None);
        // The root itself being a junction (a redirected folder) is fine.
        let through = dir.join("via");
        let made = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&through).arg(dir.join("elsewhere")).output().unwrap();
        assert!(made.status.success(), "mklink failed: {}", String::from_utf8_lossy(&made.stdout));
        let env = Env { saved_games: Some(through.clone()), ..Env::default() };
        assert_eq!(resolve(&SavePlace { root: SaveRoot::SavedGames, rel: "x".into() }, &env, None), Some(through.join("x")));
        let _ = std::process::Command::new("cmd").args(["/C", "rmdir"]).arg(root.join("jn")).output();
        let _ = std::process::Command::new("cmd").args(["/C", "rmdir"]).arg(&through).output();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_link_on_the_way_is_refused() {
        let dir = std::env::temp_dir().join(format!("carla-place-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Saved Games").join("real")).unwrap();
        let env = Env { saved_games: Some(dir.join("Saved Games")), ..Env::default() };
        let place = SavePlace { root: SaveRoot::SavedGames, rel: "real".into() };
        assert!(resolve(&place, &env, None).is_some());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&dir, dir.join("Saved Games").join("link")).unwrap();
            let place = SavePlace { root: SaveRoot::SavedGames, rel: "link/x".into() };
            assert_eq!(resolve(&place, &env, None), None);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_place_survives_json_and_unknown_roots_do_not() {
        let place = SavePlace { root: SaveRoot::LocalAppDataLow, rel: "A/B".into() };
        let text = serde_json::to_string(&place).unwrap();
        assert_eq!(text, r#"{"root":"local_app_data_low","rel":"A/B"}"#);
        assert_eq!(serde_json::from_str::<SavePlace>(&text).unwrap(), place);
        // A root a newer CarlaOS adds only drops the hint.
        let unknown: SavePlace = serde_json::from_str(r#"{"root":"system32","rel":"x"}"#).unwrap();
        assert_eq!(unknown.root, SaveRoot::Unknown);
        assert_eq!(resolve(&unknown, &env(), Some(&abs("/g"))), None);
        let found: crate::link::SavesFound = serde_json::from_str(r#"{"key":"k","manifest":{"files":[]},"place":{"root":"future","rel":"x"}}"#).unwrap();
        assert_eq!(found.place.unwrap().root, SaveRoot::Unknown);
    }
}
