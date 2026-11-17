---
title: CarlaOS current state
type: status
status: snapshot
updated: 2026-11-17
---

# Current state

> This is a dated snapshot, not live GitHub state. Re-check branches, PRs and CI before acting.

## Main branch

The Git workflow setup PR #30 was merged on 17 November 2026. It added the repository-level Claude instructions, the local-first Git workflow and a Git workflow specialist agent.

## Open work observed during this snapshot

- **PR #29 — Immersive Ally look, install states, auto artwork and game discovery.** Open at the time of this snapshot. The Linux Rust/TypeScript checks passed. The Windows installer build succeeded, but its installer smoke-test failed because the test tried to write to a per-user Windows Run registry key that did not exist on a clean CI runner. The test failure was in the smoke-test fixture setup, not in compilation. The PR description states that no test was performed on a physical Ally or against live artwork providers.
- **PR #31 — Windows installer smoke-test fixture fix.** Opened as an independent fix to create the per-user Run registry key when absent. Its CI must pass before merge.

Always retrieve the latest PR metadata and checks rather than relying on this historical list.

## Validation limits

A successful Linux build and Windows installer smoke test do not prove:
- ASUS-specific power, refresh-rate or controller integrations on an actual Ally;
- sleep/resume and game foreground lifecycle on the handheld;
- the full experience with real Steam libraries, emulators and remote desktops;
- correctness and availability of artwork returned by live external providers.

## Immediate next review

Before merging the immersive feature work, re-review the current diff after any incoming Claude changes, confirm PR dependencies, rerun or inspect CI, and specifically check privacy/opt-out behaviour for automatic external artwork lookups. Review discovery scan bounds and ensure nothing is executed or silently added. Validate installer changes via Windows CI.

Update this note after a milestone, merge, release or materially changed project state. Do not append a daily diary.
