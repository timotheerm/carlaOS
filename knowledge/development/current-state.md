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
**Local test run 2026-11-17** (Windows, branch `docs/dev-environment`, no source changes): companion 56 tests, core 199 unit tests and 6 `windows_live` tests passed (companion had one flaky failure on the first run, [KI-009](known-issues.md)). The shell crate (`apps/shell/src-tauri`) and `npm run build` were not run. CI (`Build` workflow: `cargo test --locked`, `npm run build`, Windows
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

"Not recorded" = no doc states it was tried on hardware.

## Process state

- Release line: v0.22.0 was the latest tag fetched; ~87 commits on `main`.
- Open PRs at last check (2026-11-17): none.
- Dev-environment setup (agents, skills, vault) is on branch `docs/dev-environment`, uncommitted — see [session log](session-log.md).

## How to refresh this note

Run `/update-memory`, or ask the `memory-curator` agent to re-verify a row. Change a cell only
with evidence (file path, test name, CI run, or a dated hardware test) and update `updated:`.
