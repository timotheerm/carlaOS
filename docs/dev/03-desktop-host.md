# 03 - The desktop host (`carla-companion`)

Audience: an AI or senior developer who has never seen CarlaOS and must add or change desktop-side features safely.
Scope: everything under `crates/companion/**`, plus the parts of `packaging/` and `scripts/` that install, start or test it.
Facts here come from reading the code at base commit `bd34c68`; user-facing behaviour is described in `docs/*.md` (for example `docs/STREAMING.md`, `docs/DASHBOARD.md`, `docs/SAVE-SYNC.md`, `docs/STREAM-SCREEN.md`), which this file deliberately does not repeat.
Anything not confirmed by code is marked **UNVERIFIED:** with what would confirm it.

Conventions used below: "Ally" is the handheld running the CarlaOS shell (the Tauri app in `apps/shell`); "desktop" is the Windows PC running this program; "core" is the `carla-core` crate (`crates/core`), which holds everything shared with the shell, including the link protocol (`crates/core/src/link`).

---

## 1. What it is and how it runs

### 1.1 One binary, three roles

The desktop host is a single executable, `carla-companion.exe` (package `carla-companion`, `crates/companion/Cargo.toml`; Windows product name "CarlaOS Desktop Host"). The same binary acts as:

1. **The background companion** (`carla-companion serve`): a long-running process with no window. It runs the HTTP API the Ally talks to (TCP port 47600, `carla_core::link::PORT`), answers UDP discovery and measuring packets on the same port number, serves the local dashboard, and runs the background watchers (section 5).
2. **The launcher** (`carla-companion launch`): a short-lived process that Sunshine starts for each stream. It starts the chosen game, keeps itself alive for as long as the game runs, and exits when the game has closed, which is how Sunshine learns the stream is over.
3. **A command-line tool**: every other subcommand (section 6) is an administrator command that reads or writes the data directory or talks to Sunshine, and (for `ally ...`) cooperates with the running background companion.

The three roles share state only through **files in the data directory** (section 7) and the Sunshine web API. There is no IPC between the launcher and the background companion other than small JSON note files (`pending.json`, `stream.json`, `playing.json`, `reveal.json`, `end.json`, `screen.json`, `virtual.json`).

The binary is a **console-subsystem** executable: there is no `#![windows_subsystem = "windows"]` anywhere in `crates/companion`. That single fact explains the two "no window" mechanisms below.

### 1.2 Start-up with Windows

- `autostart::set(true)` in `crates/companion/src/main.rs` writes the per-user registry value `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\CarlaOS Companion` with the command line

  `conhost.exe --headless "<path to carla-companion.exe>" serve`

  `conhost.exe --headless` runs a console program with no visible window. `autostart::set(false)` (called by `carla-companion remove`) deletes the value. On non-Windows `autostart::set` is a no-op returning `Ok`.
- It is called from `setup()` (which runs when the user double-clicks the program, or `carla-companion setup`, or when `carla-companion dashboard` finds no config). It is never called by `serve`.
- The same `conhost.exe --headless <exe> serve` form is used by `carla-companion dashboard` when it has to start the companion (with a plain `Command::new(exe).arg("serve")` fallback if conhost cannot be spawned), and by the installer (below).
- The installer `packaging/host.nsi` installs per user (`RequestExecutionLevel user`) into `$LOCALAPPDATA\Programs\CarlaOS Desktop Host`, creates Start-menu entries "CarlaOS Desktop Host" (no argument), "CarlaOS Dashboard" (argument `dashboard`, also on the desktop), "Update CarlaOS Desktop Host" (argument `update`), and writes the uninstall key. When it detects that the Run value already exists (an update), it rewrites the value to the new install path and starts `conhost.exe --headless "$INSTDIR\carla-companion.exe" serve` immediately so the Ally does not lose the desktop until the next sign-in. Before copying files it runs `taskkill /F /IM carla-companion.exe` (a running exe is locked). Uninstalling runs `carla-companion remove`, deletes the Run value, kills the process, and **leaves the data directory alone** (installing, updating, downgrading and uninstalling never touch it).
- `packaging/full.nsi` (the combined installer) offers the host as a section and, at the end, runs `carla-companion.exe` with no arguments (the "set up this desktop" step).
- `packaging/smoke-test.ps1` asserts: the installed exe answers `info` with the expected version, an update keeps the Run value pointing at the installed copy in the `conhost.exe --headless "<exe>" serve` form, and uninstall removes the exe. Treat that exact Run-value string as a contract: the installers and `main.rs` must agree.
- `packaging/package.mjs` expects `target/release/carla-companion.exe` (built with `cargo build --release --locked -p carla-companion`, see `.github/workflows/build.yml`) and wraps it with `host.nsi`.

### 1.3 The launcher process Sunshine runs, and the null-stdio rule

Sunshine knows exactly one application, named `CarlaOS` (`carla_core::link::SUNSHINE_APP`). Its `cmd` is `"<exe path>" launch` (function `launcher_command()` in `main.rs`; the path is the running binary's own path, quoted). Sunshine starts that command in a console of its own and passes the stream's picture size in environment variables `SUNSHINE_CLIENT_WIDTH`, `SUNSHINE_CLIENT_HEIGHT`, `SUNSHINE_CLIENT_FPS` (read by `screen::asked_by_sunshine`).

Because the binary is console-subsystem, that console would appear on the desktop and therefore **in the stream's picture**. So `launcher::run` immediately calls `hide_console()`, which on Windows calls `FreeConsole()` from `kernel32`.

Consequence (the **null-stdio rule**): once the console is freed, the standard handles Windows passed down are invalid, and any child process spawned with inherited stdio fails with "The handle is invalid" (os error 6); the game would never start and the stream would end at once. Therefore **every process the launcher (or anything it calls) spawns must have `stdin`, `stdout`, `stderr` set to `Stdio::null()`**:

- `carla_core::library::launch::quiet(&mut Command)` (private to `crates/core/src/library/launch.rs`) does this for every game/store/URL start (`launch::start`, `open_address`, and the other call sites in that file).
- `unseen()` in `crates/companion/src/setup.rs` does the same plus `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP` for Sunshine start (`sc start SunshineService`, or spawning `sunshine.exe`).
- Any new code reachable from `launcher::run` (launch, `screen::switch`, `virtual_screen::*`, `stage::Curtain`) must not use `Command::status()/output()/spawn()` without nulling stdio, and must not `println!`/`eprintln!` expecting output; it must write to `Files::log` (section 12) instead. `println!` on an invalid handle is silently lost in Rust; **UNVERIFIED:** whether `eprintln!` can ever panic here (Rust ignores a missing stderr handle, but it has not been exercised in this repo; confirm by running `carla-companion launch` under a Sunshine-like parent with no console).
- Code paths that run only in the interactive CLI (setup, `winget`, PowerShell elevation, `firewall`) do not have this restriction, since they run in a real console or hidden shell.

### 1.4 Process and port lifecycle

- `serve(files, config)` in `main.rs` binds `0.0.0.0:<config.port>` through `server::bind`. If the bind fails (port taken) it logs "Could not listen on port ... another companion most likely runs already." and returns `Err`. `carla-companion serve` then exits 1; the no-argument mode (below) instead sleeps forever so its window can still answer questions.
- **No-argument mode** (double-click): `setup` if there is no `config.json`; `show_code` (writes `enrol.json`); spawns the `attend` thread (polls `enrol-ask.json` every 500 ms and asks "Is that your Ally, showing that number? (y/n)" on stdin, writing `enrol-answer.json`); spawns a thread that opens the dashboard after 800 ms; then runs `serve` **in this process**. If another companion already holds the port, this window only keeps `attend` alive (sleeps in 1-hour steps). This is why linking approval works through the files, not through HTTP: whichever process has the console answers.
- `dashboard` command: see 6.2.
- `update_from_afar` (Ally asks over `POST /v1/update`) and `update_here` (CLI) both end in `carla_core::update::install(Part::DesktopHost, ...)`, whose installer stops the running companion (taskkill) and starts the new one (section 1.2). The API refuses while a game runs (`409`).
- `sleep_soon` puts the PC to sleep 3 s after the reply, via `SetSuspendState(0,0,0)` in `powrprof` (sleep, not hibernate, wake events left on).

---

## 2. Module map

All files are in `crates/companion/src/`. `main.rs` declares `mod ally; describe; files; launcher; logview; screen; server; setup; stage; sunshine; virtual_screen;`. `dashboard.rs` is **not** declared in `main.rs`: `server.rs` includes it as a child module with `#[path = "dashboard.rs"] mod dashboard;` and `dashboard.rs` starts with `use super::*;`, so it sees `Server`'s private fields and helpers (`locked`, `json`, `refuse`, `body`, `label`, `Answer`, ...). `dashboard.html` is compiled in with `include_str!("dashboard.html")` (edit, then rebuild).

| File | Lines (approx) | Responsibility | Key items | Side effects |
| --- | --- | --- | --- | --- |
| `main.rs` | 1300 | Entry point, CLI dispatch, set-up, wiring | `run`, `main`, `HELP`, `setup`, `sunshine_ready`, `serve`, `build`, `dashboard`, `attend`, `show_code`, `allies`, `console`, `saves`, `find_saves`, `stream`, `parse_add`, `parse_clean`, `autostart`, `power`, `vitals`, `network`, `installed`, `update_from_afar`, `update_here` | Registry Run key, firewall (via setup.rs), Sunshine install/claim, spawns threads, prints to console, `std::process::exit` |
| `server.rs` | 3100 (about 1600 code, 1500 tests) | The API: `Server` struct, signed routes, enrolment, HTTP worker pool, request log | `Server`, `Request`, `Reply`, `Installs`, `Server::handle/signed/trusted`, `bind`, `bind_within`, `serve`, `serve_with`, `work`, `Load`, `Limits`, `Slot`, `read_body`, `body_time`, `route_shape`, `who`, `request_line`, `noisy`, `notice`, `takes_long` | Network I/O, writes data files via `Files`, spawns threads |
| `dashboard.rs` (+ `dashboard.html`) | 1000 + 570 | The local dashboard: `/` page and `/local/*` JSON routes | `Server::local`, `overview`, `allies`, `stream_view`, `checks`, `games_view`, `find_games`, `add_found`, `forget`, `install_driver`, `newer`, `own_name`, `pulsed`, `asked_for`, `paired_names`, `Vitals` | Writes files (new code, forget, add game), spawns driver-install thread |
| `files.rs` | 940 | All on-disk state of the data directory | `Files`, `Config`, `Device`, `Code`, `Extra`, `SaveFolder`, `Suggestion`, `Pending`, `Playing`, `Streaming`, `Seen`, `Pulsed`, `default_dir`, `now` | File reads/writes/removes, `host.log`/`launcher.log` appends, DPAPI sealing via `carla_core::secret` |
| `launcher.rs` | 280 | What Sunshine runs for a stream | `run`, `wait`, `play`, `rehearse`, `hide_console`, `POLL` (2 s) | `FreeConsole`, starts the game, switches screens, draws the curtain, writes `stream.json`/`playing.json` |
| `stage.rs` | 780 | The "curtain": a pink loading window over the desktop during a stream | `Stage` (pure decision logic), `Look`, `Scene`, `Window`, `Role`, `Curtain`, `role()`, `LIFT_AFTER` (75 s), `os` module (Win32) | Creates a top-most window, raises/lowers store windows, steals focus for the game |
| `screen.rs` | 220 | Switch the desktop's real screen mode for a stream and always put it back | `Screen` trait, `Main`, `asked_by_sunshine`, `switch`, `put_back`, `tidy`, `watch` | Display mode changes, `screen.json`, log lines |
| `virtual_screen.rs` | 480 | A virtual monitor per stream via the Virtual Display Driver (VDD) | `Driver` trait, `Vdd`, `enter`, `leave`, `tidy`, `settle`, `watch`, `install`, `with_mode`, `GRACE` (90 s) | Named pipe `\\.\pipe\MTTVirtualDisplayPipe`, edits `vdd_settings.xml`, display layout, `virtual.json`, `winget` |
| `sunshine.rs` | 300 | Client for Sunshine's local web API | `Streamer` trait, `Sunshine`, `answers`, `claim_fresh`, `app`, `place`, `pick` | HTTPS to `https://localhost:47990` (certificate verification disabled, loopback only) |
| `setup.rs` | 445 | First-run helpers: install Sunshine, claim credentials, firewall, start Sunshine | `USER`, `RULE`, `sunshine_exe`, `install_sunshine`, `Starter`, `Launch`, `Machine`, `claim`, `Claimed`, `firewall_commands`, `open_firewall`, `reset_credentials`, `elevated`, `unseen` | `winget`, `netsh`, elevated PowerShell (UAC), `sc start`, spawns `sunshine.exe` |
| `ally.rs` | 540 | `carla-companion ally ...`: administer an Ally through the running companion | `request`, `show`, `ask`, `run`, `Request`, `HELP`, `sheet`, `chosen` | Writes `ally-ask-<id>.json`, reads `ally-answer-<id>.json`, writes `art-choices.json/html`, opens a browser |
| `describe.rs` | 170 | `carla-companion describe` (text for games added by hand; Steam/Wikipedia lookup) | `parse`, `run`, `lookup_with`, `same_game` | Network lookup via `carla_core::library::meta` (title only), writes `games.json` |
| `logview.rs` | 220 | `carla-companion log` | `parse`, `Tail`, `show`, `run` | Reads `host.log` / `launcher.log`, prints |

### 2.1 Notes per module

**`files.rs` (`Files`)** is a cheap `Clone` handle holding only the directory path and an optional `Env` (test override for well-known folders). It has **no internal cache and no lock**; every accessor reads or writes the JSON file on each call. Writer pattern (`Files::write`): serialise with `serde_json::to_string_pretty`; if the file already contains exactly that text, do nothing; otherwise write `<name>.<pid>.<counter>.new` next to it and `rename` over the target (so readers never see half a file and two writers never share a temp file). Reader (`Files::read`) returns `None` for a missing file **or a file that fails to parse**, so every `Option`/`unwrap_or_default` caller treats corruption as "absent". Read-modify-write sequences (`add_device`, `add_extra`, `set_save_folder`, ...) are not serialised by a lock (only `SUGGESTIONS` serialises suggestion changes, and `saves_step` serialises save-folder changes), so two concurrent modifications of the same file can lose one update. In practice the writers are an administrator at a keyboard, the dashboard, or an Ally, so this is rare, but do not assume atomic updates. **UNVERIFIED:** whether any two real writers (for example `add_device` during `enrol_finish` and `forget_device` from the dashboard) can overlap in practice; would be confirmed by a concurrency test.

**`server.rs`**: `Server` is constructed with `Server::new(files, config, streamer, scan, launcher, hardware, network, achievements, sleep)` and customised with builder methods `with_updater`, `with_dashboard`, `with_virtual_screens`, `with_starter`, `with_driver_installer`, `with_installs`, `with_graphics`, `with_probe`, and (tests only) `with_finder`, `with_admin_hold`. All "I/O to the world" is injected (`streamer: Option<Box<dyn Streamer>>`, `scan`, `network`, `achievements`, `sleep`, `updater`, `installs`, `screens: Option<Box<dyn Driver>>`, `finder`, `vitals`, `newer`), which is what makes the server testable without Windows, Steam or Sunshine. When you add a new external dependency to the server, follow this pattern: add a field with a harmless default in `Server::new`, a `with_*` builder, wire the real one in `build()` in `main.rs`, and pass a fake in `desk_with` in the tests.

**`main.rs` wiring** (`build`): `streamer(&config).ok()` gives `None` when Sunshine credentials are missing (the server then answers 503 "The companion has not been given Sunshine's user name and password..." for anything needing Sunshine); `scan = installed` (all `carla_core::library::STORES` via `scan_store`, using `steam::find_root()`); `Starter::for_machine`; `.with_updater(update_from_afar)`; `.with_dashboard(vitals, || update::check(Part::DesktopHost))`; `.with_virtual_screens(Box::new(virtual_screen::Vdd))`; `.with_driver_installer(Arc::new(virtual_screen::install))`; `.with_installs(Installs {...})` for Steam library installs through `carla_core::library::{owned, downloads}`. `serve()` additionally adds `.with_graphics(carla_core::link::codec::graphics())` and `.with_probe(udp_clone)`.

---

## 3. The HTTP server internals

Source: `crates/companion/src/server.rs`. Library: `tiny_http` 0.12 (synchronous, thread-per-connection inside the library; we add our own worker pool on top).

### 3.1 Binding and socket timeouts

- `bind(addr)` calls `bind_within(addr, IO_WAIT)` where `IO_WAIT = 10 s`. It creates a `std::net::TcpListener`, sets `SO_RCVTIMEO` and `SO_SNDTIMEO` on the **listening** socket via a hand-written `setsockopt` FFI into `ws2_32` (Windows only), and relies on Windows making accepted sockets inherit those options. `tiny_http` has no timeouts of its own.
- Inheritance is **verified, not assumed**: `inherited()` connects to the listener itself on loopback, accepts that connection and reads `read_timeout()` back. The result `Result<(), String>` is the second element of the tuple `bind` returns. `serve()` in `main.rs` logs either "Socket timeouts active: a client that goes quiet mid-request is let go." or "WARNING: socket timeouts are not active (...); relying on the body deadline only." (also to stderr).
- On non-Windows `socket_timeouts` is a no-op, so `inherited` fails and the result is `Err`; test `the_timeouts_of_accepted_sockets_are_checked_not_assumed` asserts `Ok` on Windows and `Err` elsewhere.
- UDP: `serve()` also binds `UdpSocket` on the same port number. A clone goes to `Server::with_probe` (used by `POST /v1/probe/burst` to send measurement bursts from the port the Ally asked at), and the original to a thread running `carla_core::link::discover::answer(&socket, &|| server.hello())`. **UNVERIFIED:** the exact UDP packet formats (discovery question `carlaos? v1`, echo packets); confirm in `crates/core/src/link/discover.rs` and `probe.rs`.
- Windows firewall: `setup::firewall_commands` adds inbound allow rules named `CarlaOS Desktop Host` for TCP and UDP `localport=47600`, `remoteip=localsubnet,100.64.0.0/10` (home network and Tailscale CGNAT range only), program-scoped, `profile=any`.

### 3.2 The worker pool

`serve_with(server: Arc<Server>, http: tiny_http::Server, limits: Limits)`:

- `Limits { workers, long, body }`, default `WORKERS = 8`, `LONG_AT_ONCE = 4`, `BODY_WITHIN = 30 s`. Tests pass their own.
- It spawns **`limits.workers` threads**, each looping `while let Ok(request) = http.recv() { work(&server, &load, request) }`; `serve_with` then `join`s them (so it blocks for the life of the process).
- One request occupies one worker for its whole life, including a slow body upload and any long wait inside a handler. The design goal: slow requests must never make the Ally's cheap requests (status, games, pulse) wait.
- Counters in `Load` (shared via `Arc`):
  - `busy: AtomicUsize` with RAII `Slot`; `Load::start()` returns `(Slot, crowded)`, `crowded` when `busy >= workers`. When crowded, `warn_due()` (at most once per 10 s) writes "All N workers are busy: new requests wait until one is free."
  - `long: AtomicUsize`; `Load::start_long()` returns `None` when `long` slots are all taken (see 3.3).
  - `detached: AtomicUsize`, limit `DETACHED_AT_ONCE = 16` (see 3.5).
  - `warned: Mutex<Option<Instant>>`, `lines: Mutex<Option<SyncSender<String>>>`.

### 3.3 Long-request slots

`takes_long(path)` is true for `/v1/saves/*` (all save steps and `saves/find`), `/v1/admin/next`, `/v1/install`, `/v1/stream/pair`, `/local/find`, `/local/games`. Rules in `work`:

- A long request needs `trusted = linked_device || (loopback && path starts with "/local/")`.
- **Untrusted** long requests (a stranger, or an unknown device id) do not take a slot and their body limit is cut to 4096 bytes. They then fall through to `Server::handle`, which refuses them with 401.
- **Trusted** long requests take a slot via `start_long()`. If all `long` (4) slots are taken the request is **not read**; the reply is 503 "This desktop is busy with other requests. Try again in a moment." and the line "Too many long requests at once: ... was turned away." is logged.
- With 8 workers and 4 long slots, at least 4 workers are always free for short requests.
- When you add a route that can block (waits on Sunshine, disk scans, user interaction, long polling), add its path to `takes_long`. The existing long-polling route is `GET /v1/admin/next`, which holds its worker up to `admin_hold` (20 s; tests use 400 ms) in a 200 ms sleep loop.
- A single-flight guard exists for the dashboard's "look for games": `Server.finding: AtomicBool` makes a second click return 409 instead of occupying another worker.

### 3.4 Request body reading and deadlines

In `work`, before anything is read:

- Max body (`most`): 64 KiB by default; for a **linked** device only: a save write (`/v1/saves/<key>/write`) `sync::LARGEST_FILE + 4096` (32 MiB + 4 KiB, `LARGEST_FILE = 32 MiB` in `crates/core/src/library/sync.rs`), `/v1/saves/<key>/retire` 1 MiB, `/v1/admin/done` 2 MiB. Nothing large is ever read from an unknown device before its signature is checked (`Server::knows(device)` only checks that the id is in `devices.json`; the signature check happens later in `handle`).
- Time to receive the body (`body_time(base = limits.body, most)`): if `most <= 64 KiB` then `min(base, 5 s)`; else `base + most/1 MiB seconds`, capped at `max(180 s, base)`. With defaults: small bodies 5 s, a 32 MiB save 62 s.
- `read_body(reader, most, within)`: wraps the reader in `take(most)`, reads 16 KiB chunks, and returns `TimedOut` if the deadline passes between chunks. This bounds a client that trickles bytes (each read succeeds, so socket timeouts alone would never fire). Socket timeouts (10 s per read/write) bound a client that goes silent or dies.
- Outcome mapping: `TimedOut`/`WouldBlock` error becomes HTTP 408 "The request did not arrive in time."; any other read error becomes 400; a successful body goes to `local()` (if loopback) or `handle()`.

### 3.5 Detached responders

`tiny_http` reads whatever remains of a request body before it lets a request be dropped/answered. For a client that vanished that takes up to a socket timeout. If the body read failed or the request was turned away as `too_many`, `work` therefore does not call `respond` on the worker. Instead:

- `load.start_detached()` succeeds (fewer than 16 detached threads): a new thread calls `request.respond(response)` and decrements the counter. The worker is free immediately.
- Otherwise (16 already waiting): `std::mem::forget(request)` and the line "Too many connections are stuck mid-request: one was let go without an answer." The connection is left open and unanswered rather than holding a worker.

For normal successful reads the worker calls `request.respond(response)` inline.

### 3.6 Routing and trust

`Server::handle(&Request, now) -> Reply` is the whole protocol layer and is testable without a socket:

1. **Open routes** (no signature): `GET /v1/hello`, `POST /v1/enrol/start`, `/v1/enrol/finish`, `/v1/enrol/ask`, `/v1/enrol/reveal`.
2. Everything else needs a **trusted signed request**: `trusted()` looks up the device in `devices.json`, opens its sealed key, verifies `sign::request_ok(key, method, path, time, nonce, body, sign)` from headers `x-carla-device`, `x-carla-time`, `x-carla-nonce`, `x-carla-sign` (constants in `carla_core::link::sign`; signature window `WINDOW = 300 s`), and then checks the nonce against `replay: Mutex<Replay>`. A genuine but old/repeated request yields **409** "The Ally and the desktop disagree about the time. Check the clock on both." (still signed). An unknown device, wrong key or altered body yields **401** "This device is not linked to the desktop." (unsigned reply).
3. A trusted request goes to `Server::signed`, a `match (method, path)`; unknown routes return 404 "The companion does not know that request."
4. Replies to signed requests carry `x-carla-sign` (`sign::reply(key, nonce, status, body)`), including 4xx/5xx errors. Errors are JSON `{"error": "<plain-language text>"}` (`Problem`). The shell shows these texts to the player, so write them as full sentences naming the desktop (`self.config.name`) where helpful.

Signed routes (`Server::signed`):

| Method and path | Handler | Notes |
| --- | --- | --- |
| `POST /v1/probe/burst` | `burst` | UDP burst to the Ally's address; 409 during a stream or when another burst runs; `bursting: AtomicBool` |
| `POST /v1/stream/fit` | `fit` | Answers which mode the stream would get (virtual screen present, or nearest real mode via `carla_core::display::choose`) |
| `POST /v1/link/pulse` | `pulse` | Ally's heartbeat (`Pulse`): version, Moonlight state, battery, optional `Health` measurement; stored in `ally-pulse-<id>.json`; feeds the chart in `pings` |
| `GET /v1/status` | `status` | `Status { state: Ready/Busy/NotReady, playing, problem, addresses, macs, hardware, graphics, ... }` |
| `GET /v1/games` | `listed` | Store games (scan) + hand-added games (`manual:<key>`, only those whose exe still exists) |
| `GET /v1/cover/<game_id>` | `cover` | Picture bytes of a listed game |
| `GET /v1/achievements/<game_id>` | inline | Only Steam, only installed games |
| `GET /v1/probe/<bytes>` | inline | Random filler up to `quality::LARGEST_PROBE`, for throughput measurement |
| `GET /v1/library` | `library` | Steam library games not installed here, free disk bytes |
| `POST /v1/install` | `install` | Long; only ids offered by `installs.offered`; 409 while a game runs |
| `GET /v1/downloads` | inline | What Steam is fetching |
| `POST /v1/stream/prepare` | `prepare` | Section 9 |
| `POST /v1/stream/pair` | `pair` | Long; completes Moonlight pairing through Sunshine |
| `POST /v1/power/sleep` | `sleep` | 409 while a game runs |
| `POST /v1/stream/end` | `end` | Writes `end.json`; the launcher closes the game |
| `POST /v1/stream/reveal` | inline | Writes `reveal.json` if `streaming`; launcher lifts the curtain |
| `GET /v1/games/update/<id>` | inline | Forces a fresh scan, returns that game's store-update state |
| `GET /v1/admin/next`, `POST /v1/admin/done` | `admin_next`, `admin_done` | Remote administration channel (long poll); see `docs/REMOTE-ADMIN.md` |
| `POST /v1/update` | `update` | 409 while a game runs |
| `POST /v1/saves/find` | `saves_find` | Long; may record a suggestion |
| `POST /v1/saves/<key>/{read,write,remove,protect,retire}` | `saves_step`, `retire` | Long; section 10 |

Every signed request also runs `note_seen(device, from, now, prepared)`, which updates `ally-seen-<id>.json` (`Seen { at, address, prepared }`) at most once per 10 s per device (immediately when the address changes or for `/v1/stream/prepare`). `Seen.prepared` is how `Files::allies` decides which Ally is "the streaming one" (the one that asked for a stream last, while a stream is going on).

The shared request/response types (`Prepare`, `Pair`, `Status`, `SaveFile`, `SavesFind`, `Hello`, ...) live in `carla_core::link` and `carla_core::remote`: **changing them changes the Ally/desktop protocol and is out of scope for this crate alone** (see section 14.2).

Enrolment (summary; `crates/core/src/link/enrol.rs` has the cryptography): `enrol_start` (6-digit code path; each attempt, right or wrong, uses one of `CODE_TRIES = 5` and the code lives `CODE_LIFETIME = 600 s`), `enrol_ask`/`enrol_reveal`/`enrol_finish` (approval path with no code; someone at the desktop must say yes; `APPROVE_LIFETIME = 180 s`, one device at a time, a commit-then-reveal exchange whose short "check" number both screens show). The pending exchanges live in `Server.exchanges: Mutex<HashMap<enrol_id, Waiting>>` (in memory; lost on restart; stale ones are dropped on the next call). Success calls `Files::add_device`, which seals the device key with DPAPI.

### 3.7 Response headers

All bodies get `content-type`. HTML responses (`text/html`) additionally get `cache-control: no-store`, `x-frame-options: DENY` and `content-security-policy: default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; img-src data:; frame-ancestors 'none'`. That CSP means the dashboard page cannot load any external resource: everything must be inline, images must be `data:` URIs, and `fetch` may only target the same origin.

### 3.8 Request logging, the log queue and noise suppression

Every request except the "too common" ones produces **one line** in `host.log`, written **after** the reply has been sent and the worker's slots released:

```
GET /v1/status -> 200 in 3 ms (012345)
POST /v1/saves/:folder/write -> 409 in 2500 ms (012345) SLOW: Quest is still running on HOME PC.
```

- Format from `request_line(method, path, status, took, who, reason)`: `<METHOD> <route shape> -> <status> in <ms> ms (<who>)`, then ` SLOW` if it took at least 1 s (except `/v1/admin/next`, which is meant to wait), then `: <error text>` for status >= 400 (the `error` field of the JSON body).
- `route_shape(path)` never logs which game, save folder or file: `/v1/cover/:game`, `/v1/achievements/:game`, `/v1/games/update/:game`, `/v1/probe/:bytes`, `/v1/saves/:folder/<read|write|remove|protect|retire>` (other verbs become `?`), unknown paths become `(unknown)`. `/local/...` paths are logged as typed but passed through `find::visible`, stripped of control characters and cut to 40 characters (they are text from whoever asked). **When you add a `/v1` route, add it to the `PLAIN` array in `route_shape`; its declared length (`[&str; 21]`) must be changed with it, otherwise the line is logged as `(unknown)` (and treated as noise for strangers).** Routes with an id segment need a new `(prefix, ":game")`-style entry instead.
- `who(device, linked, local)`: first six characters of the id for a linked device (`label`), `stranger` for an unknown id, `unsigned` for no id, `this PC` for the dashboard.
- **Queue and writer thread.** `Load::say` does `try_send` on a bounded `sync_channel(1024)`; a dedicated thread drains it into `Files::host`. A full queue **drops** the line. A slow disk therefore can never hold a worker. (When no queue is installed, e.g. `Load` used outside `serve_with`, it writes directly.) Consequence for tests: request lines arrive asynchronously (section 13).
- **Too common** (`too_common`): successful `GET /` and `GET /local/*` (the dashboard polls every 2 s) are not logged; failures of them are.
- **Noise suppression** (`Server::noisy(kind, who, now)`, state in `noise: Mutex<HashMap<kind, (window start, hidden count, last who)>>`, window `noise_window = 10 s`): for **anonymous** requests (not local, not a linked device, not `/v1/enrol/*`) that were refused (status >= 400), unknown (`route_shape == "(unknown)"`), or `GET /v1/hello`, only the first line of each *kind* (the status code, or "unknown route") per window is written in full. The rest are only counted; when the window rolls over (on the next request of that kind, or via `flush_noise` which the watcher thread calls every 5 s) one line "N more requests were refused or unknown (<kind>) in the last 10 s, the last from <who>." is written. Requests from linked devices and the dashboard are always logged in full. Refusal details ("Refused POST /v1/x: device abcdef is not linked here ...") use the kind `"refused detail"` and are always written for a linked device and noise-limited otherwise.
- What is never logged: bodies, headers, query strings, keys, signatures, tokens, PINs, device names (player text), save contents, game or folder names in routes.

---

## 4. Threads and locks

### 4.1 Threads in a running companion

| Thread | Started by | Lifetime | Job |
| --- | --- | --- | --- |
| 8 workers | `serve_with` | process | Answer requests (3.2) |
| Log writer | `serve_with` | process | Drain the request-line queue into `host.log` |
| Notice watcher | `serve_with` | process | Every 5 s `Server::notice` (changes only: Sunshine up/down, Ally connected/streaming, Moonlight paired) and `flush_noise` |
| UDP answerer | `serve()` | process | `discover::answer` on the UDP socket |
| Screen watcher | `screen::watch` | process | Every 5 s `screen::tidy` (put the real screen back) |
| Virtual-screen watcher | `virtual_screen::watch` | process | `settle` once at start (if `config.virtual_screen`), then every 5 s `tidy` |
| Sunshine starter | `setup::ensure_sunshine_running_later` in `build()` | one-shot | `Starter::ensure()` at start-up |
| Burst sender | `Server::burst` | one-shot | Sleep 200 ms, `probe::send_burst`, clear `bursting` |
| Driver installer | `Server::install_driver` | one-shot | `virtual_screen::install()` (winget), clear `installing_driver` |
| Update thread | `update_from_afar` | one-shot | Sleep 1 s, download, run installer |
| Sleep thread | `sleep_soon` | one-shot | Sleep 3 s, suspend |
| Detached responders | `work` | up to 16 | See 3.5 |
| tiny_http connection threads | library | per connection | Accept/parse; **UNVERIFIED:** thread-per-connection is stated in a test comment ("each one keeps a connection thread of the HTTP library"); confirm in the `tiny_http` 0.12 source |
| `attend`, dashboard opener | `run` (no-argument mode only) | process / 800 ms | Console linking prompt; open Edge |
| Curtain thread | `stage::os::start` (launcher process only) | one stream | Message loop and 300 ms timer of the loading window |

The launcher process is single-threaded apart from the curtain thread: `launcher::run` loops on the main thread every `POLL` (2 s).

### 4.2 What each lock guards

`locked(&mutex)` in `server.rs` is `lock().unwrap_or_else(|e| e.into_inner())`: **poisoning is ignored everywhere**, so a panicking handler does not wedge the server. Use `locked`, never `.lock().unwrap()`.

Fields of `Server` (all `Mutex` unless noted):

| Field | Guards | Held across I/O? |
| --- | --- | --- |
| `replay` | `carla_core::link::sign::Replay` nonce cache | No (only inside `trusted`) |
| `exchanges` | Pending link exchanges (`Waiting`/`Approve`) | **Yes**: `enrol_ask` holds it across `Files::use_code` (disk), `enrol_reveal` across `set_asking`, `enrol_finish` across `add_device` (DPAPI + disk) |
| `games` | Cached `(Instant, Vec<Found>)` scan result | No |
| `scanning` | `()`: serialises disk scans so concurrent askers share one | **Yes, by design**: held across the whole scan (`self.scan` plus the consoles' file scan). Readers of `games` never take it, so asking "what is known" never waits for a scan |
| `health` | Cached `(Instant, Result<(),String>)` answer of "is Sunshine all right?" | No |
| `checking` | `()`: serialises the actual Sunshine call | **Yes**: held across `Streamer::check` (HTTP to Sunshine, 5 s timeout) |
| `cleared` | `Instant` when games were last invalidated (a game was installed) | No |
| `noise` | Log-noise counters (3.8) | No (the `say_hidden` write happens after the guard is dropped) |
| `seen` | Throttle map for `note_seen` | **Yes**: held across `Files::ally_seen` (read) and `set_ally_seen` (write) |
| `newest` | Cached "newer release?" answer (6 h) | **Yes**: held across `(self.newer)()`, a network call to the release server |
| `pings` | Per-Ally ping history for the chart (60 values, in memory only) | No |
| `asked` | The last `Prepare` seen (game, screen, time) for the dashboard | No |
| `clients` | Cached list of Moonlight names paired in Sunshine (30 s) | **Yes**: held across `Streamer::paired` (HTTP, 5 s) |
| `candidates` | Result of the last "look for games" (ids with file stamps), 10 min | `add_found` holds it across `stamp()` metadata reads and `Files::add_extra` |
| atomics: `finding`, `bursting`, `installing_driver` | Single-flight guards | n/a |

Other locks:

- `static SAVES: Mutex<()>` inside `Server::saves_step`: held for every save step except `read` (write, remove, protect, retire), across the actual file I/O, so that a retire's check-then-delete cannot interleave with another Ally's write.
- `static SUGGESTIONS: Mutex<()>` in `files.rs`: serialises `suggest`/`dismiss_suggestion` read-modify-write of `save-suggestions.json`.
- `static WRITING: Mutex<()>` in `carla_core::applog`: process-wide, held across one log append and, when the file exceeds its limit, across the cut-to-newest-half rewrite. It does **not** protect against another *process* (the launcher also appends to `host.log`): **UNVERIFIED:** whether a launcher append can be lost during a background-companion truncation; would be confirmed by a stress test appending from two processes.
- `static SYSTEM: Mutex<Option<(System,bool)>>` in `main.rs::vitals`: CPU/memory sampler for the dashboard (first answer has no CPU figure because usage is a delta).
- `Load.warned`, `Load.lines`: tiny, never across I/O.
- `setup::Starter.state: Mutex<Attempt>` + `Condvar done`: one Sunshine start attempt at a time; a second caller waits on the condvar for the first one's outcome; `gap` 60 s prevents restart loops; the lock is **not** held while Sunshine starts or while waiting for it to answer.
- `stage::STATE` (Windows) and `Curtain.told: Arc<Mutex<Told>>`: the curtain's window procedure can be re-entered by other programs' windows, so the code drops the `STATE` guard **before** any call that talks to windows (see the comment in `os::tick`). Preserve that ordering if you edit it.

Rule of thumb used throughout: **cache + try_lock**. A cheap path returns the cached value without locking the expensive mutex; a worker that finds the expensive mutex busy returns the previous value instead of waiting (`games_listing` and `health`). Keep that property when you add a slow dependency: never make `GET /v1/status` or `GET /v1/games` wait for disks, Sunshine or the network.

### 4.3 Time

`files::now()` returns Unix seconds as `i64`; all stored timestamps are Unix seconds. Staleness constants in `files.rs`: `ASK_LIFETIME = 60`, `PENDING_LIFETIME = 120`, `PLAYING_BEAT = 5` (so a `playing.json` older than 10 s is "not playing"), `STREAM_SILENCE = 30`. `carla_core::link::health::CONNECTED_FOR = 90` defines "Ally connected". Handlers take `now` as a parameter so tests can drive time.

---

## 5. Background watchers and caches

| Watcher / cache | Where | Period / age | Behaviour |
| --- | --- | --- | --- |
| **Presence** (Ally connected, streaming, Moonlight paired) | `Server::notice` on the notice thread | 5 s | Compares with `Noticed` (last state) and writes a log line **only on change**: "Ally abcdef: connected.", "was heard from: connected.", "was not heard from for 90 s: no longer connected.", "streaming started/ended.", "Moonlight is paired with Sunshine." (device names are player text and are never logged). "Connected" is derived from `ally-seen-<id>.json` and `ally-listening-<id>.json` timestamps within `CONNECTED_FOR = 90 s` (see `Files::allies`) or from being the streamer. |
| **Sunshine health** | `Server::health` | 15 s (`HEALTH_AGE`) | Used by `/v1/status`, `prepare`, the dashboard and the notice thread; logs "Sunshine is ready." / "Sunshine is not ready: ..." on change. Cached result is returned while another worker is already checking (`checking.try_lock`). |
| **Games scan cache** | `Server::games(fresh)` / `games_listing` | 60 s (`SCAN_AGE`) | `games(false)` accepts a scan up to 60 s old; `games(true)` accepts only a scan that **finished after** the call began, so several concurrent askers share one scan. `games_listing()` (what `GET /v1/games` uses) returns the previous list immediately if a scan is running, otherwise does `games(true)` (that is, every Ally list request normally rescans). The scan runs `installed()` plus every console from `consoles.json` (`emulation::scan`, labelled source `emu`). `cleared` + the `begun >= cleared` check make a scan that started before a game install finish without being stored (test `a_scan_that_began_before_an_install_is_not_kept`). `find(game_id)` tries the cached list and then a fresh scan. |
| **Moonlight pairing names** | `Server::paired_names` | 30 s (`CLIENTS_AGE`) | `Streamer::paired()` = `GET /api/clients/list` `named_certs[].name`, matched case-insensitively against the Ally's device name. `None` when Sunshine is too old to say. |
| **Newer release** | `Server::newer` | 6 h (`NEWER_AGE`), or forced by `POST /local/newer` | Calls `update::check(Part::DesktopHost)`. |
| **Screen tidy** | `screen::watch` | 5 s | `screen::tidy`: if `screen.json` exists and `!files.streaming(now)` (no launcher heartbeat for 30 s), restore the display mode (`display::restore()`) and delete the note. Retried every 5 s if restore fails (the note stays). |
| **Virtual screen tidy** | `virtual_screen::watch` | 5 s | `settle` once at start: if there is no note, no stream and the driver shows a virtual monitor (the driver's default is one always on), remove it. `tidy`: if `virtual.json` exists, no stream is going on, and the note is older than `GRACE = 90 s`, call `leave`. |
| **Sunshine start** | `Starter::ensure` | at start-up and on demand | Section 8. |
| **Probe burst throttle** | `bursting` atomic | per burst | One measurement burst at a time. |
| **Candidates (add a game)** | `Server.candidates` | 10 min (`FOUND_AGE`) | Section 11. |

The dashboard "Games" card uses `games_view()`, which **never triggers a scan** when anything is cached (it reads `games` directly and only falls back to `games(false)` if nothing is known), to keep the page's 2-second poll cheap.

---

## 6. CLI reference

Source: `HELP` and `run()` in `crates/companion/src/main.rs`. Exit code: the `i32` returned by the handler; an `Err(String)` prints the text to stderr and exits 1, and **when there were no arguments at all** it also waits for Enter ("Press Enter to close") so a double-clicked window does not vanish. An unknown subcommand prints `HELP` and exits 0. Commands that need an existing `config.json` use `configured()` ("The companion is not set up yet. Run it without arguments first.").

Environment: `CARLA_COMPANION_DIR` overrides the data directory (section 7).

### 6.1 Reference table

| Command | What it does | Touches |
| --- | --- | --- |
| *(no arguments)* | Set up if needed, show a linking code, open the dashboard, attend linking prompts, and run the server in this process (details 1.4). | everything |
| `dashboard` | Section 6.2. | processes |
| `setup` | Set up again, **asking for the name**. Saves config, `sunshine_ready` (install Sunshine through winget if missing; claim/ask for credentials; make the CarlaOS app entry), `firewall_ready`, `autostart::set(true)`. Prints "Done. If the companion is already running, sign out and in again (or restart it)". | `config.json`, registry, firewall, Sunshine |
| `code` | Open linking for 10 minutes and print a new 6-digit code (`Files::new_code`). | `enrol.json` |
| `allies [--json]` | List linked Allies: connected/streaming, last heard, route (home or Tailscale), ping/jitter/loss grade, the picture streams get, tips, Moonlight state, battery. `--json` prints `{"allies":[...]}` (`Files::allies`). | read-only |
| `firewall` | Open the firewall for this program if not already (elevated PowerShell, UAC prompt). | firewall |
| `sunshine` | Run `sunshine_ready` (may install/claim), save config, and when the user name is `carlaos` print Sunshine's page URL, user name and **password**. | `config.json`, Sunshine |
| `devices` | List linked device ids and names. | read-only |
| `forget ID` | Stop accepting a device; deletes `ally-seen-<id>.json` and `ally-pulse-<id>.json`. | `devices.json` |
| `games` | List games added by hand with key, path, "(watching X)", "(described)", "(program not found, hidden from the Ally)". | read-only |
| `describe KEY TEXT` | Write the text the Ally's game page shows for a hand-added game (empty text clears). `describe KEY --lookup` asks Steam and Wikipedia (**only the title is sent**) and prints; `--lookup --apply` stores it, but only when the found name equals the title (`same_game`); otherwise it refuses. | `games.json` |
| `find [FOLDER...]` | Look for games (usual game folders, or the given folders; each must be a directory) and print a ready-to-paste `carla-companion add ...` line per candidate; hides ones already added. A candidate whose name has unsafe characters gets a note instead of a line. | read-only |
| `add NAME PROGRAM [--args OPTIONS] [--cover PICTURE] [--watch EXE] [--describe TEXT]` | Add a hand-added game (`Files::add_extra`). PROGRAM must be an absolute path to an existing file; a `.lnk` shortcut is resolved to its target and options; same program again updates it. `--watch` names the real game exe for launchers that hand over and exit (validated by `carla_core::library::launch::valid_watch`). | `games.json` |
| `drop KEY` | Remove a hand-added game (nothing deleted from disk). | `games.json` |
| `consoles` | List consoles (emulator + folder), game count, "(emulator not found, hidden from the Ally)". | read-only |
| `console add NAME EMULATOR FOLDER [--endings iso,chd] [--options OPTIONS]` | Add a console; known emulators fill endings/options (`emulation::suggest`). The id is `d` + 8 hex of the key of `emulator|folder`, so it can never equal an Ally-side console id; same emulator+folder replaces. | `consoles.json` |
| `console drop ID` | Forget a console. | `consoles.json` |
| `saves` | List games whose saves are kept in step: key, title, folder, "(folder not found)", "(kept clean after a stream)". | read-only |
| `saves set NAME FOLDER` | Say where a game keeps saves here. NAME matches (case-insensitively) a listed game's id (`steam:123`) or title or a hand-added game; otherwise it is matched with the Ally's game of exactly that name. Refuses non-absolute, non-directory, "too general" (`saves::sensible`), or too-large folders (`sync::manifest`). | `saves.json` |
| `saves guess NAME` | Suggest folders from folder names (`savefind::guess`); sets nothing. | read-only |
| `saves find NAME` | Snapshot, wait for Enter while you play and save, snapshot, and diff (`savefind::diff`); needs a console. | read-only |
| `saves suggestions` | List suggestions that came from where the Ally keeps a game's saves on another desktop. | read-only |
| `saves accept KEY [--yes]` | Turn a suggestion into a save folder. **Without `--yes` it only describes and exits 1.** Re-resolves the place and refuses a folder that "loads code". | `saves.json`, `save-suggestions.json` |
| `saves dismiss KEY` | Forget a suggestion. | `save-suggestions.json` |
| `saves clear KEY` | Stop keeping a game's saves in step (nothing deleted). | `saves.json` |
| `saves clean KEY on --yes` / `saves clean KEY on` / `saves clean KEY off` | The "keep clean" mode (section 10.4). `on` without `--yes` only warns and exits 1. The parser (`parse_clean`) accepts exactly these shapes; anything else (`ON`, `yes`, extra words, `off --yes`) is refused so a typo can never enable deletion. | `saves.json` |
| `saves copies KEY` | List safe copies (`Backups::list`): id, files, KB, age. | read-only |
| `saves restore KEY COPY` | Put a safe copy back (the current content is first kept as a new copy). Refused while a game runs (`files.playing`). | save folder, `save-copies/` |
| `ally ...` | Administer a linked Ally (section 6.3). | `ally-*.json` |
| `stream` | Show what a stream does to this desktop's screen, three toggles, the current mode, and what modes a 16:9 and a 16:10 Ally would get. | read-only |
| `stream screen on|off` | `config.match_screen`: switch the real screen to the Ally's size during a stream. | `config.json` |
| `stream curtain on|off` | `config.curtain`: loading screen until the game shows. | `config.json` |
| `stream virtual on|off` | `config.virtual_screen`; `off` also tears down any virtual screen (`virtual_screen::leave`). | `config.json` |
| `stream virtual install` | `winget install VirtualDrivers.Virtual-Display-Driver`, turn the option on, wait 3 s, `settle`. | `config.json`, driver |
| `stream virtual test [WxH [RATE]]` | 15 s demonstration without an Ally (default 1920x1080 at 60). The monitor goes dark meanwhile. | display, `virtual.json` |
| `stream test [WxH [RATE]]` | 10 s rehearsal of the real-screen switch plus the curtain (`launcher::rehearse`); refuses if a stream is on. | display, `stream.json`, `screen.json` |
| `stream restore` | Leave any virtual screen, `display::restore()`, clear `screen.json`. | display |
| `sleep-test` | Sleep the PC in 3 s as an Ally would (waits 5 s to keep the process alive). | power state |
| `status` | Name, linked devices, store game counts (Steam/Epic/GOG), hand-added count, uninstalled Steam library size, Steam downloads, addresses, wake-up adapter MACs, and Sunshine state (it **runs `ensure_app`**, so it can write the Sunshine app entry). | Sunshine |
| `log [--lines N] [--follow] [--launcher]` | Print the newest N (default 100, 1..=10000) lines of `host.log` (or `launcher.log` with `--launcher`); `--follow`/`-f` polls every 500 ms and prints only new lines (the file is cut to its newest half now and then, so the last printed line, not a byte offset, is the cursor; "Following ..." goes to stderr). `-n` is a synonym of `--lines`. | read-only |
| `log path [--launcher]` | Print the log file path. | read-only |
| `serve` | Run the API with no console interaction (what Windows start-up runs). Needs config. | everything |
| `update` | Download and install the newest release (`update::update_now(Part::DesktopHost)`); waits 30 s for the installer to stop this process; on failure prints the error and "Press Enter". Also the Start-menu "Update" entry. | installer |
| `remove` | Remove the start-up Run value. Data directory is kept (path printed). | registry |
| `launch` | The launcher Sunshine runs (section 9). Not meant to be run by hand. | see section 9 |
| `info` | Print `AppInfo::current(Role::Companion)` as JSON (version etc.). | read-only |

### 6.2 `dashboard`

`dashboard(files)`: use the config or run `setup(files, false)`; if nothing answers on `127.0.0.1:<port>` within 500 ms, start `conhost.exe --headless <exe> serve` and wait up to 8 s for the port; then open `http://127.0.0.1:<port>/` as an Edge app window (`cmd /C start "" msedge --app=<url> --window-size=1240,860`, with `CREATE_NO_WINDOW`), falling back to the default browser. Non-Windows just prints the URL. This is what the "CarlaOS Dashboard" shortcuts run (minimised, so only the app window shows).

### 6.3 `ally ...`

`ally::run(files, words, serving)`; `serving` is true when the configured TCP port **cannot be bound** (meaning a companion is already listening). The flow: parse words into a `Request { op, args, patience }` (`ally::request`), choose the Ally (`--device ID` or the only one), `files.ask_ally(device, command)` writes `ally-ask-<id>.json` as `(unix_time, Command{id, op, args})`, then polls every 250 ms for `ally-answer-<id>.json` with the same command id. The Ally picks the command up through `GET /v1/admin/next` (long poll, which also writes `ally-listening-<id>.json`) and returns its result with `POST /v1/admin/done`. Timeouts: if nobody fetched it within `ASK_LIFETIME` (60 s) it prints the "The Ally did not fetch the request..." text and removes the ask; once fetched it waits `patience + 60 s`. Operation names must exist in `carla_core::remote::OPS` (a unit test checks every typeable line). Ops: `status games rescan add_game add_cloud remove_game save_folder desktops cleanup_scan cleanup_apply cleanup_undo update restart startup picture check power art_find art_set art_clear art_key`. The picture search writes `art-choices.json` (remembered for `art use NUMBER`) and `art-choices.html` (opened in a browser; only thumbnails passing `meta::trusted_picture` are included, all text HTML-escaped). See `docs/REMOTE-ADMIN.md` for the user-facing description and the on-Ally permission.

To add an op: add the word parsing arm in `ally::request`, a `show()` arm, extend `carla_core::remote::OPS` and implement the op on the Ally side (shell). Op set and `Command`/`Done` types are shared interfaces.

---

## 7. The data directory

Location (`files::default_dir`): `CARLA_COMPANION_DIR` if set (testing), else `%LOCALAPPDATA%\CarlaOS\companion`, else (non-Windows development) `$HOME/.local/share/carlaos-companion`. It is per Windows account. Everything below lives directly in it unless a subfolder is named. All JSON files are pretty-printed and written atomically (7.2).

Secrets are sealed with `carla_core::secret::seal/open`: on Windows that is DPAPI (`CryptProtectData`, tied to the Windows account, so a copied file is useless to another account or machine); elsewhere it is stored as-is (development and tests only). Sealed values are stored hex-encoded in the JSON (`files::hex/unhex`). Never log, print or commit them.

### 7.1 File inventory

| File | Format (Rust type) | Writer | Reader | Lifecycle |
| --- | --- | --- | --- | --- |
| `config.json` | `Config { id, name, port, sunshine_url, sunshine_user, sunshine_password (sealed hex), match_screen, curtain, virtual_screen }`. The three booleans default to `true` when absent (`#[serde(default = "yes")]`). `id` is `random_hex(8)`, `port` defaults to 47600, `sunshine_url` to `https://localhost:47990`. | `setup`, `sunshine` and `stream screen/curtain/virtual` commands | everything (`Files::config`) | Created by first set-up; never deleted by the program. The server holds a **copy** read at start-up: changes made by CLI commands reach a running companion only after it restarts, except where code re-reads (`launcher::run` and `virtual_screen::watch` call `files.config()` themselves). |
| `devices.json` | `Vec<Device { id, name, key (sealed hex), added }>` | `Files::add_device` (enrolment), `forget_device` | `Files::devices` (every signed request) | Grows with each linked Ally; `forget` or the dashboard's "Forget it" removes an entry. Every signed request re-reads and parses this file (`knows`, `trusted`). |
| `enrol.json` | `Code { code, expires, tries }` | `new_code` (CLI `code`, no-argument start, dashboard "Link an Ally"), `use_code` (increments `tries`) | `use_code`, `code`, `linking_until` (drives `Hello.approve`) | Deleted when it expires, is used up (`CODE_TRIES = 5`), or on a successful or refused link (`clear_code`, which also deletes the two files below). |
| `enrol-ask.json` | `link::health::Asking { enrol_id, device_name, from, check, at }` | `set_asking` (in `enrol_reveal`) | `asking(now)` (the console prompt in `attend`) | Valid `APPROVE_LIFETIME = 180 s`; cleared by `clear_code`. |
| `enrol-answer.json` | `Answered { enrol_id, allow }` | `answer_asking` (the console prompt) | `asking_answer` (the Ally retries `enrol/finish` and gets 409 "Waiting for someone at the desktop to allow it." until it exists) | Removed by `set_asking` and `clear_code`. |
| `games.json` | `Vec<Extra { key, title, exe, args, cover?, watch?, description? }>` sorted by lower-case title. `key = manual_key(exe path)`; `description` is omitted when absent. | `add_extra`, `describe_extra`, `drop_extra` | `Files::extras` (resolves `.lnk` shortcuts on every read), the server, the dashboard | Persistent. Older files without `watch`/`description` still load (`serde(default)`; tests at the bottom of `files.rs`). |
| `consoles.json` | `Vec<carla_core::library::emulation::Console { id, name, emulator, options, folder, endings }>` | `add_console`, `drop_console` | `consoles()`, scans | Persistent. |
| `saves.json` | `Vec<SaveFolder { key, game_id?, title, folder, place?, keep_clean }>`; `key = manual_key(game_id or "title:<title>")`; `place` is a portable description (`SavePlace { root, rel }`) so another desktop can suggest its own folder; `keep_clean` defaults to `false` for old files. | `set_save_folder(_in)`, `clear_save_folder`, `set_keep_clean` (CLI only), `accept_suggestion` | `save_folders()`, server save routes | Persistent. Naming the folder again writes a new entry with `keep_clean: false`. |
| `save-suggestions.json` | `Vec<Suggestion { key, game_id?, title, folder, place, at }>`, at most `MOST_SUGGESTIONS = 200` (oldest dropped); text is cleaned of control/direction characters and clipped (title 80, id 200, rel 1200) | `suggest` (from `saves_find` when the Ally sends a `place`), `dismiss_suggestion` | CLI `saves suggestions/accept` | A suggestion is dropped when its game gets a folder (`set_save_folder_in` dismisses it). |
| `save-copies/<safe-key>/<id>/` | Directory-tree copies of a game's save folder (`carla_core::library::saves::Backups`). `<safe-key>` is the save key with non-alphanumerics (except `-`) replaced by `_`; `<id>` is the Unix time zero-padded to 12 digits, suffix `-retired` for copies taken just before keep-clean deletion, `-2`, `-3` for same-second copies. | `Backups::take`, `take_retired`, `restore`, and the sync code (`protect`, replacing files) | `Backups::list`, CLI `saves copies/restore` | Ordinary copies: newest `KEEP = 5` kept; retired copies: newest `KEEP_RETIRED = 15`, never pushed out by ordinary ones. Half-made copies are `partial-<id>` and renamed into place. |
| `pending.json` | `Pending { game_id, title, source, source_id, install_path, exe?, args, watch?, screen?, at }` | `Server::prepare` (`set_pending`) | Launcher (`take_pending`: **reads then deletes**; ignored if older than `PENDING_LIFETIME = 120 s`) | One-shot note: "the game the next stream is for". |
| `stream.json` | `Streaming { at }` | Launcher `beat()` (every loop; also in `rehearse`) | `streaming(now)` true when `now - at <= STREAM_SILENCE (30 s)`; `Files::allies`, screen/virtual tidy, `burst`, `reveal` | Deleted by the launcher's `stream_over()`; if the launcher is killed it just goes stale. |
| `playing.json` | `Playing { game_id, title, at }` | Launcher loop (`set_playing(Some)` every 2 s), `set_playing(None)` when done | `playing(now)` true when `now - at <= 10 s` (`PLAYING_BEAT * 2`); used by `status` (Busy), `prepare`, `install`, `update`, `sleep`, saves routes, dashboard | Deleted by the launcher; stale-by-time if the launcher is killed (it is **not** deleted by the background companion). |
| `screen.json` | `carla_core::display::Mode { width, height, hz }` | `screen::switch` (before applying), cleared by `put_back` | `switched()`: the "someone must put the screen back" note; `tidy` | Written **before** the switch so any failure afterwards still gets undone. |
| `virtual.json` | JSON integer: Unix time the virtual screen was requested | `virtual_screen::enter` (before creating), cleared by `leave` | `virtual_screen()`, `tidy`, `settle`, dashboard "A screen of its own" | Same note-first discipline. |
| `reveal.json` | JSON integer: time | `POST /v1/stream/reveal` (`ask_reveal`) | Launcher `take_reveal` (consumed; honoured within 10 s) | One-shot. |
| `end.json` | JSON integer: time | `POST /v1/stream/end` and `/local/end` (`ask_end`) | Launcher `take_end` (consumed; honoured within 10 s) | One-shot. The launcher also drains stale ones when a game starts. |
| `ally-seen-<id>.json` | `Seen { at, address, prepared? }` | `note_seen` on signed requests (throttled) | `Files::allies`, `notice` | Deleted by `forget_device`. |
| `ally-pulse-<id>.json` | `Pulsed { at, pulse: Pulse, health: Option<Health> }`; a pulse without a measurement keeps the previous `health` | `POST /v1/link/pulse` | dashboard, `allies` command | Deleted by `forget_device`. |
| `ally-listening-<id>.json` | JSON integer: last time the Ally asked for admin requests | `admin_next` loop (`ally_listened`) | `Files::allies` (adds to "last seen"), dashboard `remote_admin` | `forget_device` removes only `ally-seen` and `ally-pulse`, so this one stays (**UNVERIFIED:** no other cleanup was found). `admin_next` rewrites it about once a second while an Ally waits. |
| `ally-ask-<id>.json` | `(i64 time, carla_core::remote::Command)` | `ask_ally` (CLI `ally ...`) | `take_ally_ask` (read then delete; dropped if older than 60 s) | One at a time per Ally; writing a new one deletes `ally-answer-<id>.json`. |
| `ally-answer-<id>.json` | `carla_core::remote::Done` | `POST /v1/admin/done` | `take_ally_answer` (only if the id matches) | Consumed by the CLI that asked. |
| `art-choices.json`, `art-choices.html` | Last `art_find` answer; a generated page | CLI `ally art ID` | CLI `ally art use N` | Overwritten each search; never cleaned. |
| `host.log` | Text lines `YYYY-MM-DD HH:MM:SS.mmm UTC <text>`; section 12 | `Files::host` (companion and launcher) | `carla-companion log`, dashboard Log card | Cut to its newest half when above 256 KiB (`HOST_LOG_MAX`). |
| `launcher.log` | Text lines `<unix seconds> <text>` | `Files::log` (launcher, `screen`, `virtual_screen`) | `log --launcher` | Deleted and restarted when above 256 KiB. The same text also goes to `host.log` prefixed `launcher: `. |
| `*.new` (transient) | `<name>.<pid>.<n>.new` | `Files::write` | none | Renamed over the target; removed on rename failure. Leftovers after a crash are harmless. |

`host.log` and `launcher.log` are text, not JSON. `host.log` lines are scrubbed (IPv4 addresses and the Windows account name hidden by `carla_core::crash::scrub`), flattened to one line and cut to 400 characters (`carla_core::applog`).

Also written elsewhere (not by `Files`): the update installer download goes to `carla_core::update::download_dir()` (**UNVERIFIED:** exact location), the Sunshine app list lives in Sunshine's own config (reached through its API), and the VDD's `vdd_settings.xml` is edited in the driver's folder (`virtual_screen::with_mode`).

### 7.2 Adding or changing a file safely

- Add accessors to `Files` using `read`/`write`/`remove`; keep file names in one place; give every new field `#[serde(default)]` (or `Option` with `skip_serializing_if`) so files from older versions still load, and add a test that loads an old-shaped file (see the tests at the bottom of `files.rs`).
- The reader returns `None` on a parse failure, so a type change that is not backward compatible silently makes existing data "disappear". Prefer additive changes.
- Notes that coordinate processes (like `pending.json`) must be written **before** the action they guard and be time-limited on read, because either process can die at any moment.
- Do not put secrets in new files unsealed. Do not store what the Ally sent without bounding its size and characters (see `suggest`, `clean_description`).
- The SQLite schema belongs to the shell and core, not to this crate: the companion has **no database**.

---

## 8. Sunshine integration

Sunshine (LizardByte) does the actual streaming; the companion only configures it, checks it and starts it. Code: `crates/companion/src/sunshine.rs` and `setup.rs`.

### 8.1 Credentials

- The companion owns a Sunshine account: user name `carlaos` (`setup::USER`) and a password nobody has to know, `link::random_hex(16)` (32 hex characters), stored sealed in `config.json` (`Config::set_sunshine`, read back by `Config::sunshine_password`).
- `setup::claim(&mut config)` returns `Claimed::{Already, Fresh, Taken, Silent}`:
  1. Existing stored credentials that pass `Streamer::check`: `Already`.
  2. Sunshine does not answer: `Silent` (the CLI says to start it and run the host again).
  3. `sunshine::claim_fresh`: on a Sunshine **nobody has set up** (it redirects `/api/apps` to its welcome page instead of answering 401) POST `/api/password` with `{currentUsername:"", currentPassword:"", newUsername, newPassword, confirmNewPassword}`; believed only if the new credentials then pass `check`: `Fresh`.
  4. Otherwise `Taken`: someone set Sunshine up by hand. `sunshine_ready` (interactive) lets the person type their own user name and password, or press Enter to reset them through `sunshine.exe --creds carlaos <new password>` run elevated followed by `net stop/start SunshineService` (`reset_credentials`, UAC prompt).
- If credentials are missing, `main::streamer` returns `Err("Sunshine's user name and password have not been given yet.")` and the server runs with `streamer = None`; every Sunshine-dependent route then answers 503 with the "Run its set-up again" text.
- The CLI `sunshine` prints the password only when the user is `carlaos` (the companion's own choice); a user-chosen password is never printed.

### 8.2 Talking to Sunshine

- `Sunshine::new(url, user, password)` refuses any URL that is not `https://` plus `localhost`, `127.0.0.1` or `[::1]` (`local()`): TLS verification is **disabled** (`disable_verification(true)`, because Sunshine uses a self-made certificate), which is acceptable only because the connection never leaves the machine. Do not loosen `local()`.
- Auth is HTTP Basic. `call()` maps any transport failure to the single text "Sunshine is not running on this desktop." and a 401 to "Sunshine refused the user name or password the companion was given. Run the companion's set-up again." A status other than 200, or a body that is not JSON: "Sunshine answered <path> in a way CarlaOS does not understand (status N). It may need updating."
- Timeouts: `SHORT = 5 s` for ordinary calls, `PAIRING = 60 s` for the call Sunshine holds open while Moonlight finishes pairing.
- `Streamer` trait (injectable; `FakeSunshine` in the tests): `check()`, `ensure_app(command)`, `pair(pin, name, from)`, `paired() -> Option<Vec<String>>`.

### 8.3 Readiness check

`Streamer::check` is `GET /api/apps` with credentials; `Ok` means Sunshine runs and accepts us. This is what `Server::health()` caches for 15 s and what the dashboard, `/v1/status` (`State::NotReady`), `prepare` and the notice thread use. `sunshine::answers(url)` is the weaker "does anything answer at all" (any HTTP response, no redirects followed, no credentials), used before credentials exist and by `Starter`.

### 8.4 The CarlaOS app entry

`ensure_app(command)`: `GET /api/apps`, find the entry named `CarlaOS`; if it is missing or its `cmd` differs from `"<exe>" launch`, `POST /api/apps` with:

```
name "CarlaOS", cmd <launch command>, output "", index <position or -1>,
exclude-global-prep-cmd false, elevated false, auto-detach false, wait-all false,
exit-timeout 5, prep-cmd [], detached [], image-path ""
```

`auto-detach: false` and `wait-all: false` tie the stream's life to the launcher process (the stream ends when the launcher exits). `ensure_app` runs at set-up, in the CLI `status`, on every `prepare`, and on every `pair`, so moving the exe to another folder is repaired at the next stream.

### 8.5 Pairing

`POST /v1/stream/pair {pin (4 digits), name}` calls `Streamer::pair(pin, name, from_address)`. Newer Sunshine lists waiting requests at `GET /api/pin` (a `pairings` array) and wants `pairing_id`: the entry whose `address` equals (or ends with `:` plus) the Ally's address is chosen (`pick`), or the only one if exactly one is waiting; otherwise it refuses to guess. It then POSTs `/api/pin {pairing_id, pin, name}` (60 s). Older Sunshine takes `{pin, name}` alone and the code retries every 2 s for 25 s. The PIN is **never written to the log** ("Pairing with Moonlight began (the PIN is not written down).").

### 8.6 Starting Sunshine

`setup::Starter` (`Launch` trait: `installed()`, `start()`; `Machine` is the real one, tests use a fake so nothing real starts):

- `ensure()`: if `ready()` (`sunshine::answers(url)`) return Ok; if not installed (`ProgramFiles\Sunshine\sunshine.exe` or `ProgramW6432`) log "Sunshine is not running and is not installed, so it is not started." and return Err; if another attempt runs, wait on the condvar and return its outcome; if the last attempt began less than `gap = 60 s` ago return Err("Sunshine was started a moment ago and does not answer yet."); otherwise start (`sc start SunshineService`, accepting exit code 0 or 1056 "already running", else spawn `sunshine.exe` with its folder as working directory, both through `unseen()`), then wait up to `limit = 20 s`, polling every `poll = 1 s`.
- Callers: a thread at companion start (`ensure_sunshine_running_later`), and `Server::prepare` when the Ally asks for a game and `Streamer::check` fails (the result is ignored; the answer the Ally gets is the one from `ensure_app`).
- Installing Sunshine happens only in the interactive set-up: `sunshine_ready` runs `winget install --id LizardByte.Sunshine --exact --source winget --silent --accept-package-agreements --accept-source-agreements` when on Windows, `sunshine.exe` is not found and nothing answers, then waits up to 90 s for it to answer.
- The Virtual Display Driver is installed only on request (`stream virtual install` or the dashboard's "Install the screen driver" button): `winget install --id VirtualDrivers.Virtual-Display-Driver --exact ...`. The dashboard route takes **no parameters** and is single-flight.

---

## 9. The stream lifecycle on the desktop

```
Ally                     background companion (serve)               Sunshine            launcher (carla-companion launch)
 |  POST /v1/stream/fit        (optional: what mode would I get)
 |  POST /v1/stream/pair       (first time only)
 |  POST /v1/stream/prepare ->  prepare(): validate game, ensure Sunshine+app,
 |                              write pending.json, maybe virtual screen enter
 |  (Moonlight connects) ---------------------------------------->  runs app "CarlaOS" -> `launch` --------->
 |                                                                                         run(): FreeConsole, beat(), maybe switch screen,
 |                                                                                         wait for pending.json, curtain, start game,
 |                                                                                         loop 2 s: playing.json, stream.json, end/reveal
 |  GET /v1/status (Busy, playing)  <-- reads playing.json
 |  POST /v1/stream/reveal or /end  -> reveal.json or end.json ----------------------------> consumed by the loop
 |                                                                                         game closes -> put screen back, leave virtual screen,
 |                                                                                         stream_over() (delete stream.json), exit
 |                                                         Sunshine sees launcher exit -> ends the stream
 |  saves routes (after the stream, the Ally pulls/pushes saves, may call retire)
 |                              watchers: tidy leftovers if the launcher was killed
```

### 9.1 Prepare (`Server::prepare`)

Input `Prepare { game_id, screen: Option<Mode> }`. Steps:

1. Resolve the game: a hand-added game (`manual:<key>`, exe must still be a file) first, else the scan (`find`, which retries with a fresh scan). Not found: 404 "That game is no longer installed on <name>." Build a `Pending` (for manual games `install_path` is the exe's folder and `watch` is carried; for stores `exe`/`args` come from the scan and `watch` is `None`).
2. If `files.playing(now)` exists: a **different** game gives 409 "<title> is still running on <name>. Close it there first."; the **same** game logs "already running; the stream picks it up again" and returns the status without writing a new pending (so a resumed stream does not start the game twice).
3. If a `Starter` is configured and `Streamer::check()` fails, `starter.ensure()` (waits up to 20 s).
4. `ensure_app(launcher)`; failure gives 503 with the reason.
5. `set_pending` writes `pending.json`.
6. If `config.virtual_screen` is on, a driver exists and the Ally sent a `screen`: `virtual_screen::enter(driver, files, want)` **now**, because Sunshine looks at the screens the moment Moonlight connects, before the launcher runs. Errors are ignored here (the launcher then falls back to switching the real screen).
7. Log "Prepare: <title> (<game id>) is ready to start when Moonlight connects, for <screen>." and return `Status`.

After a successful prepare, `asked_for` records the request for the dashboard (game, screen, time; "the same game picked up again is still the same stream").

### 9.2 The launcher (`launcher::run`)

1. `hide_console()`; read `config.json` for `match_screen` and `curtain` (default true if missing); `files.beat(now)` so the background companion knows a stream is on.
2. `screen::asked_by_sunshine(env)`: picture size from `SUNSHINE_CLIENT_WIDTH/HEIGHT/FPS` (width 320..=16384, height 200..=16384, fps default 60, capped at 480). If `match_screen`, `screen::switch(&Main, files, want)`: `display::plan(current, modes, want)` decides whether to change; it **writes `screen.json` first**, applies the mode (`display::apply`), and logs "Screen switched from X to Y for the stream." If no suitable mode exists it logs that and leaves the screen alone.
3. `wait(files, POLL, stop)`: loop `take_pending(now)`; while there is none, log once "A stream started without a game being asked for: showing the desktop." and `beat` every 2 s. A stream started from Moonlight itself (no game) therefore keeps the launcher alive so Sunshine does not end the stream before the first picture (which Moonlight reports as a desktop fault); a game asked for later starts in the same stream. In production `stop` is `|| false`, so the launcher only leaves when Sunshine kills it.
4. For an older Sunshine that sends no size, the Ally's own `pending.screen` is used for `switch` (only if nothing was switched yet).
5. `play(files, pending, curtain)`:
   - Builds `carla_core::library::Install` from the pending note.
   - `Curtain::open(title, store_name, install_path, files)` (9.4) unless `curtain` is off (`Curtain::none()`).
   - `launch::start(&install, steam_root)`: Steam through `steam://rungameid/<id>`, Epic, GOG, hand-added and emulated games through their own start routes (`crates/core/src/library/launch.rs`). On error: log "<title> could not be started: <e>", close the curtain, return 1.
   - `Watch::new(&install, child).watching(pending.watch)`: follows the child process, any process whose exe is inside the game's install folder, and (if set) any process with the watched exe name.
   - Drains stale `end.json` and `reveal.json`.
   - Loop every 2 s: `set_playing(Some(..))`, `beat`, `take_end` leads to `watch.end()` (kills the child, the processes inside the folder and those of the watched name: "End game" from the Ally or dashboard), `take_reveal` leads to `curtain.reveal()`; while `Phase::Starting` it asks `update_of(source, id, steam_root)` and tells the curtain (and `watch.still_preparing()` so a store update is not mistaken for a failed start; `UPDATE_GRACE = 3 h`); `watch.poll()` returns the phase: `Finished` logs "<title> has closed: ending the stream.", `NeverStarted` logs "<title> never appeared: ending the stream."
   - `set_playing(None)`, `curtain.closing()` (shows "See you soon"), sleep 1.5 s, `curtain.close()`, return 0.
6. Back in `run`: `screen::put_back`, `virtual_screen::leave(&Vdd, files)`, `files.stream_over()`; return the exit code. The process exits and Sunshine ends the stream.

`rehearse(files, display, want, hold)` is the same sequence without a game and without Sunshine, used by `stream test`.

### 9.3 If the launcher is killed

Sunshine kills the launcher without warning when the user quits Moonlight. So everything the launcher changes is (a) noted on disk **before** it is done and (b) undone by a watcher in the background companion:

- Real screen: `screen::tidy` puts it back when `screen.json` exists and `stream.json` is older than 30 s. The module comment in `screen.rs` also says the switch is the temporary kind that Windows itself takes back when the changing program ends (**UNVERIFIED** by code alone; needs a hardware check).
- Virtual screen: `virtual_screen::tidy` after `GRACE = 90 s` without a stream, plus `settle` at companion start. The grace exists because the virtual screen is created at *prepare*, before any launcher heartbeat exists.
- `playing.json` and `stream.json` simply go stale (10 s and 30 s).
- The curtain window belongs to the launcher process, so it disappears with it. Store windows that were raised above the curtain are lowered when the curtain closes normally; after a hard kill they keep whatever z-order Windows gives them (**UNVERIFIED:** whether `HWND_TOPMOST` windows of other programs can stay top-most; check on hardware).

### 9.4 The curtain (`stage.rs`)

- Pure decision logic `Stage::tick(Look) -> Scene` (unit-testable without Windows): show the curtain with `Lines { title, note, busy }` or hide it; raise windows (game windows, or store helper windows when no game window exists), lower earlier-raised helper windows, focus the game window the first time it is seen.
- Rules: the curtain says "Starting <title>" with an update note while no game window has been seen; once a game window exists the curtain becomes a blank backdrop for non-full-screen game windows and hides entirely when a game window covers at least 95 % of the main screen; it **lifts by itself after `LIFT_AFTER = 75 s`** with no game window (and no store update), or on `reveal`; a game that shows up later brings it back; "See you soon" when `closing`.
- Window classification `role(exe, title, covers, game_dir)`: a window whose exe is under the game's install folder (folder path of at least 4 characters) is `Role::Game`; windows of `HELPERS` programs (Steam, Epic, GOG Galaxy, EA, Ubisoft, Battle.net, WerFault ...) with a non-empty title that are not full-screen and not a known main window (`MAIN_WINDOWS`) are `Role::Helper` (store prompts the player must answer). If you add a store, extend those two lists.
- Windows part (`mod os`): a `WS_POPUP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE` window with a timer every 300 ms (`EVERY_MS`), `EnumWindows` for candidates (visible, not minimised, at least 64x64, not our own process), `SetWindowPos(..., SWP_ASYNCWINDOWPOS)` so a hung program cannot block, `AttachThreadInput` plus `SetForegroundWindow` to hand the game the keyboard. The non-Windows `os` module is a stub, so everything else builds and tests everywhere.
- Colours are hard-coded as Windows BGR values matching `apps/shell/src/tokens.css`.

---

## 10. Saves on the host

User-facing behaviour: `docs/SAVE-SYNC.md`. Here is where the code lives and the invariants.

### 10.1 Who decides what

- **Only the desktop administrator names a folder** (CLI `saves set`, `saves accept`). The Ally can never point the companion at a folder: it refers to a game by id or title and then to **files relative to the already-named folder** (`sync::Folder` validates relative paths; test "Nothing outside the named folder can be read or written").
- The comparison and merge logic (`bring_in_step`, `Way::{Fetch,Send,Same}`, `both_changed`, the manifest and fingerprint) lives in `carla_core::library::sync` and runs **on the Ally**; the desktop is a passive file server for it through `/v1/saves/*`.

### 10.2 Routes

- `POST /v1/saves/find {game_id, title, place?}` (`saves_find`): look up the entry by `game_id`, else by case-insensitive title. If there is none, nothing is read or created; at most `suggest_saves` records a suggestion from the Ally's `place` (resolved with `saveplace::resolve` against this desktop's well-known folders; refused if the folder "loads code" or neither it nor its parent exists). If there is one: refuse with 409 while the game runs (`saves_free`), build `sync::manifest(folder)` and return `SavesFound { key, manifest, place, keep_clean }`.
- `POST /v1/saves/<key>/read|write|remove|protect|retire` (`saves_step`): the key must be in `saves.json` (404 otherwise); all but `read` take the process-wide `SAVES` lock; all refuse while a game runs here. `write` body is one JSON line `SaveFile {path, modified}`, a newline, then the file bytes (at most 32 MiB + 4 KiB, 62 s to arrive). `protect` makes a safe copy (`Backups`) before the Ally replaces files; `sync::Folder::write/remove` also keep a copy when they replace something (test: after the Ally's second day, `Backups::list().len() == 1`).
- A game is saves-synced only if it has a `saves.json` entry; other games stay private (test).

### 10.3 Suggestions

`save-suggestions.json` is populated only by `saves_find` and only from a `place`: a root (`SavedGames`, `Documents`, `MyGames`, `LocalAppData`, `LocalAppDataLow`, `RoamingAppData`, `ProgramData`, `GameFolder`) plus a relative path of at most 12 parts of at most 100 characters each (`crates/core/src/library/saveplace.rs`). Nothing is synced until the administrator runs `saves accept KEY --yes`, which re-resolves the place, requires the folder to exist and the resolution to equal the stored folder, rejects code-loading places and junctions below the root (tests `a_suggestion_in_a_place_that_runs_programs_cannot_be_accepted`, `a_junction_below_the_root_cannot_be_accepted`), then applies `set_save_folder_in` (the same checks as `saves set`).

### 10.4 Keep-clean (retire)

Opt-in per game, set only at the desktop CLI (`saves clean KEY on --yes`) and **never over the protocol**: `set_keep_clean` has no network caller, and `saves_find` only reports the flag. Naming the folder again resets it to `false`.

After a stream the Ally calls `retire` with its own manifest. `Server::retire` refuses unless: the entry is `keep_clean` (403); no game runs (409); the manifest is "sound" (`ally.sound()`: sizes, relative paths and 64-hex hashes within limits; 400); the Ally holds at least one file (409); the desktop's live manifest is **identical** (same paths, sizes, hashes, count; 409). Then, in order: take a retired safe copy (`take_retired`, kept longer than ordinary ones); verify the copy's manifest equals the live one; verify the live manifest still equals it (changed meanwhile: 409); verify no game started; finally `remove_checked`, which deletes each listed file **only if its current fingerprint still equals what was checked** (a changed or missing one is skipped and counted in `skipped`), never follows links, and leaves the folder itself. Any doubt keeps everything. The whole step runs under the `SAVES` lock. A later stream first brings the saves back from the Ally (test `a_stream_from_a_keep_clean_desktop_waits_until_the_allys_saves_are_on_it`). Recovery: `carla-companion saves copies KEY`, then `saves restore KEY COPY`.

---

## 11. The dashboard

User-facing description: `docs/DASHBOARD.md`. Implementation: `crates/companion/src/dashboard.rs` (server side, `impl Server`) and `crates/companion/src/dashboard.html` (one self-contained page: CSS, markup, JS; no framework, no build step, no external requests).

### 11.1 Trust model

The page is for whoever sits at the desktop and is not part of the signed API. Three independent checks, all in the server:

1. **Loopback only.** `work` computes `here = request.remote_addr().ip().is_loopback()` and only then calls `Server::local`. A non-loopback request for `/` or `/local/*` is not answered by the dashboard; it falls into `handle`, which refuses it as an unsigned request (401).
2. **Host header.** `own_name(host)` accepts only `127.0.0.1` or `localhost` (any port). This defeats DNS rebinding: a web page elsewhere cannot point its own hostname at this machine and reach the dashboard (403 "The dashboard only opens on this PC itself.").
3. **Token.** `Server.token = link::random_hex(16)` (32 hex characters) is generated **per `Server` instance, that is per process start**. `GET /` returns the page with `__CARLA_TOKEN__` replaced by it (`const TOKEN = "..."` in the script). Every `/local/*` request must carry it in header `x-carla-token`; a missing or wrong token gives 403 "This page is out of date. Reload it." The page reloads itself on any 403 (`call()` in the script), which covers a companion restart. Because the header is custom, a cross-site page cannot send it without a CORS preflight, which the server never grants (**UNVERIFIED:** no explicit CORS handling exists; `tiny_http` answers an `OPTIONS` preflight with whatever `handle`/`local` produce, which carries no `access-control-*` headers).

Plus: the page's CSP, `no-store` and `DENY` headers (3.7); the page builds DOM with `h()` (text only; `innerHTML` is used only for constant SVG in `svg()`); and state-changing routes accept **no parameters that name paths or options**: "Add a game" posts only `{id}` of a candidate the server itself found (`deny_unknown_fields`); "Install the screen driver" takes an empty body (400 otherwise).

### 11.2 The `/local` routes

Handled in `Server::local(method, path, host, token, bytes, now)`; it returns `None` for paths that are not `/` or `/local/*`. All except the first need the token.

| Route | Handler | Notes |
| --- | --- | --- |
| `GET /` | page | token substituted; `text/html; charset=utf-8` |
| `GET /local/overview` | `overview(now)` | The whole page state as JSON `Overview`: name, version, hardware, `headline {kind: streaming, connected, waiting, link or attention; title; detail}`, `stream`, `allies[]`, `checks[]`, `vitals`, `games`, `code`, `installing_driver` |
| `GET /local/log` | inline | `{path, lines}`: newest 100 lines of `host.log` (`LOG_LINES`) |
| `GET /local/newer`, `POST /local/newer` | `newer(fresh)` | Cached 6 h; POST forces a new check |
| `POST /local/code` | inline | `Files::new_code()`: open linking and show a code |
| `POST /local/code/hide` | inline | `clear_code()` |
| `POST /local/forget` | `forget` | body `{id}`; removes the device and its in-memory pings, logs "Device abcdef was forgotten from the dashboard." |
| `POST /local/games` | inline | `self.games(true)`: rescan now (a long route) |
| `POST /local/find` | `find_games` | Look for programs that may be games (long, single-flight `finding`); stores an `Offered { id, found, stamp }` list in `candidates` |
| `POST /local/add` | `add_found` | body `{id}`; the list must be at most 10 min old (409), the id must be in it (404), the program's size and mtime (and a shortcut's target) must equal the stamp taken at find time (409); then `Files::add_extra` with the finder's title and watch hint |
| `POST /local/update` | `update` | same function as `POST /v1/update`; 409 while a game runs |
| `POST /local/end` | `end` | same function as `POST /v1/stream/end` |
| `POST /local/virtual/install` | `install_driver` | empty body only; runs the installer thread; 409 if present or already installing |

Anything else under `/local/` returns 404 "The dashboard does not know that request." The dashboard's own polling (`GET /` and `GET /local/*` with 200) is not logged (3.8). `/local/*` replies are not signed.

### 11.3 Page structure and script

Markup (ids in parentheses): header with PC name and hardware, update banner (`#update`), hero card (`#hero`, `#headline`, `#headline-detail`, `#hero-actions`), then a 12-column grid of cards: Connection to the Ally (`#connection`), Stream (`#stream`), Linked Allies with "Link an Ally" (`#allies`, `#code`, `#link-button`), This PC (`#checks`, `#vitals`), Games the Ally can stream from here (`#games`, `#games-button`), Add a game (`#found`, `#find-button`), Log (`#log-text`, `#log-refresh`, `#log-copy`); footer (`#footer`) and `#toast`.

The script is one `<script>` with `"use strict"`:

- Helpers: `$`, `h(tag, attrs, ...children)` (builds elements; `onclick` and similar become `addEventListener`), `svg(markup)` (constant icons only), `ago`, `lasted`, `gb`, `cap`, `toast`.
- `call(method, path, body)`: `fetch` with the token header and `cache: "no-store"`; on 403 reloads the page; otherwise throws `Error(data.error)` when not ok.
- `act(button, method, path, body, done)`: disable the button, `await call`, run `done(answer)`, `await refresh()`, re-enable in `finally`; errors become a toast (except the internal "reloading").
- Renderers per card: `connection(o)`, `stream(o)`, `allies(o)`, `code(o)`, `checks(o)`, `vitals(o)`, `games(o)`, `update()`, and `render(o)` (assigns everything with `replaceChildren`, skipping the re-render when `JSON.stringify(o)` is unchanged so buttons keep focus).
- Polling: `refresh()` every 2 s while `!document.hidden` (and on `visibilitychange`), `loadLog()` every 4 s, `lookForNewer(false)` every 6 h. After two failed refreshes it shows a persistent toast that the page carries on by itself.

### 11.4 The rule: never read `event.currentTarget` after an `await`

Browsers set `event.currentTarget` only while the event is being dispatched; once the handler yields (any `await`, `.then`, timer) it is `null`. A handler such as `async (e) => { await call(...); e.currentTarget.closest("li").remove(); }` throws in a real browser even though it looks right. Rule: **capture everything you need from the event synchronously at the top of the handler** (`const button = e.currentTarget, row = button.closest("li");`), then use the captured variables after awaiting. All existing handlers comply: `act(e.currentTarget, ...)` reads it synchronously as an argument; `offered()` captures `button` and `row` first (with a comment); the log-copy handler reads `$("log-text")` and `logText` before `navigator.clipboard.writeText`; the footer's "Look for a newer version" handler sets `e.currentTarget.disabled` before its first `await`.

### 11.5 The page test: `scripts/test-dashboard.mjs`

Run by the cargo test `dashboard::tests::the_add_button_of_the_page_works_when_clicked`, which spawns `node scripts/test-dashboard.mjs crates/companion/src/dashboard.html` and **skips (prints "skipped: node is not available") when node is not installed**. The script:

- reads the HTML (normalising CRLF) and extracts two source snippets by **literal string markers**: from `function h(tag, attrs, ...children) {` up to `function svg(`, and from `function offered(list) {` up to `$("find-button").addEventListener`. If you rename or reorder those, the test fails with "marker not found";
- runs them in a `vm` context with a tiny fake DOM (an `El` class with `closest`, `remove`, `append` ...) and stubs for `toast` and `act`;
- renders `offered([...])`, clicks the Add button with `event.currentTarget` set, then sets `event.currentTarget = null` (as a browser does after dispatch), waits, and requires: no throw, no unhandled rejection, the row removed, and a toast containing "Test Game was added".

If you add a click handler that does work after an `await`, extend this script with the same pattern (extract the function by markers, simulate the click, null `currentTarget`). `scripts/test-inflight.mjs` is unrelated (it tests the shell).

### 11.6 How to add a dashboard section or button safely

1. **Decide whether it is read-only or acts.** Read-only: add fields to `Overview` (or a new `GET /local/<thing>` returning its own struct) and render them. Acting: add a `POST /local/<verb>` route.
2. **Server side** (`dashboard.rs`): add the serialisable struct(s) (`#[derive(Serialize)]`), compute them in `overview()` from `self.files` and injected closures. Never do slow work inline: reuse a cached value, or the cache plus `try_lock` pattern; never scan disks or call Sunshine directly on the 2-second poll. For an action add an arm in `Server::local`'s `match (method, path)`.
3. **Make actions parameter-poor.** Accept the `{id}` of something the server previously offered, or nothing. Use `#[serde(deny_unknown_fields)]` on the request struct and reject unexpected bodies like `install_driver` does. Never accept a path, command line or option from the page.
4. **Slow or blocking actions:** run the work on a spawned thread and answer at once (the `install_driver` pattern: an atomic single-flight flag released when the thread ends), or add the path to `takes_long` and use a single-flight guard (`find_games`).
5. **Log it** with `self.files.host("...")` in plain words (no secrets, no player text); do not add a log line to polling routes.
6. **Page side** (`dashboard.html`): add a `<section class="card span-N">` with a heading id and a container; write a `function thing(o)` returning an array of nodes built with `h()`; call it in `render(o)` with `replaceChildren(...)`; wire buttons with `act(e.currentTarget, "POST", "/local/verb", body, done)`; show errors with `toast`. Use `textContent`/`h()` only; never `innerHTML` with data. Keep it inline (CSP): no external scripts, fonts or images other than `data:` URIs.
7. **Obey 11.4.** Capture from the event before any `await`.
8. **Tests** (`dashboard.rs` tests, using `desk()`): the guard-test pattern (foreign Host gives 403, no token 403, wrong token 403, GET on a POST-only route 404), the happy path, and a state check on `desk.files`. If the page has new click logic after an `await`, extend `scripts/test-dashboard.mjs`.
9. If the section changes what the page tells users, update `docs/DASHBOARD.md` (user-facing; not covered here).

---

## 12. `host.log`

- Path: `<data dir>/host.log`; print with `carla-companion log path`, tail with `carla-companion log [--follow]`; the dashboard's Log card shows the newest 100 lines.
- Writers: the background companion (requests via the queue, watchers directly, start-up lines) **and the launcher process** (`Files::log` writes `launcher: <line>` to `host.log` and the same text with a Unix-time prefix to `launcher.log`, so "one file tells the whole story"). `screen.rs` and `virtual_screen.rs` log through `Files::log`, so their lines carry the `launcher:` prefix even when the background companion wrote them.
- Format: `2026-11-17 12:04:09.123 UTC <text>`; one line per event; IPv4 addresses and the Windows account name scrubbed; at most 400 characters; control characters flattened.
- Contents (all plain words):
  - Start-up: "Socket timeouts active..." or "WARNING: socket timeouts are not active...", "Companion <version> started, listening on port N; M linked device(s).", "Could not listen on port N (...): another companion most likely runs already."
  - Requests: one line each (3.8) with `SLOW` markers and refusal reasons; "Refused <method> <route>: device abcdef is linked, but the request's signature does not match..." / "...is not linked here (forgotten, or linked to another desktop)" / "Refused a request of abcdef: it is too old, too new or repeated (the Ally's and the desktop's clocks differ)."
  - Counts of left-out noise: "N more requests were refused or unknown (401) in the last 10 s, the last from stranger."
  - Server load: "All 8 workers are busy: ...", "Too many long requests at once: ... was turned away.", "Too many connections are stuck mid-request: ...".
  - Presence and Sunshine changes (section 5).
  - Link, pair and stream events: "A device was linked: abcdef.", "Pairing with Moonlight began (the PIN is not written down).", "Pairing with Moonlight worked/failed: ...", "Prepare: <title> (<id>) is ready ...", "Installing <title> (steam:N) was asked for.", "Closing <title> was asked for; the launcher closes it.", "The Ally asked this PC to go to sleep.", "Updating to X was asked for: the companion restarts."
  - Sunshine starting: "Sunshine was not running: starting it.", "Sunshine is ready after N s.", "Sunshine could not be started: ...".
  - Launcher and screens (prefix `launcher:`): game started, closed or never appeared; screen switched or put back; virtual screen created or gone; curtain lifted ("The game did not show: the loading screen was taken away." / "The loading screen was taken away from the Ally.").
- Rules for new lines: plain words; short ids only (`label()` is the first six characters of a device id); never keys, PINs, tokens, signatures, save contents, device names, or game names inside routes. Do not log inside the 2-second dashboard poll. A line about a request should go through `load.say` (non-blocking) like the request line; existing handlers call `files.host` directly for rare events, which is a blocking disk append, so keep that off hot paths.
- Size: 256 KiB, then cut to the newest half of the lines.

---

## 13. Test harness

All companion tests run with `cargo test --locked --lib --bins` (this is a binary crate, so its tests run in the bins target). CI runs `cargo test --locked -p carla-companion` on Windows when `needs.plan.outputs.tests == 'true'`, which also exercises DPAPI sealing for real. Do **not** run `crates/core/tests/windows_live.rs` (it changes Windows configuration). On this machine cargo must run from a VS developer shell (`knowledge/development/known-issues.md`, KI-006).

### 13.1 `desk()` / `desk_with()` (in `server.rs` `mod tests`; `dashboard.rs` re-uses it as `super::super::tests::desk`)

`desk(name, down)` is `desk_with(name, down, IO_WAIT, Limits::default())`. It builds a **real companion on a real loopback socket**:

- A unique temp folder `<temp>/carla-companion-<name>-<pid>-<n>` where `n` comes from a static `AtomicUsize`, so two tests that pass the same name never share a folder (the fix for the historic flake below). It is wiped on creation, holds `cover.jpg`, and a private `profile/Saved Games` `Env`, so no test reads the real profile.
- `Files::new(dir).with_env(env)`, `Config::new("HOME PC")`, a `FakeSunshine { down, calls }` (records `app <command>` and `pair <pin> <name> <from>`; `check` fails when `down`), a fixed scan of three games (steam 10 "First Game" with cover, steam 20 "Second Game" with an update, gog 30 "Third Game" with exe and args), a fake `network`, achievements, `sleep` (increments the static `SLEEPS`), updater (`9.9.9`), `Installs` (static `ASKED_FOR` records install requests), a finder returning one candidate ("Pixel Quest"), `with_admin_hold(400 ms)` and graphics.
- Binds `127.0.0.1:0` plus a UDP socket on the same port number (retrying until both bind), starts `discover::answer` in a thread and `serve_with(server, http, limits)` in a thread, and returns `Desk { files, calls, port }`. Everything goes through real HTTP with real signing.
- `linked(&desk)` makes a code with `files.new_code()` and enrols with the real client (`carla_core::link::client::enrol`), returning a `Link` (device id and key) whose methods (`status`, `games`, `prepare`, `saves`, ...) are the same client code the shell uses.
- `desk_with(name, down, wait, limits)` lets a test shrink the socket timeout (`wait`) and `Limits { workers, long, body }` to provoke overload cheaply (1 worker, a 500 ms body deadline ...).
- `server_with(name, scan)` makes a **non-listening** `Server` for calling parts directly (`games`, `notice`, `noisy` ...), also with a counter-based unique folder.
- `handle()` can be called directly with a hand-built `Request` (test `a_captured_request_cannot_be_replayed_or_altered`).
- `dashboard.rs` tests add `get`/`ask`/`token`/`overview` helpers over raw TCP with a chosen `Host` header and `x-carla-token`.

### 13.2 How tests avoid shared state

- One data folder per `desk`/`server_with` (counter plus pid). The unit tests in `files.rs` and `launcher.rs` build `temp_dir().join(format!("carla-...-{pid}"))` with a **hand-chosen distinct name per test**; two tests using the same literal name in one process would collide, so keep names unique when you add a test (or reuse the counter pattern).
- Process-global statics exist and are shared across tests: `ASKED_FOR`, `SLEEPS` (compare before/after counts rather than absolute values), and the process-wide log lock `applog::WRITING`.
- The `Env` override keeps save-location resolution away from the real profile.
- `stage`, `screen` and `virtual_screen` logic is tested against fakes (`screen::tests::Fake`, `virtual_screen::tests::Fake` implementing the traits), so no display or driver is touched.
- `setup::Starter` is tested with `FakeLaunch` and `Starter::timed(limit, poll, gap)`, so nothing real is started and time is short.
- Tests that need real Windows behaviour are `#[cfg(windows)]` (`a_client_that_went_away_in_the_middle_of_a_request_frees_its_worker`); others branch on `cfg!(windows)` (silent clients: 6 on Windows, 3 elsewhere).

### 13.3 Historic flakiness and its causes

1. **Shared state folder (KI-009, fixed in PR #45, commit `9d16995`).** Two tests called `desk("refuse", ...)`; the helper named the folder after the name and process id and wiped it first, so parallel tests deleted each other's devices and enrolment files. `a_link_that_is_refused_at_the_desktop_links_nothing` and `strangers_and_forgotten_devices_are_refused` failed at random (about 5 runs in 6 on a developer machine). Fix: a per-desk counter.
2. **Silent-client test on a Linux CI runner (PR #57, commit `82da4fd`).** Without socket timeouts (everywhere but Windows) each silent client keeps a `tiny_http` connection thread; the test now uses fewer silent clients off Windows and a generous 10 s bound for the follow-up request ("a busy shared runner can be slow, a stuck server is never answered").
3. **Asynchronous log writes.** Request lines go through the queue and writer thread and are written after the reply, so asserting on `host.log` right after a request is racy. Use `logged(files, needle)` or `logged_all(files, &[needles])`, which poll the log every 30 ms for up to 6 s; wait for **every** line you will assert on (lines of different requests, or of the writer thread and a worker, arrive in no guaranteed order). A negative assertion ("the log never contains X") is only meaningful after a positive wait for a later line.
4. **Fixed sleeps** in timing tests (scan 400 ms, trickle 50 ms, `hello_within` bounds) are deliberately wide; if you add one, bound it generously and assert a property ("answered in under N s") rather than an exact time.
5. **Environment.** The MSVC linker is needed for cargo (KI-006), and the dashboard test silently skips without node.

### 13.4 Other checks

- `git diff --check` before finishing.
- `packaging/smoke-test.ps1` runs against the built installer in CI and writes the per-user Run key (see its source); do not run it on a developer machine without reading it, because it changes Windows configuration.
- Frontend checks (`npm run build` in `apps/shell`) are not part of the companion.

---

## 14. How to add things

### 14.1 A CLI command

1. Add the usage lines to the `HELP` string in `main.rs` (it is also printed for unknown commands).
2. Add a `Some("name") => ...` arm in `run(args)`. Keep the arm short: parse, call, print, return `Ok(code)`; return `Err(String)` for user-facing failures (the text goes to stderr and the exit code is 1; with no arguments at all it also pauses).
3. Put logic in a function or module (`fn thing(files: &Files, args: &[String]) -> Result<i32, String>`) and put **argument parsing in a separate pure function** (like `parse_add`, `parse_clean`, `logview::parse`, `describe::parse`) with unit tests that include refusals: unknown flags, missing values, odd forms. Prefer strict parsing; for anything destructive follow `saves clean`: exact keywords, required `--yes`, print what would happen and exit 1 without it.
4. Use `configured(&files)?` when set-up is required. Persist through `Files` accessors, not ad-hoc file writes.
5. A command that must reach the running companion cannot call into it directly: use the files-as-mailbox pattern (`ally::ask`) or an HTTP call to localhost. A running companion keeps its own copy of `config.json` (7.1).
6. Never read, print or log secrets except where a command exists to show them (`sunshine`, only for the companion-chosen password).
7. If the command is a toggle read by a running companion, check where it is read: `launcher::run` re-reads config at each stream; `Server.config` does not.
8. Document it in `docs/*.md` if it is user-facing.

### 14.2 A `/v1` route handler

Interface warning: the **request/response structs and route names are the Ally/desktop protocol** (`crates/core/src/link`, `crates/core/src/remote.rs`, the client in `crates/core/src/link/client.rs`). Adding a route touches that shared interface, which is a coordinated change, not something to do inside the companion alone. Within the companion the steps are:

1. Define the request and response types in `carla_core::link` (plus a client method in `link::client` for the Ally), `Serialize`/`Deserialize`, with `#[serde(default)]` on new optional fields so older Allies and desktops interoperate (the two sides ship separately; `Hello.api_version` exists, **UNVERIFIED:** how the Ally compares `API_VERSION`; check `crates/core/src/link`).
2. Add a method on `Server` returning `Answer` (`Result<(Vec<u8>, &'static str), (u16, String)>`): parse with `body::<T>(bytes)?` (400 "The request was not understood."), refuse with `refuse(status, "plain sentence")`, answer with `json(&value)`. Use the `now` passed in, never `SystemTime` directly. Mutating or disruptive actions must refuse with 409 while `self.files.playing(now).is_some()` if a running game would be harmed.
3. Add the arm to `Server::signed`'s `match (req.method, req.path)`. Signed is the default; add to the `open` list in `handle` only for routes that must work before linking, and then they need their own abuse limits, as the enrol routes have.
4. If the handler can block or wait: add the path to `takes_long`. If it receives a body larger than 64 KiB from a linked device: add a `most` case in `work` and think about `body_time`.
5. Add the route to `route_shape` (the `PLAIN` array, **and fix its length in the type `[&str; N]`**) or, for a path with an id segment, to the `(prefix, ":thing")` loop, so the request line and noise handling are right.
6. Keep reads cheap: no disk scans or network on the request path without the cache plus `try_lock` pattern (4.2).
7. Errors are plain sentences for the player; include `self.config.name` where the player must know which desktop.
8. Tests in `server.rs` `mod tests`: use `desk()` and `linked()`; assert success; a stranger (`Link { key: vec![7; 32], ..link.clone() }` gives `Error::Refused`); "while a game runs" via `desk.files.set_playing(Some(&Playing{..}))`; check the log line with `logged_all` if relevant.

### 14.3 A dashboard action

See 11.6 (full checklist). Minimal skeleton:

```rust
// dashboard.rs, inside Server::local's match (token already verified):
("POST", "/local/thing") => self.thing(bytes),

fn thing(&self, bytes: &[u8]) -> Answer {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Pick { id: String }          // an id the server offered earlier, nothing else
    let pick: Pick = body(bytes)?;
    // validate against server-held state, then act through self.files / injected closures
    self.files.host("Thing was done from the dashboard.");
    ok()
}
```

```js
// dashboard.html
h("button", { class: "btn small", type: "button", onclick: (e) => {
  const button = e.currentTarget;               // capture before any await
  act(button, "POST", "/local/thing", { id }, () => toast("Done."));
} }, "Do it")
```

### 14.4 A new background watcher

Spawn it from `serve()` in `main.rs` (like `screen::watch`) or from `serve_with` (like the notice thread); take a `Files` clone; loop with a sleep of several seconds; never panic (a panic ends only that thread, silently, and nothing restarts it; **UNVERIFIED:** no supervisor or panic hook was found in the companion; confirm in `main.rs`); write to the log only on change; if it changes machine state, apply the note-first discipline (write `<thing>.json` before the change, delete it after undoing, and make the watcher undo it when stale).

### 14.5 A new external dependency of the server

Add a trait or `fn` pointer field with a safe default in `Server::new`, a `with_*` builder, wire the real implementation in `main.rs::build`, and give `desk_with` a fake. This keeps `cargo test` independent of Windows, Steam and Sunshine.

---

## 15. Open questions / needs investigation

1. **Concurrent read-modify-write on `Files`**: no lock protects `devices.json`, `games.json`, `saves.json`, `consoles.json` and `config.json` against two simultaneous writers (only suggestions and save steps are serialised). Confirm whether any realistic overlap exists (dashboard forget versus enrol finish, CLI versus the running companion) and whether a `Files`-level mutex is wanted.
2. **Two processes appending to `host.log`**: the background companion and the launcher both write it; the in-process `WRITING` lock does not cover the other process, and the cut-to-half is a read-all-then-rewrite. Confirm that no lines are lost and the file is not corrupted under concurrent truncation.
3. **`eprintln!`/`println!` in the launcher after `FreeConsole`**: assumed harmless; confirm on a real Sunshine launch (the null-stdio rule is documented for child processes by the comment in `launch.rs`, not for the launcher's own output).
4. **Top-most windows after a killed launcher**: store windows raised with `HWND_TOPMOST` are only lowered on a normal curtain close.
5. **`forget_device` leaves `ally-listening-<id>.json`** (only `ally-seen` and `ally-pulse` are removed), and any `ally-ask`/`ally-answer` files. Harmless? Decide.
6. **Config copy in the running server**: CLI changes (`stream screen|curtain|virtual`, `sunshine`) are not seen by a running companion's `Server.config` until restart (the dashboard's "Picture for the Ally" check, `fit` and `prepare` use that copy), while `launcher::run` re-reads the file. Intended? The `stream` help says "From the next stream on".
7. **`GET /v1/games` rescans on every call** (via `games(true)`, unless a scan is already running). Is that the intended cost for large libraries, given the 60 s `SCAN_AGE` used elsewhere?
8. **UDP discovery and measuring packet formats**, and `API_VERSION` negotiation: read `crates/core/src/link/{discover,probe}.rs` and the shell client, then document them.
9. **tiny_http thread model and drop behaviour** (`mem::forget` leaves the connection open): confirm in the 0.12 source and with a socket-level test on Windows.
10. **Update download location and installer flow** (`carla_core::update`): not read for this document.
11. **A dangling doc comment** above `Reply` in `server.rs` ("The header a local program puts its token in (see `Files::local_token`)") refers to something that no longer exists (the token is `Server.token`, with header `x-carla-token` read inline in `work`); consider removing it.
12. **Does the dashboard show a pending approval request?** `Files::asking` is read only by the console prompt (`attend`) in the code read; confirm whether any dashboard card is meant to show "an Ally wants to link".
13. **Hardware verification**: nothing in this file is hardware-verified by itself; the live behaviours (VDD pipe protocol, display restore after a kill, firewall rules, Sunshine API differences across versions, `conhost --headless`, DPAPI scope) need the checks in `knowledge/development/validation-plan.md`.
