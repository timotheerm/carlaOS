//! Achievements, from two places.
//!
//! - **Steam's own**, read from the files the Steam client keeps on disk for
//!   games that have been run on this machine. Nothing is asked of Steam's
//!   servers and no account key is needed.
//! - **CarlaOS milestones**, earned by playing: they exist for every game,
//!   whatever store it came from or none, and are worked out from the play
//!   sessions CarlaOS records.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::bvdf::{self, Node};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Achievement {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The game keeps it secret until it is earned.
    pub hidden: bool,
    /// Unix seconds when it was earned; `None` while locked. `Some(0)` when
    /// it is earned but the time is not known.
    pub unlocked: Option<i64>,
}

/// A display string is either plain or one entry per language.
fn words(node: Option<&Node>) -> String {
    match node {
        Some(Node::Text(text)) => text.clone(),
        Some(map) => map
            .get("english")
            .and_then(Node::text)
            .or_else(|| map.children().find_map(|(key, v)| if key == "token" { None } else { v.text() }))
            .unwrap_or_default()
            .to_string(),
        None => String::new(),
    }
}

/// Combine a game's achievement list with what one player has earned.
pub fn from_steam_files(schema: &[u8], progress: Option<&[u8]>) -> Option<Vec<Achievement>> {
    let schema = bvdf::parse(schema)?;
    // The file has one top-level entry, named after the app id.
    let stats = schema.children().find_map(|(_, game)| game.get("stats"))?;
    let progress = progress.and_then(bvdf::parse);
    let earned = progress.as_ref().and_then(|p| p.get("cache").or_else(|| p.children().find_map(|(_, v)| v.get("cache"))));

    let mut out = Vec::new();
    for (stat_id, stat) in stats.children() {
        let Some(bits) = stat.get("bits") else { continue };
        let mine = earned.and_then(|e| e.get(stat_id));
        let mask = mine.and_then(|m| m.get("data")).and_then(Node::number).unwrap_or(0);
        let times = mine.and_then(|m| m.get("AchievementTimes"));
        let mut entries: Vec<(u32, &Node)> = bits.children().filter_map(|(bit, a)| Some((bit.parse().ok()?, a))).collect();
        entries.sort_by_key(|(bit, _)| *bit);
        for (bit, achievement) in entries {
            let display = achievement.get("display");
            let name = words(display.and_then(|d| d.get("name")));
            let id = achievement.get("name").and_then(Node::text).map(str::to_string).unwrap_or_else(|| format!("{stat_id}_{bit}"));
            let done = bit < 63 && mask & (1 << bit) != 0;
            let when = times.and_then(|t| t.get(&bit.to_string())).and_then(Node::number).unwrap_or(0);
            out.push(Achievement {
                name: if name.is_empty() { id.clone() } else { name },
                id,
                description: words(display.and_then(|d| d.get("desc"))),
                hidden: display.and_then(|d| d.get("hidden")).and_then(Node::number).unwrap_or(0) != 0,
                unlocked: done.then_some(when),
            });
        }
    }
    (!out.is_empty()).then_some(out)
}

/// A Steam game's achievements on this machine, if Steam has them on disk.
pub fn steam(steam_root: &Path, app_id: &str) -> Option<Vec<Achievement>> {
    if app_id.is_empty() || !app_id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let dir = steam_root.join("appcache").join("stats");
    let schema = std::fs::read(dir.join(format!("UserGameStatsSchema_{app_id}.bin"))).ok()?;
    // One progress file per Steam account that played here; take the one
    // touched last, which is whoever plays on this machine now.
    let suffix = format!("_{app_id}.bin");
    let progress = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("UserGameStats_") && !name.starts_with("UserGameStatsSchema_") && name.ends_with(&suffix)
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .and_then(|e| std::fs::read(e.path()).ok());
    from_steam_files(&schema, progress.as_deref())
}

const HOUR: i64 = 3600;

/// Milestones by total time played: (id, name, description, seconds).
const BY_TIME: &[(&str, &str, &str, i64)] = &[
    ("time:1", "Settling in", "Play for one hour.", HOUR),
    ("time:5", "Hooked", "Play for five hours.", 5 * HOUR),
    ("time:10", "Regular", "Play for ten hours.", 10 * HOUR),
    ("time:25", "Devoted", "Play for twenty-five hours.", 25 * HOUR),
    ("time:50", "True fan", "Play for fifty hours.", 50 * HOUR),
    ("time:100", "Part of the family", "Play for a hundred hours.", 100 * HOUR),
];

/// Milestones by number of different days played: (id, name, description, days).
const BY_DAYS: &[(&str, &str, &str, usize)] = &[
    ("days:3", "Back again", "Play on three different days.", 3),
    ("days:7", "A week of it", "Play on seven different days.", 7),
    ("days:30", "Old friends", "Play on thirty different days.", 30),
];

const MARATHON: i64 = 3 * HOUR;

/// Work out the milestones from finished play sessions, given as
/// (start, end) in Unix seconds. Each is earned at the moment it was reached.
pub fn milestones(sessions: &[(i64, i64)]) -> Vec<Achievement> {
    let mut sessions: Vec<(i64, i64)> = sessions.iter().copied().filter(|(start, end)| end > start).collect();
    sessions.sort();
    let mut out = Vec::new();
    let mut add = |id: &str, name: &str, description: &str, unlocked: Option<i64>| {
        out.push(Achievement { id: id.to_string(), name: name.to_string(), description: description.to_string(), hidden: false, unlocked });
    };

    add("first", "First steps", "Play for the first time.", sessions.first().map(|(_, end)| *end));

    for (id, name, description, needed) in BY_TIME {
        let mut total = 0;
        let when = sessions.iter().find_map(|(start, end)| {
            let before = total;
            total += end - start;
            // The second inside this session at which the total was reached.
            (total >= *needed).then(|| start + (needed - before))
        });
        add(id, name, description, when);
    }

    let mut days: Vec<i64> = Vec::new();
    let mut day_reached: Vec<i64> = Vec::new(); // when the n-th new day began
    for (start, _) in &sessions {
        let day = start.div_euclid(86_400);
        if !days.contains(&day) {
            days.push(day);
            day_reached.push(*start);
        }
    }
    for (id, name, description, needed) in BY_DAYS {
        add(id, name, description, day_reached.get(needed - 1).copied());
    }

    let marathon = sessions.iter().find(|(start, end)| end - start >= MARATHON).map(|(start, _)| start + MARATHON);
    add("marathon", "Marathon", "Play for three hours in one sitting.", marathon);
    out
}

#[cfg(test)]
mod tests {
    use super::bvdf::write::{int, map, text};
    use super::*;

    fn schema() -> Vec<u8> {
        let achievement = |bit: &str, id: &str, name: Vec<u8>, hidden: i32| {
            map(bit, &[text("name", id), map("display", &[name, text("desc", "Do the thing."), int("hidden", hidden)])])
        };
        map(
            "480",
            &[
                text("gamename", "Example"),
                map(
                    "stats",
                    &[
                        map("1", &[int("type", 1), text("name", "kills")]),
                        map(
                            "2",
                            &[
                                int("type", 4),
                                map(
                                    "bits",
                                    &[
                                        achievement("0", "ACH_WIN", map("name", &[text("english", "Winner"), text("french", "Gagnant"), text("token", "T")]), 0),
                                        achievement("1", "ACH_SECRET", text("name", "Secret"), 1),
                                        achievement("10", "ACH_LATE", map("name", &[text("token", "T"), text("german", "Spät")]), 0),
                                    ],
                                ),
                            ],
                        ),
                    ],
                ),
            ],
        )
    }

    fn progress(mask: i32) -> Vec<u8> {
        let mut file = map("cache", &[int("crc", 1), map("2", &[int("data", mask), map("AchievementTimes", &[int("0", 1_700_000_000)])])]);
        // The real file is a bare list of entries, not wrapped in an end byte.
        file.extend(int("PendingChanges", 0));
        file
    }

    #[test]
    fn steam_files_give_names_and_what_was_earned() {
        let list = from_steam_files(&schema(), Some(&progress(0b100_0000_0001))).unwrap();
        assert_eq!(list.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["ACH_WIN", "ACH_SECRET", "ACH_LATE"]);
        assert_eq!((list[0].name.as_str(), list[0].unlocked), ("Winner", Some(1_700_000_000)));
        assert_eq!((list[1].name.as_str(), list[1].hidden, list[1].unlocked), ("Secret", true, None));
        assert_eq!((list[2].name.as_str(), list[2].unlocked), ("Spät", Some(0)), "earned, time unknown; another language when there is no English");
        assert_eq!(list[0].description, "Do the thing.");
    }

    #[test]
    fn a_game_never_played_here_has_everything_locked() {
        let list = from_steam_files(&schema(), None).unwrap();
        assert!(list.iter().all(|a| a.unlocked.is_none()));
        assert_eq!(list.len(), 3);
        assert!(from_steam_files(b"rubbish", None).is_none());
        assert!(from_steam_files(&map("480", &[map("stats", &[])]), None).is_none(), "a game without achievements");
    }

    #[test]
    fn steam_files_are_found_on_disk_for_the_latest_player() {
        let root = std::env::temp_dir().join(format!("carla-ach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("appcache").join("stats");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(steam(&root, "480").is_none());
        std::fs::write(dir.join("UserGameStatsSchema_480.bin"), schema()).unwrap();
        assert_eq!(steam(&root, "480").unwrap().iter().filter(|a| a.unlocked.is_some()).count(), 0);
        std::fs::write(dir.join("UserGameStats_111_480.bin"), progress(0b11)).unwrap();
        std::fs::write(dir.join("UserGameStats_111_4800.bin"), progress(0)).unwrap();
        assert_eq!(steam(&root, "480").unwrap().iter().filter(|a| a.unlocked.is_some()).count(), 2);
        assert!(steam(&root, "../480").is_none());
        assert!(steam(&root, "").is_none());
    }

    #[test]
    fn milestones_are_earned_at_the_moment_they_were_reached() {
        let none = milestones(&[]);
        assert!(none.iter().all(|a| a.unlocked.is_none()));
        assert_eq!(none.len(), 1 + BY_TIME.len() + BY_DAYS.len() + 1);

        let day = 86_400;
        // Half an hour, then on another day forty minutes, then a long sitting.
        let list = milestones(&[(1000, 1000 + 1800), (day + 500, day + 500 + 2400), (3 * day, 3 * day + 4 * HOUR), (9, 9)]);
        let when = |id: &str| list.iter().find(|a| a.id == id).unwrap().unlocked;
        assert_eq!(when("first"), Some(2800));
        assert_eq!(when("time:1"), Some(day + 500 + 1800), "thirty minutes into the second session");
        assert_eq!(when("time:5"), Some(3 * day + (5 * HOUR - 4200)));
        assert_eq!(when("time:10"), None);
        assert_eq!(when("days:3"), Some(3 * day));
        assert_eq!(when("days:7"), None);
        assert_eq!(when("marathon"), Some(3 * day + 3 * HOUR));
    }
}
