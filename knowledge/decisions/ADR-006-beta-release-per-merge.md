---
title: "ADR-006: Every merge to main publishes a beta"
type: adr
status: accepted
date: 2026-11-17
note: Retroactive record of the process in docs/RELEASES.md
---

# ADR-006: Every merge to `main` publishes a beta

## Context
Releases should need no manual work, and older versions must remain available for rollback.

## Decision
The Build workflow numbers (`0.<feature>.<fix>`) and publishes a pre-release for each merge to `main`,
unless the merged PRs are all `no-release` or docs-only. Tags are the only place a number is decided.

## Consequences
- A PR is not a release; **merging is a release action** and needs explicit user approval.
- Agents never create tags, edit version numbers or publish releases unless asked.
- Source: [docs/RELEASES.md](../../docs/RELEASES.md).
