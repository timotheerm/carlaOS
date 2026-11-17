---
title: "ADR-004: knowledge/ is the single source of persistent project knowledge"
type: adr
status: accepted
date: 2026-11-17
---

# ADR-004: `knowledge/` is the source of persistent project knowledge

## Context
Project state, decisions and history were spread across chat sessions, PRs and `docs/`.
Claude sessions start cold and need a cheap, reliable way to recover context.

## Decision
`knowledge/` (an Obsidian-compatible vault, plain Markdown) holds status, roadmap, decisions,
known issues and the session log. `docs/` keeps product/user-facing documentation and is linked,
not duplicated. Claude's private auto-memory is not a substitute for the vault.

## Consequences
- Sessions begin by reading `knowledge/START-HERE.md`.
- Important decisions and status changes must be written to the vault (via `/update-memory` or the `memory-curator` agent).
- Notes are in git, so they are reviewed in PRs like code.
