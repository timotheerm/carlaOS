---
title: Current state
type: status
updated: 2026-11-17
baseline: main @ 20a80bf (latest tag v0.22.0)
---

# Current state

> Per-area implementation and verification status. For open PRs, branches and CI, see the dated snapshot [[../operations/current-state]] and verify with `gh`/`git`.

Three evidence levels are used. Never merge them.

- **Designed** — described in `docs/` or ADRs.
- **In source** — code exists in the repo (file listing / grep). Unit tests may or may not exist.
- **Hardware-verified** — a person ran it on a real Ally/desktop. Docs say none of the streaming,
  remote admin or save-sync paths have been; treat everything as *not* hardware-verified unless a
  dated entry below says otherwise.

Last inspection: 2026-11-17 — file listing, test-attribute counts and doc status lines only.
**Local test run 2026-11-17** (Windows, local): core 255 and companion 80 tests passed. Earlier run the same day (branch `docs/dev-environment`, no source changes): companion 56 tests, core 199 unit tests and 6 `windows_live` tests passed; companion had one flaky failure then, fixed in [KI-009](known-issues.md). Not recorded for either run: the shell crate (`apps/shell/src-tauri`) and `npm run build`. CI (`Build` workflow: `cargo test --locked`, `npm run build`, Windows
installers) is the authority for pass/fail.

## Areas

| Area (milestone) | In source | Unit tests (count of `#[test]`, approx.) | Hardware-verified | Notes |
| --- | --- | --- | --- | --- |
| Shell UI, navigation (M1) | `apps/shell/src` | none (no frontend test script) | no | |
| Library, Steam, manual exe (M2) | `crates/core/src/library`, `src-tauri/src/library.rs` | yes (in core) | not recorded | [LIBRARY](../../docs/LIBRARY.md) |
| Companion + streaming (M3) | `crates/companion/src` | ~60 across companion modules | **no** | stand-ins only: [STREAMING](../../docs/STREAMING.md) |
| Hosts, wake, Tailscale, remote admin (M4) | `core/{nap,network,presence,remote,admin}`, `src-tauri/{hosts,remote,admin,presence}.rs` | few | **no** | [REMOTE-ADMIN](../../docs/REMOTE-ADMIN.md) |
| Stores, artwork, search (M5) | `src-tauri/{art,autoart,discover}.rs`, `screens/search.tsx` | not counted | not recorded | [STORES-AND-SEARCH](../../docs/STORES-AND-SEARCH.md) |
| Consoles/emulators (M6) | `src-tauri/consoles.rs`, `screens/consoles.tsx` | not counted | not recorded | [EMULATORS](../../docs/EMULATORS.md) |
| Boot, Windows tuning, settings (M7) | `core/{boot,debloat}`, `src-tauri/boot.rs`, `screens/{startup,cleanup}.tsx` | yes (core) | not recorded | [STARTUP](../../docs/STARTUP.md), [CLEANUP](../../docs/CLEANUP.md) |
| Cloud tiles, power, quick menu (M8) | `src-tauri/{cloud,power,quick}.rs`, `core/{power,quick}` | yes (core) | not recorded | [CLOUD](../../docs/CLOUD.md), [POWER](../../docs/POWER.md), [QUICK-MENU](../../docs/QUICK-MENU.md) |
| Updates, installers (M9) | `core/update.rs`, `packaging/`, CI | yes (update) | n/a | Unsigned betas |
| Save sync | `src-tauri/savesync.rs` | stand-ins | **no** | [SAVE-SYNC](../../docs/SAVE-SYNC.md) |
| Launcher fix for streamed games (v0.22.1, PR #37) | PR #37 | Windows regression test | no | In source, unit/integration tested, not hardware-verified. [STREAMING](../../docs/STREAMING.md) |
| Crash reports: panic hook, 'did not end' marker, Settings (v0.23.0, PR #38) | PR #38 | yes | no | In source, unit/integration tested, not hardware-verified. [CRASH-REPORTS](../../docs/CRASH-REPORTS.md) |
| Freeze reports: heartbeat and watchdog (v0.24.0, PR #39) | PR #39 | yes | no | In source, unit/integration tested, not hardware-verified. [CRASH-REPORTS](../../docs/CRASH-REPORTS.md) |
| Save-folder finder, `saves guess\|find` (v0.25.0, PR #41) | PR #41 | yes | no | In source, unit/integration tested, not hardware-verified. [SAVE-SYNC](../../docs/SAVE-SYNC.md) |
| Guided add-a-game: `find`, `add --watch`, dashboard (v0.26.0, PR #42) | PR #42 | yes | no | In source, unit/integration tested, not hardware-verified. [DESKTOP-GAMES](../../docs/DESKTOP-GAMES.md) |
| Game description for hand-added games, `describe` (v0.27.0, PR #43) | PR #43 | yes | no | In source, unit/integration tested, not hardware-verified. [DESKTOP-GAMES](../../docs/DESKTOP-GAMES.md) |
| Save places on a second desktop, `saves suggestions\|accept\|dismiss` (v0.28.0, PR #44) | PR #44 | yes | no | In source, unit/integration tested, not hardware-verified. [SAVE-SYNC](../../docs/SAVE-SYNC.md) |
| Opt-in keep-desktop-clean: retire route, stream gate (v0.29.0, PR #46) | PR #46 | yes | no | In source, unit/integration tested, not hardware-verified. v0.29.0 build pending when written. [SAVE-SYNC](../../docs/SAVE-SYNC.md), [ADR-009](../decisions/ADR-009-save-places-and-opt-in-cleanup.md) |

"Not recorded" = no doc states it was tried on hardware.

## Process state

- Release line: v0.22.0 was the latest tag fetched; ~87 commits on `main`.
- Since v0.22.0: PRs #37–#46 merged, each publishing a beta (v0.22.1 to v0.29.0; v0.29.0 build pending when written). #40 and #45 were no-release.
- Open PRs at last check (2026-11-17): none besides the one carrying this note. Latest releases v0.22.1 to v0.28.0, with v0.29.0 (keep-desktop-clean, #46) building; verify with gh release list.
- Dev-environment setup (agents, skills, vault) is on branch `docs/dev-environment`, uncommitted — see [session log](session-log.md).

## How to refresh this note

Run `/update-memory`, or ask the `memory-curator` agent to re-verify a row. Change a cell only
with evidence (file path, test name, CI run, or a dated hardware test) and update `updated:`.
