---
title: "ADR-008: Standing authorisation for autonomous integration"
type: adr
status: accepted
date: 2026-11-17
---

# ADR-008: Standing authorisation for autonomous integration

## Context
Asking the user to approve every pull, branch, push and merge cost tokens and stalled work between requests. `main` has no branch protection, so nothing but the coordinator's own discipline gates a merge, and every merge publishes a beta ([ADR-006](ADR-006-beta-release-per-merge.md)).

## Decision
The user authorised the coordinator to fetch, pull (fast-forward), branch, commit, push (never force), open PRs and merge PRs without asking, provided: all CI checks finished green, the diff was inspected (plus one `carla-reviewer` pass for non-trivial changes), it is the user's own work, dependencies are merged first, and it is not in the approval list (security-sensitive design, credentials, Windows/device configuration, destructive operations). CI is awaited with one background command, not polling. Rules: `CLAUDE.md` "Autonomous integration". Permission rules in `.claude/settings.json` allow the routine git/gh commands and deny force-push, hard reset, clean, `branch -D`, tags, PR close, releases and repo deletion.

## Consequences
- Faster, cheaper integration; a green CI is the only merge gate, so CI coverage matters (frontend has no tests, KI-005; Windows hardware behaviour is never covered).
- Each merge ships a beta: PR titles/labels must choose the release type before merging.
- Tags, manual releases, version edits, force-push and history rewriting still need the user.
- Permission rules cannot catch every force-push spelling (e.g. a `+branch` refspec); the CLAUDE.md rule still forbids it.
