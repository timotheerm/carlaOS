//! The catalogue database.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use super::{GameView, Install, Kind, ManualGame, Scanned, LOCAL};

type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("The game library could not be read or saved: {e}")
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS games (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind        TEXT NOT NULL DEFAULT 'game',
    platform    TEXT NOT NULL,
    favourite   INTEGER NOT NULL DEFAULT 0,
    added       INTEGER NOT NULL,
    cover       TEXT,
    hero        TEXT
);
CREATE TABLE IF NOT EXISTS installs (
    id           INTEGER PRIMARY KEY,
    game_id      TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    place        TEXT NOT NULL,
    source       TEXT NOT NULL,
    source_id    TEXT NOT NULL,
    install_path TEXT NOT NULL,
    exe          TEXT,
    args         TEXT NOT NULL DEFAULT '',
    UNIQUE (place, source, source_id)
);
CREATE TABLE IF NOT EXISTS play_sessions (
    id      INTEGER PRIMARY KEY,
    game_id TEXT NOT NULL,
    place   TEXT NOT NULL,
    started INTEGER NOT NULL,
    ended   INTEGER
);
CREATE INDEX IF NOT EXISTS play_sessions_game ON play_sessions (game_id);
";

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn platform(source: &str) -> &'static str {
    match source {
        "steam" => "Steam",
        _ => "Standalone",
    }
}

pub struct Store {
    db: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        Self::prepare(Connection::open(path).map_err(err)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory().map_err(err)?)
    }

    fn prepare(db: Connection) -> Result<Self> {
        db.execute_batch("PRAGMA foreign_keys = ON;").map_err(err)?;
        db.execute_batch(SCHEMA).map_err(err)?;
        Ok(Self { db })
    }

    /// Make the installs of one store on one machine match what a scan found.
    /// Favourites and playtime of games that are still there are kept; games
    /// that are gone keep their history in case they come back.
    pub fn sync(&mut self, place: &str, source: &str, found: &[Scanned], now: i64) -> Result<()> {
        let tx = self.db.transaction().map_err(err)?;
        tx.execute("DELETE FROM installs WHERE place = ?1 AND source = ?2", params![place, source]).map_err(err)?;
        for s in found {
            let id = format!("{source}:{}", s.source_id);
            tx.execute(
                "INSERT INTO games (id, title, platform, added, cover, hero) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (id) DO UPDATE SET title = ?2, cover = COALESCE(?5, cover), hero = COALESCE(?6, hero)",
                params![id, s.title, platform(source), now, s.cover.as_deref().map(text), s.hero.as_deref().map(text)],
            )
            .map_err(err)?;
            tx.execute(
                "INSERT INTO installs (game_id, place, source, source_id, install_path) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, place, source, s.source_id, text(&s.install_path)],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    /// Add a game by hand. Adding the same program again updates it.
    pub fn add_manual(&mut self, game: &ManualGame, now: i64) -> Result<String> {
        let title = game.title.trim();
        if title.is_empty() {
            return Err("The game needs a name.".to_string());
        }
        let exe = text(&game.exe);
        // The id is stable for a given program, so re-adding does not duplicate.
        let source_id = exe.to_lowercase();
        let id = format!("manual:{:016x}", fnv(&source_id));
        let dir = game.exe.parent().map(text).unwrap_or_default();
        let tx = self.db.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO games (id, title, kind, platform, added, cover) VALUES (?1, ?2, ?3, 'Standalone', ?4, ?5)
             ON CONFLICT (id) DO UPDATE SET title = ?2, kind = ?3, cover = COALESCE(?5, cover)",
            params![id, title, game.kind.as_str(), now, game.cover.as_deref().map(text)],
        )
        .map_err(err)?;
        tx.execute(
            "INSERT INTO installs (game_id, place, source, source_id, install_path, exe, args)
             VALUES (?1, ?2, 'manual', ?3, ?4, ?5, ?6)
             ON CONFLICT (place, source, source_id) DO UPDATE SET install_path = ?4, exe = ?5, args = ?6",
            params![id, LOCAL, source_id, dir, exe, game.args.trim()],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(id)
    }

    /// Remove a game that was added by hand. Scanned games cannot be removed
    /// this way; they would only come back at the next scan.
    pub fn remove_manual(&mut self, id: &str) -> Result<()> {
        if !id.starts_with("manual:") {
            return Err("Only games added by hand can be removed here.".to_string());
        }
        self.db.execute("DELETE FROM games WHERE id = ?1", params![id]).map_err(err)?;
        self.db.execute("DELETE FROM play_sessions WHERE game_id = ?1", params![id]).map_err(err)?;
        Ok(())
    }

    pub fn set_favourite(&mut self, id: &str, favourite: bool) -> Result<()> {
        self.db.execute("UPDATE games SET favourite = ?2 WHERE id = ?1", params![id, favourite]).map_err(err)?;
        Ok(())
    }

    /// Every game that is installed somewhere, by title.
    pub fn list(&self) -> Result<Vec<GameView>> {
        let mut games = self
            .db
            .prepare(
                "SELECT g.id, g.title, g.description, g.platform, g.kind, g.favourite, g.added, g.cover, g.hero,
                        (SELECT COALESCE(SUM(ended - started), 0) FROM play_sessions p WHERE p.game_id = g.id AND p.ended IS NOT NULL),
                        (SELECT MAX(started) FROM play_sessions p WHERE p.game_id = g.id)
                 FROM games g
                 WHERE EXISTS (SELECT 1 FROM installs i WHERE i.game_id = g.id)
                 ORDER BY g.title COLLATE NOCASE",
            )
            .map_err(err)?;
        let mut places = self
            .db
            .prepare("SELECT DISTINCT place FROM installs WHERE game_id = ?1 ORDER BY place <> 'local', place")
            .map_err(err)?;
        let rows = games
            .query_map([], |r| {
                Ok(GameView {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    description: r.get(2)?,
                    platform: r.get(3)?,
                    kind: Kind::parse(&r.get::<_, String>(4)?),
                    favourite: r.get(5)?,
                    added: r.get(6)?,
                    cover: r.get(7)?,
                    hero: r.get(8)?,
                    playtime_minutes: (r.get::<_, i64>(9)?.max(0) / 60) as u64,
                    last_played: r.get(10)?,
                    places: Vec::new(),
                    manual: false,
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        rows.into_iter()
            .map(|mut g| {
                g.manual = g.id.starts_with("manual:");
                g.places = places
                    .query_map(params![g.id], |r| r.get(0))
                    .map_err(err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(err)?;
                Ok(g)
            })
            .collect()
    }

    /// How to start a game in one place.
    pub fn install(&self, game_id: &str, place: &str) -> Result<Install> {
        self.db
            .query_row(
                "SELECT g.title, i.source, i.source_id, i.install_path, i.exe, i.args
                 FROM installs i JOIN games g ON g.id = i.game_id
                 WHERE i.game_id = ?1 AND i.place = ?2",
                params![game_id, place],
                |r| {
                    Ok(Install {
                        game_id: game_id.to_string(),
                        title: r.get(0)?,
                        source: r.get(1)?,
                        source_id: r.get(2)?,
                        install_path: PathBuf::from(r.get::<_, String>(3)?),
                        exe: r.get::<_, Option<String>>(4)?.map(PathBuf::from),
                        args: r.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(err)?
            .ok_or_else(|| "That game is not installed there any more.".to_string())
    }

    pub fn start_session(&mut self, game_id: &str, place: &str, now: i64) -> Result<i64> {
        self.db
            .execute("INSERT INTO play_sessions (game_id, place, started) VALUES (?1, ?2, ?3)", params![game_id, place, now])
            .map_err(err)?;
        Ok(self.db.last_insert_rowid())
    }

    pub fn end_session(&mut self, session: i64, now: i64) -> Result<()> {
        self.db
            .execute("UPDATE play_sessions SET ended = MAX(?2, started) WHERE id = ?1 AND ended IS NULL", params![session, now])
            .map_err(err)?;
        Ok(())
    }

    /// Sessions left open by a crash or a power cut count as no time played,
    /// which is the only length we can be sure they were not shorter than.
    pub fn close_dangling_sessions(&mut self) -> Result<()> {
        self.db.execute("UPDATE play_sessions SET ended = started WHERE ended IS NULL", []).map_err(err)?;
        Ok(())
    }
}

/// FNV-1a, for short stable ids. Not for anything secret.
fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanned(id: &str, title: &str) -> Scanned {
        Scanned { source_id: id.into(), title: title.into(), install_path: PathBuf::from("/games").join(title), cover: None, hero: None }
    }

    #[test]
    fn a_rescan_keeps_favourites_and_playtime_and_drops_what_is_gone() {
        let mut store = Store::in_memory().unwrap();
        store.sync(LOCAL, "steam", &[scanned("1", "Alpha"), scanned("2", "Beta")], 100).unwrap();
        store.set_favourite("steam:1", true).unwrap();
        let session = store.start_session("steam:1", LOCAL, 1_000).unwrap();
        store.end_session(session, 1_000 + 90 * 60).unwrap();

        store.sync(LOCAL, "steam", &[scanned("1", "Alpha (Renamed)")], 200).unwrap();
        let games = store.list().unwrap();
        assert_eq!(games.len(), 1);
        let alpha = &games[0];
        assert_eq!(alpha.title, "Alpha (Renamed)");
        assert!(alpha.favourite);
        assert_eq!(alpha.playtime_minutes, 90);
        assert_eq!(alpha.last_played, Some(1_000));
        assert_eq!(alpha.added, 100);
        assert_eq!(alpha.places, [LOCAL]);
        assert!(store.install("steam:2", LOCAL).is_err());
    }

    #[test]
    fn manual_games_can_be_added_updated_and_removed() {
        let mut store = Store::in_memory().unwrap();
        let game = ManualGame {
            title: " Cloudberry ".into(),
            exe: PathBuf::from("/games/Cloudberry/berry.exe"),
            args: "-fullscreen".into(),
            kind: Kind::Homebrew,
            cover: None,
        };
        let id = store.add_manual(&game, 10).unwrap();
        let again = store.add_manual(&ManualGame { title: "Cloudberry 2".into(), ..game.clone() }, 20).unwrap();
        assert_eq!(id, again);

        let games = store.list().unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!((games[0].title.as_str(), games[0].kind, games[0].manual), ("Cloudberry 2", Kind::Homebrew, true));
        let install = store.install(&id, LOCAL).unwrap();
        assert_eq!(install.exe.as_deref(), Some(Path::new("/games/Cloudberry/berry.exe")));
        assert_eq!(install.install_path, PathBuf::from("/games/Cloudberry"));
        assert_eq!(install.args, "-fullscreen");

        assert!(store.remove_manual("steam:1").is_err());
        store.remove_manual(&id).unwrap();
        assert!(store.list().unwrap().is_empty());
        assert!(store.add_manual(&ManualGame { title: "  ".into(), ..game }, 30).is_err());
    }

    #[test]
    fn sessions_cut_short_count_as_no_time() {
        let mut store = Store::in_memory().unwrap();
        store.sync(LOCAL, "steam", &[scanned("1", "Alpha")], 0).unwrap();
        store.start_session("steam:1", LOCAL, 500).unwrap();
        store.close_dangling_sessions().unwrap();
        assert_eq!(store.list().unwrap()[0].playtime_minutes, 0);
    }
}
