---
name: review-pr
description: Review a CarlaOS PR or the current branch diff against its base. Read-only; reports findings only.
argument-hint: "[PR number | branch | blank = current branch]"
disable-model-invocation: true
---

# Review: $ARGUMENTS

Read-only: never push, comment, approve or merge.

1. Target: PR number → `gh pr view <n> --json number,title,headRefName,baseRefName,statusCheckRollup`; otherwise base is `origin/main` (or the documented parent). `git fetch origin --prune`, then `git diff <base>...HEAD --stat`.
2. Size decides who reviews: a few files/short diff → review it yourself; larger or multi-area → one `carla-reviewer` call with the base, branch and diff stat. Add `carla-architect` (stronger model) only if the diff touches auth/secrets/protocol/update-signing/Windows configuration.
3. Checks: `gh pr checks <n>` for a PR; `git diff --check`. Run local tests only if CI results are missing or the change is risky (`test-engineer`, `--lib --bins`).
4. Policy: no game/ROM/firmware downloads or DRM bypass; reversible Windows tuning; no hardware-validation claim without a real run; no version/tag edits; PR body has summary, tests, limitations, dependencies.
5. Report: verdict · blocking issues (`file:line`) · up to 3 non-blocking · items needing user approval · not verified. Note that merging publishes a beta.
