---
title: CarlaOS Knowledge Base
type: index
status: active
updated: 2026-11-17
---

# CarlaOS knowledge base

This is the project's durable, model-independent memory. It is plain Markdown, stored in Git, readable by Claude Code, ChatGPT/Astra, local models, VS Code and Obsidian.

## Start with these notes

- [[PROJECT]] — concise product and repository context.
- [[architecture/system-map]] — where the main application pieces live.
- [[operations/working-agreement]] — how people and coding agents work together.
- [[operations/current-state]] — a dated snapshot; verify it before relying on it.
- [[operations/second-model-review]] — how to use a second model for independent review.
- [[development/current-state]] — what is implemented, tested and hardware-verified, per area (evidence levels). Complements the dated PR snapshot above.
- [[development/roadmap]] — M0–M9 and what is next; the original baseline is [[RoséOS Phase 1 Feasibility and Architecture]] (reference only, 36 KB: read a section only when a baseline question comes up).
- [[development/known-issues]] and [[development/validation-plan]] — open problems and what to test on a real Ally.
- [[decisions/README]] — ADR index.
- [[../docs/LOCAL_SETUP_WINDOWS]] — prepare the Windows development PC.
- [[../docs/GIT_WORKFLOW]] — branches, PR dependencies and worktrees.
- [[../docs/ARCHITECTURE]] — approved architecture reference.
- [[../docs/RELEASES]] — versioning and release process.

## How this knowledge base is organised

- `architecture/` — stable system map and important boundaries.
- `decisions/` — Architecture Decision Records (ADRs), including why important choices were made.
- `operations/` — current status, development workflow and model collaboration.
- `development/` — feature-status table, roadmap, known issues, hardware validation plan and a short session log.
- `templates/` — reusable templates for decisions, feature notes and research.

The existing `docs/` directory remains the detailed product and implementation documentation. The `knowledge/` directory is the navigation, decision and continuity layer; do not copy every existing document into it.

## Reading budget

Load only the note a task needs; the vault is not meant to be read whole.

## Source-of-truth rules

1. The code, tests, CI results and merged Git history are authoritative for what the software currently does.
2. `docs/` is authoritative for detailed product/technical specifications where it agrees with the current code.
3. `knowledge/` records stable context, rationale, verified outcomes and pointers to the authoritative sources.
4. An open PR is proposed work, not current behaviour. Label it as pending and include its PR number.
5. External research must include a source link and the date checked.
6. Record uncertainty explicitly. Never turn an assumption, model suggestion or untested idea into a fact.

## Working with Obsidian

Open the **repository root** (or just the `knowledge/` folder) as an Obsidian vault, then open `knowledge/START-HERE.md`. Links into `../docs/` only resolve when the repository root is the vault. Notes are ordinary Markdown with relative links and light YAML frontmatter. Obsidian is only the interface; do not rely on Obsidian plugins, a proprietary database or local-only metadata for essential project knowledge.

Do not commit credentials, personal tokens, private logs, local machine paths or user data. Do not store large copied source files here. Prefer a short explanation and a link to the code or existing documentation.
