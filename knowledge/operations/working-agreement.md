---
title: Working agreement
type: operations
status: active
updated: 2026-11-17
---

# Working agreement

## Roles

**Human owner:** sets product priorities, approves architecture trade-offs and owns final product decisions.

**Primary Claude Code session (coordinator):** owns the task plan, selected branch, integration, conflict resolution, final validation, PR creation and merge recommendation. It is the only writing coordinator in a normal task.

**Claude subagents:** perform bounded research, architecture analysis, test analysis and independent review. Keep them read-only unless the task explicitly asks a specialist to edit a clearly isolated file set.

**Second model (Astra/ChatGPT):** provides an independent plan critique or code review from a pinned commit/diff. Treat it as a reviewer, not as another writer sharing the same worktree. Use [[second-model-review]].

**GitHub and CI:** source of truth for branch state, PR discussion and remote checks.

**Obsidian:** optional UI for the Markdown knowledge base. It does not own the memory or project state.

## Default execution pattern

1. Fetch remote refs and inspect the working tree.
2. Read `CLAUDE.md`, `knowledge/START-HERE.md` and the relevant feature documentation.
3. Inspect open PRs, their bases, head commits and dependencies.
4. Write a small implementation plan and choose the correct branch.
5. Use focused subagents for independent read-only work.
6. Have the coordinator integrate changes and avoid overlapping edits.
7. Run the appropriate tests, inspect the final diff and push one coherent PR.
8. Review the PR and all required GitHub Actions checks before merge.
9. After verified work, update only the durable notes that changed.

## Parallelism and context cost

- Start with one primary coding session and a few read-only specialist subagents.
- Use at most two independent writing worktrees until the integration process is consistently reliable.
- Do not spawn an agent merely to read a single small file or perform a trivial edit.
- Delegate bounded tasks with exact files, questions, expected output and a no-edit instruction when appropriate.
- Prefer concise summaries and file/line references over pasting whole files into the coordinator's context.
- Use the lower-cost suitable model for routine review, research and test triage; reserve the strongest model for difficult architecture, complex debugging or final high-risk review.
- Avoid Agent Teams and large multi-agent swarms initially. They add coordination and context overhead; subagents plus worktrees are sufficient for this project.

## Branch and merge rules

Follow [[../../docs/GIT_WORKFLOW]] and `CLAUDE.md`.

- Never commit directly to `main`.
- One PR per coherent feature, not one PR per agent.
- A dependent task must share its parent branch or use a stacked PR targeting the parent branch.
- The coordinator may merge its own PRs under the standing authorisation in `CLAUDE.md` "Autonomous integration" ([[../decisions/ADR-008-autonomous-integration]], which supersedes ADR-003): CI fully green, diff inspected, own work, dependencies merged, not an approval-gated change. A green CI proves only what the workflow tests.
- No agent may close, force-push, or delete another active branch/PR, and nothing may be tagged or released by hand, without the user.
- Never claim physical Ally validation unless it was performed on the device.

## Running tests locally

From a VS developer shell (command in [[../development/known-issues]], KI-006): `cargo test --locked --lib --bins`. Plain `cargo test` also runs `windows_live.rs`, which changes real registry/startup/service state and needs approval. Agent models, tools and delegation rules live in `.claude/agents/*.md` and `CLAUDE.md` "Orchestration" ([[../decisions/ADR-007-dynamic-orchestration]]); do not duplicate them here.

## Memory rules

- Record a decision when it changes the architecture or future implementation choices, not for every minor coding detail.
- Record test outcomes with commit/PR, exact commands and the environment.
- Keep dated current-state notes factual and short.
- Mark proposals and unmerged work as pending.
- Remove or revise stale guidance when its underlying behaviour changes.
- No credentials, API keys, tokens, personal data or copied private logs in the knowledge base.
