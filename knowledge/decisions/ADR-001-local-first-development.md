---
title: ADR-001 Local-first development
type: decision
status: accepted
date: 2026-11-17
---

# ADR-001: Local-first development

## Context

CarlaOS has concurrent feature work, a Rust/Tauri/React codebase and an automated GitHub Actions release pipeline. Independent remote coding sessions can start from stale branches, hide dependencies and produce PRs that are difficult to integrate.

## Decision

Use a local Windows clone as the primary development environment. Use feature branches and GitHub PRs for review and CI. Use Git worktrees only for genuinely independent concurrent tasks. Use one integration owner per coherent feature. Keep dependent work on the parent feature branch or use a shallow stacked PR.

## Alternatives considered

- Editing directly on GitHub or in independent remote sessions: rejected as the default because dependency visibility and integration control are weaker.
- One branch and PR per agent: rejected because it fragments features and increases merge conflicts.
- Multiple writers in one checkout: rejected because concurrent edits can overwrite each other.

## Consequences

- The developer needs Git, Node.js, Rust MSVC and the Tauri Windows prerequisites.
- Local tests and hardware validation can happen before a PR.
- GitHub Actions remains the remote validation gate.
- Worktrees add isolation without multiplying clones or node_modules directories.

## References

- [[../operations/working-agreement]]
- [[../../docs/GIT_WORKFLOW]]
- [[../../docs/LOCAL_SETUP_WINDOWS]]
