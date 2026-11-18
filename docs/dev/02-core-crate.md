# 02 · The core crate (`carla-core`)

`crates/core` is the library both programs link: the Ally shell (`apps/shell/src-tauri`, package
`carlaos-shell`) and the desktop companion (`crates/companion`, package `carla-companion`). Its
module comment says why: "a desktop describes itself and its games in exactly the shape the Ally
uses locally". Anything that must mean the same on both machines, or that is pure decision logic
that can be tested without Windows, belongs here.

This chapter is a map and a rulebook. It is written from the code; where the code does not settle a
claim it says **UNVERIFIED:** and how to confirm it. For the network protocol see
[06](06-link-protocol.md); for the programs that use this crate see [03](03-desktop-host.md) and
[04](04-ally-native.md); for storage layout [07](07-data-and-storage.md); recipes in
[09](09-how-to-recipes.md). Product docs for behaviour: [LIBRARY.md](../LIBRARY.md),
[SAVE-SYNC.md](../SAVE-SYNC.md), [CLEANUP.md](../CLEANUP.md), [POWER.md](../POWER.md) and the other
`docs/*.md`.

---

## 1. Purpose, boundaries and who uses what

**Design rules that hold across the crate** (observed, repeated in module comments):

1. **Decide here, act elsewhere.** Many modules "only decide" (`power`, `presence`, `quick`, `nap`,
   `boot`'s guard, `display::choose`, `link::quality`, `link::check`) and leave reading hardware or
   starting processes to the shell/companion. This keeps the logic unit-testable on any OS.
2. **Windows-only operations have a non-Windows stub** (`#[cfg(windows)]` / `#[cfg(not(windows))]`
   inner `mod os`/`mod imp`), so the crate builds and its tests run on Linux/macOS CI and dev
   machines (section 9).
3. **Never trust input from the other machine or from the UI.** Paths in save sync are re-checked
   (`sync::inside`), install requests carry an id, not a path, debloat takes ids and works out the
   operations itself.
4. **Sentences, not codes.** User-facing failures are `Result<T, String>` with a complete sentence
   written for the player or administrator (section 8).
5. **No network except where a module says so**, and then only to named places (GitHub releases,
   Steam store, Wikipedia, libretro, SteamGridDB, the linked desktop). The hard product boundary
   (CLAUDE.md): CarlaOS launches installed games only; it never downloads games/ROMs/firmware/keys
   or bypasses DRM/anti-cheat. Modules `emulation`, `downloads`, `art`, `meta` are written to respect
   this (they only start installed programs, ask a store to install an account's own game, or fetch
   pictures and text).

**Who uses which modules** (from `use carla_core::...` in each program; counted by file):

| Module | Shell | Companion | Note |
| --- | --- | --- | --- |
| `library`, `link` | yes | yes | shared data model, wire types, scanners |
| `display` | 1 file | 7 files | desktop switches its screen for a stream |
| `layout` | - | 2 files | virtual screens (companion `virtual_screen.rs`) |
| `remote` | 1 | 3 | command envelope for administering an Ally |
| `power` | 3 | 1 | |
| `secret`, `update`, `applog` | yes | yes | |
| `admin`, `boot`, `crash`, `debloat`, `nap`, `network`, `presence`, `quick` | yes | - | handheld-only. **UNVERIFIED:** `crash` in the companion; the grep found only shell files, confirm with `grep -rn "carla_core::crash" crates/companion` |

Workspace facts (`Cargo.toml`): members `crates/core`, `crates/companion`, `apps/shell/src-tauri`;
`default-members` excludes the shell (it needs WebView2/webkit2gtk), so plain `cargo test` at the root
covers core and companion only. `panic = "abort"` in the release profile (relevant to `crash`).
Dependencies of core (`crates/core/Cargo.toml`): `serde`, `serde_json`, `sha2`, `hmac`, `spake2`,
`getrandom`, `ureq` (rustls), `rusqlite` (bundled SQLite), `sysinfo`; on Windows also `winreg` and
`windows-sys` (features `Win32_Devices_Display`, `Win32_Foundation`, `Win32_Graphics_Gdi`,
`Win32_System_Com`, `Win32_UI_Shell`). `secret` additionally links `crypt32`/`kernel32` directly.

Crate-level items in `crates/core/src/lib.rs`:

| Item | Meaning |
| --- | --- |
| `VERSION` | `CARGO_PKG_VERSION`; the release number is stamped in at build (`docs/RELEASES.md`) |
| `API_VERSION: u32 = 1` | version of the companion HTTP API (`/v1/...`) |
| `Role {Shell, Companion}` | snake_case enum for which program speaks |
| `AppInfo {role, version, api_version, os, arch}` + `AppInfo::current(role)` | self description; test `app_info_round_trips_as_json` |

---

## 2. Module map

```
crates/core/src
  lib.rs            VERSION, API_VERSION, Role, AppInfo
  admin.rs          administrator PIN lock (Ally)
  applog.rs         bounded plain-text log files (both)
  boot.rs           start with Windows + stand-down guard (Ally)
  crash.rs          breadcrumbs, panic/unexpected-exit/hang reports (Ally)
  display.rs        screen modes, choose/plan, switch for a stream (both; Windows API)
  layout.rs         which screens are on, virtual screen layout (companion; Windows API)
  nap.rs            detect machine sleep from clock gaps (Ally)
  network.rs        current Wi-Fi via netsh (Ally)
  power.rs          power/screen profile per game, ASUS request bytes (Ally, decides only)
  presence.rs       recognise a game CarlaOS did not start (Ally, decides only)
  quick.rs          controller chord and presses for the quick menu (Ally, decides only)
  remote.rs         Command/Done envelope + catalogue ops for remote admin
  secret.rs         seal/open secrets (DPAPI on Windows)
  update.rs         GitHub release check, download, checksum, run installer
  debloat/          Windows clean-up engine (Ally, Windows-only effect)
    mod.rs catalogue.rs classify.rs journal.rs model.rs powershell.rs helper.ps1 tests.rs
  library/          the game catalogue and everything feeding it
    mod.rs store.rs                       model types, SQLite catalogue (Store)
    steam.rs vdf.rs bvdf.rs owned.rs      Steam scanning and the account's library
    epic.rs gog.rs                        Epic and GOG scanners
    emulation.rs ports.rs shortcut.rs pe.rs find.rs discover.rs   non-store games
    cloud.rs                              cloud gaming links
    downloads.rs                          asking Steam to install, following downloads
    launch.rs                             start a game, know when it ended
    saves.rs sync.rs savefind.rs saveplace.rs     saves: backups, sync, discovery, portable places
    achievements.rs                       Steam achievements + CarlaOS milestones
    meta.rs art.rs autoart.rs             public lookups: details and pictures
  link/             Ally <-> desktop protocol and measuring
    mod.rs client.rs sign.rs enrol.rs discover.rs wake.rs
    probe.rs quality.rs health.rs check.rs codec.rs moonlight.rs deadline.rs
  tests/windows_live.rs   integration test of the debloat helper on a real Windows (section 11)
```

The following sections describe every module. "Tests" lists the unit tests that exist (names
paraphrase the `#[test]` function names in each file) and what they prove.

---

## 3. Data structures that cross crate or process boundaries

These are the shapes other code depends on. Changing one is a change to a shared interface (wire
format, SQLite schema or UI JSON), which CLAUDE.md says to stop and report on.

| Type | Defined in | Crosses | Notes |
| --- | --- | --- | --- |
| `Scanned` | `library/mod.rs` | scanners -> `Store::sync`; companion -> `RemoteGame` | One installed game as a scanner found it: `source_id`, `title`, `install_path`, `cover`, `hero`, `exe`, `args`, `platform`, `emulated`, `update`, `description`. `Default` is derived; build with `..Scanned::default()`. For a store that starts games through an address (Epic) `args` holds that address. |
| `Offered` | `library/mod.rs` | companion/`owned` -> `Store::sync_available` | A game the account owns but is not installed on a machine. |
| `Install` | `library/mod.rs` | `Store::install` -> `launch::start` | How to start one install: `game_id`, `title`, `source`, `source_id`, `install_path`, `exe`, `args`. **Name clash:** `link::Install{game_id}` is the *wire request* to install a game on a desktop; they are unrelated types. |
| `GameView` | `library/mod.rs` | `Store::list` -> Tauri -> React (serde `Serialize`) | One title as the UI shows it: `id`, `title`, `description`, `platform`, `kind`, `places`, `installable`, `updates: Vec<Waiting>`, `playtime_minutes`, `last_played`, `added`, `favourite`, `cover`, `hero`, `manual`, `origin`, `own_art`. The field names are the contract with `apps/shell/src` TypeScript. |
| `HostRow` | `library/mod.rs` | `Store::hosts`/`save_host` <-> shell | A linked desktop: `id` (also the *place name* of its installs), `name`, `address`, `port`, `device_id`, `key` (**sealed** bytes), `hardware`, `last_seen`, `addresses`, `macs`. Admin permission, quality setting, last stream and last health are separate columns accessed by `host_admin`, `host_quality`, `host_health`, `host_streamed`. |
| `Kind` | `library/mod.rs` | catalogue + UI | `Game`, `Emulated`, `Port`, `Homebrew`; stored as text via `as_str`/`parse` (unknown text parses to `Game`). |
| `Update`, `UpdateState` | `library/mod.rs` | store scanners -> `RemoteGame.update` -> UI | A store's pending update: `Needed|Running|Paused`, `bytes`, `done`. `percent()`, `left()` produce display values. |
| `ManualGame` | `library/mod.rs` | UI -> `Store::add_manual` (`Deserialize`) | A game added by pointing at its program. |
| `Synced`, `Streamed` | `library/mod.rs` | store rows | Last save-sync outcome per (game, host); the picture a stream got (`Streamed{choice: quality::Choice, at}`). |
| `Manifest`, `Entry`, `Agreed`, `Decision`, `Way` | `library/sync.rs` | **wire** (`SavesFound.manifest`, retire body) and store | List of save files with SHA-256 each. `Manifest::digest()` is the whole-folder fingerprint (path + hash, times ignored); `sound()` is the validation every received manifest must pass. |
| `SavePlace`, `SaveRoot` | `library/saveplace.rs` | **wire** (`SavesFind.place`, `SavesFound.place`) and `games.save_place` JSON | Portable description of a save folder. `SaveRoot` has `#[serde(other)] Unknown`. |
| `Achievement` | `library/achievements.rs` | wire (`GET /v1/achievements/..`) and cache | `unlocked: Option<i64>` (`Some(0)` = earned, time unknown). |
| `Download` | `library/downloads.rs` | wire (`GET /v1/downloads`) | Steam fetch progress. |
| `Console` | `library/emulation.rs` | store + UI | An emulator + a folder of the administrator's own game files. |
| `Hello`, `Status`, `RemoteGame`, `RemoteLibrary`, `Prepare`, `Pair`, `SavesFind`, `SavesFound`, `SaveFile`, `Retired`, `Problem`, ... | `link/mod.rs` | **wire** | See [06](06-link-protocol.md). |
| `Pulse`, `Health`, `Overview`, `AllyView`, `Asking` | `link/health.rs` | wire + companion JSON files + dashboard | Everything is whole numbers in named units. |
| `Measure`, `Choice`, `Level`, `Setting` | `link/quality.rs` | store (`hosts.quality`, `streamed`), UI | The picture decision. |
| `Command`, `Done` | `remote.rs` | wire (`admin/next`, `admin/done`) | Remote administration envelope. |
| `Mode` | `display.rs` | wire (`FitAsk.screen`, `Prepare.screen`, `Pulse.screen`) | `{width, height, hz}`; `hz = 0` when unknown. |
| `Planned`, `Item`, `Op`, `Before`, `Entry`, `Tier`, `Kind` | `debloat/*` | UI JSON, helper script JSON, journal file | Re-exported from `debloat/mod.rs`. |
| `Available`, `Part` | `update.rs` | UI | A newer release and which half it is for. |
| `ReportInfo` | `crash.rs` | UI | |
| `Status`, `Unlock` (admin) | `admin.rs` | UI (`Serialize`) | `Unlock` is internally tagged (`result`). **Note:** `admin::Status` is a different type from `link::Status`. |

SQLite schema (`library/store.rs` `SCHEMA` + the `added` column list in `Store::prepare`):

| Table | Role |
| --- | --- |
| `games` | one row per title: `id` (`<source>:<source_id>`, `manual:<key>`, `emu:...`, cloud ids), `title`, `description`, `kind`, `platform`, `favourite`, `added`, `cover`, `hero`, plus added columns `save_path`, `cover_own`, `hero_own`, `origin`, `art_tried`, `save_place`, `save_place_host`, `description_from` |
| `installs` | places a title can be started from: `(place, source, source_id)` unique; `place` is `"local"` (`library::LOCAL`) or a desktop id; `pending_update` added later |
| `available` | titles a store could install at a place (not installed) |
| `play_sessions` | `started`, `ended`, added `asleep` seconds |
| `achievement_cache`, `consoles`, `save_sync`, `save_clean`, `settings`, `hosts` | as named; `hosts` gained `addresses`, `macs`, `admin`, `quality`, `streamed`, `health` |

Migration convention: `CREATE TABLE IF NOT EXISTS` for new tables; a new column is appended to the
`added` array in `Store::prepare`, which checks `pragma_table_info` and runs
`ALTER TABLE ... ADD COLUMN` if missing. There are no version numbers or destructive migrations;
tests such as `a_catalogue_from_the_first_release_gains_the_new_columns` and
`a_database_from_before_description_from_is_migrated_and_keeps_working` prove old databases open.
The database runs `journal_mode = WAL`, `synchronous = NORMAL`, `foreign_keys = ON`.
Changing a table or column type is a schema change: stop and report (CLAUDE.md).

---

## 4. Top-level modules

### 4.1 `admin.rs` — the administrator lock

* **Responsibility:** keep the player out of settings that can change Windows. Explicitly a guard
  rail, not a security boundary (the PIN file is in the user's profile).
* **API:** `AdminLock::new(path)`, `status() -> Status{pin_set, unlocked}`, `set_pin(pin)`,
  `unlock(pin, now: Instant) -> Unlock{Unlocked|Wrong|Wait{seconds}}`, `lock()`, `require() ->
  Result<(), String>` (`"This needs the administrator PIN."`). `PIN_LENGTH = 4`.
* **Invariants:** PIN is exactly 4 digits; stored salted and hashed (SHA-256, 50 000 rounds, JSON
  `{salt, hash, rounds}`); the first PIN set unlocks; once a PIN exists it cannot be replaced while
  locked; 5 wrong tries pause for 30 s (`FREE_TRIES`, `PAUSE`). Unlocked state is in memory only, so a
  new `AdminLock` on the same file starts locked.
* **Side effects:** reads/writes the PIN file at the given path.
* **Use:** every shell command that changes the system starts with `admin.require()?` (for example
  `hosts_link`, `hosts_remove`). Note: the *desktop* side has no such lock; it relies on being at the
  desktop (dashboard on loopback, console window).
* **Tests:** first PIN unlocks and later needs the PIN; PIN is not in the clear; repeated wrong PINs
  force a pause.

### 4.2 `applog.rs` — bounded log files

* **Responsibility:** a plain-text log "for a program that has no console". One line per event, stamped
  to the millisecond UTC (`stamp()`), long lines cut (400 chars), IPv4/IPv6 addresses and the Windows
  account name hidden through `crash::scrub`. When the file grows past its limit it is cut to its newest
  half.
* **API:** `Log::new(dir, name, max_bytes)`, `line(text)`, `tail(count)`, `text()`, `path()`;
  `hide_words(text, words, with)`; `Repeats::admit(key, now, every) -> Say` (let an event through once
  per interval and count suppressed ones); `merged(logs, count)` (merge several logs by time).
* **Invariants:** callers pass plain words and ids, **never** keys, PINs, tokens or player-typed text.
  Writes are serialised by a process-wide `WRITING` mutex; `Log` is cheap to clone.
* **Side effects:** filesystem only.
* **Tests:** names hidden whatever their case; same event once a minute and counted; merged logs run by
  time; line has ms stamp and is one line; long lines cut; file keeps newest part; `tail` order; parallel
  writers do not tear lines.
* **Used for:** the companion's `host.log` (`Files::host_log`) and the shell's connection log
  (`conn_log.rs`).

### 4.3 `boot.rs` — start with Windows, never be in the way

* **Responsibility:** start CarlaOS at sign-in without replacing the Windows shell; guard against a CarlaOS
  that cannot start.
* **API:** `BOOT_ARG = "--boot"`, `FAILED_STARTS = 2`; `Guard::new(path)`, `begin() -> Start{Proceed|
  StandDown}`, `ready()`, `skipped()`, `take_stood_down() -> bool`, `set_removed_delay`/`removed_delay`;
  free functions `enabled()` and `set(guard, on)`.
* **Invariants:** after two consecutive Windows-started runs that never reached "ready" (library shown),
  the next one stands down once and leaves the desktop. Choosing the desktop at start is not a failure.
  Everything `set(true)` changes is reversed by `set(false)` (the startup-delay value is only removed if
  CarlaOS added it, tracked in the guard's record `removed_delay`).
* **Windows effect (`mod os`, `cfg(windows)`):** `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
  value `CarlaOS` = `"<current exe>" --boot`; and
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Serialize\StartupDelayInMSec = 0` (only if no
  value existed). `enabled()` is true only if the Run value equals *this* program's command.
* **Non-Windows stub:** `enabled()` false, `set()` returns "Starting with the machine is set up on the
  Ally itself.".
* **Tests:** working starts never stand down; after two failed starts the next leaves the desktop alone
  once; choosing the desktop is not a failure. (The registry code itself is not unit-tested.)

### 4.4 `crash.rs` — what a client leaves behind when it dies

* **Responsibility:** diagnosis data that stays on the device until the player/administrator copies it.
  Three kinds: *notes* (bounded breadcrumb trail `breadcrumbs.log`), *panic reports*, *unexpected-exit
  reports* (a `running.json` marker exists while running; if still present at next start and the process
  is gone, a report is written), plus *hang reports* via a `Heartbeat`.
* **API (type `Crash`):** `new(dir, version)`, `note(text)`, `trail(count)`, `begin() -> Option<String>`
  (report name if the previous run ended abnormally), `end()`, `write_report`, `write_panic`,
  `write_hang(silent_for, state)`, `install_panic_hook(state_fn)`, `reports() -> Vec<ReportInfo>`,
  `mark_seen()`, `read(name)`; `Heartbeat::{beat, reset, check -> Beat}`; free `utc(at)`, `render(...)`,
  `scrub(text)`.
* **Invariants:** a note is one line, at most 300 chars, scrubbed; the trail is cut to its newest half at
  96 KiB; at most 10 reports are kept (`KEEP`); `read` only reads reports this module wrote. The panic hook
  never waits on the notes lock (`note_nowait`), because the panicking thread may hold it. Another running
  copy keeps the marker (`started while another copy was running`).
* **Side effects:** files in the given directory; process liveness uses the OS process start time
  (`start_time`, `still_running`).
* **Release profile:** `panic = "abort"`, so the hook is the last thing that runs.
* **Tests (17):** silence reported once then recovery; freeze is its own report kind; UTC dates; notes are
  one short plain line; trail bounded; unfinished run reported at next start; panic report content; IPv6
  hidden but ordinary words not; address/account scrubbing; another live copy keeps its marker; dead
  process marker reported; panic hook writes a report that is not repeated; hook never waits; only own
  reports readable; ordering and "announced once"; only the newest kept.
* **Privacy rule:** only plain words and ids in notes: never keys, PINs, tokens, save paths or typed text.

### 4.5 `display.rs` — screen modes for a stream

* **Responsibility:** a stream looks right only when the desktop's screen has the Ally's shape and size,
  so the desktop temporarily switches to the nearest mode and goes back afterwards. Never written to the
  Windows settings: `CDS_FULLSCREEN` makes it last until restored, the program ends or the user signs out.
* **Pure functions:** `Mode{width,height,hz}` (+`words()`), `fit(screen, most)` (size of the picture for
  a screen: its own when no larger than `most`, else the largest of the same shape; sides even),
  `choose(modes, want) -> Option<Mode>` (exact size, else nearest of the same shape; picks a refresh rate
  that shows every picture evenly when it can), `plan(current, modes, want)` (`None` if already right).
* **OS functions (`mod os`):** `current()`, `modes()`, `apply(Mode)`, `restore()`.
  * Windows: `EnumDisplaySettingsW`, `ChangeDisplaySettingsExW` (`unsafe`, each with a `SAFETY:` comment).
    `modes()` deduplicates what Windows lists repeatedly.
  * Non-Windows: stubs (`None`, empty, `Err`, ...).
* **Side effects:** changes the main screen's mode (Windows). Callers (`crates/companion/src/screen.rs`)
  must call `restore()`; the companion has a watcher to put the screen back after a cut-off stream.
* **Tests:** stream made for the screen it is shown on; exact size taken when available; nearest of same
  shape otherwise; refresh rate chosen to show every picture evenly; a screen already right is untouched.

### 4.6 `layout.rs` — which screens are on

* **Responsibility:** give a stream a virtual screen of its own (made by the Virtual Display Driver the
  administrator installs once) and make it the only active screen for the length of the stream, then put
  back "the layout Windows has saved for the screens that are connected".
* **API:** `Screen{name, path, maker, product, active}`, `is_virtual(&Screen)` (matches EDID maker/product
  lists, the device path fragment `#MTT1337#`, or the friendly name "VDD by MTT"), `list()`,
  `only(wanted)`, `restore()`.
* **Windows:** `QueryDisplayConfig`/`SetDisplayConfig` with `QDC_VIRTUAL_MODE_AWARE` (retries on
  `ERROR_INSUFFICIENT_BUFFER`); `only` builds one active path without modes and lets Windows fill them in;
  `restore` uses `SDC_USE_DATABASE_CURRENT`. Non-Windows: empty list / no-op errors.
* **Tests:** only that the driver's screen is recognised. The `SetDisplayConfig` calls are not tested
  automatically (hardware-dependent).
* **Used by:** companion `virtual_screen.rs` (`enter`, watcher).

### 4.7 `nap.rs` — noticing that the machine slept

* **Idea:** a ticker that runs every second and sees far more than a second pass did not run; this holds
  for classic sleep, hibernation and modern standby.
* **API:** `SHORTEST = 8_000` ms, `Nap{...}` with `seconds()`, `Napper::new(now_ms)`, `observe(now_ms) ->
  Option<Nap>` (reports a nap once), `inside(nap, started) -> i64` (the part of a nap inside a session,
  which is subtracted from playtime; stored in `play_sessions.asleep`).
* **Tests:** ordinary ticks are not sleep; a long gap is a nap and reported once; a clock set back is not
  sleep; only the part of a nap inside a game is taken off.

### 4.8 `network.rs` — Wi-Fi name and strength

* `Wifi{ssid, signal}` from `parse(netsh output)`, `wifi()` runs `netsh.exe wlan show interfaces`
  (hidden window, `CREATE_NO_WINDOW`) on Windows; `None` elsewhere.
* **Invariant:** only the language-independent bits are read: the `SSID` line and the one percentage
  value. Names containing a colon work.
* **Tests:** reads network and strength; another language and a name with a colon; no network is `None`.

### 4.9 `power.rs` — how hard the Ally works per game

* **Decides only.** `Power {Quiet, Balanced, Turbo}` (the three ASUS modes: Silent, Performance, Turbo),
  `Screen` speed (`rate(offered)`), `Profile{power, screen}`, `Kind::of(place)` (the way a game is played:
  local, stream, cloud...), `default_profile`, `RESTING = Quiet`.
* **Storage helpers:** `chosen`, `profile`, `choose(store, game_id, kind, profile)` keep the choice per
  (game, kind) in the catalogue `settings` table.
* **Hardware bytes:** `asus_request(power) -> [u8; 16]` and `ASUS_CONTROL = 0x0022_240C` are the request
  and IOCTL for the ASUS ACPI device; `Power::asus()` is the mode value and `windows_overlay()` the
  matching Windows power-mode GUID (as `u128`). Sending them is the shell's job (`apps/shell/src-tauri/src/
  power.rs`). **UNVERIFIED on hardware by this crate:** the ACPI request values are taken as true; only the
  byte layout is tested.
* **Tests:** each game keeps its own choice for each way it is played; screen goes as fast as it can or to
  60; names and the ASUS request.

### 4.10 `presence.rs` — a game CarlaOS did not start

* **Decides only** from a list of running programs the caller supplies. `Candidate::of(&Install)` builds a
  matcher (folder-based for store games, exact program for manual games; emulated games and cloud games
  are left out), `spot(candidates, running)` returns the most exact match (helpers such as `crashhandler`,
  `unins`, `setup`, `redist`, `dotnet`... and folders like `/redist/` are ignored), `Spotter::observe`
  requires `SIGHTINGS = 2` in a row before it reports.
* **Tests:** any program in a store game's folder counts; helpers do not; manual games only by their own
  program; emulator says nothing about which game; most exact wins; cloud/empty folders left out; seen twice.

### 4.11 `quick.rs` — the quick menu's controller logic

* Constants for XInput button bits (`UP=0x0001` ... `MENU=0x0010`, `VIEW=0x0020`, `A=0x1000`, `B=0x2000`),
  `HOLD = 500 ms`.
* `Pad`, `Pad::merged`, `Chord::observe(buttons, now) -> bool` (menu opens once when View+Menu are held
  together), `Presses::opened()` / `observe(pad, now) -> Vec<Press>` (what was held when opening does
  nothing until released; stick = cross), `step(level, up)` (levels move in notches within range).
* **Tests:** opens once; held-at-open does nothing until let go; stick like cross; notches stay in range.

### 4.12 `remote.rs` — remote administration envelope

* `Command{id, op, args}`, `Done{id, ok, output}` with `Done::of(id, Result<Value,String>)` and `error()`,
  `OPS: &[&str]` (the full list: `status`, `games`, `rescan`, `add_game`, `remove_game`, `save_folder`,
  `desktops`, `cleanup_scan`, `cleanup_apply`, `cleanup_undo`, `update`, `restart`, `startup`, `picture`,
  `art_find`, `art_set`, `art_clear`, `art_key`, `add_cloud`, `power`, `check`), `ids(args)`,
  `catalogue(store, op, args, now) -> Option<Result<Value,String>>` (handles the ops that only touch the
  catalogue; `None` means the shell must answer).
* **Invariant:** the Ally only asks desktops its administrator allowed (host `admin` flag set on the Ally
  with the PIN). Unknown ops are refused by `OPS`.
* **Adding an op:** add to `OPS`, handle in `catalogue` if catalogue-only, else in the shell's `remote.rs`.
  Update `docs/REMOTE-ADMIN.md`.
* **Tests:** the catalogue can be looked after from a desktop.

### 4.13 `secret.rs` — sealing keys on disk

* `seal(&[u8]) -> Result<Vec<u8>, String>`, `open(&[u8]) -> Result<Vec<u8>, String>`.
* **Windows:** DPAPI `CryptProtectData`/`CryptUnprotectData` with `CRYPTPROTECT_UI_FORBIDDEN`, tied to the
  Windows account; copying the file to another account/machine yields nothing (`open` error: "A stored key
  could not be read. Link the desktop again."). **Non-Windows:** identity (development/tests only).
* **Used for:** device keys in both programs (`HostRow.key`, companion `Device.key`) and the companion's
  Sunshine password. The same operation is therefore a "must run as the same user" requirement for the
  companion; note that this has operational consequences (e.g. running as a different account fails to
  open keys: the request becomes `401`).
* **Test:** a sealed secret opens again; on Windows sealed != plain and garbage does not open.

### 4.14 `update.rs` — self update from GitHub releases

* **Flow:** `check(part)` (GitHub API `GET .../repos/timotheerm/carlaOS/releases?per_page=30`, 20 s
  timeout, 8 MiB cap; 403/429 -> "slow down" sentence) -> `newest(releases, part, current)` picks the highest
  semantic version with an installer for this `Part` -> `download(update, dir, progress)` fetches
  `SHA256SUMS.txt` first, then the installer, hashing while writing to `<name>.partial`, refuses a
  mismatch, renames on success -> `install(part, installer)` spawns it detached (Ally: `/P /R`; host: `/S`)
  -> `update_now(part, progress)` does all three.
* **Trust rules:** only URLs "from our releases" (`ours()`), no `/` or `\` in the file name, 400 MiB cap,
  checksum from the release is mandatory (no checksum => no install). Drafts, other tags and releases
  without our installer are skipped; a local build (`0.1.0`) counts as older than any release.
* **API:** `Part{Ally, DesktopHost}` with `installer(version)` giving `CarlaOS-Ally-<v>-setup.exe` /
  `CarlaOS-Desktop-Host-<v>-setup.exe`; `Available`; `parse(version) -> (u64,u64,u64)`; `expected_sum`;
  `download_dir()` = `%TEMP%\CarlaOS-update`; `desktop_host_here()` reads
  `HKCU\...\Uninstall\CarlaOS Desktop Host` (`DisplayVersion`, `InstallLocation`) on Windows, `None`
  elsewhere.
* **Side effects:** network; writes a file in the temp dir; **starts the installer**, which replaces the
  caller: callers must exit right after. Installer naming is coupled to the release workflow
  (`docs/RELEASES.md`, `.github/workflows/build.yml`): renaming installers breaks the updater.
* **Tests:** versions are three numbers; picks the highest not the first listed; nothing when up to date or
  ahead; local build older than any release; skips drafts/other tags/missing installers; only our files
  count; reads the checksum file.

---

## 5. `debloat/` — Windows clean-up engine

Purpose: find what can be removed or switched off on the handheld's Windows, apply what the
administrator picks, and undo any of it. Three promises (module comment):

1. **Default-deny.** Only things named in `catalogue` can change; ASUS, Xbox, security, driver and
   controller parts are protected by `KEEP_*` patterns that win over any other rule.
2. **Every change has an exact opposite.** The helper reports the state found just before a change; the
   inverse is written to the journal *before* anything else happens; a change whose previous state cannot be
   restored is not made.
3. **Nothing is decided by the UI.** The UI sends item ids; what they mean is recomputed from a fresh scan.

| File | Responsibility |
| --- | --- |
| `mod.rs` | `Debloat<R: Runner>` engine: `scan() -> Report`, `apply(ids) -> Outcome`, `undo(ids) -> Outcome`; `Runner` trait (`run(Mode, &Value, elevate) -> Result<Value, Error>`); `Error {Unsupported, Declined, Helper(String), Journal(String)}` with sentences |
| `catalogue.rs` | the data: `Tier {Recommended, Optional, Manual, Kept}`, `KEEP_APPS`, `KEEP_SERVICES`, `KEEP_STARTUP`, `APPS`, `SERVICES`, `STARTUP_SOURCES`, `STARTUP`, `SETTINGS`, `PROGRAMS`; `matches(pattern, text)` (case-insensitive `*` wildcard) |
| `classify.rs` | `classify(&Inventory) -> Vec<Planned>`; `Item{id, kind, tier, title, detail, needs_admin}`, `Kind {App, Startup, Service, Setting}`, `Planned{item, ops, store_id}`. `Manual` and `Kept` items never carry operations. Package names must be `[A-Za-z0-9._-]` (`plain_name`) |
| `model.rs` | `Inventory`, `Package`, `Service`, `StartMode`, `StartupEntry`, `StartupLocation`, `RegValue {Dword|String...}`, `SettingValue`, `Program`; `Op` (`AppxRemove`, `AppxRestore`, `ServiceSet`, `StartupDisable`, `StartupRestore`, `RegSet`), `Before`, `Op::needs_admin()` (HKLM writes, services...), `Op::inverse(&Before, store_id) -> Option<Op>`; `Step`, `StepResult`, `ApplyResponse` |
| `journal.rs` | `Journal::load(path)`, `entries()`, `active()`, `record(item_id, title, kind, at, done, undo)`, `mark_undone(seq, at)`, `save()`. Plain JSON so a person (or a remote administrator) can read it. A **damaged journal is reported, never replaced**: it holds the only copy of previous values (`Error::Journal`) |
| `powershell.rs` | `PowerShell::new(dir)` implements `Runner`: writes the request JSON, runs Windows' own `System32\WindowsPowerShell\v1.0\powershell.exe` (`-NoProfile -NonInteractive -ExecutionPolicy Bypass -File helper.ps1 -Mode scan|apply -Request ... -Result ...`), reads the answer JSON. Elevation: `Start-Process -Verb RunAs` via an outer PowerShell; dismissed prompt = exit code 1223 -> `Error::Declined` |
| `helper.ps1` | embedded with `include_str!` and written to the work dir. Two modes: `scan` (read packages, services, startup entries, uninstall list, chosen registry values) and `apply` (carry out ops, report `before`). Compatible with Windows PowerShell 5.1. **Decides nothing** |
| `tests.rs` | engine tests with a fake `Runner` |

How `apply` works (`mod.rs`): scan (fresh), classify, resolve the ids (unknown, protected, `Manual`, or
already-tidy ids fail individually), build `Step`s, run the batch **twice** — first the steps that do not
need administrator rights (`elevate = false`), then those that do (`elevate = true`, one Windows prompt) —
and for each success compute the inverse from `Before` and record it in the journal. `undo` walks the
active journal entries newest first, with the same two-pass elevation. A declined prompt keeps the
machine-wide changes out and the per-user ones in. "A setting made of several values is undone as one."

* **Side effects:** Windows registry (HKCU and HKLM, 64-bit view), services, Appx packages, the startup
  approval records; the work dir (script, request/answer files, `journal.json`).
* **Platform:** `PowerShell::run` returns `Error::Unsupported` when `!cfg!(windows)`. The classification and
  journal logic is pure and tested everywhere with a fake runner.
* **Tests (`tests.rs`):** apply everything then undo restores the machine exactly (fake); per-user and
  machine changes go through separate helpers; protected/unknown/manual ids refused; declining approval keeps
  machine changes out and the rest in; multi-value setting undone as one; created key removed on undo; undoing
  something never applied fails cleanly; a damaged journal stops everything before any change. Plus the
  catalogue's own tests at the bottom of `catalogue.rs` that hold the `KEEP_*` line.
* **To add a changeable thing:** add it to the right table in `catalogue.rs` (never remove a `KEEP_*`
  entry), choose the tier, make sure `Op::inverse` can restore it exactly, and extend tests. Check the
  `windows_live` test (section 11) on a throwaway Windows machine before claiming it works. Product doc:
  [CLEANUP.md](../CLEANUP.md).

---

## 6. `library/` — the game catalogue

Module comment: "One `games` row is one title. One `installs` row is one place that title can be started
from (this machine now; a desktop to stream from later). A card in the interface only ever offers places
where the game really is."

### 6.1 `mod.rs` — shared model and entry points

Types listed in section 3. Functions:

| Function | Meaning |
| --- | --- |
| `LOCAL` | place name `"local"` |
| `STORES = ["steam", "epic", "gog"]` | stores scanned by themselves |
| `scan_store(store, steam_root) -> Vec<Scanned>` | dispatch to `steam::scan`, `epic::scan`, `gog::scan` |
| `update_of(source, source_id, steam_root) -> Option<Update>` | fresh read of a store's pending update, for just before a start |
| `manual_key(exe)` | FNV-1a 64-bit hash of the lower-cased path, 16 hex chars: the stable id suffix of a game added by hand. **Same on every machine**, so ids are `manual:<this>` everywhere |
| `clean_description(text)` / `DESCRIPTION_MAX = 1000` | one clean line, control chars removed |

Catalogue ids: `<source>:<source_id>` for stores (`steam:20`, `epic:...`, `gog:...`), `manual:<key>`,
`emu:<console>-<key>`, and cloud ids. Platform names shown on badges: `Steam`, `Epic`, `GOG`,
`Standalone` (`store.rs platform()`), or `Scanned.platform` when set.

### 6.2 `store.rs` — `Store` (SQLite)

`Store::open(path)` creates the folder and migrates; `Store::in_memory()` for tests. All methods return
`Result<T, String>` with a sentence (`"The game library could not be read or saved: ..."`). Notable
behaviour:

* **`sync(place, source, found, now)`** — in one transaction deletes that (place, source)'s `installs` and
  re-inserts from the scan; `games` rows are upserted, **keeping favourites, playtime and pictures
  chosen by hand**; games no longer found lose their install rows but keep history. Description from a
  host replaces another host's but never one typed on the Ally (`description_from = 'host'`).
* **`sync_available`** — same idea for the "installable" list (`available` table); a game known only this
  way has `added = 0` until first installed somewhere.
* **`add_manual` / `remove_manual` / `add_cloud`** — games added by hand; the id is stable for a program
  so re-adding updates; `remove_manual` only accepts ids starting `manual:` or `cloud:` (else "Only games added by hand can be removed here."). A manual game that a *desktop* lists is stored as an install of that desktop's place with source `manual`; `GameView.manual` is `false` for it (see test `a_game_added_by_hand_on_a_desktop_is_streamable_but_not_removable_here`), so the UI offers no removal on the Ally and it disappears at the next sync when the desktop drops it.
* **`list()` / `list_all()`** — `GameView`s; `list` hides titles with no install, `list_all` includes
  titles only offered somewhere. Playtime = `SUM(ended - started - asleep)` over finished sessions.
* **Sessions:** `start_session`, `end_session`, `session_slept`, `close_dangling_sessions` (sessions cut short
  count as no time), `sessions(game_id)`.
* **Pictures:** `set_cover_if_none`, `set_art_if_none`, `set_own_art` (wins over store art and survives a
  rescan), `artless(before)`, `art_tried`, `forget_art_tried`, `own_art`.
* **Hosts:** `save_host`, `hosts`, `remove_host`, `host_seen`, `host_admin/set_host_admin`,
  `host_quality/set_host_quality`, `host_health/set_host_health`, `set_host_streamed`.
* **Saves:** `save_path/set_save_path`, `save_place/set_save_place/learn_save_place` (only the desktop that
  taught a place may change it), `save_folder_of/set_save_folder_of`, `save_title_of`, `synced/set_synced/
  clear_synced`, `keep_clean/set_keep_clean`.
* **Misc:** `consoles/save_console/remove_console`, `setting/set_setting` (opaque blobs, used for sealed
  secrets and preferences), `cache_achievements/cached_achievements`, `local_installs`, `install(game_id,
  place) -> Install`, `mend_shortcuts` (points games added as a `.lnk` at the program behind it),
  `known_programs`.
* **Thread-safety:** `Store` wraps one `rusqlite::Connection` and is `&mut` for writes; the shell wraps it
  in a mutex (see [04](04-ally-native.md)). Do not hold that lock across network calls
  (CLAUDE.md-level pitfall; see also [01](01-architecture-and-runtime.md)).
* **Tests (20):** time asleep is not played; coverless/artless listing; rescan keeps favourites and playtime;
  library games offered where they could be installed; `keep_clean` survives reopening and old DBs migrate;
  save place round trip and bad values read as none; emulated games share one folder per console; update
  state per place; migrations from first release / before `description_from`; empty description does not wipe;
  manual add/update/remove; own pictures win; shortcut-added game is pointed at its program; cloud games;
  dangling sessions; a desktop adds/removes a place to play; consoles; games from other stores keep how they
  start; manual game on a desktop is streamable but not removable here.

### 6.3 Store scanners

**Common contract:** a scanner reads the store's own files (no sign-in, no network), returns
`Vec<Scanned>` for *installed* games, and leaves out add-ons, tools and half installs. Called through
`scan_store` or directly by the companion (`crates/companion/src/server.rs` `Found = (&'static str, Scanned)`).
A scanner must never panic on a damaged file: unreadable files give an empty/partial list.

| Module | Reads | Notes |
| --- | --- | --- |
| `steam.rs` | `steamapps/libraryfolders.vdf` for library folders; `appmanifest_<id>.acf` per app; cover/hero from `appcache/librarycache` (layout changed over time; several locations tried) | `find_root()` (Windows registry `HKCU\Software\Valve\Steam\SteamPath`, `HKLM\SOFTWARE\WOW6432Node\Valve\Steam\InstallPath`, then `C:\Program Files (x86)\Steam`; non-Windows `~/.local/share/Steam`, `~/.steam/steam`); `client(root)` = `steam.exe`/`steam.sh`; `Manifest`, `manifests`, `folders`, `scan`, `cover`, `update_of(root, appid)`. `StateFlags` constants (`UPDATE_REQUIRED=2`, `FULLY_INSTALLED=4`, `UPDATE_RUNNING=256`, `UPDATE_PAUSED=512`, `UPDATE_STARTED=1024`) give `Update`. A game being updated stays in the library; a first download does not |
| `vdf.rs` | Valve text KeyValues | `Value`, `parse(&str) -> Option<Value>`; quoted strings, nested blocks, `//` comments; rejects unbalanced input |
| `bvdf.rs` | Valve binary KeyValues (`appcache/stats/*.bin`, appinfo/packageinfo) | `Node`, `parse`, `parse_prefix`; `write` helpers are `pub(crate)` for test fixtures; damaged files refused |
| `owned.rs` | `appcache/packageinfo.vdf` (licensed packages) and `appinfo.vdf` | `App{appid,title,installdir}`, `library(root)`, `not_installed(root)`. Steam's **private, changing formats**: anything unexpected returns `None`/`Err` and the library then simply shows what is installed. Only Windows games, excludes package `0` (everyone), tools |
| `epic.rs` | `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item` JSON | `manifests()` (dir if exists), `scan(dir)`, `update_of(dir, app_name)` (only "being updated" from the `Pending` folder; the launcher never says "an update exists" unless it is working on it). Starting is by `com.epicgames.launcher://apps/...` address built from the manifest (`EPIC_ADDRESS`), validated plain characters |
| `gog.rs` | `HKLM\SOFTWARE\GOG.com\Games\<id>` (32-bit view): `gameName`, `path`, `exe`, `workingDir`, `launchParam`, `dependsOn` | `entries()` (Windows; empty elsewhere), `games(entries)` (pure, tested): keeps numeric ids with a real exe, drops add-ons (`dependsOn` set), dedups, sorts by title; GOG games start directly with no launcher |

### 6.4 Non-store games

| Module | Responsibility | Key API and invariants |
| --- | --- | --- |
| `emulation.rs` | A "console" = an emulator + a folder of the administrator's **own** game files | `Console{id,name,emulator,options,folder,endings}`, `GAME = "{game}"` placeholder in `options`; `suggest(emulator)`/`known_start` match well-known emulators by file name (Dolphin, PCSX2, DuckStation, PPSSPP, mGBA, melonDS, Snes9x, Mesen, Flycast, xemu, Cemu...); `checked(console)` validates; `title(file)` tidies names; `scan(&Console) -> Vec<Scanned>` (`emulated = true`, `exe` = emulator, `args` = options with the file). A folder with more than 5000 files is not a folder of games. No emulator, firmware, keys or games are bundled or fetched |
| `ports.rs` | Recognise well-known native ports by program/folder name (`soh.exe` is Ocarina of Time): `recognise(exe) -> Option<Port>`, `suggest_title`, `inspect(path) -> Inspected` (follows shortcuts, reads the program's product name) | Only a list of names; nothing downloaded or started |
| `shortcut.rs` | Parse Windows `.lnk` ("Shell Link" format) to its target, arguments, working dir: `is_shortcut`, `parse(bytes, beside)`, `read(path)` | Reads only the parts that say what to start; refuses truncated input; fills environment variables |
| `pe.rs` | `product_name(path)`: reads the "ProductName" string from a Windows program's version resource without running it | |
| `find.rs` | Suggest programs that may be a game from folders (`find(places, given) -> Vec<Candidate>`, `default_roots()`, `visible`, `add_line`) | `Candidate{title, exe, folder, kind: Game|Launcher|Shortcut, score, note, watch_hint}`; real game beats its launcher; the `watch_hint` tells the tracker which program name to watch because a launcher may exit first. Reads only names and program metadata; nothing starts |
| `discover.rs` | Look for games, ports and emulators nobody told CarlaOS about (`look(&Places, &known) -> Found`, `places()`) | Looks in game folders, desktop and Start-menu shortcuts, known install places. Adds nothing itself |
| `cloud.rs` | Cloud gaming links: `Service` list (GeForce NOW, Xbox Cloud Gaming, Boosteroid, Amazon Luna) with exactly one allowed host each; `checked(link)`, `suggest_title`, `find_browser()`, `browser_args(profile, link)` | A cloud game is a link; playing opens it in Microsoft Edge in a dedicated profile (full screen). Starting re-checks the link (`launch::start`) |

### 6.5 `downloads.rs` — asking Steam to install

* **Why:** let a desktop's Steam fetch an owned game remotely. Steam does the downloading; CarlaOS only
  asks and reads Steam's own record of each app.
* **Two ways:** the *quiet* way writes a manifest that says "update started" with the folder Steam's own
  files name (`record(app)`, flags `UPDATE_REQUIRED | UPDATE_STARTED`) and restarts Steam so it notices
  (nobody has to answer, which matters on an unattended desktop); the *asking* way hands Steam a
  `steam://install/<id>` address and shows Steam's own window that someone must answer.
* **API:** `Download{source, source_id, title, phase: Queued|Downloading|Paused|Finishing, done, total,
  update}`, `of(manifest, fetched)`, `steam(root) -> Vec<Download>`, `free_bytes(folder)`, `roomiest(root)`,
  `Started {Quiet, Already, AskedSteam}`, `Machine{restart, open}` (injected so tests do not need Steam),
  `start(root, app, machine)`, `start_here(root, app)` (real), `restart_steam`, `keep_steam_running`
  (`steam -shutdown` then `steam -silent`), `Footprint`/`Disk`/`footprints`/`disks`, `uninstall`,
  `still_installed`.
* **Side effects:** writes `appmanifest_<id>.acf` in a Steam library folder; starts/stops Steam processes;
  opens `steam://` addresses.
* **Tests:** Steam's record says how far a download is; an install starts quietly when the folder is known
  and by asking otherwise.

### 6.6 `launch.rs` — start a game and know when it ended

* `start(&Install, steam_root) -> io::Result<Option<Child>>` by `install.source`:
  * `steam` -> `steam.exe steam://rungameid/<id>` (returns no child);
  * `epic` -> only an address beginning with `EPIC_ADDRESS` and with no whitespace is opened
    (`rundll32 url.dll,FileProtocolHandler` on Windows, `xdg-open` elsewhere);
  * `cloud` -> link re-validated with `cloud::checked`, Edge launched with its own profile; the browser is
    the watched child;
  * otherwise (`gog`, `manual`, emulated) -> spawn `exe` with `split_args(args)` and a working dir (shortcut
    target followed if the stored exe is a `.lnk`); on Windows errors 740 (needs elevation) or 193 (not a
    program) fall back to `Start-Process` with the path and options passed through the **environment**
    (`CARLA_START*`), never spliced into a command.
* **Finished-ness:** `Tracker::observe(running, now) -> Phase{Starting, Running, Finished, NeverStarted}`:
  finished only after it was seen and then missed twice in a row; never started after `START_GRACE` = 180 s;
  `still_preparing` restarts the grace while a store is updating, up to `UPDATE_GRACE` = 3 h.
  `Processes` (backed by `sysinfo`) answers "is any program inside this folder running" (`any_inside`),
  `programs`, `end_inside`, `any_named`/`end_named` for `watch_name`-validated names (`valid_watch` allows only a
  plain game program name). `Watch` ties an `Install`, optional child and a watch-name together and exposes
  `poll()`, `end()`, `adopted(install)` (a game CarlaOS did not start, see `presence`).
* **Why folder-based:** asking Steam to start returns at once and many games start through a launcher that
  exits; so a game "runs" while any process whose image lives inside its folder runs.
* **Tests (14):** invalid inherited handles do not stop starting; a program of the game's name anywhere counts;
  only plain program names may be watched/closed; closing by name ends only the named program; argument
  splitting like a shortcut; Epic only through the launcher's address; finish needs seen-then-missed-twice;
  never-appears gives up; a store update is waited for; folder detection finds this test program; a game that
  still names its shortcut starts the program behind it; started program watched until it exits; cloud game only
  opens a service page; a game the player ends is gone at once. (Windows-specific tests use `cmd.exe`/`ping`.)

### 6.7 Saves: `saves.rs`, `sync.rs`, `savefind.rs`, `saveplace.rs`

* **`saves.rs`** — backups. CarlaOS never edits a save; it copies the whole save folder aside after a play
  session that changed it, keeps the last `KEEP = 5` copies and `KEEP_RETIRED = 15` for copies taken just
  before a desktop clears itself, and can put one back (a restore first backs up what is there, so it can be
  undone). `Backups::new(store, game_id)` (folder name sanitised to `[A-Za-z0-9-_]`), `list`, `take`,
  `take_retired`, `restore`, `path_of`, `overview`; `measure(dir)` (files, bytes, newest; **symlinks are not
  followed**), `sensible(dir)`, `steam_folder(root, app_id)` (Steam Cloud `remote` folder of the account that
  played last), `LARGEST = 512 MiB`.
* **`sync.rs`** — keep one game's saves identical on two machines. Knows nothing about networks: a `Side` is
  a trait with `manifest/read/write/remove/protect`; `Folder` is the local implementation (writes beside as
  `*.carla-part` and renames so a game never sees a half save; sets mtime; removes emptied parent folders up
  to the save folder; `protect()` takes a safe copy). The Ally's `RemoteSaves` (in `link/client.rs`) is the
  other implementation.
  * **Limits:** `MOST_FILES = 2000`, `LARGEST_FILE = 32 MiB`, `LARGEST = 512 MiB`.
  * **Path safety:** `inside(folder, relative)` only accepts plain relative paths: no `..`, no root/drive,
    no `\`, `:` or NUL, no control chars or `<>"|?*`, no trailing space/dot, at most 400 chars. `Manifest::sound()`
    enforces the same for anything received.
  * **Decision:** `decide(here, there, agreed) -> Decision{way: Same|Send|Fetch, both_changed}` — equal digests
    mean `Same`; an empty side never wipes a non-empty one; if the two last agreed on digests, the side that
    changed wins; otherwise the newer `modified` wins (tie -> this machine, the Ally). `mirror` copies files and
    `bring_in_step(here, there, agreed)` does the whole thing with `protect()` first; `*_for(keep_clean, ...)`,
    `decide_keeping_clean`, `may_stream(keep_clean, ally, desktop)` (hold a stream back only for a desktop known
    to clean itself, when the Ally has saves and the desktop is not known to hold exactly them),
    `gate_on_failure`, `retry_after(Attempt, n, waits)` with `AFTER_STREAM_WAITS = [3, 15, 30, 60]` s.
  * **Tests (11):** retry scheduling (three); stream gate; saves follow the player both ways; both-changed keeps
    the older in a safe copy; nothing reaches outside the save folder; damaged save stops before any removal;
    a self-cleaning desktop is topped up not fetched from; a folder that is no save folder is refused.
* **`savefind.rs`** — suggests where a game keeps its saves; **never names a folder for the sync**: the
  administrator confirms at the desktop (`carla-companion saves set`). `known_roots(env, game_folder)`,
  `Snapshot::take(roots, Limits)`, `diff(before, after, title)`, `guess(title, roots)`, `title_words`. Scoring
  prefers a folder named like a save over logs/cache, keeps slot folders with their parent, is tolerant of
  naming, skips noisy trees, respects limits, does not follow junctions. Tests include Windows-only ones using
  `mklink /J`.
* **`saveplace.rs`** — a **portable** description of a save folder: `SavePlace{root: SaveRoot, rel}` (roots:
  Saved Games, Documents, My Games, Local/LocalLow/Roaming AppData, ProgramData, GameFolder). `Env` holds
  the known folders of one machine (`Env::from_vars(var)` for tests; `Env::real()` uses
  `SHGetKnownFolderPath` on Windows so OneDrive-redirected folders are right). `portable(abs_folder, env,
  game_folder)` (longest root wins), `resolve(place, env, game_folder) -> Option<PathBuf>`, `loads_code(path,
  place)` (folders that load code, e.g. plugin dirs, are refused). It is a hint only: hostile places are
  refused (device names like `CON`, links, reparse points below the root).

### 6.8 `achievements.rs`

`Achievement{id,name,description,hidden,unlocked}`. `from_steam_files(schema, progress)` combines Steam's
own schema and progress binaries; `steam(root, app_id)` finds them for the account that played last;
`milestones(sessions)` computes CarlaOS's own milestones from recorded play sessions (so every game has
some, whatever its store). No Steam web call and no account key.

### 6.9 Public lookups: `meta.rs`, `art.rs`, `autoart.rs`

All three are the only places besides `update` and `link` that use the network, and each says it sends
"only the name searched for".

| Module | Asks | Provides |
| --- | --- | --- |
| `meta.rs` | Steam public store search/details, Wikipedia | `search(term) -> Vec<Match>`, `details(id) -> Details`, `picture(url)`, `cover(id, header)`, `trusted_picture(url)` (pictures only from the places CarlaOS asks), `picture_kind(bytes)`; limits `LARGEST_PICTURE = 8 MiB`, `WIKI_WIDTH = 640`; search terms are URL-encoded and ids must be digits |
| `art.rs` | Steam, Wikipedia, libretro box art, SteamGridDB (needs an optional free key) | `find(&Wanted) -> Found` listing up to `MOST = 30` candidates (`Candidate`, `Shape`) for the **administrator to pick**; `systems(console)`, `grid_for_steam`, `key_shape` |
| `autoart.rs` | the same places | gives games with no picture one automatically: `plan`, `search`, `fill`; only takes a picture when the source calls the game by the same name (`same_game`); retries a game with nothing found only after `TRY_AGAIN_AFTER` = 7 days; hand-chosen pictures always win |

Tests exist for parsers and decisions (no live network in tests).

---

## 7. `link/` — the Ally ↔ desktop protocol and measuring

The wire protocol is documented in full in [06](06-link-protocol.md). Module summary:

| Module | Responsibility |
| --- | --- |
| `mod.rs` | `PORT = 47600`, `SUNSHINE_APP = "CarlaOS"` (the single Sunshine application every stream goes through), wire types (`Hello`, `State`, `Status`, `FitAsk`, `ScreenFit`, `Updating`, `RemoteGame`, `RemoteLibrary`, `Install`, `Installing`, `Enrol*`, `Prepare`, `Pair`, `SavesFind`, `SavesFound`, `SaveFile`, `Retired`, `Problem`), helpers `hex`, `unhex` (crate-private), `random(len)` and `random_hex(len)` from `getrandom` (panics only if the OS has no random source) |
| `sign.rs` | HMAC-SHA256 `request`/`request_ok`/`reply`/`reply_ok`, header names, `WINDOW = 300`, `Replay::fresh` |
| `enrol.rs` | SPAKE2 `Exchange`, `Shared` (`proof`, `proof_ok`, `check`, `device_key`), `commit`, code helpers and lifetimes |
| `client.rs` | `Link` (the Ally's handle to one desktop), `hello`, `enrol`, `enrol_ask`/`Asking`, `port_open`, `set_observer`, `RemoteSaves`, `RetireAnswer`; error type `Error {Unreachable, Refused, Host(String), Untrusted}` |
| `discover.rs` | `search` (UDP broadcast), `answer` (companion's loop: hello + echo) |
| `wake.rs` | `parse_mac`, `magic_packet`, `wake(macs)` (UDP broadcast to ports 9 and 7, twice), `is_tailscale` (`100.64.0.0/10`), `usable`, `candidates(last, reported)` (best-first address list) |
| `probe.rs` | UDP echo and burst (`echo`, `echo_on`, `Echo`, `summarise`, `plan`, `send_burst`, `receive_burst`, `BurstAsk/Plan`, `Burst`), `PACKET = 1200` |
| `quality.rs` | `Level`, `Setting`, `Measure`, `Choice`, `Depth`, `Line` trait, `survey`, `measure`, `choose`, `LARGEST_PROBE`, `ms_words` |
| `health.rs` | `Grade`, `Route`, `PictureOut`, `Health` (+`Health::of`, `grade`), `Pulse`, `AllyView`, `Asking`, `Overview`, `CONNECTED_FOR = 90` |
| `check.rs` | `steps(&Facts) -> Vec<Step>`, `Report`, `Outcome {Ok, Warn, Fail, Skipped}`, `Fix {Pair, Wake, Update, Relink, Moonlight}`: walks "desktop answers -> still accepts this Ally -> Sunshine -> Moonlight installed and paired -> ports -> connection carries a picture -> screen fit -> codec", names the first broken link and the one repair CarlaOS can do. Pure: the caller gathers `Facts` |
| `codec.rs` | `Codec {H264, Hevc, Av1}` ordering; `encodes(gpu)`, `decodes(gpu)` guesses from the chip's name; `fit(desktop, ally) -> Fit` (what the two will settle on: a well-informed guess, CarlaOS does not choose); `graphics()` reads display adapter `DriverDesc` values from `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-...}` on Windows (empty elsewhere) |
| `moonlight.rs` | Driving **stock Moonlight** by command line: `find()` (`CARLA_MOONLIGHT` override, then `Program Files\Moonlight Game Streaming\Moonlight.exe`...), `new_pin()`, `stream_args(address, Picture)` (`stream <addr> CarlaOS --resolution WxH --fps N --bitrate K --display-mode borderless --no-quit-after --keep-awake --[no-]frame-pacing`), `stream`, `pair`, `paired`, `finished(child, limit)`. Moonlight is never bundled or modified |
| `deadline.rs` | `lock_within(&Mutex, wait)` and `collect_until(jobs, limit)`: wait for several jobs but never longer than a limit; late jobs finish on their own thread and are dropped. Used so one unreachable desktop cannot stall the interface |

* **Side effects:** network (TCP to the desktop, UDP to the desktop, UDP broadcast, TCP connect to Sunshine
  ports); `moonlight` spawns processes; `codec::graphics` reads the registry.
* **Tests:** `sign.rs` (3), `enrol.rs` (5), `client.rs` (6, with a tiny one-shot desktop), `discover.rs`
  (end-to-end on loopback incl. echo), `quality.rs`, `health.rs`, `check.rs`, `probe.rs`, `codec.rs`,
  `wake.rs`, `deadline.rs`, plus `description_tests` in `mod.rs` (old desktop has no description and none is
  sent when empty). End-to-end server tests are in `crates/companion/src/server.rs`.

---

## 8. Error-handling conventions

| Layer | Convention |
| --- | --- |
| Most modules (`library`, `update`, `secret`, `saves`, `sync`, `admin`, `display`...) | `Result<T, String>` where the `String` is a **complete sentence for a person**, in plain words (often with a hint: "Is the game still running?"). Callers show it as is; do not wrap in codes or add prefixes. |
| `link::client` | `Error {Unreachable, Refused, Host(String), Untrusted}` with `Display`; `Host` carries the desktop's own sentence from `Problem.error`. Matching on text is only done for `enrol::WAITING`. |
| `debloat` | `debloat::Error {Unsupported, Declined, Helper, Journal}` with `Display` sentences; item-level failures are returned inside `Outcome`, not as `Err`. |
| "Not found" | `Option<T>` for lookups that can legitimately find nothing (`scan_store` returns an empty `Vec` rather than `Err`; `epic::manifests() -> Option`). |
| Panics | Avoided in library paths; `expect("...serialisable")`/`expect("HMAC takes a key of any length")` only for conditions that cannot happen. In release the crate aborts on panic (profile), and the shell installs a panic hook through `crash`. |
| Untrusted data | Damaged files (VDF, binary VDF, manifests, journal) are **refused**, not repaired and not trusted (`None`/`Err`); a damaged debloat journal stops everything. |
| Logging | The core does not log by itself. Callers log plain words and ids through `applog`/`crash::note`; never secrets or typed text. |
| Concurrency | Library code is synchronous and uses `std::sync::Mutex` with poison tolerance (`unwrap_or_else(|e| e.into_inner())`) where static state exists (`client::OBSERVER`, `crash::WRITING`, `applog::WRITING`). Anything that may block (network, disk scans) must be called off the UI thread by the caller (Tauri `#[tauri::command(async)]`). |

---

## 9. Windows-only parts and how they are stubbed

The crate builds and tests on any OS because each OS-specific piece has a `cfg(not(windows))` twin.

| Module | Windows implementation | Elsewhere |
| --- | --- | --- |
| `boot` | `HKCU` Run key and Serialize delay value (`winreg`) | `enabled()` false; `set()` returns an error sentence |
| `display` | `EnumDisplaySettingsW`, `ChangeDisplaySettingsExW` (`windows-sys`, `unsafe`) | `current()` `None`, `modes()` empty, `apply/restore` `Err` |
| `layout` | `QueryDisplayConfig`, `SetDisplayConfig`, `DisplayConfigGetDeviceInfo` | empty list, `Err` |
| `secret` | `CryptProtectData`/`CryptUnprotectData` (`crypt32`) | identity transformation |
| `update::desktop_host_here` | `HKCU\...\Uninstall\CarlaOS Desktop Host` | `None` |
| `network::wifi` | `netsh.exe wlan show interfaces` | `None` |
| `library::steam::find_root` | registry + default path | `~/.local/share/Steam`, `~/.steam/steam` |
| `library::gog::entries` | `HKLM\SOFTWARE\GOG.com\Games` | empty |
| `library::saveplace::Env::real` | `SHGetKnownFolderPath` for Documents, Saved Games, AppData | environment variables only |
| `link::codec::graphics` | `HKLM` display class `DriverDesc` | empty |
| `debloat::PowerShell` | runs Windows PowerShell + `helper.ps1` | `Error::Unsupported` |
| `library::launch` | `rundll32`, `powershell.exe Start-Process` fallback, `CREATE_NO_WINDOW` | `xdg-open`, plain spawn |
| `update::install` | detached process flags (`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`) | plain spawn |
| tests | a few use `cmd.exe`, `ping`, `mklink /J` behind `#[cfg(windows)]` | skipped |

Rules when adding Windows-only code: put it in an inner `#[cfg(windows)] mod os` with a
`#[cfg(not(windows))] mod os` twin exposing the same signatures; keep `unsafe` blocks tiny with a `SAFETY:`
comment; keep the decision logic outside the `os` module so it is testable; never claim a Windows effect is
verified from a Linux run (CLAUDE.md "Keep levels apart": designed -> in source and tests -> hardware
verified).

---

## 10. How to add things

### 10.1 A new store scanner (example: "Ubisoft")

1. **File:** `crates/core/src/library/ubisoft.rs`; declare `pub mod ubisoft;` in `library/mod.rs`.
2. **Split pure from OS:** a pure function that turns parsed entries into `Vec<Scanned>` (like
   `gog::games(entries)`), and a thin `#[cfg(windows)]` reader (registry/manifest dir) with a
   `#[cfg(not(windows))]` stub returning empty, so the pure part is tested everywhere with fixtures.
3. **Fill `Scanned`:** `source_id` (the store's own id, stable), `title`, `install_path` (an existing folder
   when possible), `exe`/`args` if the game starts without a launcher (otherwise `args` holds the launch
   address, validated plain), `cover` if the store caches art, `update` if known. Leave out add-ons, tools,
   half installs, and anything whose program does not exist.
4. **Register:** add the name to `STORES` and to `scan_store`'s `match`. If the store can report updates,
   extend `update_of`. The catalogue id becomes `ubisoft:<source_id>` automatically (`Store::sync`);
   `store.rs platform()` maps the source to the badge text, so add an arm (`"ubisoft" => "Ubisoft"`).
5. **Start it:** `launch::start` has `steam`, `epic`, `cloud`, and a default arm that spawns `exe`. A store
   that needs a launcher address needs its own arm (validate the address like `EPIC_ADDRESS`); a store that
   gives a plain exe works through the default arm.
6. **Both programs:** the shell scans through `scan_store`; the companion has its own scan list
   (`crates/companion/src/main.rs` `installed()` / `server.rs` `Found`) — extend it too or the desktop
   will not list/stream those games. `RemoteGame.source` carries the name; `Server::prepare` builds `Pending`
   from `find(game_id)`: check it handles the new source. See [03](03-desktop-host.md).
7. **Tests:** fixture-based test of the pure function (add-ons dropped, missing exe dropped, duplicates
   removed, sorted) like `gog::keeps_games_whose_program_exists_and_leaves_out_add_ons`; a `Store::sync` test
   if the id/platform mapping is new.
8. **Docs:** `docs/LIBRARY.md` / `docs/STORES-AND-SEARCH.md`; remember the boundary: scan installed games
   only, no sign-in, no downloading.

### 10.2 A new core module

1. Create `crates/core/src/<name>.rs` (or a folder) and add `pub mod <name>;` to `lib.rs` (keep the list
   alphabetical). Start with a `//!` comment that says what it decides/does, what it never does, and where
   the OS part lives.
2. Separate **decision logic** (pure, testable, takes `now`/environment/lists as parameters) from
   **effects** (filesystem, process, registry, network). If the effect is Windows-only, use the `os` twin
   pattern (section 9).
3. Public errors: `Result<T, String>` with a sentence, or a small enum with `Display`.
4. If it persists data: SQLite via `Store` (new table with `IF NOT EXISTS`, new columns appended to
   the `added` list), or a JSON file whose directory is passed in (never hard-code `%APPDATA%` in core:
   callers own paths, which makes tests use a temp dir).
5. If it crosses a boundary (Tauri command, wire route, SQLite schema) it becomes a shared interface: derive
   `Serialize`/`Deserialize` with `#[serde(default)]` on optional fields, then read [06](06-link-protocol.md)
   section 11 first.
6. Tests in the same file. Use `std::env::temp_dir().join(format!("carla-<name>-{}", std::process::id()))`
   style folders (see `admin.rs` tests) and clean them. Inject clocks/lists; do not read the real machine.
7. Dependencies: do not add crates without the user's approval (CLAUDE.md).
8. Wire it up in the shell (`apps/shell/src-tauri/src/*.rs`, a `#[tauri::command]`, registered in `main.rs`)
   and/or the companion; add the UI in `apps/shell/src`. See [04](04-ally-native.md), [05](05-ally-ui.md),
   [09](09-how-to-recipes.md).
9. Run (from a VS developer shell on Windows, see `knowledge/development/known-issues.md` KI-006):
   `cargo test --locked -p carla-core --lib`, then the companion and shell tests if you touched shared types,
   then `git diff --check`. Plain `cargo test` also runs `windows_live` (section 11), which needs approval.

---

## 11. The `windows_live` integration test

File: `crates/core/tests/windows_live.rs`, gated by `#![cfg(windows)]`, so it is empty on other OSes.
It runs the **real** `helper.ps1` through `debloat::PowerShell` against the machine the tests run on.

What it changes on that machine (always restored by the same undo path, and the tests assert the machine
ends as it started):

| Test | Effect |
| --- | --- |
| `scan_reads_this_machine` | read-only: scans and asserts at least some items and more than 50 services, no warnings |
| `registry_values_round_trip_exactly` | creates and overwrites `HKCU\Software\CarlaOS Test <pid>` values (DWORD max/0, a string with an apostrophe), undoes them, checks the key is removed again |
| `a_startup_entry_is_switched_off_and_back_on` | adds a probe value under the HKCU `Run` key, disables it the way Task Manager does (`StartupApproved`), re-enables, then removes it |
| `a_machine_wide_value_goes_through_the_elevated_helper` | writes `HKLM\SOFTWARE\CarlaOS Test <pid>` through the **elevated** helper (triggers a UAC prompt) and removes it |
| `a_service_is_disabled_and_restored` | picks the least important catalogue service present (`Fax`, `MapsBroker`, ..., `Spooler`, `SysMain`, `DiagTrack`), disables/stops it, then restores its start type and running state (elevated) |
| `the_engine_applies_and_undoes_a_real_setting` | through the full `Debloat` engine: scans, picks the first non-`Kept` *setting* item that is not already applied (returns early if all are), applies it, checks the registry values equal the catalogue target and the journal has one change, undoes it and checks the original values are back. Which hive it touches depends on the catalogue entry (a machine-wide one raises a UAC prompt) |

**Why it needs explicit approval** (CLAUDE.md: "Changes to Windows or device configuration (registry,
services, startup, power, drivers, system settings), including running `crates/core/tests/windows_live.rs`"):
it writes to HKCU and HKLM, stops/starts a real service and raises UAC prompts. A failure in the middle can
leave a test key, a disabled service or a disabled startup entry. **Plain `cargo test` includes it**, so on a
Windows developer machine use `cargo test --locked --lib --bins` (the local check named in CLAUDE.md) or
`-p carla-core --lib`. CI runs `cargo test --locked -p carla-core -- --nocapture` on a Windows runner (a
throwaway machine; see `.github/workflows/build.yml`), which is where this test runs. **UNVERIFIED:** whether
the Windows CI job runs elevated; check the job's runner configuration and the elevated tests' behaviour in a
recent CI log.

---

## 12. Test inventory and how to run

* Unit tests live next to the code (`#[cfg(test)] mod tests`), written as sentences
  (`a_desktop_that_never_answers_uses_every_scheduled_try_and_stops`), which are the best specification of
  behaviour. Prefer the same style for new tests.
* No test hits the real network; parsers are fed bodies/fixtures. Filesystem tests use per-process temp
  directories.
* Run only what you need: `cargo test --locked -p carla-core --lib` (core unit tests),
  `-p carla-core library::sync` (a module). From Windows use a VS developer shell for the MSVC linker; Git
  Bash will not find `link.exe` (KI-006). CI (`.github/workflows/build.yml`) is authoritative.
* The Linux CI step is `cargo test --locked` (core and companion). The shell is tested on Windows
  (`cargo test --locked -p carlaos-shell`).

---

## Open questions / needs investigation

1. **Companion use of `crash`/`nap`/`presence`.** Usage counts here come from `use carla_core::...` greps;
   re-check with a broader search (`carla_core::{...}` groups and fully qualified calls) before relying on
   "handheld-only" for a module.
2. **`power::asus_request` and `ASUS_CONTROL`.** The values come from reverse-engineering ASUS's ACPI
   interface (module doc says it is the one Armoury Crate uses). Confirm on the device and record in
   `knowledge/development/validation-plan.md`.
3. **Replay of `Store` across threads.** How the shell serialises access to one `rusqlite::Connection`
   (mutex type, lock order, which commands hold it) is in [04](04-ally-native.md); confirm it matches the
   "never hold the library lock across blocking calls" rule for every `Store` call that touches disk scans.
4. **Steam private formats (`owned.rs`, `bvdf.rs`).** They have changed over the years; the module refuses
   what it does not recognise. Confirm against a current Steam build and document the version range.
5. **`library::mod.rs` module ordering** mixes `pub mod` blocks (cosmetic), and `store` is private while
   `Store` is re-exported: confirm no code needs `store::` internals.
6. **Which setting `the_engine_applies_and_undoes_a_real_setting` picks** depends on the machine (first unapplied non-`Kept` setting), so its registry footprint differs per run; confirm the list in `catalogue::SETTINGS` and which are HKLM.
7. **Unknown `Kind` text** (`Kind::parse`) silently becomes `Game`; decide whether a future new kind needs a
   migration so old Allies do not mislabel it.
8. **`windows_live` in CI.** Whether the Windows job is allowed to prompt for elevation, and what happens
   when a service in the candidate list does not exist on the runner (the test searches a list).
9. **Cloud service list** (`cloud::SERVICES`) is hard-coded with one host each; confirm how a changed
   service URL is meant to be updated (a release).
10. **Release notes for installer names** (`update::Part::installer`) must stay in step with the build
    workflow; confirm the exact file names in the latest release assets.
