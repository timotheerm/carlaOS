---
title: Known issues and technical debt
type: issues
updated: 2026-11-17
---

# Known issues and technical debt

Add items with the [issue template](../templates/issue.md). Close by moving to "Resolved" with the PR/commit.

## Open

| ID | Area | Issue | Evidence | Severity |
| --- | --- | --- | --- | --- |
| KI-001 | Verification | Streaming (pairing, Sunshine launcher, Moonlight window behaviour, wake, Tailscale) only run against stand-ins | [STREAMING.md](../../docs/STREAMING.md#not-yet-tried-on-real-machines) | High |
| KI-002 | Verification | Remote admin and save sync tested only on one machine, not between two Windows machines / real games | [REMOTE-ADMIN.md](../../docs/REMOTE-ADMIN.md), [SAVE-SYNC.md](../../docs/SAVE-SYNC.md) | Medium |
| KI-003 | Verification | Stream screen/picture Windows parts type-checked but never run | [STREAM-SCREEN.md](../../docs/STREAM-SCREEN.md), [STREAM-PICTURE.md](../../docs/STREAM-PICTURE.md) | Medium |
| KI-004 | Release | Beta installers are unsigned (Windows SmartScreen warning); M9 signing not done | [RELEASES.md](../../docs/RELEASES.md) | Medium |
| KI-005 | Testing | No frontend test script in `apps/shell/package.json` (only `tsc` + `vite build`) | `apps/shell/package.json` | Medium |
| KI-007 | Knowledge | Baseline-vs-repo deviations unconfirmed (home-app boot, save sync, restore point, service/tray, waker) | [roadmap](roadmap.md#deviations-between-baseline-and-repository-to-confirm-with-the-user) | Medium |
| KI-008 | Verification | Whole hardware checklist outstanding | [validation plan](validation-plan.md) | High |
| KI-010 | Verification | Launcher fix (#37) not yet verified on a real stream; host still ran the pre-fix build at last check; Rocket League could not start in earlier attempts | [STREAMING.md](../../docs/STREAMING.md), PR #37 | High |
| KI-011 | Stability | Ally became unresponsive after closing Moonlight's error box; cause unknown. Freeze/crash reports (#38/#39) now capture it | PRs #38, #39; [CRASH-REPORTS.md](../../docs/CRASH-REPORTS.md) | Medium |
| KI-012 | Verification | Dashboard "Add a game" page never exercised in a browser | PR #42; [DESKTOP-GAMES.md](../../docs/DESKTOP-GAMES.md) | Low |
| KI-013 | Verification | Save features (finder, place, keep-clean) never tried with a real game or between two real desktops; `savesync.rs` glue (Tauri crate) has no automated test | PRs #41, #44, #46; [SAVE-SYNC.md](../../docs/SAVE-SYNC.md) | Medium |
| KI-014 | Design limit | Keep-desktop-clean accepted limits: one Ally, no direct play on that desktop, a second Ally can start from an empty folder | PR #46; [ADR-009](../decisions/ADR-009-save-places-and-opt-in-cleanup.md) | Low |
| KI-015 | Verification | OneDrive-redirected Documents handled only via `SHGetKnownFolderPath`; untested on a real redirected profile | PR #41; [SAVE-SYNC.md](../../docs/SAVE-SYNC.md) | Low |
| KI-017 | Security | The Ally shell turns off WebView2 SmartScreen navigation checks (`--disable-features=msSmartScreenProtection` in `apps/shell/src-tauri/tauri.conf.json`). The page is local with a restrictive CSP, so no exploitable path is known, but the protection is off without a documented reason. Decide: remove it, or document why; test on a real Ally | independent review 2026-11-17 | Low |
| KI-018 | Style | `cargo fmt --all -- --check` reports differences across many files; the code uses a deliberately compact style. A whole-repo reformat would bury real diffs, so it is not done. Decide: normalise in one dedicated PR and add a CI check, or document that fmt is not part of the contract | independent review 2026-11-17 | Low |
| KI-019 | Verification | Streaming has no real-device approval evidence; run the hardware plan in layers (pair and launch one game on the home network; stream start/stop, desktop-size restoration, virtual display, 16:10, a long session; quality on Wi-Fi and Tailscale; two Windows machines for save sync, interruption recovery, keep-clean and backup restore; install, upgrade, uninstall on the Ally) and record date, machines, GPU/driver, Sunshine/Moonlight versions, network path and logs | [validation plan](validation-plan.md) | High |

## Resolved

- **KI-009** (2026-11-17): flaky companion test `server::tests::a_link_that_is_refused_at_the_desktop_links_nothing` failed once. Root cause: two tests used `desk("refuse")`, and the helper's state folder was shared and wiped in parallel. Fixed in PR #45 (test-only, no-release); 10/10 runs passed.
- **KI-006** (2026-11-17): Rust could not link: no MSVC on the machine. Fixed by installing VS 2022 Build Tools (C++ workload) via winget. Run cargo from a VS developer shell. In PowerShell: `$vs = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"; Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"; Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments "-arch=x64"`. Not usable from Git Bash (GNU `link` shadows it).
- **KI-016** (2026-11-17, PR #52): the release planner could skip a release when GitHub had not linked a just-merged PR yet. `changes.sh` now retries and counts an unresolved commit as a normal change; `scripts/release/test-changes.sh` covers it.
- **After-stream save retry** (2026-11-17, found by an independent review, PR #53): an unreachable desktop ended the retry loop after the first 3 s attempt; the 3/15/30/60 s schedule is kept for transient failures (`sync::retry_after`, tested).
