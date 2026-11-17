---
title: Second-model review workflow
type: operations
status: maintained
updated: 2026-11-17
---

# Second-model review workflow

Use Astra/ChatGPT as an independent reviewer of a specific, immutable commit or PR diff. Do not treat two models editing the same branch as parallelism.

## Best division of labour

- Claude Code: repository navigation, implementation, local test execution, Git operations and PR preparation.
- Astra/ChatGPT: challenge the plan, inspect high-risk diffs, identify missed edge cases and assess user-facing trade-offs.
- GitHub Actions: repeatable remote validation.
- Human owner: accept product trade-offs and decide whether to merge.

This is a workflow convention, not an automatic Claude-to-Astra API connection. Unless a supported connector or integration is explicitly configured, transfer the review context manually.

## Send a pinned review package

From the repository root, collect:

```powershell
git status --short --branch
git rev-parse HEAD
git diff --stat origin/main...HEAD
git diff --check
git diff --no-ext-diff --unified=5 origin/main...HEAD
```

For a PR already on GitHub:

```powershell
gh pr view <number>
gh pr diff <number>
gh pr checks <number>
```

Include:
- the repository and exact commit SHA;
- the PR number and its base branch;
- the user story and acceptance criteria;
- the relevant feature/architecture documentation;
- the diff and test/CI results;
- what was not tested, especially hardware or real external-service behaviour.

Do not send secrets, tokens, personal data, local configuration, or private user files.

## Review prompt

> Act as an independent senior reviewer for CarlaOS. Review the supplied commit and diff against its stated base, not against assumptions about what the code should do. Do not rewrite or implement anything. Identify only actionable findings, ordered by severity. For every finding include file and line, concrete failure scenario, impact, and a minimal suggested correction. Check Rust safety and concurrency, SQLite migrations and data integrity, Tauri command authorization, Windows path and registry behaviour, network timeouts and URL validation, privacy and external data sharing, UI accessibility/controller navigation, performance on a handheld, compatibility with existing PRs, and test coverage. Separate confirmed defects from questions and optional improvements. Also state what cannot be established from a diff alone. If there are no blocking findings, say so plainly.

## Reconcile findings

1. Ask Claude to verify each finding against the current source and tests.
2. Reject false positives with a short evidence-based reason.
3. Convert confirmed defects into tracked work or changes on the owning feature branch.
4. Never let the second model directly push to or rewrite Claude's branch.
5. Re-run checks after applying any accepted correction.
6. Record only high-value findings or decisions in the knowledge base.

The commit SHA matters: if the PR changes while a second model reviews it, the review is stale and must be repeated against the new head.
