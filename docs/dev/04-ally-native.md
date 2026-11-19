# 04 - The Ally shell, native side (Tauri / Rust)

Crate: `apps/shell/src-tauri` (package `carlaos-shell`). This chapter describes what runs
in the Rust half of the Ally application: start-up, shared state, every thread, every lock,
every Tauri command, the game/stream session lifecycle, the link to desktops, save sync, the
quick menu, remote administration, installs, updates, and the crash/log machinery. The React
half is chapter 05 (`05-ally-ui.md`).

All facts below were taken from the source of `apps/shell/src-tauri/**` at the base commit
(`bd34c68`). Where a fact depends on code in `crates/core` that was not read for this chapter,
or on behaviour of Tauri/WebView2/Windows that code alone does not prove, it is marked
**UNVERIFIED:** with what would confirm it. Nothing here has been run on hardware by the
author of this chapter.

Conventions: "command" means a `#[tauri::command]` function reachable from the web page with
`invoke`. "Thread" means an OS thread started with `std::thread::spawn`. "The store" means the
SQLite catalogue `carla_core::library::Store`.

---

## 1. What the crate is made of

The shell crate is thin glue: almost all logic (catalogue, link protocol, save sync algorithm,
launching, power profiles, crash report writer, update download) lives in `crates/core`
(`carla_core`). The shell adds Tauri wiring, OS calls (Windows API through the `windows` crate),
long-running threads and the one shared state object, `Library`.

| File (under `apps/shell/src-tauri/src/`) | Role |
| --- | --- |
| `main.rs` | `fn main`, module list, the `invoke_handler` registration, `app_info`, `quit` |
| `boot.rs` | Boot guard glue (`--boot` start, View held, failed-start counter), `boot_ready`, `boot_set` |
| `admin.rs` | `Admin` state (PIN lock + clean-up busy lock), `admin_*` and `cleanup_*` commands |
| `library.rs` | `Library`/`Inner`/`Active`, scan, launch (`play`), `follow` thread, adopt, reconnect, manual games |
| `consoles.rs` | Console (emulator + game folder) commands, all admin |
| `cloud.rs` | Cloud-gaming commands (`cloud_*`) |
| `discover.rs` | `discover_scan` (look for games/ports/emulators on disk), admin |
| `hosts.rs` | Everything about linked desktops: reach, refresh, pulse, measure, link, wake, check/repair, `start_stream`, Moonlight pairing |
| `savesync.rs` | Save synchronisation orchestration between this Ally and desktops |
| `details.rs` | Game page data (achievements, saves overview, backups), `meta_*` lookups, `library_updates` |
| `art.rs`, `autoart.rs` | Choosing and auto-fetching cover art; SteamGridDB key (sealed) |
| `installs.rs` | Install via Steam, download watcher, storage overview, uninstall |
| `presence.rs` | Adopting a game that was started outside CarlaOS; remembering the game window; "back to the game" |
| `quick.rs` | The quick menu window and its controller watcher |
| `power.rs` | Per-game power level / screen refresh rate, applied through ASUS ACPI + Windows power overlay |
| `system.rs` | Side-menu commands (sound, brightness, battery, Wi-Fi, sleep/restart/off) and the sleep-noticing watcher |
| `device.rs` | Thin Windows API wrappers: battery, sound, brightness (WMI), XInput pad, foreground window, sleep, `bring_forward` |
| `remote.rs` | Being administered from a desktop (the Ally polls the desktop for requests) |
| `updates.rs` | Check/install CarlaOS updates from GitHub releases |
| `crash.rs` | Crash report wiring, panic-hook state line, UI heartbeat watchdog, report commands |
| `conn_log.rs` | The connection log and merged "activity log" commands |

Non-Windows builds: `device.rs` and `power.rs` have `#[cfg(not(windows))]` stubs that return
`None`/`false`/do nothing, so the crate compiles and its tests run elsewhere. Behaviour
described below that involves Windows calls is therefore Windows-only.

Cargo notes (`Cargo.toml`): `tauri = "2"` with feature `protocol-asset`, `tauri-plugin-dialog`,
`carla-core` by path, and the `windows` crate (0.62) with a hand-picked feature list for
`cfg(windows)`. Adding a Windows API call that needs a new `windows` feature means editing that
feature list. The workspace release profile is `panic = "abort"`, `lto`, `opt-level = "s"`
(see section 8, "panics in release").

---

## 2. Tauri application configuration

### 2.1 `tauri.conf.json`

| Setting | Value | Notes |
| --- | --- | --- |
| `productName` | `CarlaOS` | |
| `identifier` | `dev.carlaos.shell` | Also the folder name under `%LOCALAPPDATA%` for all data (see 2.4) |
| `build.devUrl` | `http://localhost:1420` | Vite dev server, fixed port (`strictPort`) |
| `build.frontendDist` | `../dist` | `npm run build` output of `apps/shell` |
| `bundle.targets` | `["nsis"]` | Windows installer only |
| `bundle.windows.nsis.installMode` | `currentUser` | Per-user install, no elevation |
| `bundle.windows.webviewInstallMode` | `skip` | Installer does not bundle/bootstrapper WebView2; it is assumed present on Windows 11 |
| `app.security.csp` | `default-src 'self'; img-src 'self' asset: http://asset.localhost data:; style-src 'self' 'unsafe-inline'` | See 2.3 |
| `app.security.assetProtocol` | `{ enable: true, scope: [] }` | Scope is empty at build time and extended at run time (2.3) |

### 2.2 Windows (webviews)

There are exactly two windows, both served by the same front-end bundle.

**`main`** - declared in `tauri.conf.json`:

* `label: "main"`, `fullscreen: true`, `decorations: false`, `resizable: false`, nominal
  1920x1080.
* `additionalBrowserArgs`:
  `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required`.
  The first disables a few Edge/WebView2 UI features (Office/PDF overlays, SmartScreen); the second lets the
  UI play its synthesized sounds (`ui/sound.ts`) without a prior click. (Intent inferred from names;
  **UNVERIFIED:** confirm by removing the flag and checking whether sounds are blocked before first press.)
* Loads `index.html` with no hash, so `main.tsx` renders `<App />`.

**`quick`** - created at run time by `quick.rs::open` with `WebviewWindowBuilder`, not in the config:

* URL `WebviewUrl::App("index.html#quick")`; `main.tsx` renders `<QuickMenu />` when `location.hash === "#quick"`.
* `transparent(true)`, `decorations(false)`, `shadow(false)`, `always_on_top(true)`, `skip_taskbar(true)`,
  `resizable(false)`, `fullscreen(true)`, `visible(false)` until shown.
* The window is created on first open and reused while the game runs; `quick::ended` calls
  `window.destroy()` when a session ends so the web view does not linger ("CarlaOS is as light as before").
* It does not get `additionalBrowserArgs` (those are only in the config for `main`).
  **UNVERIFIED:** whether WebView2 shares one environment across both windows (Tauri normally does for the
  same data directory) and therefore whether the args apply to it; confirm by inspecting the quick window's
  command line in a debugger or by testing autoplay there (the quick menu plays no sound, so this is currently moot).

The same page script bundle also runs `crash.alive` heartbeats from both windows (see section 14 and
open question 3).

### 2.3 Capabilities, CSP, asset protocol

* `capabilities/default.json`: identifier `default`, `windows: ["main", "quick"]`, `permissions: ["core:default"]`.
  Only core event/window/etc. permissions are granted. Application commands declared with `#[tauri::command]`
  are callable because `build.rs` is the default `tauri_build::build()` (no `AppManifest` restricting commands).
  Dialogs are opened from Rust (`DialogExt` in `library.rs`, `details.rs`, `art.rs`), so no dialog permission is
  given to the page. **To let the page call a plugin directly, a permission must be added here; to add a window, its
  label must be added to `windows`.**
* CSP has no `connect-src`, so `default-src 'self'` applies: the page cannot `fetch` the internet. All network access
  is done in Rust (this is the architecture: the page never talks to a desktop or to GitHub). `img-src` allows
  `asset:`/`http://asset.localhost` for local pictures and `data:` for inline SVG (used in `app.css`).
  `style-src 'unsafe-inline'` is needed for React `style={{...}}`. There is no `font-src`, and no `@font-face`
  in `app.css`, so `"Nunito"` in `--font` is only used if installed; otherwise Segoe UI is used.
  **UNVERIFIED:** whether Tauri adds `ipc:` / `http://ipc.localhost` and the dev-server websocket to this CSP
  automatically (it is documented to inject its own IPC sources); confirm by running `npx tauri dev` and
  checking the console for CSP violations.
* Asset protocol: `scope: []`. Pictures only work because Rust calls
  `library::allow_pictures(app, dir)` = `app.asset_protocol_scope().allow_directory(dir, true)` for:
  Steam's `appcache/librarycache` (in `Library::scan`), `art_dir` (`<data>/art`), and `hosts::art_dir`
  (`<data>/art/desktops`). **Rule:** whenever Rust writes pictures to a new location the page must display, the
  directory has to be allowed; and `allow_pictures` does nothing if the directory does not exist yet
  (`dir.is_dir()` check), so call it *after* creating/writing files (see `meta_fetch`, `art_find`, `art_use`,
  `sync_in_background`, `hosts_refresh`). The page turns file paths into URLs with `convertFileSrc` (chapter 05).

### 2.4 Where data lives

`app.path().app_local_data_dir()` = `%LOCALAPPDATA%\dev.carlaos.shell\` (Windows). Boot, crash and
log paths are computed by hand from `LOCALAPPDATA` + identifier because they are needed before Tauri's path
resolver exists (`boot::record_path`, `crash::open`, `conn_log::open`); they fall back to the temp dir when
`LOCALAPPDATA` is unset.

| Path under the data dir | Owner | Content |
| --- | --- | --- |
| `library.sqlite` | `Library::open` | The catalogue (falls back to an in-memory store if it cannot be opened) |
| `art/` (`chosen/`, `auto/`, `steam/`, `fetched/`, `looking/`, `desktops/`) | `art.rs`, `autoart.rs`, `installs.rs`, `details.rs`, `hosts.rs` | Pictures. `looking/` is emptied at each art search |
| `saves/` | `details.rs`, `savesync.rs` | Save backups (`Backups::new(data_dir/saves, game_id)`) |
| `cloud-browser/` | `library::play` | Browser profile kept for cloud gaming so sign-ins persist |
| `admin.json` | `Admin::new` | PIN hash (salted, 50 000 rounds, in `carla_core::admin`) |
| `cleanup/` (`journal.json`, helper script) | `admin.rs` | Windows clean-up helper + change journal |
| `remote.log` | `remote::record` | One JSON line per remote request; deleted when > 256 KB |
| `boot.json` | `boot::record_path` | Boot guard record |
| `crash/` | `crash.rs` | Crash reports, trail, running marker |
| `logs/connection.log` | `conn_log.rs` | Connection log, cut to its newest half at 256 KB |

---

## 3. Start-up sequence (`main.rs::main`)

In order. Anything before `tauri::Builder` runs before any window exists.

1. `tauri::generate_context!()` (reads `tauri.conf.json`, embeds `../dist`).
2. **Boot guard.** `carla_core::boot::Guard::new(boot::record_path(identifier))`, then
   `boot::should_start(&guard)`:
   * No `--boot` argument (`carla_core::boot::BOOT_ARG`) = started by hand: always start.
   * `--boot` present (started by Windows at log-on through the per-user registry Run entry
     (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) that `carla_core::boot::set` writes; `set` also zeroes the
     `StartupDelayInMSec` Serialize delay and restores it when disabled): `view_held()` polls the XInput pad up to 8 times, 100 ms apart, for **View held and Menu not
     held**. If held, `guard.skipped()` and `main` returns: no window, no Tauri, the Windows desktop is left alone.
   * Otherwise `guard.begin()`; `Start::Proceed` means go on. After `FAILED_STARTS` (2) starts that never reached
     `boot_ready`, `begin` stands down (returns not-proceed) once, and `boot_ready` later reports
     `stood_down: true` so the UI can toast "failed to start twice" (`App.tsx::endSplash`).
   * `boot_ready` (command) is what marks the start as good (`guard.ready()`). The page calls it when the
     splash animation ends.
3. **Crash reports.** `crash::open(identifier)` builds `carla_core::crash::Crash` in `<LOCALAPPDATA>/<id>/crash`,
   stores a clone in the `CRASH` `OnceLock`. `reports.install_panic_hook(crash::state)` installs a panic hook that
   writes a report and a one-line "what was running" string (`crash::state`, which uses `try_lock` so the hook never
   waits for a lock the panicking thread may hold). `reports.begin()` writes an "unexpected" report if the
   previous run left its running marker (did not call `end`), then marks this run as running. Another live copy
   of CarlaOS keeps the marker (the second copy writes none).
4. **Connection log.** `conn_log::open(identifier, VERSION)` opens `logs/connection.log`, registers this computer's
   name for scrubbing, **installs a process-wide observer on every request made by `carla_core::link::client`**
   (`client::set_observer(observe)`), and writes `--- CarlaOS <version> started`.
5. `tauri::Builder::default()` with
   * `.manage(crash::Reports(reports))`, `.manage(boot::Boot(guard))` (state available to commands),
   * `.plugin(tauri_plugin_dialog::init())`,
   * `.setup(...)` (below),
   * `.invoke_handler(generate_handler![...])` (97 commands, section 7),
   * `.build(context).expect(...)` then `.run(...)`.
6. **`setup` closure** (runs once, before the first window loads, on the main thread):
   1. `data_dir = app_local_data_dir()`.
   2. `Library::open(&data_dir)` - opens the SQLite store (or an in-memory one on failure), calls
      `close_dangling_sessions()` (sessions left open by a crash are closed). **A failure here aborts setup**
      (the `?`), so the app does not start; the store itself is made hard to fail by the in-memory fallback.
   3. Threads/watchers are started *before* `app.manage(library)`, each given a clone of `Library` (an `Arc`):
      `quick::watch`, then `app.manage(admin::Admin::new(data_dir))` (must precede `remote::watch`, which does
      `app.state::<Admin>()`), `remote::watch`, `hosts::watch`, `installs::watch`, `presence::watch`,
      `system::watch`, `crash::watch_state` (stores a `Library` clone in the `LIBRARY` `OnceLock` for panic
      reports), `crash::watch_ui`.
   4. `app.manage(library)` - makes `State<'_, Library>` available. Commands cannot arrive before this
      completes because setup finishes before the page loads.
   5. `std::thread::spawn(power::rest)` - put the machine at the "resting" power level (CarlaOS starts between
      games).
7. **Run loop.** On `RunEvent::Exit`: `ending.end()` (normal-exit marker for crash reports) and `power::leave()`
   (Windows back to Balanced power mode, display refresh restored).
   Quitting paths: the `quit` command calls `app.exit(0)`; `update_install` and the remote "update" call
   `app.exit(0)`; the remote "restart" calls `crash::ended()` then `app.restart()` (does not go through
   `RunEvent::Exit`'s `end()`, hence the explicit `ended()`); `system::shutdown` calls `crash::ended()` before
   asking Windows to restart/shut down and `crash::resumed()` if that fails.

### Interaction with the page

`main.tsx` (page side) starts a 3 s heartbeat `crash_alive` and, 2 s after boot, wraps `invoke` with `inflight.ts`
(chapter 05). `App.tsx::endSplash` calls `boot_ready` and `crash_reports`.

---

## 4. The `Library` struct (the one shared state object)

```rust
pub struct Library(pub(crate) Arc<Inner>);          // Clone = clone the Arc

pub(crate) struct Inner {
    pub(crate) store: Mutex<Store>,                 // SQLite catalogue (one connection)
    pub(crate) steam_root: Mutex<Option<PathBuf>>,  // where Steam is, found by scan()
    pub(crate) art_dir: PathBuf,                    // <data>/art
    pub(crate) data_dir: PathBuf,                   // <data>
    pub(crate) active: Mutex<Option<Active>>,       // the one "now playing" slot
}

pub(crate) struct Active {
    pub(crate) view: SessionView,  // what the UI is told (see below)
    pub(crate) place: String,      // "local", "cloud" or a desktop id
    pub(crate) since: i64,         // unix seconds started
    pub(crate) stop: bool,         // player said they are done and we could not tell
    pub(crate) end: bool,          // with stop: the player asked for the game to be closed
}
```

`SessionView` (serialised to the page; `library_session` command and `library://session` event):

| Field | Meaning |
| --- | --- |
| `game_id`, `title` | The game |
| `phase` | `"waking"` (desktop being woken), `"starting"` (until program or stream seen), `"running"`, `"reconnecting"` (stream cut by sleep) |
| `since` | Unix seconds started or first noticed |
| `adopted` | Not started from CarlaOS (see 9.5) |
| `stream` | Streamed from a desktop (`place != local && place != cloud`) |
| `cloud` | Played in the cloud browser window |
| `note` | One line about what is waited for, e.g. a store's update ("Steam is updating it first. 41% done, ...") |

Note the TypeScript `Session` type (`data/types.ts`) mirrors this minus `since`.

### 4.1 Helpers

* `locked(&Mutex<T>) -> MutexGuard<T>`: `lock().unwrap_or_else(|e| e.into_inner())` - ignores poisoning. **Every lock
  in the crate is taken this way** (a few call `.lock().unwrap_or_else(...)` inline). Use `locked` for new code.
* `Library::store() -> MutexGuard<Store>` = `locked(&self.0.store)`. Typical use is a statement-scoped temporary:
  `library.store().hosts()?` - the guard lives until the end of that statement.
* `now()` / `now_ms()` unix seconds / milliseconds.
* `Library::candidates()`: local installs as `presence::Candidate` (used by the adoption watcher).
* `Library::scan(&AppHandle) -> Result<Vec<GameView>, String>`: the whole library refresh, in this order:
  find Steam root; `allow_pictures` for Steam's cache/art; **scan all stores off-lock**
  (`scan_store` per `STORES`, slow disk reads); set `steam_root`; read consoles (brief store lock) and scan their
  game files off-lock (`emulation::scan`); take the store lock once and `sync` each source for `LOCAL`
  (including `mend_shortcuts`); `drop(store)`; `installs::sync_local` (what Steam *could* install);
  `installs::fetch_covers` (background thread); `autoart::run` (background thread, only if enabled);
  finally `store().list_all()`.

### 4.2 The `active` slot semantics

* At most one session exists at a time. `claim` is the only way in: it takes the `active` lock, returns
  `Err("<title> is still running.")` when occupied, otherwise stores a new `Active`.
* It is released only by `play` (on a start failure) or by the `follow` thread (when the session ends), or by
  `adopt` on a start failure. Both paths also call `power::rest()` and emit `library://session` with `null`.
* Many other pieces read it as "is a game running?" and behave differently: the host pulse measures nothing,
  autoart waits, installs poll rarely (60 s), `hosts_measure` refuses, save restore refuses, `saves_sync` and
  after-game syncs back off, the quick watcher reads the pad, `system_power` marks it stopped, `storage_uninstall`
  refuses the running game, and the presence watcher stands down.

---

## 5. Background threads (everything started)

All threads are detached `std::thread::spawn` loops that run until the process ends. None has a shutdown signal.

| Thread / watcher (started in) | Interval | What it does | Network | Locks it takes |
| --- | --- | --- | --- | --- |
| `quick::watch` (`setup`) | Not playing: 500 ms. Playing, no pad: 1000 ms. Playing, pad, menu closed: 50 ms. Menu open: 16 ms | While a game is in phase `running`: remembers the front window (`presence::note_front` -> `GetForegroundWindow`), reads XInput (`device::pad`), detects the View+Menu chord held 500 ms (`carla_core::quick::Chord`), opens/closes the quick window, forwards `Press` events with `app.emit_to("quick", "quick://press", press)` | none | `active` (read phase, briefly) |
| `remote::watch` -> one `ask` thread per allowed desktop (`setup`, `hosts_set_admin`) | Loop: `reach` desktop, then `serve` (long-poll `GET /v1/admin/next`, 40 s timeout); on failure sleep 20 s and retry; thread ends when the desktop is no longer allowed | Polls the desktop; the Ally never listens | `store` (via `allowed`, briefly), `ASKING` (set of desktop ids), `active` (via `playing`) |
| `hosts::watch` (`setup`) | 45 s (`PULSE_EVERY`) | `pulse_all`: for every linked desktop, `reach`; if up and no game: `seen` + light measurement (`Depth::Quick`) + pulse; if up and a game runs: pulse only (no measuring); if off: skip | `GET /v1/status` per address; probes; `POST /v1/link/pulse` | `store` (briefly, copies hosts out first), `active` (read), `STATE` (track_reach), `PAIRED` (via `known_paired`) |
| `installs::watch` (`setup`) | 4 s while something downloads, 30 s idle, 60 s while a game is active; polled in 500 ms slices so `LOOK_NOW` cuts the wait | `look()`: read Steam's download state, ask desktops that were fetching, compute rate/ETA, emit `library://downloads` when it changes, rescan when a local first install finishes | `GET /v1/downloads` per fetching desktop | `WATCHED`, `PACE`, `active` (read), then `store` (inside `scan`) |
| `presence::watch` (`setup`) | 5 s (`LOOK_EVERY`) | If no session is active: list running programs (`sysinfo`), `spot` a library game, debounce with `Spotter`, then `adopt` it | none | `active`, `store` |
| `system::watch` (`setup`) | 1 s | Clock-jump detector (`Napper`): after a sleep calls `power::rest()` unless a game runs, emits `system://woke` with seconds slept | none | `active` (read) |
| `crash::watch_ui` (`setup`) | 5 s (`WATCH_EVERY`) | Window-hang watchdog (section 14) | none | none (atomics) |
| `power::rest` thread (`setup`) | once | Resting power mode | none | none |
| `follow` thread (per session) | 2 s (`POLL`) | Watches game/stream process, handles sleep, notes store updates, ends session (section 9) | per-stream: reconnect uses the link | `active`, `store`, `steam_root` |
| `sync_in_background` thread (per desktop when due) | Each desktop at most every 300 s (`SYNC_EVERY`), one at a time per desktop, 20 s budget | Mirror the desktop's game list and covers into the store, emit `library://changed` | `GET /v1/games`, `/v1/library`, `/v1/cover/..` | `SYNCING`, `LAST_SYNC`, `store` |
| `reach` fan-out (per call) | per call | One thread per candidate address of a desktop, joined | `GET /v1/status` each | none |
| `collect_until` workers (`hosts_refresh`) | per call | One thread per desktop, 10 s overall limit; late ones are abandoned and keep running | as `refresh` | as `refresh` |
| `savesync::all` fan-out | per call | One thread per desktop to `reach`, then copies happen one after another | status then saves endpoints | `BUSY` (held by caller thread) |
| `installs::fetch_covers` thread | per scan | Fetch up to 800 Steam covers for games not installed anywhere; stops after 8 misses in a row; emits `library://changed` every 12 | Steam public store (`meta::cover`) | `BUSY` atomic, `store` briefly |
| `autoart::run` thread | per scan / setting change | Look up covers/hero art for games without any (max 400 per run, retries after `TRY_AGAIN_AFTER`); waits in 20 s steps while a game runs; gives up after 3 unreachable | public art sources (+ SteamGridDB with key) | `BUSY` atomic, `active` (read), `store` briefly |
| `uninstall_local_steam` thread | 2 s x 90 | Shared by Storage uninstall and Remove from My Library. Wait for Steam's confirmation; when the manifest disappears, optionally remove the personal membership and rescan. A cancelled prompt leaves membership intact | none | `UNINSTALLING` (per-game duplicate guard), `store` (via membership and scan) |

Network summary: the page makes no network calls (CSP). Rust talks to (a) desktops over the signed link
(`carla_core::link::client`: HTTP to the companion on `PORT`, addresses may be LAN or Tailscale), (b) GitHub
(`update::check`, `update::download`) in `updates.rs` and `remote.rs`, (c) public metadata/art sites in
`details.rs`/`art.rs`/`autoart.rs`/`installs.rs` via `carla_core::library::meta`/`art`, (d) Steam's client by
command line, (e) `winget` (install Moonlight), (f) Moonlight CLI (pairing, streaming).
Request timeouts come from `carla_core::link::client` (`QUICK` = 3 s, `SLOW` = 40 s, `PROBE` = 4 s, `COVER` = 6 s,
`admin/next` long-poll uses `SLOW`). **UNVERIFIED:** whether `ask_status` retries once on failure (the function that
wraps `ask_once` was not read); confirm in `crates/core/src/link/client.rs`.

---

## 6. LOCK MAP

### 6.1 Rule of thumb

> **Never hold any of these locks across a network call, a process scan, a child-process wait, a file dialog, a
> `thread::sleep`, a Windows API call that can block (power mode, display mode, `SendMessage`-like calls), or an
> `app.emit` that you do not need under the lock.** Copy what you need out, drop the guard, then do the slow work.
> The codebase has been burned by this (see 6.3).

All shell locks are `std::sync::Mutex`; none is re-entrant. Taking the same mutex twice on one thread blocks
for ever. All are taken poison-tolerantly.

### 6.2 Every lock and static in the crate

| Name (file) | Type | Guards | Taken by | Hazards / notes |
| --- | --- | --- | --- | --- |
| `Inner.store` (`library.rs`) | `Mutex<Store>` | The SQLite connection (all catalogue reads/writes). One connection: every query is serialised | Everything, via `library.store()` | **The hot lock.** Never hold across network/process/OS calls. `Library::scan` deliberately releases it during disk scans. `remote::run` holds it for the duration of one `remote::catalogue` call (CPU/DB only). `power::view` holds it across `power::profile` + `power::chosen` (DB only) |
| `Inner.steam_root` (`library.rs`) | `Mutex<Option<PathBuf>>` | Cached Steam install dir | `scan` (set), `play`, `follow`, `details.rs`, `installs.rs` (all clone it out immediately) | Always `locked(..).clone()` into a local and drop |
| `Inner.active` (`library.rs`) | `Mutex<Option<Active>>` | The current session | `claim`, `play`, `follow`, `adopt`, `reconnect`, `quick.rs`, `presence.rs`, `hosts.rs` (`pulse_of`, `hosts_measure`, `check_host`), `savesync.rs`, `details.rs`, `installs.rs`, `system.rs`, `crash::state` (via `try_lock`) | Held briefly. In `follow`, `app.emit` and `crash::note` (a small file write) are called while it is held; keep it that way (short). `follow` explicitly `drop(active)` before `running.stop(end)` because closing a game scans every process. **UNVERIFIED-as-bug:** `installs_start` holds it across `downloads::restart_closes_steam()` (a full process scan via `sysinfo`), see open question 1 |
| `Admin.lock` (`admin.rs`) | `Mutex<AdminLock>` | PIN state: `unlocked` flag, failure counter, pause timer, PIN file | `Admin::with_lock` (all `admin_*`, every `require()`) | Short. `admin_unlock` does a 50 000-round hash while holding it (sync command, main thread) |
| `Admin.busy` (`admin.rs`) | `Mutex<()>` | One Windows clean-up operation at a time | `Admin::cleanup`, `cleanup_remote` | Held for the whole PowerShell run, which can wait on a Windows approval prompt. That is intentional; the command is `async` so the main thread is free |
| `SCANNING` (`library.rs`) | `AtomicBool` | "A library scan is running" | `library_scan` (RAII `Scanning` guard resets it) | Second concurrent `library_scan` returns the current catalogue immediately instead of scanning |
| `BUSY` (`savesync.rs`) | `static Mutex<()>` | One save sync at a time (two could copy a folder both ways) | `one_at_a_time` -> `take_turn` -> `carla_core::link::deadline::lock_within` (try-lock polling every 10 ms up to a deadline) | **Held for the whole sync, including network.** That is the point, so callers never block for ever: `BUSY_WAIT` = 20 s then give up (`all` returns `false`, `before_stream` refuses, `after_stream` returns). Never take `store`-then-`BUSY` ordering assumptions: `BUSY` holders take `store` and `active` briefly inside |
| `SYNCING` (`hosts.rs`) | `static Mutex<Vec<String>>` | Desktop ids being mirrored now | `start_syncing`, `Syncing::drop` | Short |
| `LAST_SYNC` (`hosts.rs`) | `static Mutex<Option<HashMap<String, Instant>>>` | When each desktop was last mirrored | `sync_due`, `mark_synced` | Short |
| `STATE` (inside `track_reach`, `hosts.rs`) | `static Mutex<Option<HashMap<String,(bool,Instant)>>>` | Last reachable/unreachable state per desktop for log lines | `track_reach` | `conn_log::line` is called while it is held (a file append; short) |
| `PAIRED` (`paired_cache()` in `hosts.rs`) | `static Mutex<Option<HashSet<String>>>` | Addresses Moonlight is known paired with | `known_paired`, `note_paired`, `stream_failed` | Short. Do not call Moonlight (`moonlight::paired`, 1-2 s) with it held; the code does not |
| `ASKING` (`hosts.rs`) | `static Mutex<Option<Arc<client::Asking>>>` | The pending "link without code" request | `hosts_link_ask`, `hosts_link_wait`, `hosts_link_cancel` | `hosts_link_wait` clones the `Arc` out and re-locks each second to check it is still the current one (`ptr_eq`); it never holds it while sleeping or asking |
| `ASKING` (`remote.rs`) | `static Mutex<Option<HashSet<String>>>` | Desktops already being polled for admin requests | `refresh`, end of `ask` | Different static from the one above (same name, different module) |
| `WATCHED` (`installs.rs`) | `static Mutex<Option<Watched>>` | Desktops that were fetching + last download list | `sync_host`, `installs_start`, `installs_list`, `look` | `look` copies the desktop list out under the lock then does network |
| `PACE` (`installs.rs`) | `static Mutex<Option<HashMap<..>>>` | Per-download progress history for rate/ETA | `paced` | CPU only |
| `LOOK_NOW` (`installs.rs`) | `AtomicBool` | Wake the installs watcher early | `installs_start`, `sync_host`, watcher | |
| `BUSY` (`installs.rs::fetch_covers`, `autoart.rs::run`) | function-local `AtomicBool`s | One cover/art worker at a time | those functions | Not reset if the thread panics (release builds abort anyway) |
| `REFRESHES` (`hosts.rs`) | `AtomicU64` | Generation counter so an abandoned (late) `refresh` writes nothing | `hosts_refresh`, `refresh` | |
| `NAMES`, `REPEATS` (`conn_log.rs`) | `static Mutex<Vec<String>>`, `static Mutex<Option<Repeats>>` | Computer names to scrub; rate limiter for repeated failures | `know_name`, `plain`, `observe` | `observe` runs on whichever thread made the request, with no other lock guaranteed free. Do not make a request while holding `NAMES`/`REPEATS` (the code does not) |
| `CRASH`, `LIBRARY`, `LOG` | `OnceLock`s | Write-once handles | set at start-up | `LIBRARY` exists only so the panic hook can describe the session |
| `PAGE_HIDDEN` (`crash.rs`) | `AtomicBool` | Page's last "I am hidden" report | `crash_alive`, `watch_ui` | Shared by both webviews (open question 3) |
| `HEART` (`crash.rs`) | `carla_core::crash::Heartbeat` (atomics) | Last beat time, frozen flag | `crash_alive`, `watch_ui` | |
| `OPEN`, `GAME_WINDOW`, `OVER_CARLAOS` (`quick.rs`); `GAME_WINDOW` (`presence.rs`); `SCREEN_CHANGED` (`power.rs`) | atomics | Quick-menu open flag; the window handle that was in front; screen rate changed flag | quick watcher, quick/presence/power commands | Two different statics named `GAME_WINDOW` (quick.rs and presence.rs). `quick::ended` clears quick's and calls `presence::ended()` to clear presence's |
| `WRITING` (`carla_core::crash`, `carla_core::applog`) | `static Mutex<()>` in core | Serialises report/log file writes | core | Not shell code; mentioned because `crash::note` and `conn_log::line` take it |

### 6.3 The fixed deadlock and the CI lint

**What happened (0.20-0.34):** In Rust edition 2021 (the workspace edition), a `MutexGuard` that is a temporary in
the *header* of a `for`, `if let`, `while let` or `match` lives until the end of the whole construct. Code of the form

```rust
for host in library.store().hosts().unwrap_or_default() {   // guard alive for the entire loop!
    seen(library, &host, ...);                               // takes library.store() again: deadlock on the same thread
}
```

kept the store lock for the whole loop and then re-took it in the body. The pulse thread hung and, since every command
needs `store`, the whole Ally stopped (no game list, no local games, no measuring) until restart.

**The fix** is visible in `hosts.rs::pulse_all`: copy the data out in a separate `let` first
(`let hosts = library.store().hosts().unwrap_or_default(); for host in hosts { ... }`), with a comment explaining why.
The regression test `hosts::watch_tests::a_round_with_a_desktop_that_answers_finishes_and_leaves_the_library_usable` runs
one `pulse_all` round against a stand-in desktop with a 30 s timeout and fails with
"library lock still held: true" if it ever deadlocks again.

**Enforcement:** CI runs (in `.github/workflows/build.yml`, Windows job)
`cargo clippy --locked -p carlaos-shell -- -A clippy::all -D clippy::significant_drop_in_scrutinee`
- i.e. *only* that lint, as a hard error. Run the same command before opening a PR that touches the shell.
The lint is heuristic: it flags a significant-drop temporary in a scrutinee when it can be shortened. It does **not** prove
the absence of every long-held lock. In particular `if let Some(x) = locked(&library.0.active).as_ref() { slow() }` where
the body uses `x` is not necessarily flagged (see `installs_start`).

**Patterns to follow:**

```rust
let hosts = library.store().hosts()?;          // statement-scoped guard: released at the semicolon
for host in hosts { ... }

let title = locked(&library.0.active).as_ref().map(|a| a.view.title.clone()); // clone out, guard gone
if let Some(title) = title { ... }

// While-conditions drop their temporaries before the body (unlike while-let), so this is fine:
while locked(&library.0.active).is_some() { std::thread::sleep(...); }   // autoart.rs
```

**Patterns to avoid:**

```rust
match library.store().hosts() { ... slow work using library.store() again ... }
if let Ok(x) = library.store().install(..) { play_slow(x) }   // guard alive through play_slow
for g in library.store().list()? { net_call(g) }              // guard alive through every net_call
```

Existing places that read like the bad pattern but are acceptable because the body is short and does not
re-lock: `quick.rs::state` (`match locked(active).as_ref()` clones fields), `quick_end_game`
(`if let Some(active) = locked(..).as_mut() { active.stop = true; ... }`), `system_power`, `library_end_session`.

### 6.4 Order and nesting seen in the code

* `BUSY` (save sync) is the outermost long-held lock; inside it `store` and `active` are taken briefly. Nothing takes
  `BUSY` while holding `store` or `active`.
* `store` is never held while taking `active` or vice versa in a nested way, *except* short reads such as
  `carla_core::power::profile(&library.store(), ...)` (store only) - keep it that way: do not nest them.
* `Admin.lock` is released before `Admin.busy` is taken in `cleanup` (`with_lock(require)` returns first).
* The page's own lock/unlock model: the admin flag is **global and has no timeout** in Rust; the page locks it
  again when an administrator page unmounts (chapter 05). A page that forgets to do so leaves the Ally unlocked.

---

## 7. Command table (all 101 `#[tauri::command]`)

Legend. **Mode**: `async` = `#[tauri::command(async)]` (runs on Tauri's async runtime thread pool, safe for
blocking work); `sync` = plain `#[tauri::command]` (Tauri 2 runs these on the **main thread**; keep them fast).
**Admin**: `require` = calls `admin.require()` first (returns `"This needs the administrator PIN."` when locked);
`-` = open to anyone. `store`/`active` = takes that lock briefly. Net = network use; Proc = child process / OS calls.

**UNVERIFIED:** that plain sync commands run on the main thread in this Tauri version (documented Tauri 2
behaviour, and consistent with the code comments such as "Not run on the interface's own thread"); confirm with the
Tauri 2 docs for `#[tauri::command]` or by logging `std::thread::current().name()` in one.

### 7.1 App, boot, admin, clean-up

| Command | Module | Mode | Admin | What it does |
| --- | --- | --- | --- | --- |
| `app_info` | main | sync | - | `AppInfo::current(Role::Shell)` (version, api_version, os, arch) |
| `quit` | main | sync | - | `app.exit(0)` |
| `boot_ready` | boot | sync | - | Tells the boot guard this start worked; returns `{enabled, stood_down}` (also called by the Startup page) |
| `boot_set` | boot | sync | require | Enable/disable start with Windows (registry Run entry in core); returns the state now in effect |
| `admin_status` | admin | sync | - | `{pin_set, unlocked}` |
| `admin_set_pin` | admin | sync | - (core enforces "unlock first" when a PIN exists) | Set a 4-digit PIN (and unlocks) |
| `admin_unlock` | admin | sync | - | Verify PIN; result `{result: "unlocked"\|"wrong"\|"wait", seconds?}`; 5 free tries then a 30 s pause |
| `admin_lock` | admin | sync | - | Lock again (the page calls this on leaving admin pages) |
| `cleanup_scan` | admin | async | require | PowerShell scan of removable Windows items (apps, startup, services, settings) |
| `cleanup_apply` | admin | async | require | Apply chosen ids; may wait on the Windows approval prompt; `busy` mutex |
| `cleanup_undo` | admin | async | require | Undo by journal |

### 7.2 Library, games, consoles, cloud

| Command | Module | Mode | Admin | What it does / resources |
| --- | --- | --- | --- | --- |
| `library_scan` | library | async | - | Full refresh (4.1). One at a time (`SCANNING`); a concurrent call returns the catalogue as it is. Reads disks, Steam files; spawns cover/art threads. Page gives up after 120 s |
| `library_list` | library | async | - | Full catalogue: `store.list_all()` |
| `library_personal_list` | library | async | - | My Library: `store.list_personal()` (explicit Ally membership + local Steam installs) |
| `library_personal_add` | library | async | - | Add a known catalogue game to this Ally's `library_entries` |
| `library_personal_remove` | library | async | - | Remove a non-local-Steam selection; refuses while a local Steam install exists |
| `library_set_favourite` | library | async | - | `store.set_favourite` |
| `library_session` | library | async | - | Current `SessionView` or null (`active`) |
| `library_end_session` | library | async | - | Sets `active.stop = true` (the follow thread then ends the session; stream process is killed) |
| `library_launch` | library | async | - | `store.install(game_id, place)` then `play` (section 9). **Blocks until the game/Moonlight has started**, for streams possibly minutes (wake up to 90 s, sync, prepare retries) |
| `library_pick` | library | async | require | Native file/folder dialog (`blocking_pick_*`); `what` = `folder`/`emulator`/`image`/anything else = program |
| `library_add_manual` | library | async | require | Add a program by hand; follows `.lnk`; copies chosen cover into `art/` |
| `library_remove_manual` | library | async | require | |
| `library_inspect` (art.rs) | art | sync | require | `ports::inspect` - what a program/shortcut is (reads a file; sync = main thread, small) |
| `consoles_list` / `consoles_remove` / `consoles_save` | consoles | async | require | `consoles_save` validates, saves, then runs a full `library.scan` (slow) |
| `consoles_suggest` | consoles | sync | require | Suggest name/options/endings by emulator file name |
| `cloud_inspect` | cloud | sync | require | Validate a cloud link; suggest title |
| `cloud_add` | cloud | async | require | `store.add_cloud` |
| `cloud_open` | cloud | async | - | Play a service's home page like a game (`play` with place `cloud`) |
| `discover_scan` | discover | async | require | Disk search for unlisted games/ports/emulators (`discover::look`) |
| `library_details` | details | async | - | Achievements, milestones, saves overview, per-desktop sync lines, session count. May call a desktop (`achievements`) and read Steam files |
| `library_updates` | details | async | - | Ask the store(s) what an update still has to do, here and on desktops (network to desktops), then update the catalogue |
| `saves_backup` | details | async | - | Back up saves now |
| `saves_restore` | details | async | require | Restore a backup; refuses while any game runs (`active`) |
| `saves_pick_folder` | details | async | require | Dialog to set a game's save folder; validated (`sensible`, `measure`) |
| `saves_forget_folder` | details | async | require | |
| `saves_sync` | savesync | async | - | `all(library, game_id, false)` (network, takes `BUSY` up to 20 s) then the per-desktop lines. Called every time a game page opens |
| `meta_search` | details | async | require | Public Steam/Wikipedia search (net) |
| `meta_fetch` | details | async | require | Fetch description + cover, write into `art/fetched`, allow the dir (net) |
| `art_find` | art | async | require | Candidate pictures + downloaded previews (threads, net); allow `art/` |
| `art_use` | art | async | require | Download and keep one candidate (net) |
| `art_pick` | art | async | require | Dialog; max 16 MB |
| `art_clear` | art | async | require | |
| `art_key` / `art_set_key` | art | async | require | SteamGridDB key: stored sealed (`secret::seal`) in settings; `art_key` returns only whether one is set |
| `art_auto` | autoart | async | - | Whether auto-art is on |
| `art_set_auto` / `art_auto_again` | autoart | async | require | Toggle (setting `auto_art`, `"0"` = off) / forget tried marks and run |

### 7.3 Desktops (hosts)

| Command | Module | Mode | Admin | What it does / resources |
| --- | --- | --- | --- | --- |
| `hosts_list` | hosts | async | - | Linked desktops from the store (no network), with stored health |
| `hosts_refresh` | hosts | async | - | Ask all desktops in parallel, 10 s total (`REFRESH_LIMIT`); may start background game mirroring. Page gives up after 15 s |
| `hosts_discover` | hosts | async | require | LAN search for companions, 2.5 s |
| `hosts_link` | hosts | async | require | Link with an address + 6-digit code, then `finish_link` |
| `hosts_link_ask` | hosts | async | require | Ask a desktop to link without a code; returns the check number both screens show |
| `hosts_link_wait` | hosts | async | require | Poll every second (up to `APPROVE_LIFETIME`) until allowed at the desktop; then `finish_link` |
| `hosts_link_cancel` | hosts | sync | - | Drop the pending request |
| `hosts_measure` | hosts | async | require | Full connection measurement (a few seconds); refuses while a game runs |
| `hosts_remove` | hosts | async | require | |
| `hosts_wake` | hosts | async | - | Send Wake-on-LAN packets; error if the desktop has not reported MAC addresses |
| `hosts_rescan` | hosts | async | require | Mirror the desktop's games now (60 s budget) |
| `hosts_sleep` | hosts | async | require | Tell the desktop to sleep (it refuses while a game runs) |
| `hosts_picture` | hosts | async | - | Picture setting + what the last stream got |
| `hosts_set_picture` | hosts | async | require | Choose stream picture (auto, auto120, or a level) |
| `hosts_update` | hosts | async | require | Ask a desktop's companion to update itself (`POST /v1/update`) |
| `hosts_check` | hosts | async | require | Step-by-step diagnosis (`check_host(repair=false)`) |
| `hosts_repair` | hosts | async | require | Do one fix (`wake`, `pair`, `update`, `moonlight`; `relink` is not doable from the Ally), then check again |
| `hosts_admin` | remote | async | - | Whether a desktop may administer this Ally |
| `hosts_set_admin` | remote | async | require | Allow/withdraw (starts/ends the desktop's request polling) |

### 7.4 Playing, quick menu, power, presence

| Command | Module | Mode | Admin | What it does |
| --- | --- | --- | --- | --- |
| `quick_state` | quick | async | - | Title, stream flag, since, sound, brightness, battery, power view, "home" flag (menu is over CarlaOS) |
| `quick_set_sound` | quick | async | - | Set master volume/mute (COM); returns new state |
| `quick_set_brightness` | quick | async | - | WMI brightness, clamped 5..100 |
| `quick_set_power` | quick | async | - | Change the running game's `Power` + `Screen`; remembered |
| `quick_close` | quick | async | - | Hide menu, put game window forward |
| `quick_home` | quick | async | - | Hide menu, bring CarlaOS to front (game keeps running) |
| `quick_show_desktop` | quick | async | - | Ask the streaming desktop to remove its loading cover (`reveal`), then close menu |
| `quick_end_game` | quick | async | - | For a stream also asks the desktop to close the game (`end_game`); sets `stop` + `end`; closes menu |
| `power_get` / `power_set` | power | async | - | A game's power level and screen speed (stored per game and kind); applies at once if that game runs |
| `session_resume` | presence | async | - | "Back to the game" (fails with "No game is running any more." if none) |

### 7.5 Installs, storage, updates, system

| Command | Module | Mode | Admin | What it does / resources |
| --- | --- | --- | --- | --- |
| `installs_start` | installs | async | - | Ask Steam to install a game here or on a desktop; may restart Steam; emits `library://downloads`; the page's `inflight` watcher ignores it by name |
| `installs_list` | installs | sync | - | Last seen downloads |
| `installs_room` | installs | async | - | Free bytes on the roomiest Steam disk |
| `storage_overview` | installs | async | **-** (the page gates it, Rust does not) | Disks and per-game footprints. Note: no `require()` here |
| `storage_uninstall` | installs | async | require | Ask Steam to uninstall, first retaining the title in My Library so it can be reinstalled; background thread rescans afterwards |
| `installs_remove_from_my_library` | installs | async | require | Ask Steam to uninstall a local Steam game; remove its My Library entry only after the Steam manifest disappears. Cancelling Steam's confirmation keeps both install and membership |
| `update_check` | updates | async | - | GitHub: newer interface; also the desktop host installed on this machine |
| `update_install` | updates | async | require | Download + run installers (host first, then Ally); the Ally installer ends this program via `app.exit(0)`; emits `update-progress` |
| `system_state` | system | async | - | Sound, brightness, battery, Wi-Fi |
| `system_set_sound` / `system_set_brightness` | system | async | - | |
| `system_power` | system | async | - | `sleep` (via `device::sleep`), `restart` or `off` (`shutdown.exe /r|/s /t 0`, with `crash::ended()` first). Marks an active session as `stop` |

### 7.6 Crash and log

| Command | Module | Mode | Admin | What it does |
| --- | --- | --- | --- | --- |
| `crash_reports` | crash | sync | - | List reports (`ReportInfo`: name, kind panic/unexpected/hang, at, size, new) |
| `crash_report_text` | crash | sync | - | Read one report by name |
| `crash_seen` | crash | sync | - | Mark reports as announced |
| `crash_note` | crash | sync | - | Add `interface: <text>` to the trail (writer clips it) |
| `crash_alive` | crash | sync | - | Heartbeat `{hidden}` from the page |
| `crash_open_folder` | crash | sync | - | `explorer.exe <crash dir>` |
| `log_tail` | conn_log | async | - | Newest N (1..5000) lines of connection log + crash trail by time |
| `log_text` | conn_log | async | - | Whole activity log for "Copy log" |
| `log_open_folder` | conn_log | async | - | `explorer.exe <logs dir>` |

### 7.7 Observations about gating

* Roughly 42 call sites of `require()`. The pattern is: **destructive, configuration, linking, update, and
  anything exposing secrets is admin-gated in Rust, not only in the UI** (the admin pages also show a PIN pad, but
  the Rust check is the real gate; `admin.rs` says so in its module comment).
* Open (not gated) by design: playing, favourites, power level, quick menu, waking a desktop, backing up saves, the
  downloads/installs list and *starting* an install, reading logs and crash reports, update *check*.
* Known gap: `storage_overview` (read-only) has no `require()`. It leaks only disk and install sizes. Decide whether
  that is intended before relying on it (open question 6).
* Remote administration bypasses the PIN on purpose (section 11).

---

## 8. Error handling conventions

* Commands return `Result<T, String>`. The `String` is shown to the player as a **single plain English sentence**
  (the page puts it straight into a toast through `message(e)`; see chapter 05). Write errors as complete
  sentences, no `Error:` prefixes, no debug output, no paths of other users, no keys. Typical:
  `"That desktop is no longer set up."`, `"A game is running. The connection is measured when it is closed."`
* Do not return `anyhow`/`Debug` formatting. Convert with `.map_err(|e| e.to_string())` and prefer a hand-written
  sentence when you know the cause (`hosts.rs` maps `Error::Unreachable` to
  `"<name> did not answer. It may be off or asleep."`).
* Quiet background work (`backup_after_play`, `sleep_again`, `end_game`, save sync) swallows errors with `let _ =` on
  purpose: "must never get in the way of playing". Failures that matter are written to the trail (`crash::note`) or the
  connection log (`conn_log::line`), or stored for the UI (`Synced.problem`).
* **Panics in release builds abort the whole process** (`[profile.release] panic = "abort"`). A panic on any thread
  (`unwrap`, `expect`, slice index) kills the shell. The panic hook writes a report first. There is one deliberate
  `expect` in `library::play` ("claimed above and only released by this command or its watcher"); do not add casual
  `unwrap()`s on data that comes from the network, the disk, Steam files, or the desktop.
* Mutex poisoning is ignored (`locked`) but in release a panic never leaves a poisoned lock because the process ends.

---

## 9. Session lifecycle (the most important flow)

### 9.1 Places

`place` is a string: `"local"` (`carla_core::library::LOCAL`, this Ally), `"cloud"` (`CLOUD`), or a desktop's
host id. `Kind::of(place)` (core power module) maps it to local/stream/cloud for power profiles.

### 9.2 `library_launch` -> `play`

1. `store.install(game_id, place)` finds the install row (title, source, exe, args, path).
2. `play(app, library, install, place)`:
   1. Build `SessionView{phase:"starting", since: now(), adopted:false, stream: place != LOCAL && !cloud, cloud, note: None}`.
   2. **`claim`** the `active` slot (error if occupied). Done *before* anything slow so a second press cannot
      start a second game and the UI sees something is happening.
   3. `app.emit("library://session", view)`.
   4. Power: read the profile first (`carla_core::power::profile(&library.store(), ...)`, store guard released at the
      end of the `let`), *then* `power::apply(profile, kind)` (ASUS ACPI device `\\.\ATKACPI`, `powrprof` overlay,
      display refresh). The comment in code: the lock must not be held while Windows changes power mode and screen.
   5. Branch on place:
      * **cloud**: `install.install_path = <data>/cloud-browser`; `launch::start(&install, None)`; watch with
        `Watch::new`. The "game" is a browser window opened on the cloud link (`install.args` = URL).
      * **local**: `savesync::all(library, game_id, true)` (bring saves in step with desktops; `launching=true`),
        `launch_may_start(synced)` - if another sync still holds the turn after 20 s the launch is **refused** with
        `"Saves are still being brought in step. Try again in a moment."` (so a game never starts while a fetch is
        writing its save folder); then `launch::start(&install, steam_root)`; `Watch::new`.
      * **stream**: `hosts::start_stream(library, game_id, place, waking, stopped, false)` returns
        `(Moonlight Child, woke)`. `waking` sets `phase = "waking"` and emits; `stopped` reads `active.stop`.
        See section 10.
   6. On failure: clear `active`, `power::rest()`, emit session `null`, return the error string.
   7. Set `phase = "starting"` again and emit; `store.start_session(game_id, place, started)` (returns a session id;
      on failure: stop the running thing, release the slot, rest, emit null, return the error).
   8. `follow(...)`: spawn the watcher thread (9.3). `play` returns the `SessionView` to the page.

So `library_launch` returns only after the game process or Moonlight was *started* (not finished). The page's
`library.launch` also receives the same view through the event.

### 9.3 `follow` (one thread per session)

```
loop every POLL = 2 s:
   napper.observe(now_ms)            // a gap in the clock = the Ally slept
     -> store.session_slept(session, inside(nap, started))   // sleep time is not play time
     -> stream && played   : reconnect(...)   // false => end session; true => reset napper, continue
        stream && !played  : nothing (stream will end by itself)
        local/cloud        : re-apply power profile (Windows may have reset it)
   if Game && !played: ask the store whether an update is pending (carla_core::library::update_of)
        -> if so watch.still_preparing() and set view.note (emit on change)
   phase = running.poll()
   lock active; if None -> break (someone cleared it)
   if active.stop -> drop lock; running.stop(active.end); break
   match phase:
      Running (and view.phase != "running") -> played = true; view.phase = "running"; note=None; emit
      Running | Starting -> nothing
      Finished -> break
      NeverStarted -> never_started = true; break
```

`Running::poll`:

* `Game(watch)`: `Watch::poll` in core (process tracking; `START_GRACE` = 180 s in `carla_core::library::launch`: a game not seen
  within that time is `NeverStarted` (**UNVERIFIED:** exact use, confirm in `Watch::poll`); `still_preparing` extends patience for store updates).
* `Stream{moonlight, since, ..}`: `moonlight.try_wait()`: still running and younger than `STREAM_SETTLE` = 15 s ->
  `Starting`; still running afterwards -> `Running`; exited within 15 s -> `NeverStarted` (a stream that ends this soon
  never showed a game); exited later -> `Finished`.

`Running::stop(end)`: stream: kill Moonlight and wait; game with `end`: `watch.end()` (closes the game; scans processes,
so it is done with the `active` lock released); game without `end`: nothing (the game is left running, CarlaOS just
stops tracking).

### 9.4 End of session (exact order)

After the loop:

1. If `never_started && place != LOCAL`: `hosts::stream_failed` forgets the pairing cache entry (so the next stream
   asks Moonlight again before trusting the cache) and notes it.
2. Notes to the crash trail (`session ended: ... played X, never started Y`) and, for streams, a connection-log line
   with how long Moonlight lived.
3. `ended = if played { now() } else { started }`; `store.end_session(session, ended)` (a game that never appeared has
   zero play time).
4. `played && place == LOCAL`: `details::backup_after_play` (snapshot the save folder if changed; quiet).
5. `sleep_again = played && Stream{woke:true}` (the desktop was asleep before we started).
6. **`*active = None`** - the slot is free from here on, which means a new session can start while the steps below
   run (this is why the save sync has `BUSY`, `launch_may_start`, and the `Yielding` side).
7. Each step is first written to the crash trail ("session end: starting power::rest" etc.), then timed into the
   connection log (`took(..)`), so a step that never returns leaves its name as the last trail line:
   `power::rest()` -> `quick::ended(&app)` (destroy quick window, reset `OPEN`, `GAME_WINDOW`,
   `presence::ended`) -> emit `library://session` with `null` -> unminimise and focus the `main` window ->
   `crash::note("session end: done")`.
8. Saves follow the player: local played -> `savesync::all(library, game_id, false)` (to desktops);
   stream played (not cloud) -> `savesync::after_stream` (fetch back, with retries); then if `sleep_again`:
   `hosts::sleep_again` (waits 20 s, then asks the desktop to sleep unless it reports busy).

The page re-reads the library on the `null` session event so playtime and "recently played" update.

### 9.5 Adopting a game started outside CarlaOS (`presence.rs`, `library::adopt`)

`presence::watch` (5 s): if `active` is none and `library.candidates()` is non-empty, `Processes::programs()` lists
the exe of every running process; `carla_core::presence::spot` matches against installed candidates;
`Spotter::observe` debounces (must be seen consistently). Then `store.install(game_id, LOCAL)` and `library::adopt`:
claim with `phase: "running"`, `adopted: true`; apply the power profile; `start_session`; emit; `Watch::adopted(&install)`;
`follow(... played = true ...)`. Losing a race against a launch from CarlaOS is fine (claim fails, ignored).
For adopted sessions the page shows a "Back to the game" button; `session_resume` hides the quick menu and calls
`presence::back_to_game` (bring the remembered game window forward; if unknown, minimise CarlaOS).

### 9.6 Reconnect after sleep (`reconnect`)

Only for streams that had reached `Running` (`played`). Steps: `running.stop(false)` (kill the frozen Moonlight),
note, `phase = "reconnecting"` (emit), then loop up to `RECONNECT_FOR` = 90 s: if `stop` was requested return false;
call `hosts::start_stream(..., resuming = true)` (same steps as a normal start but **skips `before_stream` save sync**
because the game is still open on the desktop and using the saves); on error sleep 4 s and retry; on success replace
`running` with the new `Stream`, set `phase = "starting"`, return true. After the deadline emit
`system://notice` "The stream could not be picked up again. <reason>" and return false (session ends).

### 9.7 Phase summary (what the UI shows)

`waking` ("Waking the desktop") -> `starting` ("Starting" / "Connecting to <desktop>") -> `running` ("Playing") ->
(`reconnecting`) ... -> session `null` = back to the library. `note` can accompany `starting`.

---

## 10. Desktops (`hosts.rs`)

### 10.1 Data and secrets

A linked desktop is a `HostRow` in the store: id, name, address, port, `device_id`, sealed `key`, hardware, last_seen,
`addresses` (extra known addresses), `macs`. The shared key is stored **sealed** (`carla_core::secret::seal`, Windows
data protection on Windows; plain bytes on other OSes) and opened only inside `link_of` when a request is made.
It never crosses to the page. `HostView` (to the page) carries id, name, `state`
(`"ready"|"busy"|"not_ready"|"unreachable"|"unknown"`), `playing`, `problem`, `latency_ms`, `hardware`, `last_seen`,
`address`, `route` (`"local"|"remote"` = Tailscale), `can_wake`, `version`, `health`.
(The TypeScript `HostState` also lists `"asleep"` and `"off"`, which only the preview sample data uses.)

### 10.2 `reach(host) -> Result<(Link, Status, ms), Error>`

* Candidates = `wake::candidates(host.address, host.addresses)`.
* One thread per candidate runs `link.status()` (`GET /v1/status`, `QUICK`); all are joined.
* Sort so that a Tailscale address (`wake::is_tailscale`) does not beat a home address that answers now;
  return the first success; else the first non-"Unreachable" error (a refusal or the desktop's own words says more than silence);
  else `Unreachable`.
* This is the single entry for "talk to a desktop": almost every other function starts with `reach` and then uses the
  returned `Link` (which has the address that answered).

### 10.3 `refresh` and `hosts_refresh` (deadline)

`refresh(app, library, host, generation)` = `reach` + bookkeeping:
`track_reach` (writes reachable/unreachable transitions to the connection log), `seen` (updates name/hardware/addresses/macs/
last_seen in the store), view with health, `latency_ms`, `route`, `state`, `version`, `playing`, `problem`; if `sync_due`
(300 s per desktop) starts `sync_in_background`. A desktop that refuses or complains is "not_ready", not unreachable.

`hosts_refresh` builds one boxed job per desktop and runs `collect_until(jobs, REFRESH_LIMIT = 10 s)`: all in parallel;
answers that miss the deadline become `late_view` ("unreachable", "did not answer in time") and `track_reach(false)`.
The late job keeps running; `REFRESHES` (a generation counter incremented at each `hosts_refresh`) makes an overtaken
`refresh` skip its writes (no record, no log, no sync). The page adds its own 15 s guard (chapter 05).

### 10.4 `watch` / `pulse_all` / `measure_host`

Every 45 s (`PULSE_EVERY`): see section 5. `measure_host` runs `quality::survey(link, away, depth)`
(`Depth::Quick` in the background, `Depth::Full` for the user-triggered `hosts_measure` and linking), chooses a picture
(`quality::choose` with the stored `Setting` and codec fit), stores `Health`, and **pulses the desktop**
(`link.pulse(&pulse_of(...))`: Ally version, Moonlight installed/paired, what is playing, battery, charging, screen mode,
health) so the desktop dashboard can show it. `pulse_of` uses `known_paired` only ("asking Moonlight would start it").
While a game runs: no measurement and `hosts_measure`/`check_host` skip it ("measuring packets would share its stream").

### 10.5 Linking (three entry points, one finish)

* Code: `hosts_link(address, code)` -> `client::enrol(address, PORT, code, this_device())`.
* Without a code: `hosts_link_ask(address)` -> `client::enrol_ask` stores an `Asking` in `ASKING`, returns `check`
  (the number both screens show); `hosts_link_wait` polls `asking.finish()` once a second up to
  `carla_core::link::enrol::APPROVE_LIFETIME`; `still_ours` makes a newer ask supersede it; `hosts_link_cancel` clears it.
* `finish_link(app, library, hello, link)`: `save_host` (key sealed) -> `pair_moonlight` (install Moonlight with `winget`
  if missing, pick a PIN, start Moonlight pairing, hand the PIN to the desktop over the link, wait up to 15 s, verify;
  failure becomes the `warning`, linking still succeeds) -> `note_paired` -> `sync_games` (60 s budget) -> `mark_synced`
  -> `allow_pictures` -> `measure_host(Full)` -> `refresh` -> `Linked{host, games, warning}`.
  This all runs on the command's thread; the page shows a busy state.

### 10.6 Games mirroring (`sync_games`)

`link.games()` (`GET /v1/games`, `SLOW`) -> for each source in `steam, epic, gog, manual, emu` build `Scanned` rows
(only entries whose id is `<source>:<source_id>` and whose `emulated` matches the source) -> `store.sync(host.id, source, rows, now)`
(this removes desktop games that are no longer there). Covers: fetched one by one (`GET /v1/cover/<id>`) only for games that
have no cover here yet and whose `source_id` is alphanumeric (path safety), cached as `art/desktops/<source>-<id>.jpg`;
stops fetching after 3 silent failures in a row or past `until`. Then `installs::sync_host` (what Steam on the desktop could
install, `GET /v1/library`, and whether it is downloading).

### 10.7 Waking

* `hosts_wake` (open to anyone): `wake::wake(&host.macs)`; returns an error if the desktop never reported MACs.
* `wake_and_wait(host, stopped)` (used by `start_stream` and repair): loop up to `WAKE_LIMIT` = 90 s: send the packet
  immediately and every 10 s, sleep 2 s, honour `stopped`, `reach`; `Unreachable` = keep waiting, other error = stop.
* `sleep_again` and `end_game` are quiet one-shots that never fail the session.

### 10.8 `start_stream` (the stream start steps)

Logged step by step to the connection log with elapsed ms.

1. `store.hosts()` find the desktop (`"That desktop is no longer set up."`).
2. `moonlight::find()` else `"Moonlight is not installed on this Ally."`.
3. `reach`; if unreachable and the desktop reported MACs: call `waking()` and `wake_and_wait` (sets `woke = true`);
   if unreachable with no MACs: error `"<name> did not answer. It may be off or asleep."`.
4. `seen(...)`.
5. If not `resuming`: `savesync::before_stream(library, game_id, &host, &link)?` (section 11) - can refuse the stream.
6. `picture_for`: decide the stream picture (measure `Depth::Quick` unless the picture is `Setting::Fixed`),
   record it as the last streamed choice. Compute the Ally's own `Mode(width, height, fps)`.
7. `link.prepare_for(game_id, Some(screen))` (`POST /v1/stream/prepare`, `SLOW`): the desktop starts the game and sets its
   screen. Retries up to 10 times, 3 s apart, only for the desktop's `"Sunshine is not running"` message (just woken) and
   while `!stopped()`; `Unreachable` -> `"<name> stopped answering."`.
8. `ensure_paired(&exe, &link)`: from the `PAIRED` cache, else ask Moonlight (`moonlight::paired`, 1-2 s), else re-pair
   over the link (`pair_moonlight`).
9. `moonlight::stream(&exe, &link.address, picture)` -> the child process, returned to `play`/`reconnect`.

### 10.9 `check_host` / `hosts_check` / `hosts_repair`

`check_host(library, host, repair)` gathers `Facts` (Moonlight installed/paired, answer + latency, connection survey
unless a game runs, Sunshine ports open on the desktop (`SUNSHINE_PORTS` with 2 s connects), screen fit, GPU codec fit) into
`carla_core::link::check::steps` for a list of `Step{key, title, outcome ok|warn|fail|skipped, detail, fix}`. With
`repair`, it performs the fixes in order (wake, install Moonlight, then re-gather; pair; update) and gathers again.
`hosts_repair(fix)` performs exactly one fix and re-checks. The remote `check` op uses `check_host` as well.

### 10.10 Moonlight

External program found by `carla_core::link::moonlight::find`. CarlaOS uses its CLI: `paired`, `pair` (spawn, PIN),
`finished` (close Moonlight's window after pairing), `stream` (spawn the stream child). Installing: `winget install --id
MoonlightGameStreamingProject.Moonlight --exact --source winget --silent ...` with `CREATE_NO_WINDOW` (0x0800_0000).
Pairing automation is the reason a player never sees a PIN screen.

---

## 11. Save sync (`savesync.rs`)

Goal: a game continues where it stopped whichever machine ran it last. Only for games whose save folder the administrator
named on **both** sides (`store.save_folder_of`, a desktop's `saves_placed`). Everything is quiet: failures never block
play, except the explicit "keep clean" gate.

### 11.1 Building blocks

* `BUSY: Mutex<()>` + `take_turn(lock, what, wait)` using `deadline::lock_within` (polling `try_lock`, 10 ms). Logs
  waits >= 1 s and give-ups to the connection log. `BUSY_WAIT` = 20 s.
* `folder(library, game_id)` -> `(save folder, title)`; console games use the console's name as title.
* `with(library, game_id, title, folder, host, link, after_stream, yielding) -> bool` ("worth trying again soon"):
  1. `link.saves_placed(game_id, title, known_place)` asks the desktop whether it has a folder for this game; learns the
     save place from the first desktop that says it (`learn_save_place`), stores the desktop's `keep_clean` flag.
  2. `Ok(None)` -> the desktop has no folder: clear the stored sync row and `keep_clean`, return false.
  3. `Unreachable` or a "still running" message -> return true (retry later). Other error -> record a `problem` row.
  4. Build the Ally-side `Folder{folder, backups: <data>/saves/<game>, now}` and `sync::bring_in_step_for(keep_clean, here,
     &remote, agreed)` (core algorithm; the last agreed digest comes from the stored row; the way is `Same|Send|Fetch`, with
     `both_changed` producing a safe copy of the loser).
  5. Record a `Synced` row (`outcome`, `files`, `both_changed`, `problem`). With `after_stream`, once verified, run
     `clear_desktop` (below).
  * `yielding`: wraps the Ally side in `Yielding`: **a game that has started here is looked for only at `protect()`, before
    anything changes** (then the copy aborts with `GAME_STARTED`, which `later()` treats as "retry later"). Once a copy has
    begun it always finishes, because a half-copied folder looks like a change on both sides and would be pushed over the
    desktop's real saves. The game's own start waits for the copy or is refused, so the copy need not stop.
* `later(text)` = the text contains `"still running"`; the sentinel the core and `GAME_STARTED` both use.

### 11.2 The three entry points

* **`all(library, game_id, launching) -> bool`**: take `BUSY` (20 s). If it cannot, return `false`. If not `launching` and a
  game runs, do nothing (`true`). `reach` all desktops in parallel threads (one slow desktop must not delay the rest), then
  `with(..., after_stream = false, yielding = !launching)` one after another. Called: before a local launch
  (`launching = true`), after a local game, and from `saves_sync`.
* **`before_stream(library, game_id, host, link) -> Result<(), String>`**: take `BUSY` or refuse (`launch_may_start(false)`);
  `with(...)` to send what the Ally played; then ask the desktop *again* what it holds now (`saves_placed`) and the **keep-clean
  gate**: if the desktop's administrator flagged that it clears its live copy of the saves after streams (`keep_clean`),
  and the Ally has saves, the stream is only allowed when `sync::may_stream(true, &ours, theirs)` says the desktop holds
  exactly the Ally's saves now; otherwise it is held back with `NOT_ON_DESKTOP` (otherwise the stream would start from
  nothing and its new saves would overwrite the real ones). If the desktop cannot be asked, go by the last stored
  `keep_clean` (persisted in the database so it survives a restart) via `sync::gate_on_failure`.
* **`after_stream(library, game_id, place)`**: after a played stream: wait `AFTER_STREAM_WAITS` = [3, 15, 30, 60] s between
  tries; each try takes `BUSY` (gives up silently on timeout), stops if a game is running (`Attempt::GameHere`), `reach`es
  the desktop, runs `with(after_stream = true, yielding = true)` (the desktop may have just woken, or the game may still be
  closing - "still running" -> try again), and `sync::retry_after(outcome, tries, &waits)` (core) decides the next wait or
  stops. Runs on the session's follow thread after the slot is free.

### 11.3 Keep-clean and retiring

`clear_desktop` (after a verified stream sync): if no game is active, compute the Ally's manifest and require that its digest
equals the agreed digest, then `remote.retire(&mine)`. Answers: `Done` (desktop removed its live copy: remember keep-clean,
note to the trail), `NotFlagged`/`Unsupported` (normal), `Kept(why)` (note), `Err(e)` -> the line `"Kept on the desktop: <e>"` is
stored as the sync row's `problem`. Success is never shown on the game page ("a success there would look like an error").

### 11.4 `saves_sync` command and the game page

`saves_sync(game_id)` calls `all(..., false)` then `views` (rows joined with host names, only if a local save folder exists).
The game page (`pregame.tsx`) calls it on every open and shows `inStep(...)` sentences.

---

## 12. Remote administration from a desktop (`remote.rs`)

* **Direction:** the Ally polls; it never listens. `remote::watch` is called once at start; `refresh` starts one `ask` thread
  per desktop that the administrator allowed (`store.host_admin(id)`, set by `hosts_set_admin`, which requires the PIN).
* `ask`: while the desktop is allowed: `reach`, `serve`, sleep 20 s. `serve`: while allowed, long-poll `link.admin_next()` (40 s);
  `Ok(Some(command))` -> `run`; `Ok(None)` -> poll again; `Err` -> return (the outer loop sleeps and retries).
  After `run`, `link.admin_done(&Done::of(id, result))` is tried up to 3 times (2 s apart); `record` appends a JSON line to
  `remote.log`.
* A desktop that was not allowed is never asked, so nothing it says reaches this code. Withdrawing permission takes effect at the
  next question (about 20 s) because the loop re-checks `allowed()`.
* **No PIN is asked for remote operations**: the permission was granted with the PIN when the administrator allowed the desktop.
  `Admin::cleanup_remote` is the PIN-bypassing variant of `cleanup`.
* Operations (`run`): the catalogue ops handled by `carla_core::remote::catalogue(&mut library.store(), op, args, now)`
  (store lock held for that call; emits `library://changed` unless the op is `games` or `desktops`), then `status`, `rescan`
  (full `library.scan`), `cleanup_scan|apply|undo`, `update` (refused while a game runs; downloads and runs the installer on the
  serve thread, then `app.exit(0)`), `startup` (read/set start-with-Windows), `art_find`, `art_set`, `art_clear`, `art_key`,
  `check` (with optional `repair`, refused while a game runs), `restart` (refused while a game runs; `crash::ended()` then
  `app.restart()`), anything else: `"This version of CarlaOS on the Ally does not know that request. Update it first."`.
* To add a remote operation: add the op name handling in `run`, keep it quick or accept that it blocks that desktop's serve
  thread, guard anything disruptive with `not_while_playing`, and extend the desktop side (companion / `carla_core::remote`).

---

## 13. Installs, downloads, storage, art, updates

### 13.1 Installing games (`installs.rs`)

* The store catalogue includes games that could be installed (`sync_local` on scan via `owned::not_installed(steam_root)`;
  `sync_host` from a desktop's `/v1/library`). `installs_start(game_id, place)`:
  `store.offered(game_id, place)` -> `(source, source_id, title)`; only `steam` is supported.
  Local: needs Steam root; refuses if a game is running **and** starting the install would restart (and close) Steam
  (`downloads::restart_closes_steam()`); `downloads::start_here` returns whether Steam is showing its own window
  (`Started::AskedSteam`). Desktop: `reach`, `link.install(game_id)` (`POST /v1/install`, `SLOW*2`), add to `WATCHED`.
  Sets `LOOK_NOW`, emits `library://downloads` immediately with a fresh `look()`.
* `look()`: local downloads from Steam's files; for each desktop that was fetching: `reach` + `link.downloads()`; list goes through
  `paced` (rate smoothed 0.6/0.4, rounded to 100 kB/s; ETA rounded to minutes beyond 2 min). A first install that disappears from
  the list triggers `library.scan(app)` (local) or `sync_games` (desktop, 20 s budget) and `library://changed`.
* `DownloadView{game_id, place, title, phase, done, total, update, rate, eta}`; phases from `carla_core::library::downloads::Phase`
  (page type: queued | downloading | paused | finishing).

### 13.2 Storage

`storage_overview`: disks Steam installs to (`downloads::disks`) and installed game footprints (`downloads::footprints`), joined with
`last_played`. `storage_uninstall` (admin): refuses the running game, asks Steam (`downloads::uninstall_here`; Steam shows its own
confirm), a thread polls every 2 s for up to 3 minutes for the game to disappear, then rescans and emits `library://changed`.

### 13.3 Pictures (`art.rs`, `autoart.rs`)

* Manual: `art_find` (candidates + previews fetched in parallel batches of 8 into `art/looking`, `art::MOST` cap),
  `art_use` (trusted source check `candidate.trusted()`), `art_pick` (local file, <= 16 MB), `art_clear`.
  Picture bytes must be a recognised type (`meta::picture_kind`: JPEG/PNG/WebP). Files are renamed each time
  (`<key>-<cover|wide>-<ms hex>.<ext>`) so the page never shows a cached older picture; replaced files that CarlaOS itself
  created under `art/chosen` are deleted.
* Automatic: `autoart::run` after each scan (unless the `auto_art` setting is `"0"`).
* The SteamGridDB key is a secret: stored sealed in the `settings` table, validated by shape, never returned to the page
  (`art_key` returns a bool).

### 13.4 Updates (`updates.rs`)

* `update_check` (open): GitHub release check for `Part::Ally`, plus, if the desktop host is installed on this same machine
  (`update::desktop_host_here`), for `Part::DesktopHost`. Returns `{current, available, host_here}`.
* `update_install` (admin): re-runs the check itself ("so the page cannot ask for anything but our releases"), downloads the
  host installer first (it works unseen and restarts the companion), then the Ally installer, which **ends this program**
  (`app.exit(0)`). Emits `update-progress {received, total}` with the two sizes summed.
  Everything is blocking on the command thread (hence `async`). **UNVERIFIED:** signature/checksum verification of installers
  (done in `carla_core::update`, not read); an approval-gated area per CLAUDE.md, so confirm before changing.

---

## 14. Quick menu, power, system menu

### 14.1 Quick menu (`quick.rs`)

* **Opening:** only while the session phase is `running`, the watcher reads the pad directly (XInput) because the game owns
  the controller and CarlaOS is not focused. The chord is View + Menu held `HOLD` = 500 ms (`carla_core::quick::Chord`).
* `open`: get-or-build the `quick` window; remember the front window as the game (`GAME_WINDOW`, `device::front`), set
  `OVER_CARLAOS` if the front window belongs to this process (menu over the library), show it, `bring_forward` (attaches thread
  input briefly), focus, `OPEN = true`, emit `quick://open` (so the page resets to the top and reloads levels).
* While open the watcher feeds a `Presses` state machine (initial repeat 380 ms then 130 ms) and sends
  `quick://press` with `"up"|"down"|"left"|"right"|"select"|"back"` **to the quick window only** (`emit_to`). The quick window
  also accepts the keyboard.
* `close`: hide the window and bring the game window forward (most games need it to take input again). `hide`: hide without
  restoring (caller decides). `ended`: destroy window and reset state (called by `follow` at session end).
* Commands (section 7.4). `quick_end_game` for a stream first asks the desktop to close the game (`hosts::end_game`, quiet) and
  then sets `stop` and `end` on the active session; the `follow` thread then kills Moonlight and ends the session.

### 14.2 Power (`power.rs`)

* A profile = `Power` (`quiet|balanced|turbo`) + `Screen` (`smooth|saver`), chosen per game **and kind** (local, stream, cloud)
  and stored in the catalogue (`carla_core::power::choose/profile/chosen`); defaults come from core.
* `apply(profile, kind)`: Windows only: `set_asus` (DeviceIoControl on `\\.\ATKACPI` with `asus_request`; silent no-op without
  the device), `set_windows` (loads `powrprof.dll`, calls `PowerSetActiveOverlayScheme` by name), and for kinds that set the
  screen: pick the refresh rate from the rates the current mode offers (`Screen::rate`) and `ChangeDisplaySettingsExW` with flag 0
  (not saved in the registry). `SCREEN_CHANGED` records that it must be put back.
* `rest()` between games (`RESTING`), `leave()` at exit (Balanced). Both call `screen_back()` (re-apply the saved registry mode).
* The `unsafe` blocks each carry a `// SAFETY:` comment; keep that convention.

### 14.3 System menu (`system.rs`, `device.rs`)

Sound (Core Audio endpoint volume, COM per thread), brightness (WMI `WmiMonitorBrightness*`, clamped 5..100 so the menu stays
readable), battery (`GetSystemPowerStatus`; none when 128 flag), Wi-Fi (`carla_core::network::wifi`), sleep (Modern Standby: broadcast
`SC_MONITORPOWER` off; else `SetSuspendState`), restart/off (`shutdown.exe`). The XInput `pad()` merges up to 4 controllers.
`system::watch` emits `system://woke` after a sleep; the page uses it to toast and refresh desktops when slept >= 120 s.

---

## 15. Crash, freeze and connection logging

### 15.1 Reports (`crash.rs`, `carla_core::crash`)

* Kinds: `panic` (panic hook), `unexpected` (previous run left the running marker: crash outside the hook, forced close,
  Windows shutdown, power cut), `hang` (watchdog).
* **Trail:** `crash::note(text)` appends a scrubbed line to a rolling trail written with each report. Use it for session
  steps, desktop reachability changes, stream events. The trail is merged by time with the connection log in `log_tail`.
* **Panic hook state line:** `crash::state()` returns what session was active using `try_lock`; if the lock is held it says
  `"the session state was being changed"` rather than blocking.
* **Watchdog (`watch_ui`, every 5 s):** judges the UI only when the `main` window `is_visible()` and not minimised **and** the page
  last said `hidden = false` (a covered or screen-off page has its timers slowed by the engine and would be falsely "frozen").
  `HEART.reset(now)` when the machine slept (tick gap > 15 s) or when judging (re)starts; `HEART.check(now, 30)` returns
  `Beat::Froze(secs)` after 30 s without a beat: notes it and `crash.write_hang(secs, &state())`. A later beat after a freeze
  notes "interface responding again". A running session does **not** excuse a silent window.
* Page side: beats every 3 s and on `visibilitychange` (`main.tsx`).
* `crash_reports`, `crash_report_text`, `crash_seen`, `crash_open_folder`; the page's "Crash reports" screen copies one.
  Only `panic` and `hang` reports are announced with a toast at start (`App.tsx`); `unexpected` waits in Settings.
* Scrubbing: `carla_core::crash::scrub` / `applog::hide_words` hide IPv4 addresses and the Windows account name; `conn_log`
  additionally replaces known computer names with `<pc>`.

### 15.2 Connection log (`conn_log.rs`)

`logs/connection.log` (<= 256 KB, cut to the newest half). Written: failed or slow (>= 1500 ms) link requests (`describe`: route,
method, path, outcome, ms), reachability transitions, timing of `hosts_refresh` that cut desktops or took >= 1.5 s, stream start
steps, session-end step timings, background game mirroring results. Repeats of the same failure of the same path are written
at most once per 60 s with a counter (`Repeats`). **Never written:** addresses, request/response bodies, headers, keys, PINs,
tokens, anything typed by the player. Missing cover (404 on `/v1/cover/`) is not news. Test coverage: `conn_log::tests`.
`line()` is a no-op before `open`.

---

## 16. Events emitted to the page

| Event | Emitted by | Payload | Listener |
| --- | --- | --- | --- |
| `library://session` | `library.rs` (`play`, `adopt`, `reconnect`, `follow`, waking closure) | `SessionView` or `null` (session over) | `library/store.ts::start` |
| `library://changed` | `hosts.rs` (`sync_in_background`), `installs.rs`, `autoart.rs`, `remote.rs` | `()` (catalogue changed behind the page's back) | `library/store.ts` -> `reload()` |
| `library://downloads` | `installs.rs` (`watch`, `installs_start`) | `DownloadView[]` | `library/store.ts` |
| `system://notice` | `library.rs::reconnect` (only user-visible use today) | one sentence `string` | `App.tsx` (toast) |
| `system://woke` | `system.rs::watch` | seconds slept (`i64`) | `App.tsx` |
| `update-progress` | `updates.rs::update_install` | `{received, total}` bytes | `updates/backend.ts::install` (listen for the duration of the call) |
| `quick://press` | `quick.rs::watch` via `emit_to("quick", ...)` | `"up" \| "down" \| "left" \| "right" \| "select" \| "back"` | `screens/quick.tsx` |
| `quick://open` | `quick.rs::open` via `window.emit` | `()` | `screens/quick.tsx` |

`window.emit` in Tauri 2 is generally a broadcast to all listeners (**UNVERIFIED:** confirm with the Tauri `Emitter` docs); the main
window does not listen for `quick://open`, so it is harmless either way. All emits ignore errors (`let _ =`).
Note one oddity: the constant `NOTICE_EVENT` lives in `library.rs` although the event name says "system"; `system.rs` owns `WOKE_EVENT`.

---

## 17. How to add a command safely (checklist)

1. **Decide the module.** Put it with related logic (`hosts.rs` for desktops, `library.rs` for catalogue/sessions, a new file only
   for a genuinely new area; if new, add `mod name;` in `main.rs`).
2. **Choose `async` or sync.**
   * Anything that can block: network, disk scan, process list, child process, dialog, waiting on a lock another thread may hold
     for long (`BUSY`), Windows COM/WMI/power calls, `thread::sleep` -> `#[tauri::command(async)]` with a **plain `fn`** (not an
     `async fn`; the attribute runs the sync body on the async runtime pool, so the main/UI thread stays responsive).
   * Only pure, instant work (reading a static, flipping an atomic, a tiny file read) -> plain `#[tauri::command]`.
     Sync commands run on the main thread and a stall freezes the window. When in doubt use `(async)`.
   * The Rust commands in this repo are all non-`async fn`; do not introduce `async fn` + `await` without checking how
     `State<'_, T>` borrows interact with it (Tauri requires `Result` returns for async fns with borrowed state).
3. **Admin gate.** If the command changes configuration, links/unlinks, installs/updates, reveals or stores secrets, runs
   anything destructive or administrative, or the page would hide it behind the PIN pad, add `admin: State<'_, Admin>` and call
   `admin.require()?;` as the **first line**. Do it in Rust even if the page already shows a PIN pad. Open commands must be
   things any player may do.
4. **Locks.** Follow section 6: copy data out of `library.store()` / `locked(&library.0.active)` into locals before slow work;
   do not use a lock guard as an `if let`/`match`/`for` scrutinee when the body does anything non-trivial; do not nest `store` and
   `active`; never keep a guard across `reach`, `link.*`, `moonlight::*`, `sysinfo` scans, dialogs, `sleep`. Run
   `cargo clippy --locked -p carlaos-shell -- -A clippy::all -D clippy::significant_drop_in_scrutinee`.
5. **Errors.** Return `Result<T, String>` with a **single plain sentence** the player can read. Use `.map_err(|e| e.to_string())`
   or a hand-written message; no `Debug`, no secrets, no raw addresses.
6. **State access.** Use `State<'_, Library>`, `State<'_, Admin>`, `AppHandle` as parameters (Tauri injects them); do not create
   globals unless you also add them to the lock map. `Library` is `Clone` (an `Arc`): clone it into threads you spawn. If a
   thread should survive past the command, document its interval and lock use here (section 5).
7. **Types crossing to the page** must derive `Serialize`; argument types `Deserialize`. Field names go over the wire as written
   (snake_case unless the struct has `#[serde(rename_all = "camelCase")]`, as `PowerView` does). Argument names are matched by
   **camelCase** from JS to snake_case in Rust: `invoke("hosts_remove", { id })`, `invoke("library_launch", { gameId, place })` maps
   to `game_id`.
8. **Register it** in the `tauri::generate_handler![...]` list in `main.rs` (path `module::name`). A command that is not
   listed compiles but every `invoke` fails at run time with "command not found".
9. **Add the page-side binding.** Calls to the shell are not all in `native.ts`: most live as methods on `library` in
   `library/store.ts` (with a browser-preview fallback behind `inTauri`), and others in `cleanup/backend.ts`, `updates/backend.ts`,
   `native.ts` (boot, crash, activity) or directly in `screens/quick.tsx` and `screens/sidemenu.tsx`. See chapter 05, section "How to
   add a setting / action". Update `data/types.ts` for new payload shapes.
10. **Long-running commands** should be added to nothing in `inflight.ts` unless they legitimately wait for a person or a long job
    (then add them to its `IGNORED` regex so they do not write "still running" notes).
11. **Events.** If the page must react to something that happens on a thread, emit an event (`app.emit("area://what", payload)`)
    and document it in section 16; the page must listen in `store.ts` or the component.
12. **Tests.** Shell tests are in-file `#[cfg(test)]` modules (`hosts::watch_tests`, `savesync::tests`, `conn_log::tests`); run
    `cargo test --locked -p carlaos-shell` (Windows CI job; needs the UI built). Write a test for any new lock-sensitive loop
    modelled on `a_round_with_a_desktop_that_answers_finishes_and_leaves_the_library_usable` (spawn it on a thread, wait with a
    timeout, then assert the library is still usable).
13. **Update docs/memory** if behaviour or a shared interface changed (Tauri command signatures are an approval-gated interface
    in `CLAUDE.md`: changing an existing command's signature needs agreement).

---

## 18. Tests that exist in the shell crate

* `hosts::watch_tests`: the 45 s pulse round finishes and leaves the store usable (the deadlock regression).
* `savesync::tests`: refusing a launch while saves are syncing; `take_turn` giving up and recovering; two syncs never overlap;
  `Yielding` finishes a copy it began and changes nothing if a game was seen before it began; the `GAME_STARTED` text counts as
  "retry later".
* `conn_log::tests`: what is worth writing, cover 404 handling, repeat grouping, name scrubbing, line format.
* There is no test that exercises a full `play`/`follow` session (needs a real `AppHandle` and process control), no test of
  `quick.rs`, `power.rs` (Windows), `device.rs`, `remote.rs::run`, or `installs.rs`.

---

## Open questions / needs investigation

1. **`installs_start` holds the `active` lock across a full process scan.** In
   `if let Some(running) = locked(&library.0.active).as_ref() { if downloads::restart_closes_steam() { ... } }` the guard
   (needed in the body for `running.view.title`) stays alive while `restart_closes_steam()` runs a `sysinfo` process refresh.
   It is short in practice but violates the rule in 6.1, and the CI lint does not flag it. Confirm by timing the call on the Ally,
   and consider copying the title out first.
2. **Where `async` commands run exactly.** This chapter relies on the documented Tauri 2 rule that plain sync commands run on
   the main thread and `(async)` ones on the runtime pool. Confirm with a one-line thread-name log, especially for the sync
   `admin_unlock` (50 000-round hash) and `crash_*` commands.
3. **Both windows beat `crash_alive`.** `main.tsx` runs for the `main` and the `quick` window, so the quick window (hidden or
   destroyed, `document.hidden` true) also reports `hidden = true` into the single `PAGE_HIDDEN` atomic, and its beats feed the
   same `HEART`. Whether this masks a frozen `main` window (the watchdog may skip judging) while the quick window is hidden or
   open is **UNVERIFIED**; confirm by logging `PAGE_HIDDEN` transitions during a game.
4. **`quick` window web-view arguments** (see 2.2) and whether `additionalBrowserArgs` applies to it.
5. **Installer integrity checks** in `carla_core::update` (signature/hash) were not read; relevant to `update_install` and the
   remote `update` op, which both download and execute an installer. Confirm before touching.
6. **`storage_overview` is not admin-gated** while the sibling `storage_uninstall` is. Confirm intent.
7. **Remote `update` runs on the serve thread** and can block that desktop's request loop for the whole download; confirm acceptable.
8. **Reachability constants.** `reach` fans out one thread per candidate with `QUICK` (3 s); whether the core client retries inside
   `ask_status` (which would stretch worst-case time beyond 3 s) was not read.
9. **`library_launch` is not in `inflight.ts`'s `IGNORED` list**, yet for a cold desktop it legitimately waits up to a couple of
   minutes, so a harmless "still running after 8 s" note is written to the trail on every streamed launch with a wake. Intended?
10. **Test gaps** listed in section 18; in particular nothing guards `follow`'s end-of-session ordering or `reconnect`.
11. **`GAME_WINDOW` is duplicated** in `quick.rs` and `presence.rs` with different meanings (window in front when the menu opened vs.
    last non-CarlaOS front window during a game). Confirm that the "Back to the game" path and the quick-menu "close" path never
    need to agree.
