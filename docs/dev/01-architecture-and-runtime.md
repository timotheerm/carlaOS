# 01 · Architecture and runtime

This chapter is the map. It explains what runs where, which threads exist, which locks they
share, and the rules that keep the whole system from freezing. Read it before changing anything
that touches the network, a background thread, the library database, or a process. The later
chapters go deep on each part:

| Chapter | Covers |
| --- | --- |
| [02-core-crate.md](02-core-crate.md) | `crates/core` (`carla-core`): the shared library used by both programs |
| [03-desktop-host.md](03-desktop-host.md) | `crates/companion` (`carla-companion`, "CarlaOS Desktop Host") |
| [04-ally-native.md](04-ally-native.md) | `apps/shell/src-tauri` (`carlaos-shell`): the Ally app's Rust side |
| [05-ally-ui.md](05-ally-ui.md) | `apps/shell/src`: the Ally app's React interface |
| [06-link-protocol.md](06-link-protocol.md) | The Ally ↔ desktop protocol, signing, every route |
| [07-data-and-storage.md](07-data-and-storage.md) | SQLite catalogue, migrations, every file on both machines |
| [08-build-test-release.md](08-build-test-release.md) | Toolchain, tests, CI, versions, installers |
| [09-how-to-recipes.md](09-how-to-recipes.md) | Step-by-step recipes for common changes |
| [10-pitfalls-and-open-questions.md](10-pitfalls-and-open-questions.md) | Lessons learned the hard way, and what is still unknown |

Product behaviour (what the player and administrator see) is described in the user-facing
documents in [`docs/`](../) (for example [STREAMING.md](../STREAMING.md),
[SAVE-SYNC.md](../SAVE-SYNC.md)). The approved architecture and milestones are in
[ARCHITECTURE.md](../ARCHITECTURE.md). Project status, decisions (ADRs) and known issues live in
[`knowledge/`](../../knowledge/START-HERE.md). This `docs/dev/` guide is about the code.

---

## 1. The product in one paragraph

CarlaOS turns an ASUS ROG Xbox Ally (RC73YA, Windows 11) into a console-like handheld. The
player turns it on and picks a game; everything else is set up beforehand or remotely by an
administrator. A game can run on the Ally itself, be streamed from a desktop PC in the house or
over Tailscale (stock Sunshine on the desktop, stock Moonlight on the Ally), or be a cloud game in
a browser window. CarlaOS launches games that are already installed; it never downloads games,
ROMs, firmware or keys and never bypasses DRM or anti-cheat. That boundary is non-negotiable and
reviewers check for it.

## 2. Two programs, one core

```
                   ┌───────────── Ally (or a laptop standing in for it) ─────────────┐
                   │  CarlaOS  (Tauri 2 app, identifier dev.carlaos.shell)           │
                   │   ├─ React UI  (apps/shell/src)        ── WebView2 window(s)    │
                   │   ├─ Rust side (apps/shell/src-tauri)  ── commands + watchers   │
                   │   │     └─ carla-core (catalogue, scanners, link client, ...)   │
                   │   └─ starts: Moonlight (stock, hidden), games, Edge (cloud)     │
                   └───────────────────────────┬─────────────────────────────────────┘
                     signed HTTP + UDP probes  │  (home network or Tailscale; no router ports)
                   ┌───────────────────────────┴─────────────────────────────────────┐
                   │  CarlaOS Desktop Host  (carla-companion, one per desktop PC)    │
                   │   ├─ background companion: HTTP API, dashboard, watchers        │
                   │   │     └─ carla-core                                           │
                   │   ├─ `carla-companion launch`: the program Sunshine starts      │
                   │   └─ manages: Sunshine (stock), screen mode / virtual screen    │
                   └─────────────────────────────────────────────────────────────────┘
```

| Cargo package | Path | Kind | Runs on |
| --- | --- | --- | --- |
| `carla-core` | `crates/core` | library | both |
| `carla-companion` | `crates/companion` | binary (bin target only, **no lib target**) | desktop |
| `carlaos-shell` | `apps/shell/src-tauri` | binary (Tauri app) | Ally |

The workspace root `Cargo.toml` lists all three as members but only `crates/core` and
`crates/companion` as `default-members`, because the shell needs WebView2 to build. A plain
`cargo test` at the root therefore never runs the shell's tests: use `-p carlaos-shell`
(see [08](08-build-test-release.md)). Edition 2021 for all crates. Release profile: LTO,
`panic = "abort"` (a panic ends the process; the shell installs a panic hook first so a crash
report is written).

## 3. Processes

### On the Ally

| Process | Started by | Lifetime | Notes |
| --- | --- | --- | --- |
| `carlaos-shell.exe` ("CarlaOS") | Windows at sign-in (Run key, when "start with the Ally" is on) or the player | Until quit / Exit to Windows | One instance. Two windows: `main` (the console UI) and `quick` (the quick menu, created on demand, destroyed when a game ends). See [04](04-ally-native.md). |
| `Moonlight.exe` | The shell, per stream (`carla_core::link::moonlight::stream`) and for pairing/`list` checks | One stream | Started with stdio set to null and no window of its own; watched with `try_wait`. |
| The game / store client / Edge | The shell (`carla_core::library::launch::start`) | One session | Watched by process folder or an explicit watch name. |

### On the desktop

| Process | Started by | Lifetime | Notes |
| --- | --- | --- | --- |
| `carla-companion.exe` (background) | Windows at sign-in, through a Run value the companion writes itself during its first-run setup (`autostart::set(true)` from `setup()` in `crates/companion/src/main.rs`); a fresh install does not write it (`packaging/host.nsi` only rewrites it on an update) | Always | HTTP API on TCP port 47600 and UDP probes on the same port number; loopback dashboard; watchers. |
| `carla-companion.exe launch` (the launcher) | **Sunshine**, as the command of its "CarlaOS" application, when Moonlight connects | One stream | Frees its console (`FreeConsole`) so it is not in the picture. **Every child it starts must get null stdio** or `CreateProcess` fails with "The handle is invalid" (os error 6); see [10](10-pitfalls-and-open-questions.md). |
| `sunshine.exe` / `SunshineService` | Windows, the user, or the companion (`setup::ensure_sunshine_running`) | Always | Stock Sunshine; the companion talks to its local web API only. |

Details of the installers and the smoke test that checks this: [03](03-desktop-host.md) §1 and
[08](08-build-test-release.md).

## 4. Threads and workers on the Ally

Everything the Ally does in the background is a plain `std::thread`. There is no async runtime
of our own; Tauri runs `#[tauri::command(async)]` commands on its own worker pool and
non-async `#[tauri::command]` commands **on the main (UI) thread**. Started in
`apps/shell/src-tauri/src/main.rs` (`setup`):

| Worker | Code | Interval / trigger | Network | Takes `Library.store()` | Notes |
| --- | --- | --- | --- | --- | --- |
| Quick-menu controller watch | `quick::watch` | 16–50 ms while a game runs, 0.5–1 s otherwise | no | rarely | Opens the quick menu on the controller chord; remembers the game window. |
| Remote administration | `remote::watch` | one thread per desktop allowed to administer this Ally: reach it, long-poll `Link::admin_next` and serve commands, then sleep 20 s and loop; the set of threads is refreshed at start and by `hosts_set_admin` | yes | yes, per command | Runs administrator commands sent from a desktop. |
| Desktop pulse ("watch") | `hosts::watch` → `hosts::pulse_all` | every 45 s (`PULSE_EVERY`), first round 45 s after start | yes | yes (copies the host list, then per host) | Asks every desktop how it is, measures lightly, tells each desktop how the Ally is. **This is where the v0.20–v0.34 freeze lived** (§7). |
| Downloads/installs watch | `installs::watch` | every 4 s while something downloads, 30 s when idle, 60 s while a game runs (checked in 0.5 s slices) | yes | yes | Steam install progress on this Ally and on desktops. |
| Presence (spotting games started outside CarlaOS) | `presence::watch` | every 5 s (`LOOK_EVERY`) | no | yes | Adopts a running game as a session. |
| System (volume/brightness/battery, resting power) | `system::watch` | every 1 s | no | no (reads `active`) | |
| Freeze watchdog | `crash::watch_ui` | every 5 s | no | no | Writes a "Stopped responding" report when the visible page stops sending its heartbeat for 30 s. |
| Power rest | `power::rest` (one-shot thread) | at start | no | no | Puts Windows in the resting power mode. |
| Session follower | `library::follow` | one per game/stream, polls every 2 s (`POLL`) | via savesync/hosts | yes | Watches a running game or Moonlight; ends the session; runs save sync. |
| Background game sync | `hosts::start_syncing` | one per desktop at a time, ≤ 20 s budget, at most every 5 min | yes | yes (short) | Mirrors a desktop's game list and covers into the catalogue. |
| Save sync after a stream | `savesync::after_stream` | 3/15/30/60 s retries after a stream | yes | yes (short) | Serialised by `savesync::BUSY`. |
| Auto artwork | `autoart` | background, single-flight | yes (Steam, Wikipedia, SteamGridDB) | yes | |

The interface itself also polls: `apps/shell/src/library/store.ts` calls `hosts_refresh` on an
interval while the window is visible and no game is running (with a 15 s client-side timeout and
a 10 s native deadline), plus event listeners. See [05](05-ally-ui.md).

## 5. Locks on the Ally (lock map)

All locks are `std::sync::Mutex` (non-reentrant). Poisoning is ignored on purpose through the
`library::locked()` helper (`lock().unwrap_or_else(|e| e.into_inner())`).

| Lock | Where | Guards | Rule |
| --- | --- | --- | --- |
| `Library.0.store` (via `Library::store()`) | `library.rs` | The SQLite `Store` (catalogue, hosts, sessions, settings) | **The shared one.** Hold it for one database call only. Never across a network request, a process start/stop, a Windows setting change, or a call to a function that takes it again. |
| `Library.0.active` | `library.rs` | The one "now playing" session slot (`Active`) | Short. Claimed by `claim()`, cleared at session end. Readers use it to know whether a game runs. |
| `Library.0.steam_root` | `library.rs` | Cached Steam folder | Short. |
| `savesync::BUSY` | `savesync.rs` | One save-sync pass at a time | Taken with a time limit (`link::deadline::lock_within`, 20 s); a launch is refused rather than started while a pass may still write. |
| `hosts::SYNCING`, `LAST_SYNC`, `STATE`, `PAIRED` (`paired_cache()`), `ASKING` | `hosts.rs` | Background-sync bookkeeping, reachability tracking, Moonlight pairing cache, the link-in-progress | Short; never held across I/O. |
| `remote::ASKING` | `remote.rs` | Which desktops have a long-poll thread | Short. |
| `installs::PACE`, `WATCHED` | `installs.rs` | Download progress pacing | Short. |
| `conn_log::NAMES`, `REPEATS` | `conn_log.rs` | Names hidden in logs, repeat suppression | Short. |
| `carla_core::crash` writer lock / `applog` writer lock | core | Appending to the trail / log files | The panic hook uses a `try_lock` variant so it can never wait. |

Lock order, where two are taken: `BUSY` before `store`/`active`. Nothing takes `store` and then
waits for another lock while still holding it.

### The rule that matters most: guards in headers

In Rust 2021 a temporary created in the header of a `for`, `if let`, `while let` or `match`
lives **until the end of the whole construct**, not the end of the header. So

```rust
for host in library.store().hosts().unwrap_or_default() {   // guard lives for the whole loop
    seen(&library, &host, ...);                              // takes library.store() again: never returns
}
```

holds the store lock for the whole loop. Write instead:

```rust
let hosts = library.store().hosts().unwrap_or_default();     // guard dropped at the semicolon
for host in hosts { ... }
```

CI enforces this for the shell with
`cargo clippy -p carlaos-shell -- -A clippy::all -D clippy::significant_drop_in_scrutinee`
(the lint flags exactly this shape). Apply the same rule in `crates/core` and
`crates/companion` even though the lint is not enforced there yet.

## 6. Threads and locks on the desktop (summary)

The companion serves HTTP with `tiny_http` and a fixed pool of worker threads (8), plus a capped
number of "long" slots for slow routes (saves, pairing, installs, the administrator long-poll,
dashboard scans). Socket read/write timeouts (10 s, Windows) and per-route body deadlines stop a
dead client from holding a worker. Request lines go to `host.log` through a bounded queue and one
writer thread, so the disk never delays a worker. Shared state lives in `Files` (JSON files in
the data directory, written atomically) and small in-memory caches (game scan, Sunshine health)
that are filled outside their locks and served stale while a refresh runs. Details, every thread
and every limit: [03-desktop-host.md](03-desktop-host.md).

## 7. Post-mortem: the client freeze (v0.20.0 – v0.34.0)

Kept here because it explains several rules above and several defensive pieces of code that may
now be simplified.

**Symptom.** 10–60 s after CarlaOS started (sooner if the player measured connections), the
Ally stopped responding to anything that touched the library: game list, local game launches,
measuring, refreshing, streaming. The window still drew; only a restart helped. The desktop's
`host.log` showed the Ally's last request as a lone `GET /v1/status` and then silence.

**Cause.** `hosts::watch` iterated `library.store().hosts()...` in a `for` header (guard held for
the loop, §5) and called `seen()`/`measure_host()`, which lock the store again, on the same
thread. A `std::sync::Mutex` locked twice on one thread never returns on Windows. The first round
after a desktop answered (45 s after start) deadlocked that thread while it held the store lock,
and every other user of the library waited for ever behind it. Introduced in v0.20.0
(commit `15dbb1b`, PR #34). Fixed in v0.34.1 (PR #61): the list is copied out first; a
regression test (`hosts::watch_tests::a_round_with_a_desktop_that_answers_finishes_and_leaves_the_library_usable`)
drives the real round with the real `Library`; CI runs the shell tests and the clippy lint.

**What earlier fixes did.** Before the cause was found, several changes defended against the
symptoms: a 10 s deadline on `hosts_refresh` and a 15 s interface timeout (PR #53), a 20 s limit
on the save-sync lock with launch refusal (PR #53), a wrapper timing every interface command
(PR #56, which itself caused a white screen in 0.33.0, fixed in PR #58), and a freeze watchdog.
They are harmless but were aimed at the wrong target. **Recommendation (not done yet):** after the
fix is confirmed on the device for a while, review them for removal or simplification; see
[10](10-pitfalls-and-open-questions.md).

## 8. Where a request goes: three common flows

**Measuring a desktop (Settings → Desktops → Measure).** UI `hosts_measure` → `hosts::reach`
(asks every known address in parallel, `GET /v1/status`, 3 s) → `seen` (store) →
`measure_host` → `carla_core::link::quality::survey` (HTTP probes `/v1/probe/{bytes}`: empty
probes for delay, 128 KB then up to 2 MB for speed; UDP echo; for a full measure a 1 s UDP
burst) → `set_host_health` (store) → `link.pulse` (tells the desktop). No lock is held during
the network part.

**Streaming a game.** UI `library_launch(game, place=desktop id)` → `library::play` → `claim`
(the session slot) → `hosts::start_stream`: reach (or wake with Wake-on-LAN, up to 90 s) →
`savesync::before_stream` (the Ally's newer saves go first; a keep-clean desktop gates the start)
→ choose the picture (measures quickly) → `Link::prepare_for` (the desktop records what to start
and switches its screen) → `ensure_paired` (Moonlight ↔ Sunshine) → start Moonlight hidden →
`library::follow` watches Moonlight until it exits → session end → `savesync::after_stream`. On
the desktop, Sunshine starts `carla-companion launch` when Moonlight connects; the launcher reads
the pending request, starts the game, watches it, and exits when it closes, which ends the stream.

**Starting a local game.** UI `library_launch(game, place="local")` → `play` → `claim` →
power profile → `savesync::all(launching=true)` (fetch newer saves from desktops; refused if a
previous pass still holds the save lock) → `launch::start` → `follow`.

## 9. Configuration is data

Everything the administrator sets is plain data: the SQLite catalogue on the Ally and JSON files
on each desktop. There is no registry configuration of our own except the Run key for starting
with Windows (`boot.rs`) and the reversible Windows tuning (`crates/core/src/debloat`, every
change recorded with its previous value). See [07-data-and-storage.md](07-data-and-storage.md).

## 10. Open questions / needs investigation

- **UNVERIFIED:** whether the quick-menu window is ever created or destroyed off the main thread
  in a way Tauri does not allow on Windows (it is destroyed from the session follower thread in
  `quick::ended`). No hang has been traced to it since the store-lock fix, but it was a suspect.
- The defensive timeouts added before the freeze was understood (§7) have not been re-evaluated.
- `crates/core` and `crates/companion` are not linted with `significant_drop_in_scrutinee` in CI;
  the companion has several `if let ... = locked(...)` headers that are currently harmless.
