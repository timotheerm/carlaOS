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
| KI-009 | Testing | Flaky test `server::tests::a_link_that_is_refused_at_the_desktop_links_nothing` failed once, passed on rerun (56/56) | 2026-11-17 run | Low |
| KI-007 | Knowledge | Baseline-vs-repo deviations unconfirmed (home-app boot, save sync, restore point, service/tray, waker) | [roadmap](roadmap.md#deviations-between-baseline-and-repository-to-confirm-with-the-user) | Medium |
| KI-008 | Verification | Whole hardware checklist outstanding | [validation plan](validation-plan.md) | High |

## Resolved

- **KI-006** (2026-11-17): Rust could not link: no MSVC on the machine. Fixed by installing VS 2022 Build Tools (C++ workload) via winget. Run cargo from a VS developer shell. In PowerShell: `$vs = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"; Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"; Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments "-arch=x64"`. Not usable from Git Bash (GNU `link` shadows it).
