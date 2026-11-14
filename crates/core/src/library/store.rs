//! The catalogue database.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use super::emulation::Console;
use super::{GameView, HostRow, Install, Kind, ManualGame, Scanned, Streamed, Synced, Update, Waiting, LOCAL};
use crate::link::quality::Setting;

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
CREATE TABLE IF NOT EXISTS achievement_cache (
    game_id TEXT PRIMARY KEY,
    json    TEXT NOT NULL,
    at      INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS consoles (
    id       TEXT PRIMARY KEY,
    name     TEXT NOT NULL,
    emulator TEXT NOT NULL,
    options  TEXT NOT NULL,
    folder   TEXT NOT NULL,
    endings  TEXT NOT NULL,
    added    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS save_sync (
    game_id      TEXT NOT NULL,
    host_id      TEXT NOT NULL,
    digest       TEXT,
    at           INTEGER NOT NULL,
    outcome      TEXT NOT NULL,
    both_changed INTEGER NOT NULL DEFAULT 0,
    files        INTEGER NOT NULL DEFAULT 0,
    problem      TEXT,
    PRIMARY KEY (game_id, host_id)
);
CREATE TABLE IF NOT EXISTS settings (
    name  TEXT PRIMARY KEY,
    value BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS hosts (
    id        TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    address   TEXT NOT NULL,
    port      INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    key       BLOB NOT NULL,
    hardware  TEXT NOT NULL DEFAULT '',
    last_seen INTEGER,
    added     INTEGER NOT NULL
);
";

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn platform(source: &str) -> &'static str {
    match source {
        "steam" => "Steam",
        "epic" => "Epic",
        "gog" => "GOG",
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
        // Columns added after the first release. SQLite has no "add column if
        // missing", so look first.
        let added = [("installs", "pending_update", "TEXT"), ("hosts", "addresses", "TEXT NOT NULL DEFAULT '[]'"), ("hosts", "macs", "TEXT NOT NULL DEFAULT '[]'"), ("games", "save_path", "TEXT"), ("hosts", "admin", "INTEGER NOT NULL DEFAULT 0"), ("hosts", "quality", "TEXT NOT NULL DEFAULT 'auto'"), ("hosts", "streamed", "TEXT"),
            // Pictures the administrator chose. They win over what a store or a
            // desktop offers, and a look through the library leaves them alone.
            ("games", "cover_own", "TEXT"),
            ("games", "hero_own", "TEXT"),
            ("games", "origin", "TEXT NOT NULL DEFAULT ''"),
        ];
        for (table, column, kind) in added {
            let has: bool = db
                .query_row("SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2", params![table, column], |r| r.get::<_, i64>(0))
                .map(|n| n > 0)
                .map_err(err)?;
            if !has {
                db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind};")).map_err(err)?;
            }
        }
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
                "INSERT INTO games (id, title, platform, added, cover, hero, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (id) DO UPDATE SET title = ?2, platform = ?3, cover = COALESCE(?5, cover), hero = COALESCE(?6, hero)",
                params![
                    id,
                    s.title,
                    if s.platform.is_empty() { platform(source) } else { &s.platform },
                    now,
                    s.cover.as_deref().map(text),
                    s.hero.as_deref().map(text),
                    if s.emulated { Kind::Emulated } else { Kind::Game }.as_str(),
                ],
            )
            .map_err(err)?;
            tx.execute(
                "INSERT INTO installs (game_id, place, source, source_id, install_path, exe, args, pending_update) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, place, source, s.source_id, text(&s.install_path), s.exe.as_deref().map(text), s.args, s.update.and_then(|u| serde_json::to_string(&u).ok())],
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
        let id = format!("manual:{}", super::manual_key(&exe));
        let dir = game.exe.parent().map(text).unwrap_or_default();
        let tx = self.db.transaction().map_err(err)?;
        tx.execute(
            "INSERT INTO games (id, title, kind, platform, added, cover, description, origin) VALUES (?1, ?2, ?3, 'Standalone', ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET title = ?2, kind = ?3, cover = COALESCE(?5, cover),
                description = CASE WHEN ?6 = '' THEN description ELSE ?6 END, origin = ?7",
            params![id, title, game.kind.as_str(), now, game.cover.as_deref().map(text), game.description.trim(), game.origin.trim()],
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
                "SELECT g.id, g.title, g.description, g.platform, g.kind, g.favourite, g.added,
                        COALESCE(g.cover_own, g.cover), COALESCE(g.hero_own, g.hero),
                        (SELECT COALESCE(SUM(ended - started), 0) FROM play_sessions p WHERE p.game_id = g.id AND p.ended IS NOT NULL),
                        (SELECT MAX(started) FROM play_sessions p WHERE p.game_id = g.id),
                        g.origin, g.cover_own IS NOT NULL OR g.hero_own IS NOT NULL
                 FROM games g
                 WHERE EXISTS (SELECT 1 FROM installs i WHERE i.game_id = g.id)
                 ORDER BY g.title COLLATE NOCASE",
            )
            .map_err(err)?;
        let mut places = self
            .db
            .prepare("SELECT DISTINCT place FROM installs WHERE game_id = ?1 ORDER BY place <> 'local', place")
            .map_err(err)?;
        let mut waiting = self
            .db
            .prepare("SELECT place, pending_update FROM installs WHERE game_id = ?1 AND pending_update IS NOT NULL ORDER BY place <> 'local', place")
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
                    updates: Vec::new(),
                    manual: false,
                    origin: r.get(11)?,
                    own_art: r.get(12)?,
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        rows.into_iter()
            .map(|mut g| {
                g.places = places
                    .query_map(params![g.id], |r| r.get(0))
                    .map_err(err)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(err)?;
                let pending: Vec<(String, String)> =
                    waiting.query_map(params![g.id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<rusqlite::Result<_>>().map_err(err)?;
                g.updates = pending
                    .into_iter()
                    .filter_map(|(place, json)| serde_json::from_str::<Update>(&json).ok().map(|update| Waiting { place, update }))
                    .collect();
                // Only what was added by hand on this machine can be removed here.
                g.manual = g.id.starts_with("manual:") && g.places.iter().any(|p| p == LOCAL);
                Ok(g)
            })
            .collect()
    }

    /// Note what a store said just now about one install's update.
    pub fn set_update(&mut self, game_id: &str, place: &str, update: Option<Update>) -> Result<()> {
        let json = update.and_then(|u| serde_json::to_string(&u).ok());
        self.db.execute("UPDATE installs SET pending_update = ?3 WHERE game_id = ?1 AND place = ?2", params![game_id, place, json]).map_err(err)?;
        Ok(())
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

    /// The pictures the administrator chose for a game: (cover, wide picture).
    pub fn own_art(&self, game_id: &str) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
        let row: Option<(Option<String>, Option<String>)> =
            self.db.query_row("SELECT cover_own, hero_own FROM games WHERE id = ?1", params![game_id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)?;
        let (cover, hero) = row.ok_or("That game is not in the library any more.")?;
        Ok((cover.map(PathBuf::from), hero.map(PathBuf::from)))
    }

    /// Choose a game's cover (`wide` false) or its wide picture. `None` gives
    /// back the one it had by itself. Returns the file that was replaced,
    /// which nothing shows any more.
    pub fn set_own_art(&mut self, game_id: &str, wide: bool, picture: Option<&Path>) -> Result<Option<PathBuf>> {
        let (cover, hero) = self.own_art(game_id)?;
        let column = if wide { "hero_own" } else { "cover_own" };
        self.db.execute(&format!("UPDATE games SET {column} = ?2 WHERE id = ?1"), params![game_id, picture.map(text)]).map_err(err)?;
        Ok(if wide { hero } else { cover }.filter(|old| Some(old.as_path()) != picture))
    }

    /// Something CarlaOS remembers that belongs to no game or desktop.
    pub fn setting(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.db.query_row("SELECT value FROM settings WHERE name = ?1", params![name], |r| r.get(0)).optional().map_err(err)
    }

    pub fn set_setting(&mut self, name: &str, value: Option<&[u8]>) -> Result<()> {
        match value {
            Some(value) => self.db.execute("INSERT INTO settings (name, value) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET value = ?2", params![name, value]),
            None => self.db.execute("DELETE FROM settings WHERE name = ?1", params![name]),
        }
        .map_err(err)?;
        Ok(())
    }

    /// Games that were added as a shortcut before CarlaOS could follow one:
    /// each is pointed at the program behind its shortcut, so that it starts.
    /// Says how many were mended.
    pub fn mend_shortcuts(&mut self) -> Result<usize> {
        let rows: Vec<(i64, String, String)> = {
            let mut query = self.db.prepare("SELECT id, exe, args FROM installs WHERE source = 'manual' AND place = ?1 AND lower(exe) LIKE '%.lnk'").map_err(err)?;
            let rows = query.query_map(params![LOCAL], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?.collect::<rusqlite::Result<_>>().map_err(err)?;
            rows
        };
        let mut mended = 0;
        for (row, exe, args) in rows {
            let Ok(seen) = super::ports::inspect(Path::new(&exe)) else { continue };
            let dir = seen.exe.parent().map(text).unwrap_or_default();
            // Options the administrator typed win over the shortcut's own.
            let args = if args.trim().is_empty() { seen.args } else { args };
            self.db.execute("UPDATE installs SET exe = ?2, install_path = ?3, args = ?4 WHERE id = ?1", params![row, text(&seen.exe), dir, args]).map_err(err)?;
            mended += 1;
        }
        Ok(mended)
    }

    /// Consoles, in the order they were added.
    pub fn consoles(&self) -> Result<Vec<Console>> {
        let mut query = self.db.prepare("SELECT id, name, emulator, options, folder, endings FROM consoles ORDER BY added, name").map_err(err)?;
        let rows = query
            .query_map([], |r| {
                Ok(Console {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    emulator: PathBuf::from(r.get::<_, String>(2)?),
                    options: r.get(3)?,
                    folder: PathBuf::from(r.get::<_, String>(4)?),
                    endings: list(&r.get::<_, String>(5)?),
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        Ok(rows)
    }

    /// Add a console, or change one. Returns it with its id.
    pub fn save_console(&mut self, console: &Console, now: i64) -> Result<Console> {
        let mut console = console.clone();
        if console.id.is_empty() {
            console.id = format!("c{:x}", now.max(0)) + &self.consoles()?.len().to_string();
        }
        let endings = serde_json::to_string(&console.endings).map_err(err)?;
        self.db
            .execute(
                "INSERT INTO consoles (id, name, emulator, options, folder, endings, added) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (id) DO UPDATE SET name = ?2, emulator = ?3, options = ?4, folder = ?5, endings = ?6",
                params![console.id, console.name, text(&console.emulator), console.options, text(&console.folder), endings, now],
            )
            .map_err(err)?;
        Ok(console)
    }

    /// Forget a console. Its game files are not touched; their cards go at
    /// the next look through the library.
    pub fn remove_console(&mut self, id: &str) -> Result<()> {
        self.db.execute("DELETE FROM consoles WHERE id = ?1", params![id]).map_err(err)?;
        Ok(())
    }

    /// Remember a linked desktop. Linking the same desktop again replaces it.
    /// `key` is stored as given, so seal it first.
    pub fn save_host(&mut self, host: &HostRow, now: i64) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO hosts (id, name, address, port, device_id, key, hardware, last_seen, added)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (id) DO UPDATE SET name = ?2, address = ?3, port = ?4, device_id = ?5, key = ?6, admin = 0",
                params![host.id, host.name, host.address, host.port, host.device_id, host.key, host.hardware, host.last_seen, now],
            )
            .map_err(err)?;
        Ok(())
    }

    // Linking again is a new start: the desktop may administer the Ally only
    // once that has been allowed again.

    /// Linked desktops, in the order they were added.
    pub fn hosts(&self) -> Result<Vec<HostRow>> {
        let mut query = self
            .db
            .prepare("SELECT id, name, address, port, device_id, key, hardware, last_seen, addresses, macs FROM hosts ORDER BY added, name")
            .map_err(err)?;
        let rows = query
            .query_map([], |r| {
                Ok(HostRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    address: r.get(2)?,
                    port: r.get(3)?,
                    device_id: r.get(4)?,
                    key: r.get(5)?,
                    hardware: r.get(6)?,
                    last_seen: r.get(7)?,
                    addresses: list(&r.get::<_, String>(8)?),
                    macs: list(&r.get::<_, String>(9)?),
                })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        Ok(rows)
    }

    /// Whether this desktop may administer the Ally.
    pub fn host_admin(&self, id: &str) -> Result<bool> {
        let allowed: Option<bool> = self.db.query_row("SELECT admin FROM hosts WHERE id = ?1", params![id], |r| r.get(0)).optional().map_err(err)?;
        Ok(allowed.unwrap_or(false))
    }

    pub fn set_host_admin(&mut self, id: &str, allowed: bool) -> Result<()> {
        self.db.execute("UPDATE hosts SET admin = ?2 WHERE id = ?1", params![id, allowed]).map_err(err)?;
        Ok(())
    }

    /// The picture the administrator chose for streams from this desktop,
    /// and what the last stream from it got.
    pub fn host_quality(&self, id: &str) -> Result<(Setting, Option<Streamed>)> {
        let row: Option<(String, Option<String>)> =
            self.db.query_row("SELECT quality, streamed FROM hosts WHERE id = ?1", params![id], |r| Ok((r.get(0)?, r.get(1)?))).optional().map_err(err)?;
        let (setting, streamed) = row.unwrap_or_default();
        Ok((Setting::parse(&setting).unwrap_or_default(), streamed.and_then(|s| serde_json::from_str(&s).ok())))
    }

    pub fn set_host_quality(&mut self, id: &str, setting: Setting) -> Result<()> {
        self.db.execute("UPDATE hosts SET quality = ?2 WHERE id = ?1", params![id, setting.name()]).map_err(err)?;
        Ok(())
    }

    pub fn set_host_streamed(&mut self, id: &str, streamed: &Streamed) -> Result<()> {
        let json = serde_json::to_string(streamed).expect("serialisable");
        self.db.execute("UPDATE hosts SET streamed = ?2 WHERE id = ?1", params![id, json]).map_err(err)?;
        Ok(())
    }

    /// Note that a desktop answered, at which address, and what it said about
    /// itself. `host` carries the new values; its key is left alone.
    pub fn host_seen(&mut self, host: &HostRow, now: i64) -> Result<()> {
        let json = |v: &[String]| serde_json::to_string(v).expect("serialisable");
        self.db
            .execute(
                "UPDATE hosts SET name = ?2, hardware = ?3, last_seen = ?4, address = ?5, addresses = ?6, macs = ?7 WHERE id = ?1",
                params![host.id, host.name, host.hardware, now, host.address, json(&host.addresses), json(&host.macs)],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Forget a desktop and the games that were only there.
    pub fn remove_host(&mut self, id: &str) -> Result<()> {
        if id == LOCAL {
            return Ok(());
        }
        let tx = self.db.transaction().map_err(err)?;
        tx.execute("DELETE FROM installs WHERE place = ?1", params![id]).map_err(err)?;
        tx.execute("DELETE FROM save_sync WHERE host_id = ?1", params![id]).map_err(err)?;
        tx.execute("DELETE FROM hosts WHERE id = ?1", params![id]).map_err(err)?;
        tx.commit().map_err(err)
    }

    /// Finished play sessions of one game, as (start, end) in Unix seconds.
    pub fn sessions(&self, game_id: &str) -> Result<Vec<(i64, i64)>> {
        let mut query = self
            .db
            .prepare("SELECT started, ended FROM play_sessions WHERE game_id = ?1 AND ended IS NOT NULL ORDER BY started")
            .map_err(err)?;
        let rows = query.query_map(params![game_id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?.collect::<rusqlite::Result<Vec<_>>>().map_err(err)?;
        Ok(rows)
    }

    /// Where the administrator said a game keeps its saves, if anywhere.
    pub fn save_path(&self, game_id: &str) -> Result<Option<PathBuf>> {
        let path: Option<Option<String>> =
            self.db.query_row("SELECT save_path FROM games WHERE id = ?1", params![game_id], |r| r.get(0)).optional().map_err(err)?;
        Ok(path.flatten().filter(|p| !p.is_empty()).map(PathBuf::from))
    }

    pub fn set_save_path(&mut self, game_id: &str, path: Option<&Path>) -> Result<()> {
        self.db.execute("UPDATE games SET save_path = ?2 WHERE id = ?1", params![game_id, path.map(text)]).map_err(err)?;
        Ok(())
    }

    /// What happened the last time a game's saves were brought in step with
    /// each desktop.
    pub fn synced(&self, game_id: &str) -> Result<Vec<Synced>> {
        let mut query = self
            .db
            .prepare("SELECT host_id, digest, at, outcome, both_changed, files, problem FROM save_sync WHERE game_id = ?1 ORDER BY host_id")
            .map_err(err)?;
        let rows = query
            .query_map(params![game_id], |r| {
                Ok(Synced { host_id: r.get(0)?, digest: r.get(1)?, at: r.get(2)?, outcome: r.get(3)?, both_changed: r.get(4)?, files: r.get(5)?, problem: r.get(6)? })
            })
            .map_err(err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(err)?;
        Ok(rows)
    }

    /// Remember it. A row without a digest (something went wrong) keeps the
    /// digest both machines last agreed on.
    pub fn set_synced(&mut self, game_id: &str, row: &Synced) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO save_sync (game_id, host_id, digest, at, outcome, both_changed, files, problem) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (game_id, host_id) DO UPDATE SET digest = COALESCE(?3, digest), at = ?4, outcome = ?5, both_changed = ?6, files = ?7, problem = ?8",
                params![game_id, row.host_id, row.digest, row.at, row.outcome, row.both_changed, row.files, row.problem],
            )
            .map_err(err)?;
        Ok(())
    }

    /// A desktop no longer keeps this game's saves in step.
    pub fn clear_synced(&mut self, game_id: &str, host_id: &str) -> Result<()> {
        self.db.execute("DELETE FROM save_sync WHERE game_id = ?1 AND host_id = ?2", params![game_id, host_id]).map_err(err)?;
        Ok(())
    }

    /// Remember a desktop's answer about a game's achievements, so they can be
    /// shown while that desktop is asleep.
    pub fn cache_achievements(&mut self, game_id: &str, json: &str, now: i64) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO achievement_cache (game_id, json, at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (game_id) DO UPDATE SET json = ?2, at = ?3",
                params![game_id, json, now],
            )
            .map_err(err)?;
        Ok(())
    }

    /// The remembered answer and when it was given.
    pub fn cached_achievements(&self, game_id: &str) -> Result<Option<(String, i64)>> {
        self.db
            .query_row("SELECT json, at FROM achievement_cache WHERE game_id = ?1", params![game_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .map_err(err)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::quality::{self, Level};

    fn scanned(id: &str, title: &str) -> Scanned {
        Scanned { source_id: id.into(), title: title.into(), install_path: PathBuf::from("/games").join(title), ..Scanned::default() }
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
    fn an_update_that_has_to_finish_is_shown_for_the_place_it_is_in() {
        use crate::library::{Update, UpdateState};
        let mut store = Store::in_memory().unwrap();
        let update = Update { state: UpdateState::Needed, bytes: 1_000, done: 250 };
        store.sync(LOCAL, "steam", &[scanned("1", "Alpha")], 100).unwrap();
        store.sync("desk", "steam", &[Scanned { update: Some(update), ..scanned("1", "Alpha") }, scanned("2", "Beta")], 100).unwrap();
        let games = store.list().unwrap();
        assert_eq!(games[0].updates, [Waiting { place: "desk".into(), update }]);
        assert!(games[1].updates.is_empty());
        let json = serde_json::to_value(&games[0]).unwrap();
        assert_eq!(json["updates"][0], serde_json::json!({ "place": "desk", "state": "needed", "bytes": 1_000, "done": 250 }));
        // Asked again just before playing: done on the desktop, started here.
        store.set_update("steam:1", "desk", None).unwrap();
        store.set_update("steam:1", LOCAL, Some(Update { state: UpdateState::Running, ..update })).unwrap();
        let games = store.list().unwrap();
        assert_eq!(games[0].updates.iter().map(|w| (w.place.as_str(), w.update.state)).collect::<Vec<_>>(), [(LOCAL, UpdateState::Running)]);
        // The next look through the store has the last word.
        store.sync(LOCAL, "steam", &[scanned("1", "Alpha")], 200).unwrap();
        assert!(store.list().unwrap()[0].updates.is_empty());
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
            description: "A berry who wants to fly.".into(),
            origin: String::new(),
        };
        let id = store.add_manual(&game, 10).unwrap();
        // Adding it again without a description keeps the one it had.
        let again = store.add_manual(&ManualGame { title: "Cloudberry 2".into(), description: String::new(), ..game.clone() }, 20).unwrap();
        assert_eq!(id, again);
        assert_eq!(store.list().unwrap()[0].description, "A berry who wants to fly.");

        // Saves, sessions and remembered achievements hang off the game.
        assert_eq!(store.save_path(&id).unwrap(), None);
        store.set_save_path(&id, Some(Path::new("/saves/berry"))).unwrap();
        assert_eq!(store.save_path(&id).unwrap(), Some(PathBuf::from("/saves/berry")));
        store.set_save_path(&id, None).unwrap();
        assert_eq!(store.save_path(&id).unwrap(), None);
        assert_eq!(store.save_path("nothing").unwrap(), None);
        let open = store.start_session(&id, LOCAL, 100).unwrap();
        let done = store.start_session(&id, LOCAL, 200).unwrap();
        store.end_session(done, 260).unwrap();
        assert_eq!(store.sessions(&id).unwrap(), [(200, 260)], "only finished sessions count");
        store.end_session(open, 150).unwrap();
        let ok = Synced { host_id: "desk".into(), digest: Some("abc".into()), at: 10, outcome: "fetched".into(), both_changed: true, files: 3, problem: None };
        store.set_synced(&id, &ok).unwrap();
        let failed = Synced { digest: None, at: 20, outcome: "problem".into(), both_changed: false, files: 0, problem: Some("The disk is full.".into()), ..ok.clone() };
        store.set_synced(&id, &failed).unwrap();
        assert_eq!(store.synced(&id).unwrap(), [Synced { digest: Some("abc".into()), ..failed }], "a failure keeps what was agreed");
        store.clear_synced(&id, "desk").unwrap();
        assert!(store.synced(&id).unwrap().is_empty());
        assert_eq!(store.cached_achievements(&id).unwrap(), None);
        store.cache_achievements(&id, "[]", 5).unwrap();
        store.cache_achievements(&id, "[1]", 6).unwrap();
        assert_eq!(store.cached_achievements(&id).unwrap(), Some(("[1]".to_string(), 6)));

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
    fn pictures_the_administrator_chose_win_and_survive_a_look_through_the_library() {
        let mut store = Store::in_memory().unwrap();
        let from_store = Scanned { cover: Some(PathBuf::from("/steam/cover.jpg")), hero: Some(PathBuf::from("/steam/hero.jpg")), ..scanned("1", "Alpha") };
        store.sync(LOCAL, "steam", std::slice::from_ref(&from_store), 1).unwrap();
        let shown = |store: &Store| {
            let g = store.list().unwrap().remove(0);
            (g.cover, g.hero, g.own_art)
        };
        assert_eq!(shown(&store), (Some("/steam/cover.jpg".into()), Some("/steam/hero.jpg".into()), false));

        assert_eq!(store.set_own_art("steam:1", false, Some(Path::new("/art/a.png"))).unwrap(), None);
        store.sync(LOCAL, "steam", std::slice::from_ref(&from_store), 2).unwrap();
        assert_eq!(shown(&store), (Some("/art/a.png".into()), Some("/steam/hero.jpg".into()), true));
        // Choosing another says which file is no longer shown.
        assert_eq!(store.set_own_art("steam:1", false, Some(Path::new("/art/b.png"))).unwrap(), Some(PathBuf::from("/art/a.png")));
        assert_eq!(store.set_own_art("steam:1", true, Some(Path::new("/art/wide.jpg"))).unwrap(), None);
        assert_eq!(store.own_art("steam:1").unwrap(), (Some("/art/b.png".into()), Some("/art/wide.jpg".into())));
        // Giving them back shows the store's own again.
        assert_eq!(store.set_own_art("steam:1", false, None).unwrap(), Some(PathBuf::from("/art/b.png")));
        assert_eq!(store.set_own_art("steam:1", true, None).unwrap(), Some(PathBuf::from("/art/wide.jpg")));
        assert_eq!(shown(&store), (Some("/steam/cover.jpg".into()), Some("/steam/hero.jpg".into()), false));
        assert!(store.set_own_art("steam:404", false, None).is_err());

        // Settings, and the console a port is from.
        assert_eq!(store.setting("key").unwrap(), None);
        store.set_setting("key", Some(b"one")).unwrap();
        store.set_setting("key", Some(b"two")).unwrap();
        assert_eq!(store.setting("key").unwrap().as_deref(), Some(&b"two"[..]));
        store.set_setting("key", None).unwrap();
        assert_eq!(store.setting("key").unwrap(), None);
        let port = ManualGame { title: "Ocarina".into(), exe: PathBuf::from("/games/soh/soh.exe"), args: String::new(), kind: Kind::Port, cover: None, description: String::new(), origin: " Nintendo 64 ".into() };
        let id = store.add_manual(&port, 5).unwrap();
        assert_eq!(store.list().unwrap().into_iter().find(|g| g.id == id).unwrap().origin, "Nintendo 64");
    }

    #[test]
    fn a_game_added_as_a_shortcut_is_pointed_at_its_program() {
        let dir = std::env::temp_dir().join(format!("carla-mend-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let game = dir.join("Hookshot");
        std::fs::create_dir_all(&game).unwrap();
        let exe = game.join("hookshot.exe");
        std::fs::write(&exe, b"program").unwrap();
        let link = dir.join("Hookshot.lnk");
        std::fs::write(&link, crate::library::shortcut::sample(&exe.to_string_lossy(), "--fullscreen", "", true)).unwrap();
        let broken = dir.join("Gone.lnk");
        std::fs::write(&broken, crate::library::shortcut::sample(&dir.join("gone.exe").to_string_lossy(), "", "", true)).unwrap();

        let mut store = Store::in_memory().unwrap();
        let added = |exe: &Path| ManualGame { title: "A".into(), exe: exe.to_path_buf(), args: String::new(), kind: Kind::Game, cover: None, description: String::new(), origin: String::new() };
        let id = store.add_manual(&added(&link), 1).unwrap();
        let gone = store.add_manual(&added(&broken), 1).unwrap();
        assert_eq!(store.mend_shortcuts().unwrap(), 1);
        let install = store.install(&id, LOCAL).unwrap();
        assert_eq!((install.exe.as_deref(), install.install_path.as_path(), install.args.as_str()), (Some(exe.as_path()), game.as_path(), "--fullscreen"));
        // One that leads nowhere is left as it was, and nothing is mended twice.
        assert_eq!(store.install(&gone, LOCAL).unwrap().exe.as_deref(), Some(broken.as_path()));
        assert_eq!(store.mend_shortcuts().unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sessions_cut_short_count_as_no_time() {
        let mut store = Store::in_memory().unwrap();
        store.sync(LOCAL, "steam", &[scanned("1", "Alpha")], 0).unwrap();
        store.start_session("steam:1", LOCAL, 500).unwrap();
        store.close_dangling_sessions().unwrap();
        assert_eq!(store.list().unwrap()[0].playtime_minutes, 0);
    }

    #[test]
    fn a_desktop_adds_a_place_to_play_and_takes_it_away_again() {
        let mut store = Store::in_memory().unwrap();
        store.sync(LOCAL, "steam", &[scanned("10", "Here And There")], 1).unwrap();
        let host = HostRow {
            id: "home".into(),
            name: "HOME PC".into(),
            address: "192.168.1.20".into(),
            port: 47600,
            device_id: "d1".into(),
            key: vec![1, 2, 3],
            hardware: String::new(),
            last_seen: None,
            addresses: Vec::new(),
            macs: Vec::new(),
        };
        store.save_host(&host, 5).unwrap();
        assert!(!store.host_admin("home").unwrap(), "administering the Ally is never allowed by itself");
        store.set_host_admin("home", true).unwrap();
        assert!(store.host_admin("home").unwrap() && !store.host_admin("elsewhere").unwrap());
        store.save_host(&host, 5).unwrap();
        assert!(!store.host_admin("home").unwrap(), "linking again takes the permission back");
        assert_eq!(store.host_quality("home").unwrap(), (Setting::default(), None));
        let choice = quality::choose(Setting::Fixed(Level::Light), None, true);
        store.set_host_quality("home", Setting::Fixed(Level::Light)).unwrap();
        store.set_host_streamed("home", &Streamed { choice, at: 12 }).unwrap();
        assert_eq!(store.host_quality("home").unwrap(), (Setting::Fixed(Level::Light), Some(Streamed { choice, at: 12 })));
        store.sync("home", "steam", &[scanned("10", "Here And There"), scanned("20", "Only There")], 6).unwrap();

        let games = store.list().unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].places, ["local", "home"]);
        assert_eq!(games[1].places, ["home"]);

        let seen = HostRow {
            name: "Home PC".into(),
            hardware: "Ryzen".into(),
            address: "100.90.1.2".into(),
            addresses: vec!["192.168.1.20".into(), "100.90.1.2".into()],
            macs: vec!["aa:bb:cc:dd:ee:ff".into()],
            key: Vec::new(),
            ..host.clone()
        };
        store.host_seen(&seen, 9).unwrap();
        let hosts = store.hosts().unwrap();
        assert_eq!((hosts[0].name.as_str(), hosts[0].hardware.as_str(), hosts[0].last_seen), ("Home PC", "Ryzen", Some(9)));
        assert_eq!(hosts[0].key, [1, 2, 3], "seeing a desktop never touches its key");
        assert_eq!(hosts[0].address, "100.90.1.2");
        assert_eq!(hosts[0].addresses, ["192.168.1.20", "100.90.1.2"]);
        assert_eq!(hosts[0].macs, ["aa:bb:cc:dd:ee:ff"]);

        // Linking again keeps one row and takes the new key.
        store.save_host(&HostRow { key: vec![9], ..host }, 10).unwrap();
        assert_eq!(store.hosts().unwrap().len(), 1);
        assert_eq!(store.hosts().unwrap()[0].key, [9]);
        assert_eq!(store.hosts().unwrap()[0].macs, ["aa:bb:cc:dd:ee:ff"], "and still knows how to wake it");

        store.remove_host("home").unwrap();
        assert!(store.hosts().unwrap().is_empty());
        let games = store.list().unwrap();
        assert_eq!(games.len(), 1, "a game that was only on the desktop is no longer offered");
        assert_eq!(games[0].places, ["local"]);
    }

    #[test]
    fn consoles_are_kept_and_their_games_are_marked_as_emulated() {
        let mut store = Store::in_memory().unwrap();
        let typed = Console { id: String::new(), name: "GameCube".into(), emulator: PathBuf::from("/emu/dolphin"), options: "-b -e \"{game}\"".into(), folder: PathBuf::from("/roms/gc"), endings: vec!["iso".into(), "rvz".into()] };
        let saved = store.save_console(&typed, 100).unwrap();
        assert!(!saved.id.is_empty());
        let second = store.save_console(&Console { name: "PlayStation 2".into(), ..typed.clone() }, 100).unwrap();
        assert_ne!(saved.id, second.id, "two consoles added in the same second are still two");
        store.save_console(&Console { name: "GameCube and Wii".into(), ..saved.clone() }, 200).unwrap();
        let consoles = store.consoles().unwrap();
        assert_eq!(consoles.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["GameCube and Wii", "PlayStation 2"]);
        assert_eq!(consoles[0].endings, ["iso", "rvz"]);

        let knight = Scanned { exe: Some(PathBuf::from("/emu/dolphin")), args: "-b -e \"/roms/gc/Tin Knight.iso\"".into(), platform: "GameCube and Wii".into(), emulated: true, ..scanned(&format!("{}-abc", saved.id), "Tin Knight") };
        store.sync(LOCAL, "emu", &[knight], 300).unwrap();
        let games = store.list().unwrap();
        assert_eq!((games[0].platform.as_str(), games[0].kind, games[0].manual), ("GameCube and Wii", Kind::Emulated, false));
        assert_eq!(store.install(&games[0].id, LOCAL).unwrap().exe.as_deref(), Some(Path::new("/emu/dolphin")));

        store.remove_console(&saved.id).unwrap();
        assert_eq!(store.consoles().unwrap().len(), 1);
    }

    #[test]
    fn games_from_other_stores_keep_how_they_are_started() {
        let mut store = Store::in_memory().unwrap();
        let tides = Scanned { exe: Some(PathBuf::from("/games/Tides/tides.exe")), args: "-skipintro".into(), ..scanned("1207", "Hollow Tides") };
        store.sync(LOCAL, "gog", &[tides], 1).unwrap();
        store.sync(LOCAL, "epic", &[Scanned { args: "com.epicgames.launcher://apps/Jade?action=launch&silent=true".into(), ..scanned("Jade", "Velvet Circuit") }], 1).unwrap();
        store.sync(LOCAL, "steam", &[scanned("10", "Neon Harbor")], 1).unwrap();
        let games = store.list().unwrap();
        assert_eq!(games.iter().map(|g| (g.id.as_str(), g.platform.as_str())).collect::<Vec<_>>(), [("gog:1207", "GOG"), ("steam:10", "Steam"), ("epic:Jade", "Epic")]);
        let gog = store.install("gog:1207", LOCAL).unwrap();
        assert_eq!((gog.exe.as_deref(), gog.args.as_str()), (Some(Path::new("/games/Tides/tides.exe")), "-skipintro"));
        assert!(store.install("epic:Jade", LOCAL).unwrap().args.starts_with("com.epicgames.launcher://"));
        // One store's scan never touches another store's games.
        store.sync(LOCAL, "gog", &[], 2).unwrap();
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn a_game_added_by_hand_on_a_desktop_is_streamable_but_not_removable_here() {
        let mut store = Store::in_memory().unwrap();
        let key = crate::library::manual_key("D:\\Games\\Lanterns\\Lanterns.exe");
        assert_eq!(key, crate::library::manual_key("d:\\games\\lanterns\\LANTERNS.EXE"), "the same program whatever the capitals");
        store.sync("home", "manual", &[scanned(&key, "Paper Lanterns")], 1).unwrap();
        let games = store.list().unwrap();
        assert_eq!((games[0].id.clone(), games[0].platform.as_str(), games[0].manual), (format!("manual:{key}"), "Standalone", false));
        assert_eq!(games[0].places, ["home"]);
        assert_eq!(store.install(&games[0].id, "home").unwrap().source, "manual");
        // Taken off the desktop's list: gone here at the next look.
        store.sync("home", "manual", &[], 2).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn a_catalogue_from_the_first_release_gains_the_new_columns() {
        let dir = std::env::temp_dir().join(format!("carla-store-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("library.sqlite");
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch(
                "CREATE TABLE hosts (id TEXT PRIMARY KEY, name TEXT NOT NULL, address TEXT NOT NULL, port INTEGER NOT NULL,
                 device_id TEXT NOT NULL, key BLOB NOT NULL, hardware TEXT NOT NULL DEFAULT '', last_seen INTEGER, added INTEGER NOT NULL);
                 INSERT INTO hosts VALUES ('home', 'HOME PC', '192.168.1.20', 47600, 'd1', x'0102', '', NULL, 1);",
            )
            .unwrap();
        }
        for _ in 0..2 {
            let store = Store::open(&path).unwrap();
            let hosts = store.hosts().unwrap();
            assert_eq!((hosts[0].name.as_str(), hosts[0].key.as_slice()), ("HOME PC", [1u8, 2].as_slice()));
            assert!(hosts[0].addresses.is_empty() && hosts[0].macs.is_empty());
        }
    }
}
