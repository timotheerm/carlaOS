//! Noticing a game that CarlaOS did not start.
//!
//! A game can be running without CarlaOS having started it: it was started
//! from Steam's own window, by a shortcut, or CarlaOS itself was restarted
//! while the game stayed open. Such a game is recognised from the programs
//! that are running, so it gets its playtime, the quick menu and the "now
//! playing" bar like any other.
//!
//! This module only decides, from a list of running programs. Reading that
//! list is the caller's job.

use std::path::{Path, PathBuf};

use crate::library::Install;

/// Stores keep each game in a folder of its own, so any program inside that
/// folder is the game. Anything else is only recognised by its exact program.
const STORES_WITH_FOLDERS: [&str; 3] = ["steam", "epic", "gog"];

/// Programs that live in a game's folder without being the game: installers
/// of things the game needs, crash reporters, uninstallers.
const HELPERS: [&str; 14] = [
    "crashhandler",
    "crashpad",
    "crashreport",
    "crashsender",
    "unins",
    "setup",
    "install",
    "redist",
    "dxwebsetup",
    "dotnet",
    "ndp4",
    "easyanticheat_setup",
    "touchup",
    "cleanup",
];

/// Folders inside a game's folder that only hold such helpers.
const HELPER_FOLDERS: [&str; 4] = ["/_commonredist/", "/redist/", "/redistributables/", "/__installer/"];

fn normal(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

/// One installed game, as far as recognising it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub game_id: String,
    /// With a trailing slash; empty when the folder says nothing.
    folder: String,
    /// The game's own program, when it is known.
    program: Option<String>,
}

impl Candidate {
    pub fn of(install: &Install) -> Option<Self> {
        // A cloud game is a page in a browser, not a program of its own.
        if install.source == "cloud" {
            return None;
        }
        let mut folder = String::new();
        if STORES_WITH_FOLDERS.contains(&install.source.as_str()) {
            folder = normal(&install.install_path);
            if !folder.ends_with('/') {
                folder.push('/');
            }
            // A folder this short would match half the machine.
            if folder.len() < 8 {
                folder.clear();
            }
        }
        let program = install.exe.as_deref().map(normal).filter(|p| !p.is_empty());
        if folder.is_empty() && program.is_none() {
            return None;
        }
        Some(Self { game_id: install.game_id.clone(), folder, program })
    }

    /// How surely a running program is this game: nothing, or the length of
    /// what matched, so the most exact match can win.
    fn matches(&self, program: &str) -> Option<usize> {
        if self.program.as_deref() == Some(program) {
            return Some(usize::MAX);
        }
        (!self.folder.is_empty() && program.starts_with(&self.folder) && !is_helper(program, self.folder.len())).then_some(self.folder.len())
    }
}

/// Whether a program inside a game's folder is something other than the game.
fn is_helper(program: &str, folder_len: usize) -> bool {
    // Names are compared without regard to capitals on every system.
    let inside = program[folder_len.saturating_sub(1)..].to_lowercase();
    if HELPER_FOLDERS.iter().any(|f| inside.contains(f)) {
        return true;
    }
    let name = inside.rsplit('/').next().unwrap_or(&inside);
    HELPERS.iter().any(|h| name.contains(h))
}

/// The one game that is running, judged from the running programs. Nothing
/// when no game is, and nothing when it could be more than one: an emulator
/// that is open says nothing about which of its games is being played.
pub fn spot<'a>(candidates: &'a [Candidate], running: &[PathBuf]) -> Option<&'a Candidate> {
    let mut best: Option<(usize, &Candidate)> = None;
    let mut tied = false;
    for program in running.iter().map(|p| normal(p)) {
        for candidate in candidates {
            let Some(score) = candidate.matches(&program) else { continue };
            match best {
                Some((top, found)) if score == top && found.game_id != candidate.game_id => tied = true,
                Some((top, _)) if score <= top => {}
                _ => {
                    best = Some((score, candidate));
                    tied = false;
                }
            }
        }
    }
    best.filter(|_| !tied).map(|(_, candidate)| candidate)
}

/// How many looks in a row must show the same game before it counts. One
/// look could be a store checking a game's files.
pub const SIGHTINGS: u8 = 2;

/// Turns single looks into a decision.
#[derive(Debug, Default)]
pub struct Spotter {
    seen: Option<String>,
    times: u8,
}

impl Spotter {
    /// Give what one look found. Says the game once it has been seen
    /// [`SIGHTINGS`] times in a row, and then starts counting afresh.
    pub fn observe(&mut self, found: Option<&str>) -> Option<String> {
        match found {
            Some(id) if self.seen.as_deref() == Some(id) => self.times += 1,
            Some(id) => {
                self.seen = Some(id.to_string());
                self.times = 1;
            }
            None => {
                self.seen = None;
                self.times = 0;
            }
        }
        if self.times >= SIGHTINGS {
            self.times = 0;
            self.seen.take()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(id: &str, source: &str, folder: &str, exe: Option<&str>) -> Install {
        Install {
            game_id: id.into(),
            title: id.into(),
            source: source.into(),
            source_id: id.into(),
            install_path: PathBuf::from(folder),
            exe: exe.map(PathBuf::from),
            args: String::new(),
        }
    }

    fn candidates(installs: &[Install]) -> Vec<Candidate> {
        installs.iter().filter_map(Candidate::of).collect()
    }

    fn running(programs: &[&str]) -> Vec<PathBuf> {
        programs.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_store_game_is_recognised_by_any_program_in_its_folder() {
        let games = candidates(&[install("steam:1", "steam", "/games/steamapps/common/Neon Harbor", None), install("steam:2", "steam", "/games/steamapps/common/Other", None)]);
        let found = spot(&games, &running(&["/usr/bin/steam", "/games/steamapps/common/Neon Harbor/bin/harbor.exe"]));
        assert_eq!(found.map(|c| c.game_id.as_str()), Some("steam:1"));
        assert_eq!(spot(&games, &running(&["/usr/bin/steam"])), None);
    }

    #[test]
    fn helpers_in_a_games_folder_are_not_the_game() {
        let games = candidates(&[install("steam:1", "steam", "/games/steamapps/common/Neon Harbor", None)]);
        for helper in [
            "/games/steamapps/common/Neon Harbor/_CommonRedist/vcredist/2019/VC_redist.x64.exe",
            "/games/steamapps/common/Neon Harbor/UnityCrashHandler64.exe",
            "/games/steamapps/common/Neon Harbor/unins000.exe",
            "/games/steamapps/common/Neon Harbor/EasyAntiCheat/EasyAntiCheat_Setup.exe",
        ] {
            assert_eq!(spot(&games, &running(&[helper])), None, "{helper}");
        }
    }

    #[test]
    fn a_game_added_by_hand_is_only_recognised_by_its_own_program() {
        let games = candidates(&[install("manual:1", "manual", "/home/me/Desktop", Some("/home/me/Desktop/port.exe"))]);
        assert_eq!(spot(&games, &running(&["/home/me/Desktop/notes.exe"])), None, "its folder is not its own");
        assert_eq!(spot(&games, &running(&["/home/me/Desktop/port.exe"])).map(|c| c.game_id.as_str()), Some("manual:1"));
    }

    #[test]
    fn an_emulator_says_nothing_about_which_game_is_played() {
        let games = candidates(&[
            install("emu:a", "emu", "/roms", Some("/emulators/dolphin.exe")),
            install("emu:b", "emu", "/roms", Some("/emulators/dolphin.exe")),
        ]);
        assert_eq!(spot(&games, &running(&["/emulators/dolphin.exe"])), None);
    }

    #[test]
    fn the_most_exact_match_wins() {
        let games = candidates(&[
            install("steam:1", "steam", "/games/steamapps/common/Collection", None),
            install("manual:1", "manual", "/games/steamapps/common/Collection/Port", Some("/games/steamapps/common/Collection/Port/port.exe")),
        ]);
        let found = spot(&games, &running(&["/games/steamapps/common/Collection/Port/port.exe"]));
        assert_eq!(found.map(|c| c.game_id.as_str()), Some("manual:1"));
    }

    #[test]
    fn cloud_games_and_folders_that_say_nothing_are_left_out() {
        assert!(Candidate::of(&install("cloud:1", "cloud", "/data/cloud-browser", Some("/bin/edge"))).is_none());
        assert!(Candidate::of(&install("steam:1", "steam", "/", None)).is_none());
        assert!(Candidate::of(&install("manual:1", "manual", "/somewhere", None)).is_none());
    }

    #[test]
    fn a_game_counts_once_it_was_seen_twice_in_a_row() {
        let mut spotter = Spotter::default();
        assert_eq!(spotter.observe(Some("steam:1")), None);
        assert_eq!(spotter.observe(None), None, "gone again: a store was only checking its files");
        assert_eq!(spotter.observe(Some("steam:1")), None);
        assert_eq!(spotter.observe(Some("steam:2")), None, "another game starts the count afresh");
        assert_eq!(spotter.observe(Some("steam:2")), Some("steam:2".to_string()));
        assert_eq!(spotter.observe(Some("steam:2")), None, "said once");
    }
}
