---
name: start-task
description: Begin non-trivial CarlaOS branch work - check git/PR state, pick branch and base, state a plan. Skip for small edits on an existing branch.
argument-hint: <short task description>
disable-model-invocation: true
---

# Start: $ARGUMENTS

1. `git status --short --branch` and `git fetch origin --prune`. Preserve existing changes; mention unrelated dirty files.
2. `gh pr list --state open --json number,title,headRefName,baseRefName`. If any PR is open and could overlap or be a dependency, ask `git-workflow-manager`; otherwise decide yourself.
3. Branch (never `main`): independent → `git switch -c <feat|fix|docs>/<name> origin/main`; depends on an unmerged PR → that PR's branch or a deliberate stacked branch; already on a suitable non-main branch → keep it.
4. Read only what the task needs (see CLAUDE.md "Project memory" table): the area's row in `current-state.md` and its `docs/*.md`, plus `known-issues.md` or an ADR only if relevant.
5. State the plan in ≤ 6 lines: branch, base (`git rev-parse --short HEAD`), affected files/areas, agents to use (if any, with why), checks to run, and any step needing user approval.
