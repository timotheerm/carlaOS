---
name: start-task
description: Start a CarlaOS task safely by inspecting Git state, PR dependencies, project memory and the correct branch before coding.
---

# Start a CarlaOS task

1. Run `git status --short --branch`. Do not overwrite or discard existing work.
2. Run `git fetch origin --prune`.
3. Read `CLAUDE.md`, `knowledge/START-HERE.md`, `knowledge/operations/current-state.md` and the relevant feature docs.
4. Inspect open PRs with `gh pr list`. For relevant PRs, check base/head branches, diff, dependency notes and CI using `gh pr view` and `gh pr checks`.
5. Decide whether the task is independent, belongs on an existing feature branch, or depends on an unmerged PR. Never silently base dependent work on `main`.
6. State the task, acceptance criteria, branch, base commit, dependencies, files likely to change and validation plan.
7. For architecture work, ask the `carla-architect` agent for a read-only design review. For bounded risk areas, request a read-only reviewer or test engineer.
8. Do not start parallel writing sessions until file ownership and branch/worktree isolation are clear.
9. Wait for the user's approval of the plan for broad, risky or architectural changes. For small, clearly scoped fixes, proceed once the base and scope are established.

The output must separate verified facts from assumptions and must not claim a PR or branch is ready without checking the current remote state.
