---
title: Hardware validation plan
type: checklist
updated: 2026-11-17
---

# Hardware validation plan

Everything below is **written and tested only with stand-ins** per the docs; no entry is
hardware-verified until you tick it and record the date and device in [current-state](current-state.md).
Order: cheapest and most foundational first. Source of each item is the "Not yet tried" section of the linked doc.

Devices: **Ally** (RC73YA, CarlaOS-Ally installer), **HOME PC** (Desktop Host installer + Sunshine), optionally a **second PC** and a **non-home network** (phone hotspot).
Take a Windows restore point before test 7 (Windows tuning).

## 0. Install (unsigned beta)

- [ ] Install `CarlaOS-Ally-<ver>-setup.exe` on the Ally; accept "Windows protected your PC → Run anyway". ([RELEASES](../../docs/RELEASES.md))
- [ ] Install `CarlaOS-Desktop-Host-<ver>-setup.exe` on HOME PC; click through every page and the set-up window at the end.
- [ ] Both appear in *Installed apps*; both uninstall cleanly.

## 1. Library (M2) — [LIBRARY](../../docs/LIBRARY.md)

- [ ] A Steam game and a standalone exe launch, and quitting returns to CarlaOS.
- [ ] Game started outside CarlaOS is picked up; shell resumes after sleep ([QUICK-MENU](../../docs/QUICK-MENU.md)).
- [ ] Controller and touch navigation readable at 7 inches; no dropped frames.

## 2. Link and pairing (M3) — [STREAMING](../../docs/STREAMING.md)

First check the AMD driver on HOME PC: Sunshine reports crashes on RX 9070 XT with Adrenalin 26.6.x; 25.9.1 reported working.

- [ ] Ally links to HOME PC by approval, without typing codes.
- [ ] Moonlight pairs with Sunshine through the companion (both PIN flows).
- [ ] **Stream from HOME PC**: game appears, Moonlight window stays hidden, closing the game returns to CarlaOS.
- [ ] Sunshine runs the launcher and the stream ends when the game closes.
- [ ] Companion starts with Windows; credentials survive a reboot (Windows data protection).

## 3. Stream screen and picture — [STREAM-SCREEN](../../docs/STREAM-SCREEN.md), [STREAM-PICTURE](../../docs/STREAM-PICTURE.md)

- [ ] Desktop screen switches to the Ally's size and is restored afterwards (try `carla-companion stream test` at the desktop first).
- [ ] The dark "Starting <game>" curtain looks right, sits above the taskbar, game gets the keyboard.
- [ ] Virtual screen path (`carla-companion stream virtual test`): other screens switch off and the layout comes back.
- [ ] Moonlight at a non-1080p/720p size in borderless mode.
- [ ] Steam update screen shows progress, then starts the game.
- [ ] Quality choice on real Wi-Fi and via Tailscale: note picked level (smooth/sharp/balanced/light) vs. real experience; `--bitrate` honoured. Thresholds are untuned.

## 4. Remote and second desktop (M4) — [REMOTE-ADMIN](../../docs/REMOTE-ADMIN.md), [STREAMING](../../docs/STREAMING.md)

- [ ] Reach HOME PC through Tailscale from another network.
- [ ] Wake a sleeping HOME PC from the same network (remote wake needs an always-on device; baseline says so).
- [ ] From a desktop, administer the Ally between two Windows machines: status, add game, update; refusal while a game runs; "Windows asks for approval" prompt appears on the Ally.
- [ ] Second PC: states, switching hosts, unified Play/Stream card.

## 5. Saves — [SAVE-SYNC](../../docs/SAVE-SYNC.md)

- [ ] Real game: play on Ally, continue on desktop, return. Note the baseline said save *sync* was not planned; this was added later.

## 6. Updates — [RELEASES](../../docs/RELEASES.md)

- [ ] Ally updates itself from GitHub: download, close, reopen on the new version.
- [ ] Desktop updates on request from a linked Ally.

## 7. Boot and Windows tuning (M7) — [STARTUP](../../docs/STARTUP.md), [CLEANUP](../../docs/CLEANUP.md)

**Changes Windows configuration — do deliberately, with a restore point.**

- [ ] Cold boot goes straight into CarlaOS; sleep/resume works; Xbox button / Armoury Crate still work.
- [ ] Each clean-up item can be applied and undone from Settings; ASUS, drivers, security, controller and Xbox components are untouched.
- [ ] Power profiles per game ([POWER](../../docs/POWER.md)).

## 8. Remaining features — spot checks

- [ ] Epic/GOG detection, search, artwork ([STORES-AND-SEARCH](../../docs/STORES-AND-SEARCH.md), [ARTWORK](../../docs/ARTWORK.md), [IMMERSIVE](../../docs/IMMERSIVE.md)).
- [ ] One emulator launch ([EMULATORS](../../docs/EMULATORS.md)); no firmware/keys are ever fetched.
- [ ] One cloud tile ([CLOUD](../../docs/CLOUD.md)); desktop dashboard page ([DASHBOARD](../../docs/DASHBOARD.md)).

## New since v0.22

Added by merged PRs #37–#46 (beta builds, no hardware run yet). Nothing here is verified.

- [ ] Install Desktop Host >= 0.29 and retry a streamed game; `launcher.log` shows no `handle is invalid` ([KI-010](known-issues.md), PR #37).
- [ ] Crash and freeze reports appear in Settings, and copy works ([CRASH-REPORTS](../../docs/CRASH-REPORTS.md)).
- [ ] `carla-companion find` and dashboard "Add a game" on a real game library, plus one pre-launcher game added with `--watch` ([DESKTOP-GAMES](../../docs/DESKTOP-GAMES.md)).
- [ ] `saves guess` / `saves find` with a real game ([SAVE-SYNC](../../docs/SAVE-SYNC.md)).
- [ ] `describe --lookup` on a hand-added game.
- [ ] `saves suggestions` / `saves accept` on a second desktop.
- [ ] `saves clean KEY on --yes` end to end with a real game, then restore from `saves copies`.

## Recording results

Tell Claude the outcome per line (pass/fail, device, date, notes). It will update
[current-state](current-state.md) and [known-issues](known-issues.md) via `/update-memory`.
