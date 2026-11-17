---
title: "ADR-009 Save places and opt-in cleanup"
type: decision
status: accepted
date: 2026-11-17
---

# ADR-009: Save places and opt-in cleanup

## Context

Save sync between the Ally and desktop hosts needs two rules: where a game's save folder is named, and when a desktop may delete its own saves after a stream. A portable "save place" shared between desktops could otherwise be mistaken for a real save folder, and cleanup on the wrong machine could remove the only copy. Shipped in PRs #41 (save-folder finder), #44 (save places on a second desktop) and #46 (opt-in keep-desktop-clean).

## Decision

1. A save folder is only ever named at the machine it is on. A portable save place can be shared between desktops, but it is only a suggestion. It becomes a real save folder only after it is confirmed at that desktop (`saves accept`).
2. Deleting a desktop's saves is opt-in per game, and can only be enabled at that desktop (`saves clean KEY on --yes`; it cannot be set over the protocol). The desktop removes its live files only when all of the following hold:
   - the game is not running there;
   - the Ally's manifest of its own copy matches the desktop's live files exactly (paths, sizes, SHA-256);
   - a full backup was written, read back and compared, and each file is re-hashed just before it is removed.
   The way back is protected too:
   - **Stream gate:** a flagged game does not start a stream unless its saves are verifiably on the desktop. If the desktop cannot be asked, the last-known flag, kept in the Ally's database so it survives a restart, holds the stream back.
   - **Subset top-up rule:** for a flagged game, a non-empty desktop that holds only files the Ally holds identically (an interrupted send) is topped up from the Ally and never fetched over the Ally's good save.
3. Protocol changes for this are additive: new optional fields with serde defaults. The owner approved these protocol changes.

## Alternatives considered

- Setting the desktop's folder automatically from the Ally's place: rejected, it would let a remote device name a folder on another machine.
- Looking saves up in an online database (PCGamingWiki, Ludusavi manifest): not chosen for now; the offline finder (saves guess|find) needs no network and sends nothing.
- Always deleting the desktop copy, or deleting without an exact match and a read-back backup: rejected as a data-loss risk.
- Making cleanup the default: rejected; it is opt-in per game, set only at the desktop.

## Consequences

- Easier: a desktop never names a save folder for another machine, and a shared place cannot clean saves by itself.
- Harder: a second desktop must confirm each suggested place before it is used.
- Constrained: one Ally only; no direct play on the desktop that cleans its saves.
- Backups are the only other copy if the Ally loses its save.
- A second Ally can start from an empty folder (see [KI-014](../development/known-issues.md)).
- Real-game and two-desktop behaviour is not hardware-verified (see [KI-013](../development/known-issues.md)).

## References

- PR #41 (save-folder finder), PR #44 (save places), PR #46 (keep-desktop-clean, v0.29.0 build pending when written).
- [SAVE-SYNC](../../docs/SAVE-SYNC.md).
- [KI-013](../development/known-issues.md), [KI-014](../development/known-issues.md).
