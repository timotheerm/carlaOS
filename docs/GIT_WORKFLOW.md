# Git workflow and local development

## Recommended operating model

Use a local clone as the primary development environment. GitHub is the review, CI and release destination—not the place where separate Claude sessions independently improvise branches.

The safe loop is:

1. Clone once and keep a clean, up-to-date `main`.
2. Start a task on a feature branch based on `origin/main`.
3. Implement and test locally.
4. Push the branch and open one PR for the coherent feature.
5. Let GitHub Actions validate the PR.
6. Review and merge when satisfied.
7. The existing Build workflow publishes a beta from `main` after merge.

Do not treat a PR as a release candidate that must be manually rebuilt or tagged. Per `docs/RELEASES.md`, merging to `main` triggers the automated beta release.

## First-time local setup (Windows PowerShell)

```powershell
git clone https://github.com/timotheerm/carlaOS.git
cd carlaOS
git switch main
git pull --ff-only origin main
```

Follow the build instructions in the repository README. Keep secrets and local machine configuration out of Git.

## Starting a normal task

```powershell
git fetch origin --prune
git switch main
git pull --ff-only origin main
git switch -c feat/short-task-name
```

Make changes, run the relevant tests/build checks, then:

```powershell
git status --short
git diff --check
git diff
git push -u origin HEAD
gh pr create --base main
```

Use `fix/` for fixes and `docs/` for documentation-only work. Use a descriptive branch name.

## Parallel Claude sessions: use worktrees, not one shared folder

For independent work, each Claude session gets its own worktree and branch. Example:

```powershell
git fetch origin --prune
git worktree add ..\carlaOS-dashboard -b feat/dashboard origin/main
git worktree add ..\carlaOS-settings -b feat/settings origin/main
```

Open each directory in a separate Claude Code session. Each session must only modify its own worktree/branch. When finished, test and open one PR per genuinely independent feature.

To remove a worktree after its branch is safely pushed/merged:

```powershell
git worktree list
git worktree remove ..\carlaOS-dashboard
```

Do not remove a worktree containing uncommitted work.

## Dependencies and stacked PRs

Prefer a single feature branch and PR when subtasks are part of one feature. Do not split work merely to make more agents or PRs.

If a child change genuinely requires an unmerged parent PR:

- Parent branch starts from `origin/main`; parent PR targets `main`.
- Child branch starts from the parent branch; child PR targets the parent branch.
- Clearly state `Depends on #<parent>` in the child PR.
- Merge the parent first.
- Then rebase the child onto updated `origin/main`, change its PR base to `main`, and rerun all checks.

Never create two PRs against `main` when one silently relies on the other's unmerged code. Avoid deep stacks.

## Integration rules

- One integration owner (the main Claude session) decides what enters a feature branch.
- Do not have concurrent sessions edit the same files or shared interfaces.
- Before integrating an agent's work, inspect its diff and run tests.
- Never reset, discard, overwrite or force-push another session's work.
- Never merge, close, retarget or delete an existing PR without the user's approval.
- Keep PRs focused and reasonably small; one PR per coherent feature/fix.

## PR readiness checklist

- [ ] Correct base branch; dependencies are explicit.
- [ ] Diff contains only intended changes.
- [ ] No accidental secrets, generated files, local settings or build output.
- [ ] `git diff --check` passes.
- [ ] Relevant Rust tests, TypeScript checks, UI build and Windows checks have been run as applicable.
- [ ] PR description lists what changed, tests run, what was not tested, and dependencies.
- [ ] GitHub Actions checks pass.

## Current repository release behaviour

Read `docs/RELEASES.md`. Currently, merges to `main` are the release trigger. Do not manually tag or publish versions as part of ordinary feature work.

## One-time GitHub repository settings

Check these settings manually in **Repository → Settings → Rules → Rulesets** (or **Branches**, depending on the GitHub UI). The GitHub integration used for this setup could not read branch-protection settings, so their current state is not confirmed.

Recommended protection for `main`:

- Require pull requests before merging.
- Require the Build workflow's **Tests and UI build** and **Windows installers** checks to pass. Select the exact check names GitHub reports after a successful run.
- Require all review conversations to be resolved.
- Block force pushes and branch deletion.
- Keep squash merging enabled; disable merge commits and rebase merging if you want one consistent history.
- Enable automatic deletion of merged topic branches.
- Do not require one approving review if this repository is operated by a single owner; that can make your own PRs impossible to merge. The owner should still review and explicitly approve each PR.

Do not turn on automatic merging for all PRs. A green workflow does not replace review of feature behaviour, privacy, dependency order or physical Ally testing.
