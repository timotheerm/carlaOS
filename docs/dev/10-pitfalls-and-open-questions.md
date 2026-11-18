# 10 · Pitfalls and open questions

Every item here cost real time once. Each says what happened, why, and the rule that prevents it.
Check this list before you open a PR. The "Open questions" at the end collect what is still
unknown across the whole codebase; each chapter also ends with its own list.

---

## A. Pitfalls (with their history)

### A1. A lock guard in a `for` / `if let` / `while let` / `match` header lives for the whole construct

- **What happened.** The Ally froze for good 45 s after start (v0.20.0–v0.34.0). `hosts::watch`
  looped over `library.store().hosts()...`; the guard lived for the loop and `seen()` locked the
  store again on the same thread. See the post-mortem in
  [01 §7](01-architecture-and-runtime.md#7-post-mortem-the-client-freeze-v0200--v0340).
- **Rule.** Read into a local (`let x = library.store().thing();`) before the header. Never call
  a function that takes the same lock while holding it. CI denies
  `clippy::significant_drop_in_scrutinee` for the shell; follow the same rule in core and
  companion.
- **Also.** `if let Ok(p) = f(&library.store()) { slow_windows_call(p) }` holds the store during
  the slow call. Same fix.

### A2. A console-less process gives children invalid standard handles

- **What happened.** Every streamed game failed to start ("The handle is invalid", os error 6) and
  Moonlight showed a misleading "DRM protected content" style error. The launcher Sunshine starts
  calls `FreeConsole` so no console is in the picture; afterwards Windows' default inherited
  handles are invalid and `CreateProcess` fails.
- **Rule.** Every `Command` that can run from the launcher sets stdin/stdout/stderr to
  `Stdio::null()` (`crates/core/src/library/launch.rs` `quiet()`), and the same for anything the
  desktop host starts (Sunshine, winget). Fixed in v0.22.1; a Windows test reproduces the broken
  handle state in a child process.

### A3. Nothing may throw before the first render

- **What happened.** 0.33.0 showed only a white screen. A diagnostic wrapper patched
  `window.__TAURI_INTERNALS__.invoke` at the top of `main.tsx`, before React rendered; an
  exception there stops the page.
- **Rule.** Diagnostics and patches are installed after the first screen, inside `try/catch`, and
  must hand every call to the original unchanged (`apps/shell/src/inflight.ts`). There is no
  frontend test runner, so add a small node test for risky startup code
  (`scripts/test-inflight.mjs`).
- **UNVERIFIED:** the exact exception in 0.33.0 was never captured (no device logs); the fix made
  every failure mode harmless rather than proving the cause.

### A4. `event.currentTarget` is `null` after an `await`

- **What happened.** The desktop dashboard's "Add a game" button reported
  "Cannot read properties of null (reading 'closest')" although the game was added.
- **Rule.** In page scripts, read `event.currentTarget` (and the row/element derived from it)
  synchronously at the start of the handler. `scripts/test-dashboard.mjs` covers the Add button.

### A5. Tests that share a folder or a timing assumption flake

- Two companion tests both used `desk("refuse", …)`; the helper named its folder after that and
  wiped it, so parallel tests deleted each other's files. Every desk now gets a unique folder.
- Host-log tests waited for one log line and asserted on another written a moment later by the
  log writer thread. Wait for every line you assert on (`logged_all`).
- A fake server answered after one `read()`; when the request arrived in two pieces the
  connection reset and the client saw "unreachable". Read the whole request first.
- Holding server slots while writing a log line made sequential requests get 503 under load.
- **Rule.** Unique temp folders per test, no shared statics, wait for observable conditions with
  a bounded poll, never sleep-and-hope. Run a suspicious suite 10–30 times in a row before calling
  it fixed.

### A6. Plain `cargo test` changes the machine

`crates/core/tests/windows_live.rs` creates and removes real registry values, start-up entries
and service settings on the machine it runs on. A plain `cargo test` at the root runs it. Use
`cargo test --locked --lib --bins` locally; run the live tests only on a throwaway machine (CI
does) or with the owner's approval.

### A7. Building on Windows

- Rust needs the MSVC linker: run cargo from a Visual Studio developer environment
  (`vcvars64.bat` / "Developer PowerShell for VS"). In Git Bash, GNU `link` from MSYS shadows
  MSVC's `link.exe` and every build script fails to link.
- Two worktrees sharing one `CARGO_TARGET_DIR` can run stale test binaries after switching
  branches; touch a changed file or use a target directory per worktree when results look wrong.
- Working copies are CRLF (`core.autocrlf=true`), the index is LF. Scripted edits must normalise
  line endings; inserting `"\r\n"` text through a script once produced bare-CR compile errors.

### A8. Release numbering surprises

- Every merge to `main` publishes a beta. A PR titled `Fix …` (or labelled `fix`/`bug`) gives a
  fix version; label `no-release` or docs-only changes publish nothing.
- The planner once skipped a real release because GitHub had not yet linked a just-merged PR to
  its commit; unresolved commits now count as normal changes (`scripts/release/changes.sh`).
- Merging two release PRs in quick succession: wait for the first one's `main` build and tag
  before merging the next, or both land in one version.
- CI runs are grouped per branch; a newer push or a run on `main` can **cancel** a job, which
  shows as a failed check. Re-run it rather than "fixing" code.

### A9. Players and administrators see different things

Messages to the player are one plain line that says what to do. Detail goes to the crash trail,
`connection.log` (Ally) or `host.log` (desktop). Administrator-only actions are enforced in the
native command (`admin.require()`), not only hidden in the interface.

### A10. Protocol compatibility

The Ally and desktops update independently. Every new field is optional with a default; an
older peer answering 404 means "not supported"; unknown enum values must not fail a whole
message (`SaveRoot::Unknown` is the pattern). Removing or renaming a field breaks older peers.

### A11. Saves are user data

Nothing may delete or overwrite saves without: an exact manifest match (paths, sizes, SHA-256),
a backup written and read back, and the game not running. The keep-desktop-clean feature
(ADR-009) shows the full set of guards, including the stream gate that survives a restart. A
"helpful" retry or a stop in the middle of a copy can lose a save; several reviews caught exactly
that.

### A12. Safety boundary

CarlaOS never downloads games, ROMs, firmware or keys and never bypasses DRM or anti-cheat.
Discovery reads files and never runs programs it finds. Windows tuning is reversible and records
the previous value. Paste-ready command lines are only printed when every character is safe.

---

## B. Candidates for simplification (not done)

These were added while the freeze's cause was unknown (§A1). They work, but each adds code paths.
Re-evaluate them once the fix has been confirmed on devices for a while:

| Piece | Where | Question |
| --- | --- | --- |
| 10 s deadline on `hosts_refresh`, late results dropped by generation | `hosts.rs`, `link::deadline::collect_until` | Still needed now that refresh threads no longer block on the store? A single slow desktop is bounded by request timeouts anyway. |
| 15 s interface timeout around `hosts_refresh` | `library/store.ts` | Same question. |
| 20 s limit on `savesync::BUSY` and the launch refusal | `savesync.rs` | The lock is no longer held across a deadlock; can a pass still take long enough to matter? |
| Interface command timing wrapper | `inflight.ts` | It cannot see native threads (it missed the real freeze) and caused 0.33.0's white screen. |
| Freeze watchdog judging a window while a session is marked active | `crash.rs` | Useful as a report; keep or simplify. |

## D. Findings from writing this guide (not fixed; check before relying on them)

Found while reading the code for this guide. Each is a candidate issue, not a confirmed bug; the
chapter named has the detail.

| Finding | Where | Chapter |
| --- | --- | --- |
| The replay-nonce set is in memory only: a signed request captured within the 300 s window could be accepted once more after the desktop host restarts. | `crates/companion/src/server.rs` signing check | [06](06-link-protocol.md) |
| The desktop's own refusals (400/408/503) are unsigned, so a linked Ally reports them as "untrusted" instead of the real reason. | `server.rs` / `client.rs` | [06](06-link-protocol.md) |
| Body-size caps are chosen from the claimed `x-carla-device` id before the signature is checked. | `server.rs` | [06](06-link-protocol.md) |
| Only `SaveRoot` has a `#[serde(other)]` fallback; adding a variant to any other protocol enum breaks older readers. | `crates/core/src/link/mod.rs` | [06](06-link-protocol.md) |
| Removing a desktop on the Ally does not tell the desktop (PR #60, unmerged when this was written, adds `POST /v1/link/leave`). | `hosts_remove` | [06](06-link-protocol.md) |
| `installs_start` holds the `active` lock across a full process scan (not caught by the lint, which only covers headers). | `apps/shell/src-tauri/src/installs.rs` | [04](04-ally-native.md) |
| `storage_overview` has no `admin.require()`. Decide whether it should. | `installs.rs` | [04](04-ally-native.md) |
| Both windows run `main.tsx`, so the hidden quick-menu window also sends the freeze heartbeat (`crash_alive`) and could mask a frozen main window. | `apps/shell/src/main.tsx`, `crash.rs` | [05](05-ally-ui.md) |
| `GameDetail` calls hooks after an early return; removing a game while its page is open could blank the UI (no error boundary). | `apps/shell/src/screens/pregame.tsx` | [05](05-ally-ui.md) |
| `Library::open` falls back to an in-memory catalogue when `library.sqlite` cannot be opened; whether the player is told is unknown. | `apps/shell/src-tauri/src/library.rs` | [07](07-data-and-storage.md) |
| A doc comment above `Reply` in `server.rs` refers to a function that no longer exists. | `crates/companion/src/server.rs` | [03](03-desktop-host.md) |
| `scripts/release/*.sh` are CRLF in a Windows working copy (no `.gitattributes`); running them from Git Bash locally may fail on `
`. CI checks out LF. | `scripts/release` | [08](08-build-test-release.md) |

## C. Open questions across the codebase

- **UNVERIFIED:** whether destroying the quick-menu window from the session follower thread
  (`quick::ended`) is safe in Tauri on Windows. It was a suspect for a hang; nothing has been
  traced to it since §A1 was fixed.
- Moonlight occasionally captured the wrong monitor or stretched a 21:9 desktop: Sunshine's
  capture adapter/output are not set by CarlaOS. A virtual screen per stream (driver installed
  from the dashboard) avoids it. Not verified end to end on hardware.
- 120 fps streams require the Ally's Windows display at ≥ 100 Hz and a measured ~72 Mbit/s (or a
  fixed "smooth" picture); nothing above 120 is requested. Not tuned on real Wi-Fi.
- Almost nothing has been verified on a real Ally with a real desktop; see
  [`knowledge/development/validation-plan.md`](../../knowledge/development/validation-plan.md) and
  the known-issues list.
