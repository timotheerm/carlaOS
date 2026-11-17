# CarlaOS — instructions for coding agents

## Project
CarlaOS is a pink, console-style gaming shell for the ASUS ROG Xbox Ally (RC73YA) on Windows 11. Read `README.md` and the relevant documents in `docs/` before changing architecture or release behaviour.

## Project memory and AI workflow
- Before starting, read `knowledge/START-HERE.md`, the dated `knowledge/operations/current-state.md`, and the relevant feature notes.
- Treat code, tests, merged history and CI as truth. Open PRs are proposals; mark them pending in notes.
- Keep durable, provider-neutral project memory in `knowledge/` as Markdown. Keep detailed product specifications in `docs/` and Claude-specific agents/skills in `.claude/`.
- Record important architecture or product trade-offs as ADRs in `knowledge/decisions/`. Update memory only after behaviour or decisions are verified; do not create a diary.
- Use `.claude/agents/` for bounded specialist work and `.claude/skills/` for repeatable procedures. Read-only specialists should not implement fixes.
- Claude Code is the primary coding coordinator. A second model (Astra/ChatGPT) may independently review a pinned diff but must not concurrently edit the same branch. See `knowledge/operations/second-model-review.md`.

## Non-negotiable Git rules
- Treat `main` as the stable integration and release branch. Never commit directly to `main`.
- Work from a local clone for normal development. Fetch before starting: `git fetch origin --prune`.
- Every coding task must have an explicit branch and a known base commit.
- Do not create a new PR until you have inspected all open PRs and their head/base branches.
- Default to one PR per coherent feature or fix, not one PR per agent, file, or subtask.
- Independent work may branch from `origin/main`. Work that depends on unmerged changes must either be added to the same feature branch/PR or explicitly use a stacked PR whose base is the parent feature branch.
- Never create a PR against `main` if its implementation depends on another unmerged PR.
- Do not let concurrent agents edit the same files or shared interfaces without an agreed integration owner.
- The coordinator owns integration, conflict resolution, final tests, PR creation and merge readiness.
- Do not merge, close, retarget, force-push, or delete another active PR/branch unless the user explicitly asks.
- Never force-push casually. If rewriting a branch is necessary, inspect its remote head first and use `--force-with-lease`.

## Before coding
1. Run `git status --short --branch`; preserve all user changes.
2. Fetch remote refs and inspect the current branch and `origin/main`.
3. Inspect open PRs and identify dependencies.
4. Read the relevant source, tests, docs and recent changes.
5. State the intended branch, base branch, affected areas and validation plan.

## Branch strategy
- Independent task: branch from latest `origin/main`, named `feat/<short-name>`, `fix/<short-name>`, or `docs/<short-name>`.
- Related subtasks: prefer one feature branch and one PR, with agents contributing through separate commits or patches coordinated by the main session.
- Genuine dependency requiring separate review: use a stacked PR. The child branch starts from the parent branch and the child PR targets that parent branch. Document the dependency in the PR body. Merge bottom-up; after the parent merges, rebase the child onto `origin/main`, update its PR base to `main`, and rerun checks.
- Avoid long-lived stacks. Keep them shallow and merge the foundation first.

## Before opening or updating a PR
- Confirm the diff against the PR base contains only the intended task.
- Run relevant tests and build checks. For broad changes, follow the checks in `README.md` and CI.
- Check `git status`, `git diff --check`, and inspect the final diff.
- PR body must state summary, tests run, limitations, and dependencies.
- For independent work, target `main`. For stacked work, target the direct parent branch.
- Do not claim Windows hardware validation unless it actually ran on Windows hardware.

## Release policy
- Merging into `main` triggers the repository's automated beta build/release process. A PR is not a release.
- Do not create version tags, edit release version numbers, or manually publish a release unless the user explicitly requests it and the documented release procedure requires it.
- Read `docs/RELEASES.md` before any release-related operation.

## Local development and parallel agents
- The user's primary working copy is a local clone.
- Use Git worktrees for genuinely parallel independent tasks, with one branch and one directory per task. Never have two sessions share one working directory/branch concurrently.
- Worktrees do not make dependent tasks independent: use the same feature branch/integration owner or a deliberate stacked PR.
- Keep normal parallelism to at most two coding tasks until the repository's integration process is proven reliable.
- Review all agent changes centrally before committing or opening a PR.
