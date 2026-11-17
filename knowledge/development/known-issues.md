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
| KI-016 | Release | The release planner can wrongly skip a release: a push build looks up the pull request of each commit (scripts/release/changes.sh, gh api commits/<sha>/pulls) and GitHub may not have linked a just-merged PR yet, so only an earlier `no-release` PR is counted and the build says "everything merged since the last release is no-release". Seen on #48 (2026-11-17); the next merge released it. Idea: retry the lookup, or treat an unanswerable commit as a normal change | [plan.sh](../../scripts/release/plan.sh), run 44112061941 | Medium |

## Resolved

- **KI-009** (2026-11-17): flaky companion test `server::tests::a_link_that_is_refused_at_the_desktop_links_nothing` failed once. Root cause: two tests used `desk("refuse")`, and the helper's state folder was shared and wiped in parallel. Fixed in PR #45 (test-only, no-release); 10/10 runs passed.
- **KI-006** (2026-11-17): Rust could not link: no MSVC on the machine. Fixed by installing VS 2022 Build Tools (C++ workload) via winget. Run cargo from a VS developer shell. In PowerShell: `$vs = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"; Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"; Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments "-arch=x64"`. Not usable from Git Bash (GNU `link` shadows it).
