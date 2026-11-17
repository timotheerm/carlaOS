---
title: ADR-003 Human-approved integration
type: decision
status: accepted
date: 2026-11-17
---

# ADR-003: Human-approved integration and automated validation

## Context

Merging to `main` is connected to the repository's automated beta build/release process. A coding agent can produce a working branch while still missing product regressions, hardware limitations or feature dependencies.

## Decision

Agents may prepare branches and PRs, run checks and recommend a merge. The human owner retains approval of merge and product release. The integration owner must inspect the exact diff, dependency chain and required CI results before recommending a merge. Use the repository's documented merge strategy and release process; do not manually create tags or releases during ordinary development.

## Consequences

- "Mergeable" is not the same as "reviewed".
- A green CI run proves only what the workflow actually tests.
- Hardware-dependent behaviour must be labelled unverified until tested on an actual Ally.
- Existing PRs must not be merged, closed, retargeted or force-pushed without explicit permission.

## References

- [[../operations/working-agreement]]
- [[../../docs/GIT_WORKFLOW]]
- [[../../docs/RELEASES]]
