# 07. Data and storage

Everything CarlaOS keeps on disk, on the Ally and (in summary) on a desktop: the SQLite catalogue, the loose files beside it, how secrets are sealed, what may never be written down, and what survives an update, a restore or an uninstall.

Audience: a developer or AI that has never seen the project and must add or change a stored thing without losing a player's library or leaking a secret.

Conventions used in this guide: paths are repo-relative unless they start with `%LOCALAPPDATA%`; claims I could not confirm from code are marked **UNVERIFIED:** with what would confirm them.

Related chapters: `docs/dev/03-desktop-host.md` documents the desktop host's data folder in detail; this chapter only summarises it. Build, test and release are in `docs/dev/08-build-test-release.md`.

---

## 1. Overview: what is stored where

| Where | What | Owner (code) | Section |
| --- | --- | --- | --- |
| `%LOCALAPPDATA%\dev.carlaos.shell\library.sqlite` (+ `-wal`, `-shm`) | The catalogue: games, installs, available titles, personal-library membership, play sessions, consoles, linked desktops, save-sync state, settings | `crates/core/src/library/store.rs`, opened by `apps/shell/src-tauri/src/library.rs` | 2 to 5 |
| `%LOCALAPPDATA%\dev.carlaos.shell\art\...` | Pictures (covers, wide pictures) CarlaOS fetched or the administrator chose | `apps/shell/src-tauri/src/{art,autoart,details,hosts,installs,library}.rs` | 7.3 |
| `...\saves\<game>\<timestamp>\` | Backups of a game's save folder | `crates/core/src/library/saves.rs`, called from `details.rs` and `savesync.rs` | 7.4 |
| `...\admin.json` | The administrator PIN as salted, iterated SHA-256 | `crates/core/src/admin.rs` | 8.2 |
| `...\cleanup\` | Windows clean-up helper script, request/result files, change journal | `crates/core/src/debloat/` | 7.5 |
| `...\remote.log` | Short record of what a desktop asked the Ally to do | `apps/shell/src-tauri/src/remote.rs` | 7.6 |
| `...\cloud-browser\` | Browser profile used only for cloud gaming | `apps/shell/src-tauri/src/library.rs` and `crates/core/src/library/cloud.rs` | 7.7 |
| `...\boot.json` | Start-up guard record | `crates/core/src/boot.rs` | 7.1 |
| `...\crash\` | Crash/hang/unexpected-exit reports, breadcrumbs, run marker | `crates/core/src/crash.rs` | 7.2 |
| `...\logs\connection.log` | Connection log of the Ally's side of the link | `apps/shell/src-tauri/src/conn_log.rs`, `crates/core/src/applog.rs` | 7.2 |
| `%TEMP%\CarlaOS-update\` | Downloaded installers waiting to run | `crates/core/src/update.rs` | 7.8 |
| `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `CarlaOS` | Start with Windows | `crates/core/src/boot.rs` | 7.1 |
| `%LOCALAPPDATA%\CarlaOS\companion\` | The desktop host's folder | `crates/companion/src/files.rs` | 9 |

The program files are separate from the data. The Ally installer puts the program under `%LOCALAPPDATA%\CarlaOS` (per `docs/RELEASES.md`); its data lives under the Tauri app identifier folder. Updating and uninstalling the program do not (by design) touch the data; see section 11.

### 1.1 The data folder and the Tauri identifier

The identifier is `dev.carlaos.shell` (`apps/shell/src-tauri/tauri.conf.json`, key `identifier`). There are two independent ways the code arrives at the same folder, and both must keep agreeing:

1. **After the app has started** the shell asks Tauri: `app.path().app_local_data_dir()` in `apps/shell/src-tauri/src/main.rs`. On Windows this is `%LOCALAPPDATA%\<identifier>`. That value becomes `data_dir` of `Library` and `Admin`.
2. **Before Tauri exists** (start-up guard, crash reporter, connection log) the code cannot ask Tauri, so it computes the folder by hand: `%LOCALAPPDATA%` joined with `context.config().identifier`. See `boot::record_path`, `crash::open` and `conn_log::open` in `apps/shell/src-tauri/src/`. If `LOCALAPPDATA` is not set they fall back to the system temp directory.

Consequence for developers: **never change `identifier`** in `tauri.conf.json` without a migration. Doing so would orphan the catalogue, the PIN, the sealed keys and every report in one stroke, and the hand-computed paths and Tauri's path would still agree with each other, so nothing would complain.

The `0.1.0` in `tauri.conf.json` is a placeholder stamped at release time (`scripts/release/stamp-version.mjs`); the identifier is not touched by stamping.

Tauri's WebView2 also keeps its own profile (cache, local storage) somewhere under this identifier's folder. **UNVERIFIED:** the exact subfolder (commonly `EBWebView`) was not checked in this repository; confirm by listing `%LOCALAPPDATA%\dev.carlaos.shell` after one run of the installed app.

### 1.2 Directory tree on an Ally after normal use

```
%LOCALAPPDATA%\dev.carlaos.shell\
  library.sqlite              the catalogue
  library.sqlite-wal          write-ahead log (journal_mode = WAL)
  library.sqlite-shm          shared memory index for the WAL
  admin.json                  PIN hash (salt, hash, rounds)
  boot.json                   start-up guard record
  remote.log                  one JSON line per request from an allowed desktop
  art\
    chosen\                   pictures the administrator picked or applied
    auto\                     pictures found by the automatic look-up
    desktops\                 covers fetched from linked desktops
    fetched\                  covers fetched by "look up details"
    steam\                    covers of Steam library games not installed here
    looking\                  previews of a running art search (emptied each search)
    <hex>.<ext>               artwork copied when adding a game by hand
  saves\
    <safe game id>\
      000001700000000000\     one backup (zero padded Unix seconds)
      000001700000000000-retired\
  cleanup\
    helper.ps1  journal.json  request-*.json  result-*.json (transient)
  cloud-browser\              cloud-gaming browser profile
  crash\
    breadcrumbs.log  running.json  seen.txt  panic-<unix>.txt  hang-<unix>.txt  unexpected-<unix>.txt
  logs\
    connection.log
  (WebView2 profile folder, see 1.1)
```

Every path in the tree is derived from `data_dir` in `apps/shell/src-tauri/src/library.rs` (`Inner.data_dir`, `Inner.art_dir`) except the three hand-computed ones (`boot.json`, `crash\`, `logs\`).

---

## 2. The SQLite catalogue: mechanics

| Fact | Detail | Source |
| --- | --- | --- |
| Engine | SQLite through `rusqlite` with the `bundled` feature (SQLite is compiled in, nothing to install) | `crates/core/Cargo.toml` |
| File | `<data_dir>\library.sqlite` | `Library::open` in `apps/shell/src-tauri/src/library.rs` |
| Access type | `carla_core::library::Store`, one `rusqlite::Connection` | `crates/core/src/library/store.rs` |
| Concurrency | One `Store` inside `Mutex<Store>` in the shell's `Library`; every command locks it via `library.store()`. No connection pool | `Inner.store` |
| Foreign keys | `PRAGMA foreign_keys = ON` on every open | `Store::prepare` |
| Journal | `PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;` best effort (errors ignored). Comment in code: fewer waits because the catalogue is written after every scan and game and storage is shared with the game being played | `Store::prepare` |
| Timestamps | `INTEGER`, Unix seconds (not milliseconds) unless noted | all callers use `library::now()` |
| Placement of SQL | **All** SQL is inside `store.rs`. The Tauri crate and other core modules call typed methods (`set_synced`, `host_seen`, ...) | convention, no raw SQL elsewhere |
| In-memory | `Store::in_memory()` for tests and as a fallback | `store.rs` |

### 2.1 The silent in-memory fallback

`Library::open` does:

```rust
let mut store = Store::open(&data_dir.join("library.sqlite")).or_else(|_| Store::in_memory())?;
let _ = store.close_dangling_sessions();
```

If the file cannot be opened or migrated (corrupt, locked, permissions), CarlaOS starts anyway with an **empty in-memory catalogue** ("A console that will not start is worse than one with an empty library", comment in code). The damaged file is neither renamed nor reported. Effects a developer must keep in mind:

- Anything the player does in that run (favourites, linked desktops, play time) is lost at exit.
- A rescan repopulates installed games, but linked desktops and their sealed keys are gone for that run.
- **UNVERIFIED:** whether the interface tells the player the catalogue is in-memory. Nothing in `library.rs` does so; confirm by searching `apps/shell/src` for a user-visible notice. If you add one, do it without changing the Tauri command signatures (they are a shared interface).

### 2.2 WAL files and copying the database

Because of WAL mode, the committed state may be partly in `library.sqlite-wal`. Copying only `library.sqlite` while CarlaOS is running can give a stale or inconsistent copy. A safe offline copy is: close CarlaOS, copy all three files (`.sqlite`, `-wal`, `-shm`) together (or none of the `-wal`/`-shm` after a clean close, since SQLite checkpoints on last close). See section 11.

### 2.3 Startup housekeeping on the catalogue

`Store::close_dangling_sessions` runs at every start: every `play_sessions` row with `ended IS NULL` gets `ended = started`, so a session left open by a crash or power cut counts as **zero** time played (the only length known not to be too long).

### 2.4 Mutation style (patterns to copy)

- **Re-scan replaces, never merges:** `sync(place, source, found, now)` deletes all `installs` of that `(place, source)` and re-inserts what was found, inside one transaction. `games` rows are upserted (`ON CONFLICT (id) DO UPDATE`) and never deleted by a scan, so favourites, `added`, play sessions and owned pictures survive uninstall and reinstall.
- **Upserts keep user choices:** every upsert states explicitly which columns an incoming value may overwrite (for example `cover = COALESCE(?5, cover)`, description only when empty or host-provided).
- **Transactions** wrap multi-statement changes (`sync`, `sync_available`, `add_manual`, `add_cloud`, `remove_host`).
- **JSON in TEXT columns** is used for small structured values (`endings`, `addresses`, `macs`, `streamed`, `health`, `pending_update`, `save_place`, `achievement_cache.json`). They are written with `serde_json` and read with `unwrap_or_default` or `.ok()`: **an unreadable JSON value reads as "none", never as an error**. Keep that property when adding fields.

---

## 3. Tables and columns

The schema has two parts: the base `SCHEMA` constant (the first release's tables, `CREATE TABLE IF NOT EXISTS`) and the `added` list of later columns applied with `ALTER TABLE` (section 4). The tables below show **base columns and added columns together**; added columns are marked *(added)*.

There is no `PRAGMA user_version` and no migration table.

### 3.1 `games`: one row per title

| Column | Type | Meaning |
| --- | --- | --- |
| `id` | TEXT PK | Catalogue id, the same on every machine. See section 5 |
| `title` | TEXT NOT NULL | Display title |
| `description` | TEXT NOT NULL DEFAULT '' | One clean line, at most `DESCRIPTION_MAX` (1000) characters; line breaks become spaces (`clean_description` in `library/mod.rs`) |
| `kind` | TEXT NOT NULL DEFAULT 'game' | Section of the interface: `game`, `emulated`, `port`, `homebrew` (`Kind::as_str`/`Kind::parse`; an unknown value reads as `game`) |
| `platform` | TEXT NOT NULL | The badge text. Store name (`Steam`, `Epic`, `GOG`, `Standalone`), the cloud service's name, or, for emulated games, the **console's name** (e.g. "GameCube and Wii") |
| `favourite` | INTEGER NOT NULL DEFAULT 0 | 0 or 1 |
| `added` | INTEGER NOT NULL | When first installed somewhere. **0 means "known only from a store's library, never installed"**; the first `sync` that installs it sets it (`CASE WHEN added = 0 THEN ?4`) |
| `cover` | TEXT | Path of a picture (file path as text). Automatic: scanner/Steam cache/downloaded. NULL = none |
| `hero` | TEXT | Path of the wide picture. Same rules |
| `save_path` *(added)* | TEXT | Folder the administrator or finder chose for this game's saves **on this machine** |
| `cover_own` *(added)* | TEXT | Cover the administrator chose; wins over `cover` (`COALESCE(cover_own, cover)` in listings); a look through the library never touches it |
| `hero_own` *(added)* | TEXT | Same for the wide picture |
| `origin` *(added)* | TEXT NOT NULL DEFAULT '' | For a `port`: the console the game first came out on ("Nintendo 64"). Used only for picture search |
| `art_tried` *(added)* | INTEGER NOT NULL DEFAULT 0 | Unix seconds when CarlaOS last looked for pictures by itself; looks are repeated only after `TRY_AGAIN_AFTER` (7 days, `library/autoart.rs`); reset to 0 by `forget_art_tried` |
| `save_place` *(added)* | TEXT (JSON) | Where the game's saves are "in words that hold on any machine" (`SavePlace`, `library/saveplace.rs`), e.g. `{"root":"local_app_data_low","rel":"A/B"}` |
| `save_place_host` *(added)* | TEXT | Id of the desktop that reported `save_place`; only that desktop may change it afterwards (`learn_save_place`). NULL after the administrator sets it (`set_save_place`) |
| `description_from` *(added)* | TEXT NOT NULL DEFAULT '' | `'host'` if the description came from a desktop, `''` if the administrator typed it on the Ally. Only host text is replaced by later host text |

Relationships: a game exists without installs (shows only with `list_all` if it has an `available` row). Deleting a `games` row cascades to `installs` (foreign key `ON DELETE CASCADE`).

Read by: `Store::list`/`list_all`/`list_personal` (the interface), `artless` (automatic art), `save_*` methods, `own_art`.

Written by:

| Writer | Columns |
| --- | --- |
| `Store::sync` (scan or desktop sync) | `id, title, platform, added, cover, hero, kind, description, description_from` |
| `Store::sync_available` | inserts `id, title, platform, added = 0, cover`; fills `cover` only if NULL |
| `Store::add_manual` | `id, title, kind, platform = 'Standalone', added, cover, description, description_from, origin` |
| `Store::add_cloud` | `id, title, kind = 'game', platform, added` |
| `set_favourite` | `favourite` |
| `set_cover_if_none`, `set_art_if_none`, `art_tried`, `forget_art_tried` | `cover`/`hero`/`art_tried` |
| `set_own_art` | `cover_own`/`hero_own` |
| `set_save_path`, `set_save_folder_of` | `save_path` (the latter for all games of a console) |
| `set_save_place`, `learn_save_place` | `save_place`, `save_place_host` |

**Emulated games share one save folder per console.** `save_folder_of` / `set_save_folder_of` work on every row where `kind = 'emulated' AND platform = <console name>`, because an emulator keeps the saves of all its games together. The key is the console's **name**, not its id; two consoles given the same name would share a folder. **UNVERIFIED:** whether the interface prevents duplicate console names (check `apps/shell/src-tauri/src/consoles.rs`).

### 3.2 `installs`: one row per place a title can be started from

| Column | Type | Meaning |
| --- | --- | --- |
| `id` | INTEGER PK | Row id; used only internally (for example by `mend_shortcuts`) |
| `game_id` | TEXT NOT NULL, FK `games(id)` ON DELETE CASCADE | The title |
| `place` | TEXT NOT NULL | `local` (this Ally, constant `LOCAL`), `cloud` (constant `CLOUD` in `library/cloud.rs`), or **a linked desktop's id** (`hosts.id`) |
| `source` | TEXT NOT NULL | `steam`, `epic`, `gog`, `manual`, `emu`, `cloud` |
| `source_id` | TEXT NOT NULL | The store's own id (Steam app id, Epic `AppName`, ...), for `manual` the lower-cased exe path (local) or the desktop's key; for `emu` `<console id>-<key>`; for `cloud` the key of the link |
| `install_path` | TEXT NOT NULL | Folder watched to know whether the game runs. For emulated games: the emulator's folder. For cloud games: `''` here (the shell substitutes `cloud-browser` at launch) |
| `exe` | TEXT | Program to start when the game starts without its store; for emulated games the emulator |
| `args` | TEXT NOT NULL DEFAULT '' | Options for `exe`; for a store that starts games by an address, that address; for emulated games the console's options with `{game}` replaced by the file; for cloud games **the link** |
| `pending_update` *(added)* | TEXT (JSON) | `Update { state, bytes, done }` when the store has an update to finish before the game starts, else NULL |

Constraints: `UNIQUE (place, source, source_id)`, index `installs_game (game_id)`.

Read by: `listing` (places and updates for each card), `install(game_id, place)` (how to start), `local_installs` (process recognition, `presence`), `known_programs` (discovery), `mend_shortcuts`.

Written by: `sync` (delete-and-insert), `add_manual`/`add_cloud` (upsert), `set_update`, `mend_shortcuts`, `remove_host` (deletes by `place`).

### 3.3 `available`: games a store could install somewhere

| Column | Type | Meaning |
| --- | --- | --- |
| `place` | TEXT | Where it could be installed (`local` or a desktop id) |
| `source` | TEXT | Currently only `steam` is written (`installs.rs`) |
| `source_id` | TEXT | Store id |
| `game_id` | TEXT NOT NULL | The `games.id`. **No foreign key**; the row is guaranteed by `sync_available` inserting the `games` row first |

PK `(place, source, source_id)`, index `available_game (game_id)`.

Written by `sync_available` (delete-and-insert per `(place, source)`). Read by `listing` (the `installable` places of a card, and "owned but not installed" cards for `list_all`), `offered`, `coverless`. Writers in the shell: `installs::sync_local` (this Ally's Steam library) and `installs::sync_host` (a desktop's).

### 3.4 `play_sessions`

| Column | Type | Meaning |
| --- | --- | --- |
| `id` | INTEGER PK | Session id kept in memory for the running game |
| `game_id` | TEXT NOT NULL | The title. **No foreign key** (removal code deletes rows explicitly) |
| `place` | TEXT NOT NULL | Where it was played |
| `started` | INTEGER NOT NULL | Unix seconds |
| `ended` | INTEGER | NULL while running; set to `MAX(now, started)` by `end_session` |
| `asleep` *(added)* | INTEGER NOT NULL DEFAULT 0 | Seconds the machine slept during the session; **not** played time |

Index `play_sessions_game (game_id)`.

Played time of a game = `SUM(MAX(ended - started - asleep, 0))` over finished sessions, divided by 60 and shown as minutes (in `listing`). Last played = `MAX(started)`.

Written by `start_session`, `end_session`, `session_slept` (from `apps/shell/src-tauri/src/library.rs`), `close_dangling_sessions` (start-up), `remove_manual` (deletes the sessions of a removed game). **Note:** `remove_host` does not delete sessions, and a game removed with `remove_manual` loses its history; a scanned game that is uninstalled keeps it.

### 3.5 `achievement_cache`

| Column | Type | Meaning |
| --- | --- | --- |
| `game_id` | TEXT PK | Title |
| `json` | TEXT NOT NULL | A desktop's answer about the game's achievements, stored as received |
| `at` | INTEGER NOT NULL | When it was cached |

Purpose: show achievements while the desktop sleeps. Written by `cache_achievements` (`details.rs`), read by `cached_achievements`. Never deleted, never expired by `Store`.

### 3.6 `consoles`: emulator set-ups made on the Ally

| Column | Type | Meaning |
| --- | --- | --- |
| `id` | TEXT PK | Generated by `save_console`: `c<hex now><count>` when empty |
| `name` | TEXT NOT NULL | Shown on the badge of its games |
| `emulator` | TEXT NOT NULL | Path of the emulator program |
| `options` | TEXT NOT NULL | Emulator options with `{game}` where the game file goes |
| `folder` | TEXT NOT NULL | Folder scanned for game files |
| `endings` | TEXT NOT NULL | JSON array of file endings (lower case, no dot) that are games |
| `added` | INTEGER NOT NULL | Sort order (`ORDER BY added, name`) |

Written by `save_console`, `remove_console` (`consoles.rs`). Removing a console does not delete its game files; its cards disappear at the next scan (`scan` replaces `emu` installs). Read by `consoles`, `known_programs`, and the scan (`library.rs`).

### 3.7 `save_sync`: last save synchronisation per (game, desktop)

| Column | Type | Meaning |
| --- | --- | --- |
| `game_id`, `host_id` | TEXT, PK together | The pair |
| `digest` | TEXT | What both machines held when they last agreed; kept when a later attempt fails (`COALESCE(?3, digest)`) |
| `at` | INTEGER NOT NULL | When |
| `outcome` | TEXT NOT NULL | `same`, `sent`, `fetched` or `problem` |
| `both_changed` | INTEGER NOT NULL DEFAULT 0 | Both sides had new saves; older ones are in a safe copy |
| `files` | INTEGER NOT NULL DEFAULT 0 | File count |
| `problem` | TEXT | Plain-words reason when `outcome = 'problem'` |

Written by `set_synced`, `clear_synced`, `remove_host` (deletes by `host_id`). Read by `synced`. Caller: `apps/shell/src-tauri/src/savesync.rs`. The save-sync algorithm itself is in `crates/core/src/library/sync.rs`; see `docs/SAVE-SYNC.md`.

### 3.8 `save_clean`: "keep the desktop's live saves clean"

| Column | Type | Meaning |
| --- | --- | --- |
| `game_id`, `host_id` | TEXT, PK together | The pair |
| `keep_clean` | INTEGER NOT NULL | What the desktop last said about clearing its live saves after a stream |

Kept across restarts so a stream can still be held back when the desktop cannot be asked. Written by `set_keep_clean`, read by `keep_clean` (`savesync.rs`). **Not deleted by `remove_host`**; stale rows for removed desktops stay (harmless, tiny). **UNVERIFIED:** whether anything else prunes them; grep for `save_clean` shows only the two accessors in `store.rs`.

### 3.9 `settings`: loose name/value pairs

| Column | Type | Meaning |
| --- | --- | --- |
| `name` | TEXT PK | Key |
| `value` | BLOB NOT NULL | Bytes; the caller defines the format |

API: `setting(name) -> Option<Vec<u8>>`, `set_setting(name, Some(bytes) | None)` (None deletes the row). Known names (from `grep` of callers):

| Name | Format | Written by | Notes |
| --- | --- | --- | --- |
| `steamgriddb_key` | DPAPI-sealed bytes of the key text (`secret::seal`) | `apps/shell/src-tauri/src/art.rs` | Only its presence is exposed to remote callers (`remote.rs`: `key_set`). Must pass `art::key_shape` when read |
| `auto_art` | `b"0"` = off; **absent = on** | `apps/shell/src-tauri/src/autoart.rs` | Absent means enabled, so default-on needs no migration |
| `power:<kind>:<game id>` | JSON of `Profile { power, screen }` (`power` one of `quiet`, `balanced`, `turbo`; `screen` one of `smooth`, `saver`) | `crates/core/src/power.rs` (`choose`) | `<kind>` is `local`, `stream` or `cloud` (from the place of the game), per game. Absent = the kind's default profile |

To add a setting: choose a namespaced name, define absent = default, store bytes (JSON for structures), and add it to this table. Never store a secret unsealed (section 8).

### 3.10 `hosts`: linked desktops

| Column | Type | Meaning |
| --- | --- | --- |
| `id` | TEXT PK | The desktop's own id; **also the `place` value of that desktop's `installs`/`available` rows** |
| `name` | TEXT NOT NULL | Desktop's name (refreshed by `host_seen`) |
| `address` | TEXT NOT NULL | Address that answered last |
| `port` | INTEGER NOT NULL | Companion port |
| `device_id` | TEXT NOT NULL | What the desktop calls this Ally |
| `key` | BLOB NOT NULL | **The device key, sealed** with `secret::seal` before it is stored (`HostRow.key` doc; the shell does `key: secret::seal(&link.key)?` when linking and `secret::open(&host.key)?` when building a `Link`). `save_host` stores it **as given**, so callers must seal first |
| `hardware` | TEXT NOT NULL DEFAULT '' | What the desktop said about itself |
| `last_seen` | INTEGER | Unix seconds of the last answer |
| `added` | INTEGER NOT NULL | Sort order |
| `addresses` *(added)* | TEXT NOT NULL DEFAULT '[]' | JSON array of every address the desktop reported |
| `macs` *(added)* | TEXT NOT NULL DEFAULT '[]' | JSON array of hardware addresses, for wake-on-LAN |
| `admin` *(added)* | INTEGER NOT NULL DEFAULT 0 | 1 = this desktop may administer the Ally. **Reset to 0 whenever the desktop is linked again** (`save_host` upsert sets `admin = 0`) |
| `quality` *(added)* | TEXT NOT NULL DEFAULT 'auto' | The administrator's chosen picture setting for streams from it (`quality::Setting` name) |
| `streamed` *(added)* | TEXT (JSON) | `Streamed { choice, at }`: what the last stream got |
| `health` *(added)* | TEXT (JSON) | `link::health::Health`: the connection as last measured |

Written by: `save_host` (link), `host_seen` (every successful poll; does not touch the key), `set_host_admin`, `set_host_quality`, `set_host_streamed`, `set_host_health`, `remove_host`. Read by `hosts`, `host_admin`, `host_quality`, `host_health`.

`remove_host` deletes (in one transaction) the desktop's `installs`, `available`, `save_sync` and `hosts` rows. It does **not** delete `games` rows (they simply stop being listed if they have no remaining install or availability), `play_sessions`, `save_clean`, or the pictures fetched into `art\desktops\`. `remove_host(LOCAL)` is a no-op by design.

### 3.11 Indexes

| Index | On |
| --- | --- |
| `available_game` | `available (game_id)` |
| `installs_game` | `installs (game_id)` |
| `play_sessions_game` | `play_sessions (game_id)` |

(plus the implicit primary-key and `UNIQUE` indexes). Listing is done with correlated subqueries per game; the library is small (hundreds to low thousands), so there is no pagination.

### 3.12 `library_entries`: the Ally's personal My Library

| Column | Type | Meaning |
| --- | --- | --- |
| `game_id` | TEXT PK, FK `games(id)` ON DELETE CASCADE | A game explicitly selected for this Ally's personal library |
| `added` | INTEGER NOT NULL | Unix seconds when it was selected |

This table is local to the Ally's catalogue. It does not represent a Steam account, a desktop library or a second game record. `Store::list_personal` returns rows selected here **plus every game with an install where `place = 'local'` and `source = 'steam'`**. Other installed games, remote games and uninstalled Steam offers are included only after `add_to_my_library`.

Written by `add_to_my_library` (`INSERT OR IGNORE`, so repeat selection keeps its first timestamp) and `remove_from_my_library`. Deleting a game cascades to this table. There is deliberately no migration backfill: an existing catalogue starts with an empty membership table, while local Steam installs appear through the default rule.

Two uninstall paths differ:

- **Remove from My Library:** a local Steam install is passed to `installs_remove_from_my_library`; membership is deleted only after Steam's manifest shows the install is gone. Cancelling Steam's confirmation leaves the install and selection intact.
- **Settings, Storage:** `storage_uninstall` first ensures a membership row exists, then asks Steam to uninstall. This frees local space while keeping the title selected and available to install again.

Removing a desktop game from My Library deletes only this Ally's membership; it does not delete or uninstall anything on the desktop.

---

## 4. Migrations: how the schema evolves

### 4.1 The mechanism

`Store::prepare` runs in this order on every open:

1. `PRAGMA foreign_keys = ON`.
2. `PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;` (errors ignored).
3. `execute_batch(SCHEMA)`: every statement is `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`, so on an existing database this creates only what is missing.
4. The `added` array of `(table, column, type)` tuples. For each, it checks `SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2` and, only if the column is absent, runs `ALTER TABLE {table} ADD COLUMN {column} {type};`.

There is no version number. The database "version" is simply which columns exist. Running `prepare` any number of times yields the same result: **idempotent by construction**. There are no down-migrations.

### 4.2 Adding a column (the only supported change to an existing table)

1. Add one tuple to the `added` array in `Store::prepare`, with a comment saying what it means. **Do not edit the `CREATE TABLE` in `SCHEMA`**. The base schema is deliberately the first release's; a test asserts the base has no `description_from`, and `a_catalogue_from_the_first_release_gains_the_new_columns` builds a first-release table by hand.
2. The type string must be valid after `ADD COLUMN`. SQLite rules that follow from this:
   - A `NOT NULL` column **must have a constant `DEFAULT`** (`TEXT NOT NULL DEFAULT ''`, `INTEGER NOT NULL DEFAULT 0`). Without it the `ALTER` fails and, because `prepare` propagates the error, `Store::open` fails and the shell falls back to the in-memory catalogue (section 2.1), which is a silent data-loss mode.
   - No `PRIMARY KEY` or `UNIQUE` columns, no non-constant defaults.
   - Nullable columns without a default are fine (`TEXT`).
3. Choose the default so that existing rows have a meaningful value. Example in the codebase: `description_from TEXT NOT NULL DEFAULT ''` means "an existing description counts as typed here", so an old catalogue never has its hand-typed text replaced by a desktop's.
4. Use the new column through named-column SQL only (never `SELECT *`, never positional `INSERT` without a column list), so that older programs reading a newer catalogue after a "go back" install keep working. Old code ignores unknown columns; an old `INSERT` that omits a new column works only if the column has a default or is nullable, which rule 2 guarantees.
5. Add a test (below).

### 4.3 Adding a table

Append a `CREATE TABLE IF NOT EXISTS` (and any `CREATE INDEX IF NOT EXISTS`) to `SCHEMA`. Existing databases get it at the next open. Remember the foreign key habit: `installs.game_id` has one with `ON DELETE CASCADE`; `available`, `play_sessions`, `save_sync` and `save_clean` do not, so you must delete their rows explicitly in `remove_manual`/`remove_host`-style code.

### 4.4 Things the mechanism cannot do

- Rename, retype, drop or add constraints to a column. SQLite and this mechanism do not support it here. If ever needed, it needs a real versioned migration (for example `PRAGMA user_version` plus a table rebuild). That is a **schema change: stop and get approval** (`CLAUDE.md`: SQLite schema is a shared interface).
- Backfill data conditionally. A one-off backfill after `ALTER` would run at every start unless it guards itself; keep it idempotent (for example `UPDATE ... WHERE col = ''`).
- Detect that an older program is reading a newer catalogue. `docs/RELEASES.md` says plainly that whether every older version copes with a newer one's data "has not been tried".

### 4.5 Test pattern

Tests live at the bottom of `store.rs` (`#[cfg(test)] mod tests`) and use `Store::in_memory()`. For migrations copy one of:

- `a_database_from_before_description_from_is_migrated_and_keeps_working`: create a `Connection::open_in_memory()`, run only `SCHEMA`, insert an old-shaped row, assert the new column is absent, call `Store::prepare(db)`, assert the default and the behaviour that depends on it.
- `a_catalogue_from_the_first_release_gains_the_new_columns`: create an on-disk file with the first-release `hosts` table, then `Store::open` it **twice** to prove idempotence and that existing values (including the key blob) survive.

Run: `cargo test --locked -p carla-core library::store` (from a VS developer shell on Windows, see `docs/dev/08-build-test-release.md`).

---

## 5. The games and installs model

> One `games` row is one **title**. One `installs` row is one **place** that title can be started from. A card in the interface only ever offers places where the game really is. (`crates/core/src/library/mod.rs` module comment.)

```
games (id, title, kind, platform, favourite, pictures ...)
  |  1
  |  *
installs (game_id, place, source, source_id, install_path, exe, args)
              place = 'local' | '<desktop id>' | 'cloud'
available (place, source, source_id, game_id)     "could be installed there"
play_sessions (game_id, place, started, ended, asleep)
```

### 5.1 Places

| `place` | Meaning | Created by |
| --- | --- | --- |
| `local` | This machine | scans in `Library::scan` and `add_manual` |
| `<hosts.id>` | A linked desktop: the game is started there and streamed here | `hosts::sync_games` -> `Store::sync(host.id, source, ...)` for the sources `steam`, `epic`, `gog`, `manual`, `emu` |
| `cloud` | A cloud-gaming service in a browser window | `add_cloud` |

`GameView.places` lists `local` first, then the others alphabetically; `installable` lists the places where a store could install the game and has not yet.

### 5.2 Ids

The id of a `games` row is **built from the source and the source's own id** and is the same on every machine, which is what lets one card carry several places:

| Id form | Built by | Example | Notes |
| --- | --- | --- | --- |
| `steam:<app id>` | `Store::sync`: `format!("{source}:{source_id}")` | `steam:504230` | Also used for owned-but-not-installed games from `sync_available` |
| `epic:<AppName>` | same | `epic:<AppName>` | Epic's own app name from its manifest |
| `gog:<id>` | same | `gog:1207658924` (numeric id, from a test in `gog.rs`) | GOG ids are numeric (the scanner rejects non-numeric ones) |
| `manual:<key>` | `add_manual`: `manual_key(exe)` | `manual:00ab12...` | `manual_key` is the FNV-1a 64-bit hash of the **lower-cased** exe path, 16 hex digits (`library/mod.rs`). Not for security. The same key is used on a desktop, so a program added on both machines is one title |
| `emu:<console id>-<key>` | `emulation::scan` builds `source_id = "{console.id}-{manual_key(path)}"`, then `sync(..., "emu", ...)` | `emu:c1a2b0-3f...` | Local or from a desktop (the desktop sends `emu:<console>-<key>` in `RemoteGame.id`, `crates/core/src/link/mod.rs`) |
| `cloud:<key>` | `add_cloud`: `manual_key(link)` | `cloud:9d8c...` | `place = 'cloud'`, `source = 'cloud'`, the link in `installs.args` |

Consequences and traps:

- Titles are **not merged across stores**: the same game on Steam and Epic is two `games` rows (`docs/LIBRARY.md`: "for now").
- The id is the identity for favourites, play time, saves, pictures, power profiles (`settings` key) and sync state. Changing how an id is computed orphans all of that. `manual_key` must stay byte-for-byte stable and equal between `crates/core` and the companion (both call `carla_core::library::manual_key`).
- Desktop games are accepted only when `g.id == format!("{source}:{}", g.source_id)` (`hosts.rs` `sync_games`), which stops a desktop from injecting an id that collides with another source's.
- `remove_manual` accepts only ids starting with `manual:` or `cloud:` ("scanned games cannot be removed this way; they would only come back at the next scan"). In the interface a card is removable (`GameView.manual`) only if it is `manual:` **with a `local` place**, or `cloud:`. A desktop's own manual games can only be removed on that desktop.

### 5.3 How the catalogue stays in step with reality

| Event | Effect on the tables | Code |
| --- | --- | --- |
| Scan of this Ally | For each store in `STORES = ["steam","epic","gog"]` and for `emu`: replace `installs(local, source)`; upsert `games`; then refresh `available(local, steam)` from Steam's owned-but-not-installed list | `Library::scan` -> `Store::sync`, `installs::sync_local` |
| Desktop answers its game list | Replace `installs(<desktop>, source)` for five sources; fetch covers into `art\desktops`; refresh `available(<desktop>, steam)` | `hosts::sync_games`, `installs::sync_host` |
| Administrator adds a program | Upsert `games(manual:key)` and `installs(local,'manual',exe-lower)`; picture copied to `art\` | `library_add_manual` |
| Administrator adds a cloud game | Upsert `games(cloud:key)` and `installs(cloud,'cloud',key)` | `cloud.rs` -> `add_cloud` |
| Game uninstalled in its store | Install row disappears at next scan; `games` row, favourite and sessions stay | `sync` |
| Desktop unlinked | Its installs/available/save_sync/hosts rows deleted | `remove_host` |
| Game starts / ends | `play_sessions` row created / `ended` set | `library.rs` |
| Ally slept during a session | `asleep` incremented | `session_slept` |

### 5.4 Description ownership (a worked example of "who may overwrite")

`description` and `description_from` encode three rules in `sync`:

1. A host's description fills an empty description or replaces one that is already `'host'`.
2. It never replaces text typed on the Ally (`description_from = ''`).
3. An empty description from one desktop never wipes another desktop's (test `an_empty_description_from_one_desktop_does_not_wipe_another_desktops`).

Use the same shape (value plus an "origin" column) for any other field that two sources may write.

---

## 6. Settings and per-game choices summary

| What | Where stored | Default when absent |
| --- | --- | --- |
| Automatic artwork on/off | `settings.auto_art` | on |
| SteamGridDB key | `settings.steamgriddb_key` (sealed) | none |
| Power and screen profile per game and kind | `settings.power:<kind>:<game id>` | kind default |
| Quality setting per desktop | `hosts.quality` | `auto` |
| Desktop may administer | `hosts.admin` | 0 |
| Favourite | `games.favourite` | 0 |
| Save folder (this machine) | `games.save_path` | none |
| Save place (portable) | `games.save_place` | none |
| Administrator PIN | `admin.json` (not in SQLite) | no PIN set |
| Start with Windows | registry `Run\CarlaOS` (not in SQLite) | off |

---

## 7. Every other file the Ally writes

All under `%LOCALAPPDATA%\dev.carlaos.shell\` unless stated.

### 7.1 `boot.json`: start-up guard

- **Code:** `crates/core/src/boot.rs` (`Guard`), path from `apps/shell/src-tauri/src/boot.rs::record_path`.
- **Format:** JSON `{ "unfinished": u32, "stood_down": bool, "removed_delay": bool }`, all fields `#[serde(default)]`.
- **Lifecycle:** When Windows starts CarlaOS with the `--boot` argument, `begin()` increments `unfinished`; `ready()` (the library is on screen) resets it to 0. After `FAILED_STARTS = 2` unfinished starts, the next start **stands down** (does not show CarlaOS, leaves the Windows desktop), zeroes the counter and sets `stood_down` so the next normal start can say so once (`take_stood_down`). Holding View while starting skips CarlaOS once.
- **Failure mode:** an unreadable file counts as a clean record. "The guard must never be the reason CarlaOS does not start." Write errors are ignored.
- **`removed_delay`:** CarlaOS set `HKCU\...\Explorer\Serialize\StartupDelayInMSec = 0` (only if no value existed) and must remove it when start-with-Windows is turned off.
- **Registry (not a file):** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `CarlaOS` = `"<exe path>" --boot`. Changing it is gated by the administrator lock (`boot_set` calls `admin.require()`). Anything that edits Windows configuration needs explicit user approval during development (`CLAUDE.md`).

### 7.2 `crash\` and `logs\`: diagnostics

| File | Written by | Format | Lifecycle and caps |
| --- | --- | --- | --- |
| `crash\breadcrumbs.log` | `Crash::note` (`crates/core/src/crash.rs`) | One line each: `YYYY-MM-DD HH:MM:SS UTC <text>`; text flattened, control characters removed, max `NOTE_LEN = 300` characters | Appended. When it passes `NOTES_MAX = 96 KB` it is cut to its newest half |
| `crash\running.json` | `Crash::begin` | JSON `{pid, start, started, version}` | Exists while the program runs; removed on normal end (`end`) and by the panic hook. If present at start and that process is gone, an `unexpected` report is written. `start` guards against pid reuse. A second copy started while the first is alive writes no marker |
| `crash\seen.txt` | `mark_seen` | Unix seconds of the newest report shown | Used for the `new` flag |
| `crash\panic-<unix>.txt`, `hang-<unix>.txt`, `unexpected-<unix>.txt` (suffix `-2`, `-3` if two in the same second) | `write_panic`, `write_hang`, `begin` | Plain text from `crash::render`: kind, version, time, OS, summary, state line, detail (backtrace for panics), the last `NOTES_IN_REPORT = 60` notes | `KEEP = 10` newest kept, older deleted on every new report. Only names matching `parse_name` can be read through `Crash::read`, which blocks path tricks |
| `logs\connection.log` | `conn_log` using `carla_core::applog::Log` | One line each: `YYYY-MM-DD HH:MM:SS.mmm UTC <text>`, max `LINE_LEN = 400` | Cut to its newest half past `MAX_BYTES = 256 KB`. The activity-log view merges it with the breadcrumbs (`applog::merged`, at most 5000 lines) |

How they get written (so you can add events): `crash::note(text)` and `conn_log::line(text)` in the shell; both are safe before initialisation (no-op) and never fail the caller. The panic hook uses `note_nowait` (`try_lock`) so it can never deadlock on the thread that panicked. Panic profile: the release profile is `panic = "abort"` (`Cargo.toml`), so the hook is the last code that runs.

Frozen-interface detection: the page calls `crash_alive` every few seconds; a watchdog (`crash::watch_ui`) writes a `hang` report after 30 s of silence from a visible, non-hidden window (`docs/CRASH-REPORTS.md`).

**Nothing is sent anywhere.** Reports stay on the device until a person copies one (Settings, Crash reports / Activity log).

### 7.3 `art\`: pictures

All files are pictures verified by content (`meta::picture_kind(bytes)` accepts JPEG, PNG, WebP; the file extension is chosen from the content, not from the source name).

| Folder or pattern | Written by | File name | Lifecycle |
| --- | --- | --- | --- |
| `art\chosen\` | `art::keep` (administrator applied or picked a picture) | `<manual_key(game id)>-<wide|cover>-<millis hex>.<ext>` (new name each time so the interface never shows a cached old picture) | The replaced file is deleted by `forget`, **only if it lies inside `chosen\`** |
| `art\auto\` | `autoart::keep` | `<manual_key(game id)>-<wide|cover>.<ext>` | Overwritten by later looks. `set_art_if_none` writes the path into `games.cover/hero` only when empty. Looks cap at `MOST = 400` games per pass, repeat after 7 days, pause while a game is running, off via `settings.auto_art = "0"` |
| `art\desktops\` | `hosts::sync_games` | `<source>-<source_id>.jpg` | Fetched only for games that have no cover here yet, only when `source_id` is alphanumeric (a path-traversal guard), stops after 3 silent failures or the time budget (`SYNC_BUDGET = 20 s`) |
| `art\fetched\` | `details::meta_fetch` | `<id with ':' -> '-'>.<ext>` | Overwritten on repeat |
| `art\steam\` | `installs::fetch_covers` | `<appid>.jpg` | Public Steam store covers of owned-not-installed games; up to 800 per pass; stops after 8 misses in a row (offline) |
| `art\looking\` | `art::previews` | `<manual_key(thumb url)>.<ext>` | **Emptied and recreated at each search** |
| `art\<hex>.<ext>` | `library::keep_artwork` | `<nanos hex>.<ext>` | Copy of a picture chosen with the file dialog when adding a game by hand, so it survives the original being moved. Never deleted automatically |

The shell lets the interface show these through the Tauri asset protocol: `allow_pictures(app, dir)` adds each folder to `asset_protocol_scope` (and Steam's `appcache\librarycache`). If you add a new art folder you must allow it the same way or the interface will not display files from it (`tauri.conf.json` has `"assetProtocol": {"enable": true, "scope": []}`; the scope is extended at run time).

`games.cover`, `hero`, `cover_own`, `hero_own` store **absolute paths as text**. Moving the data folder would break them. **UNVERIFIED:** whether any code re-bases stored picture paths; nothing in `store.rs` does.

### 7.4 `saves\`: backups of save folders

- **Code:** `Backups` in `crates/core/src/library/saves.rs`. Created in `details.rs` and `savesync.rs` as `Backups::new(&data_dir.join("saves"), game_id)`.
- **Layout:** `saves\<safe id>\<backup id>\...`. `safe id` = the game id with every character other than ASCII letters, digits and `-` turned into `_` (so `steam:480` becomes `steam_480`). Backup id = the Unix time zero-padded to 12 digits, plus `-2`, `-3` for the same second; a copy taken just before a desktop cleared its live saves has the suffix `-retired`.
- **Caps:** `KEEP = 5` ordinary backups per game; `KEEP_RETIRED = 15` for the "retired" ones, counted apart ("it may be the only other copy"). A save folder larger than `LARGEST = 512 MB` or with more than 20,000 files is not copied ("that is not a save folder").
- **Atomicity:** a backup is copied into `partial-<id>` and renamed, so a half-made backup is never listed.
- **Restore:** first takes a new backup of what is in the save folder now (so a restore can be undone), never prunes the backup being restored, then **clears the folder's contents (not the folder, because the game may hold it)** and copies the backup back.
- **Links are not followed** (`symlink` entries are skipped in `walk` and `copy_tree`).
- **Rule:** CarlaOS never edits a save in place; it copies, and restores whole folders.

### 7.5 `cleanup\`: the Windows clean-up engine

- **Code:** `crates/core/src/debloat/` (`powershell.rs`, `journal.rs`), directory chosen by `apps/shell/src-tauri/src/admin.rs`.
- `helper.ps1`: rewritten from an embedded constant at every operation.
- `request-<pid>-<n>.json`, `result-<pid>-<n>.json`: transient, deleted after each call.
- `journal.json`: pretty JSON of every change made and how to undo it (`Entry { seq, item_id, title, kind, applied_at, done, undo, undone_at }`). Written **to `journal.json.tmp` and renamed**, so a crash never leaves half a journal. **Never delete or hand-edit it**: it is the only record that lets the undo path restore Windows settings.
- Every clean-up command requires the administrator lock on the Rust side (`Admin::cleanup`), except those arriving from an allowed desktop (`cleanup_remote`).

### 7.6 `remote.log`

- **Code:** `record` in `apps/shell/src-tauri/src/remote.rs`.
- **Format:** one JSON object per line: `{"at": <unix>, "desktop": "<desktop name>", "asked": "<operation>", "ok": true|false}`.
- **Cap:** when it exceeds 256 KB the file is **deleted** and started again (not trimmed to a half).
- It records the desktop's **name** as stored, which is not passed through the `applog` scrubber. Keep this file free of arguments and results.

### 7.7 `cloud-browser\`

A browser profile directory passed to the browser when a cloud game is started so services stay signed in between games (`library.rs`, `launch::start`, `cloud::browser_args`). Created on demand (`create_dir_all`). It holds whatever the browser puts there, including the player's cloud-service sign-in cookies. CarlaOS does not read it. The browser is started with `--user-data-dir=<profile>`, `--app=<link>`, `--start-fullscreen` and similar (`cloud::browser_args`; the Chromium-style flags suggest Edge, **UNVERIFIED:** which executable `cloud.rs` picks). **UNVERIFIED:** whether the profile is cleaned on uninstall.

### 7.8 `%TEMP%\CarlaOS-update\`

Downloaded installers (`update::download_dir`). Files are fetched only from this repository's GitHub release downloads and only run when the SHA-256 of the installer matches the release's `SHA256SUMS.txt` (`crates/core/src/update.rs`; build side in chapter 08). `update::download` removes a partial file on failure and an existing target before writing; **UNVERIFIED:** whether a finished installer is deleted after it ran (nothing in `updates.rs` does so; it is in the temp folder, which Windows may clean).

### 7.9 Steam files CarlaOS touches (not its own data)

`library/downloads.rs` can write a Steam `appmanifest_<appid>.acf` into the roomiest Steam library's `steamapps` folder with `create_new(true)` (never over an existing record) to start an install quietly, then restarts Steam so it reads it. It asks Steam to uninstall with `steam://uninstall/<id>`. These are Steam's files; treat them as an integration, not as CarlaOS storage.

---

## 8. Secrets and how they are stored

### 8.1 Inventory

| Secret | Where | Protection |
| --- | --- | --- |
| Device key for each linked desktop | `hosts.key` (BLOB) | DPAPI-sealed (`secret::seal`) |
| SteamGridDB API key | `settings.steamgriddb_key` | DPAPI-sealed |
| Administrator PIN | `admin.json` | Salted, 50,000-round SHA-256; not reversible. **Not** DPAPI-sealed |
| Desktop: device keys of linked Allies, Sunshine password | desktop host's `devices.json`, `config.json` | DPAPI-sealed, hex-encoded (section 9) |
| Pairing code / enrolment answers | desktop host folder, short-lived | plain, expiring (section 9) |

### 8.2 The seal: `crates/core/src/secret.rs`

- **Windows:** `CryptProtectData` / `CryptUnprotectData` from `crypt32` with `CRYPTPROTECT_UI_FORBIDDEN (0x1)`, no extra entropy. The blob is tied to the **Windows account**: copying the file to another machine or account yields nothing. `open` on a blob sealed by another account fails with "A stored key could not be read. Link the desktop again."
- **Other platforms (development and tests):** `seal` and `open` are the identity function. Secrets are **plain** on Linux/macOS builds. Never run production on those.
- API: `seal(&[u8]) -> Result<Vec<u8>, String>`, `open(&[u8]) -> Result<Vec<u8>, String>`. Memory is copied out and Windows' buffer is freed with `LocalFree`.
- Rule: **store the sealed bytes, never the plain secret; call `open` as late as possible and do not keep the plain value.** The shell does `Link { key: secret::open(&host.key)? }` per use.

Consequences:

- A restored catalogue copied to another Windows account (or a reinstalled Windows) keeps its games but loses every device key and the SteamGridDB key. The player must link desktops again. `secret::open` errors are surfaced in plain words ("Link the desktop again").
- The PIN hash is not tied to the account. The module comment in `crates/core/src/admin.rs` says plainly: "a guard rail, not a security boundary: the PIN file sits in the user's own profile".

### 8.3 The administrator PIN (`crates/core/src/admin.rs`)

- 4 digits (`PIN_LENGTH`). File `admin.json`: `{ "salt": 32 hex chars from two OS-seeded `RandomState` hashes, "hash": hex SHA-256 iterated `rounds` times over `"<salt>:<pin>"`, "rounds": 50000 }`.
- Setting a PIN is allowed the first time, afterwards only while unlocked. Five wrong tries (`FREE_TRIES`) pause unlocking for 30 s (in memory only: restarting resets the counter).
- The unlocked state is **in memory only**; it is never persisted.
- Guarded commands call `Admin::require()` on the Rust side; the interface cannot skip it.

### 8.4 What other code does with secrets

- The device key is used to sign requests to a desktop (`crates/core/src/link/sign.rs`, SPAKE2 enrolment in `link/enrol.rs`). Details are in the link chapter; this chapter's concern is only that the stored form is sealed.
- A desktop allowed to administer the Ally is recorded as `hosts.admin = 1` (set with the PIN). It is cleared when the desktop is linked again.

---

## 9. The desktop host's data directory (summary)

Detailed in `docs/dev/03-desktop-host.md`. Facts relevant to storage design:

- **Folder:** `%LOCALAPPDATA%\CarlaOS\companion` (`default_dir` in `crates/companion/src/files.rs`). The environment variable `CARLA_COMPANION_DIR` overrides it; **tests rely on this to get an isolated folder** (see the flaky-test history in chapter 08: two tests that shared one state folder). Without `LOCALAPPDATA` it falls back to `~/.local/share/carlaos-companion` (Linux development).
- **Files (JSON unless noted):** `config.json` (id, name, port, Sunshine URL/user, **sealed** Sunshine password in hex, toggles), `devices.json` (linked Allies, **sealed** device keys in hex), `games.json` (programs added by hand), `consoles.json`, `saves.json`, `save-suggestions.json`, `enrol.json`, `enrol-ask.json`, `enrol-answer.json` (short-lived linking state), `ally-seen-<device>.json`, `ally-pulse-<device>.json`, `pending.json` (a game a stream is for, taken once), `art-choices.json` and `art-choices.html` (artwork choices page, `ally.rs`), `save-copies\` (safe copies of saves), `host.log` and `launcher.log` (text).
- **Write discipline:** `Files::write` writes to `<name>.<pid>.<counter>.new` and renames, and skips the write when the text is unchanged, so readers never see half a file and idle polling does not churn the disk. `Files::read` returns `None` on any error (missing or corrupt reads as "nothing").
- **Logs:** `host.log` is an `applog::Log` capped at 256 KB (cut to the newest half); `launcher.log` is **deleted** when it exceeds 256 KB.
- **Not in SQLite.** The desktop has no catalogue; it scans Steam/Epic/GOG on demand and answers over the link.
- **UNVERIFIED:** whether any other companion file exists beyond those found by `grep` of `crates/companion/src` (for example from `sunshine.rs`, `stage.rs`, `virtual_screen.rs`). Confirm against `docs/dev/03-desktop-host.md` and the listing of a used host folder.

Uninstall of the desktop host leaves this folder alone (`packaging/host.nsi` deletes only the program files, shortcuts and registry keys, and the companion's own `remove` command unregisters start-up).

---

## 10. Privacy rules: what may never be logged or stored in clear

These come from the doc comments in `crash.rs`, `applog.rs`, `conn_log.rs`, `files.rs` and `docs/CRASH-REPORTS.md`. They are rules for **every** writer (breadcrumbs, connection log, host log, launcher log, remote log, crash reports).

**Never write:**

- Keys of any kind (device keys, SteamGridDB key, Sunshine password), PINs (the Moonlight pairing PIN is deliberately "not written down"), tokens, signatures, nonces.
- Anything the player typed (search terms, names, titles they entered, paths they chose).
- Paths of saves, contents of saves, or file contents.
- Request or response bodies and headers of the link.
- IP addresses (the connection log does not write addresses at all), Windows account names.
- Computer names (see below).

**Always allowed:** plain words, short ids (game ids, desktop ids), counts, durations, HTTP status and path (path only, for the connection log), phases and outcomes.

**What the code does to help (best effort, never a guarantee):**

| Helper | Does | Where |
| --- | --- | --- |
| `crash::scrub(text)` | IPv4 -> `x.x.x.x`; IPv6 -> `x:x:x:x` (words like `power::rest` stay); `C:\Users\<name>\...` -> `C:\Users\<user>\...` | applied to every crash report part (summary, detail, state, every note) |
| `applog::one_line` | Flattens to one line, drops control characters, collapses whitespace, runs `scrub`, cuts to 400 characters | every `Log::line` |
| `conn_log::know_name` + `applog::hide_words` | Registers computer names (the Ally's `COMPUTERNAME` and each desktop's name, minimum 3 characters) and replaces them case-insensitively with `<pc>` | `conn_log::line` |
| `Repeats::admit` | A repeating event is written at most once per interval (60 s) with a "left out N" count | `conn_log` |
| `Crash::read` | Accepts only report names `parse_name` produces | crash report viewer |

Developer checklist when you add a `crash::note`, `conn_log::line` or `Files::host` line:

1. Does it contain any value the player typed, a path, a key or a token? Remove it.
2. Could it contain a desktop's own error text? That text may include its computer name: route it through `conn_log::line` (which hides known names) rather than writing it raw.
3. Is it in a hot loop? Use `Repeats` or write only on change.
4. Does the interface or a doc claim what is "never written"? Update `docs/CRASH-REPORTS.md` and the module doc comment if you change it.

`remote.log` holds the desktop's stored name, which the scrubber does not see. **UNVERIFIED:** whether this is acceptable for sharing a copy of `remote.log`; today the Ally has no "copy remote.log" action in the code searched.

---

## 11. Backup, restore, update and uninstall

### 11.1 What a backup of an Ally consists of

To preserve a player's CarlaOS state, copy `%LOCALAPPDATA%\dev.carlaos.shell\` (with CarlaOS closed). Then:

| Item | Survives copy to the **same** Windows account | Survives copy to a **different** account or machine |
| --- | --- | --- |
| Games, installs, favourites, play time, consoles, settings rows | Yes | Yes, but `installs.install_path`/`exe`/`args` and picture paths are absolute and may point nowhere; a scan on the new machine rewrites installs |
| Linked desktops' rows (`hosts`) | Yes | Rows yes, **keys no** (DPAPI), so desktops must be linked again |
| SteamGridDB key | Yes | No |
| `admin.json` PIN | Yes | Yes (hash, not sealed) |
| Pictures in `art\` | Yes | Files yes; paths stored in the database are absolute |
| Save backups in `saves\` | Yes | Yes |
| `cleanup\journal.json` | Yes | **No meaning**: it describes this machine's Windows changes; do not carry it |

No automated export/import exists. **UNVERIFIED:** any other backup feature; none was found in `apps/shell/src-tauri/src`.

### 11.2 Update, going back, uninstall (documented behaviour)

From `docs/RELEASES.md` and the installers (`packaging/`):

- **Update or reinstall of the Ally** installs the new program over the old; the data folder is separate and is left alone. **Going back** to an older version reads whatever a newer one wrote. Additive column migrations (section 4) make this likely to work but it "has not been tried".
- **Desktop host installer** never touches `%LOCALAPPDATA%\CarlaOS\companion`. Uninstalling removes the program, its shortcuts and registry entries and tells the companion to stop starting with Windows.
- **Ally uninstaller.** The Ally installer is Tauri's NSIS bundle (`bundle.targets = ["nsis"]`, `installMode: currentUser`, no custom template found in `packaging/`). `docs/RELEASES.md` states the library, settings and linked desktops are kept when uninstalling. **UNVERIFIED:** whether Tauri's default NSIS uninstaller offers an option to delete the application data folder and what its default is; confirm by running the installed uninstaller interactively on a throwaway machine.
- **What CarlaOS leaves behind in Windows** if the player simply deletes the program: the `Run\CarlaOS` registry value (points at a missing exe, harmless), possibly `Explorer\Serialize\StartupDelayInMSec = 0` (if CarlaOS added it; `boot.json` remembers), and the data folder. Turn "start with Windows" off in Settings before uninstalling to undo these (`boot::set(.., false)` removes the Run value and restores the delay).
- **Windows clean-up changes** the administrator applied (`cleanup\journal.json`) remain applied after uninstall; undo them in the app first.

### 11.3 Deleting the catalogue deliberately

Deleting `library.sqlite` (and its `-wal`/`-shm`) while CarlaOS is closed gives a fresh catalogue on next start: scans repopulate installed games; favourites, play time, linked desktops, consoles, per-game power profiles and the SteamGridDB key are lost. `admin.json`, art and save backups are not part of the database and remain.

---

## 12. How to add a new stored thing: decision guide

| You need to store... | Use | Why |
| --- | --- | --- |
| A property of a title that the interface lists or sorts | A new `games` column (section 4.2) | Listed in one query; migrates automatically |
| A per-place fact of a title | A new `installs` column | Replaced at each scan; make sure `sync` does not wipe what you need (it deletes and re-inserts `installs`, so only scanner-derivable data belongs there) |
| A per-(game, desktop) fact that must survive scans | A new table keyed `(game_id, host_id)` like `save_sync`/`save_clean` | Not touched by `sync` |
| A small global preference or per-game choice | A `settings` row, absent = default | No migration, no schema change |
| A secret | `secret::seal` bytes in a BLOB or `settings` row | Section 8 |
| A cache of a remote answer | A table like `achievement_cache` (`json`, `at`) | Add an expiry rule yourself, none exists |
| A file larger than a few KB | A file under an existing folder (7.3) or a new folder under `data_dir`; store only its path/name in the database | Keeps SQLite small |
| A diagnostic | `crash::note` or `conn_log::line`, obeying section 10 | Bounded files, scrubbed |

Always also: add a test, update this chapter, and check if `docs/*.md` for that feature should change. A change to the SQLite schema is a shared-interface change; the repository rules in `CLAUDE.md` say to stop and get approval before doing it.

---

## 13. Open questions / needs investigation

1. **In-memory fallback is silent.** `Library::open` hides a failed open of `library.sqlite`. Should the damaged file be renamed to `library.sqlite.bad` and the player told? Confirm what the interface does today (search `apps/shell/src` for any notice).
2. **No schema version.** Without `PRAGMA user_version` an older program cannot refuse a catalogue that a much newer one rewrote. Is "go back" supported at all, and for how many versions? `docs/RELEASES.md` says it has not been tried.
3. **Absolute picture paths** in `games.cover/hero/*_own` break if the data folder moves or a picture file is deleted. Is there a cleanup/re-fetch path? (`artless` re-looks only for NULL columns, not for missing files.)
4. **`remove_host` leaves** `save_clean`, `play_sessions` (per desktop place), `art\desktops\` files and orphan `games` rows. Intentional?
5. **`achievement_cache` and `save_clean` never expire.** Are there size or age limits desired?
6. **`remote.log`** is deleted at 256 KB rather than trimmed, and holds a desktop name that is not scrubbed.
7. **Tauri NSIS uninstaller and the data folder.** Confirm whether it removes `%LOCALAPPDATA%\dev.carlaos.shell` and under what choice (see 11.2).
8. **WebView2 profile location** under the identifier folder and its size (1.1).
9. **Update downloads in `%TEMP%\CarlaOS-update`:** are they deleted after install? (7.8)
10. **`cloud-browser` profile:** which browser executable and is the profile ever cleaned? (7.7)
11. **Two consoles with the same name** share one save folder (3.1). Check `consoles.rs` for a duplicate-name guard.
12. Exact `source_id` forms for `epic` and `manual` on a desktop were taken from code comments and tests, not from a live run.
13. **Desktop host file list completeness** (section 9) should be reconciled with `docs/dev/03-desktop-host.md`.
