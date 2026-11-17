---
title: Components
type: architecture
updated: 2026-11-17
---

# Components

Paths are relative to the repository root.

## Shell — `apps/shell`

- Frontend `src/` (React/TS): screens in `src/screens/`, controller/touch navigation in `src/nav/`, design tokens `tokens.css`, native bridge `native.ts`.
- Native side `src-tauri/src/*.rs`: one module per capability (library, installs, hosts, power, quick menu, boot, cloud, consoles, savesync, updates, remote, admin, presence, art/autoart, discover, device, details, system).
- Check: `npm run build` in `apps/shell` (`tsc --noEmit && vite build`).

## Core — `crates/core`

Shared logic: `library/` (catalogue, scanners), `link/` (companion protocol), `debloat/`
(reversible Windows tuning), plus `admin`, `boot`, `display`, `layout`, `nap`, `network`,
`power`, `presence`, `quick`, `remote`, `secret`, `update`. Live-Windows test: `tests/windows_live.rs`.

## Companion — `crates/companion`

Per-desktop background program: `server.rs` (protocol), `sunshine.rs` (Sunshine management),
`launcher.rs`, `stage.rs` / `screen.rs` / `virtual_screen.rs` (game-only screen at the Ally's size),
`ally.rs`, `setup.rs`, `dashboard.rs` + `dashboard.html`.

## Packaging and release

`packaging/` (three installers + install check), `scripts/release/` (versioning, notes),
`scripts/check-windows.sh`, `.github/workflows/build.yml`. See [docs/RELEASES.md](../../docs/RELEASES.md).

## Feature docs

| Topic | Docs |
| --- | --- |
| Library | [LIBRARY](../../docs/LIBRARY.md), [INSTALLING](../../docs/INSTALLING.md), [STORES-AND-SEARCH](../../docs/STORES-AND-SEARCH.md), [DESKTOP-GAMES](../../docs/DESKTOP-GAMES.md) |
| Streaming | [STREAMING](../../docs/STREAMING.md), [STREAM-PICTURE](../../docs/STREAM-PICTURE.md), [STREAM-SCREEN](../../docs/STREAM-SCREEN.md), [DASHBOARD](../../docs/DASHBOARD.md) |
| Administration | [REMOTE-ADMIN](../../docs/REMOTE-ADMIN.md), [SAVE-SYNC](../../docs/SAVE-SYNC.md) |
| Consoles, cloud, art | [EMULATORS](../../docs/EMULATORS.md), [CLOUD](../../docs/CLOUD.md), [ARTWORK](../../docs/ARTWORK.md) |
| Windows tuning | [CLEANUP](../../docs/CLEANUP.md), [POWER](../../docs/POWER.md), [STARTUP](../../docs/STARTUP.md) |
| UI feel | [IMMERSIVE](../../docs/IMMERSIVE.md), [CONSOLE-FEEL](../../docs/CONSOLE-FEEL.md), [QUICK-MENU](../../docs/QUICK-MENU.md) |
| Process | [RELEASES](../../docs/RELEASES.md), [GIT_WORKFLOW](../../docs/GIT_WORKFLOW.md) |
