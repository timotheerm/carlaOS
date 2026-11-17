---
name: finish-task
description: Finish a CarlaOS task by validating the diff, preparing the PR and updating durable project memory only with verified facts.
---

# Finish a CarlaOS task

1. Run `git status --short`, `git diff --check` and inspect the complete diff against the correct base.
2. Run the relevant Rust, Windows, frontend and packaging checks. Record exact commands and results. Do not claim tests not run.
3. Check for accidental generated files, secrets, local config, large assets and unrelated changes.
4. Confirm the PR base is correct and dependencies are described. One PR should represent one coherent feature/fix.
5. Write a PR summary with behaviour changed, tests run, tests not run, known limitations and dependency PRs.
6. Ask `carla-reviewer` for an independent read-only pass when risk justifies it.
7. Update `knowledge/operations/current-state.md` only when the overall project state materially changes. Update an architecture map or feature note when verified behaviour changes. Add an ADR only for a meaningful decision.
8. Ask `memory-curator` to check for stale or duplicate notes when a feature materially changes architecture or product behaviour.
9. Verify GitHub CI against the pushed head SHA and present the merge recommendation.
10. Do not merge, publish a release, delete a branch or force-push unless the user explicitly approves that action.

A clean finish leaves an auditable branch/PR, evidence-based validation notes and no unnecessary memory churn.
