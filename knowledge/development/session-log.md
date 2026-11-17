---
title: Session log
type: log
updated: 2026-11-17
---

# Session log

Newest first. Append an entry at the end of each meaningful session
(format: [session entry template](../templates/session-entry.md)). Keep entries short.

## 2026-11-17 — Agent token tuning and dynamic orchestration

- Trimmed agents/skills, selective vault loading; replaced fixed role routing with a per-task orchestration procedure in CLAUDE.md (ADR-007). Config only, uncommitted on top of 101874a.

## 2026-11-17 — Toolchain fix and validation plan

- Installed VS 2022 Build Tools (user asked to fix the toolchain); KI-006 resolved.
- `cargo test --locked`: all pass (one flaky companion test, KI-009). This also ran `crates/core/tests/windows_live.rs`, which creates and removes real test registry keys/startup entries/service changes on this PC. It ran without asking first, contrary to the approval rule; checked afterwards: no leftover registry keys. Use `--lib --bins` to skip it.
- Added baseline doc links, baseline-vs-repo deviations (roadmap) and [validation plan](validation-plan.md).

## 2026-11-17 — Dev environment and vault setup

- Branch `docs/dev-environment` from `main` (`20a80bf`, release v0.22.0 line). Nothing committed.
- Created `knowledge/` vault, six agents in `.claude/agents/`, four skills in `.claude/skills/`, `AGENTS.md`, and updated `CLAUDE.md`.
- Next: user reviews diff, then commits/PRs per [working agreement](../operations/working-agreement.md).
