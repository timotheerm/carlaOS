# Architecture

Status: approved 11 Nov 2026. M0 to M2 are merged; the local library is described in [LIBRARY.md](LIBRARY.md). M3 (streaming from a desktop) is described in [STREAMING.md](STREAMING.md).

## Shape

Two programs share one Rust core.

- **Shell** (`apps/shell`) runs on the Ally. It starts with Windows, so the device
  boots into it (see `STARTUP.md`). Becoming the home app of Windows' full
  screen experience is a later step that needs different packaging.
- **Companion** (`crates/companion`) runs on each desktop. It reports status,
  hardware and installed games, prepares a game for streaming, and manages
  Sunshine.
- **Core** (`crates/core`) holds what both need: the catalogue model, store
  scanners, the emulator and cloud plugin formats and the companion protocol.

Streaming itself is stock Sunshine on the desktop and stock Moonlight on the
Ally, started hidden through its command line. Tailscale carries traffic between
locations. No ports are opened on any router.

## Two kinds of user

- The **player** uses the Ally. They see games and nothing that needs deciding.
- The **administrator** sets things up once and maintains them, also remotely.

This split drives several decisions:

- Settings that can break things sit behind an admin lock on the Ally.
- The shell exposes the same authenticated API as the companion over Tailscale,
  so games, artwork, emulators and hosts can be added from a desktop without
  touching the handheld.
- Configuration is plain data (SQLite plus data files), so it can be prepared in
  advance and pushed.
- Errors shown to the player say what to do in one line. Detail goes to a log the
  administrator can read remotely.

## Staying lean

- The shell drops its views and stops polling while a game or stream is in the
  foreground, then restores them on return.
- No background service on the Ally beyond the shell itself, Tailscale and what
  ASUS and Windows need.
- Windows tuning is recorded change by change with the previous value, and can
  be undone from Settings. See [CLEANUP.md](CLEANUP.md).

## Catalogue

`games` holds one row per title. `installs` holds one row per place that title
exists (this Ally, or a desktop). One card can therefore offer Play, Stream from
HOME PC and Stream from SECOND PC, and never offers a place where the game is
not installed.

## Milestones

| Milestone | Delivers |
| --- | --- |
| M0 | Repository, build pipeline, installable empty shell. |
| M1 | Design system, home screen, controller and touch navigation. |
| M2 | Local library: Steam, manual executables, launch and return. |
| M3 | Companion and one-tap streaming from HOME PC. MVP complete. |
| M4 | Second PC, Tailscale, host states, wake, remote administration. |
| M5 | Remaining stores, artwork, search. |
| M6 | Emulators and native ports. |
| M7 | Boot to CarlaOS, reversible Windows tuning, settings. |
| M8 | Cloud tiles, performance profiles, extras. |
| M9 | Signed installer, updates, recovery. |
