---
title: Roadmap (M0–M9)
type: roadmap
updated: 2026-11-17
---

# Roadmap

The M0–M9 milestones are preserved from [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md).
They are the original product baseline; do not renumber or drop them.
Implementation status is tracked in [current-state](current-state.md), not here.

## Baseline document

The original product baseline is
[RoséOS Phase 1 Feasibility and Architecture](<../RoséOS Phase 1 Feasibility and Architecture.md>)
(also as [PDF](<../RoséOS Phase 1 Feasibility and Architecture.pdf>)), dated 11 Nov 2026. It still says
RoséOS; the project is now CarlaOS. Its roadmap (section 5) is the M0–M9 list below. Treat it as
approved design, not as proof of implementation.

### Deviations between baseline and repository (to confirm with the user)

| Topic | Baseline | Repository |
| --- | --- | --- |
| Boot / home app | M7: registered as the Windows full-screen-experience home app | `docs/ARCHITECTURE.md`: that is "a later step needing different packaging"; `docs/STARTUP.md` starts CarlaOS at sign-in (the baseline's named fallback) |
| Save sync | "Syncing saves between machines is not planned" | `docs/SAVE-SYNC.md` and `savesync.rs` exist |
| Companion form | Rust Windows service plus tray app | No "service" or "tray" in `crates/companion/src` (grep); it is started with Windows via Run key per STREAMING.md |
| Input service, Waker (always-on wake device) | Listed components | Not verified in source; `core/nap.rs` / wake exist, same-network only per STREAMING.md |
| M7 restore point before tuning | Required | No "restore point" anywhere in source or docs (grep): likely missing, per-change undo exists |
| M9 crash recovery, security review, signed installer | Required | Signing absent; others unverified |

## Milestones

| Milestone | Delivers (approved scope) | Status — see [current-state](current-state.md) |
| --- | --- | --- |
| M0 | Repository, build pipeline, installable empty shell | Merged per ARCHITECTURE.md |
| M1 | Design system, home screen, controller and touch navigation | Merged per ARCHITECTURE.md |
| M2 | Local library: Steam, manual executables, launch and return | Merged per ARCHITECTURE.md |
| M3 | Companion and one-tap streaming from HOME PC (MVP complete) | Code present; not validated on hardware |
| M4 | Second PC, Tailscale, host states, wake, remote administration | Code present; not validated on hardware |
| M5 | Remaining stores, artwork, search | Code + docs present; see current-state |
| M6 | Emulators and native ports | Code + docs present; see current-state |
| M7 | Boot to CarlaOS, reversible Windows tuning, settings | Code + docs present; not validated on hardware |
| M8 | Cloud tiles, performance profiles, extras | Code + docs present; see current-state |
| M9 | Signed installer, updates, recovery | Partial: updates present; builds are **unsigned** |

"Present" means source files and docs exist. It does not mean complete or hardware-verified.

## Proposed next work (candidates, not approved)

Ordered by value; confirm with the user before large items.

1. **Hardware validation pass on a real Ally + desktop** — highest value; most docs end with "Not yet tried on real machines" (see [known-issues](known-issues.md)).
2. **Code signing** (M9 gap) — security-sensitive; needs a decision and credential handling (explicit approval required).
3. **Recovery path** (M9) — verify what exists vs. the approved scope.
4. **Test coverage audit** — shell frontend has no test script in `package.json`; native shell modules have few tests.
5. **Full-screen-experience home app** — design intent only; needs its own ADR.
