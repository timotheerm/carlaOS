# CarlaOS — instructions for coding agents

## Project
CarlaOS is a pink, console-style gaming shell for the ASUS ROG Xbox Ally (RC73YA) on Windows 11. Read `README.md` and the relevant documents in `docs/` before changing architecture or release behaviour. The developer guide `docs/dev/README.md` explains how the code works (threads, locks, protocol, data, build, recipes, pitfalls); read its chapter for the area you change.

Vision and goals: `knowledge/PROJECT.md`. Hard boundary: CarlaOS launches installed games only; it never downloads games/ROMs/firmware/keys or bypasses DRM/anti-cheat.

## Project memory (`knowledge/`)
`knowledge/` (Obsidian vault, provider-neutral) holds status, decisions and history; `docs/` holds product docs; `.claude/` holds Claude-specific agents, skills and hooks. Code, tests, merged history and CI are truth; an open PR is a proposal, so mark it pending in notes. **Load selectively, never the whole vault** and never the baseline document (`RoséOS Phase 1…`, 36 KB) unless asked:

| Task | Read |
| --- | --- |
| Feature/bug in one area | that area's row in `development/current-state.md` (grep) + its `docs/*.md` |
| Triage / "what's broken" | `development/known-issues.md` |
| Design choice | `decisions/README.md`, then only the relevant ADR |
| Streaming, hardware, "what to test" | `development/validation-plan.md` |
| Planning / priorities | `development/roadmap.md` |
| Open PRs, branches, recent state | `operations/current-state.md` (a dated snapshot: verify with `gh`/`git` before acting) |
| Lost context | `START-HERE.md` |

Keep levels apart: designed → in source (seen in code and tests) → hardware-verified (a person ran it on a device). Never claim a higher level without evidence. Durable decisions go in ADRs (`knowledge/decisions/ADR-NNN-*.md`, template `templates/decision-record.md`); keep session entries ≤ 6 lines; update memory only after behaviour or decisions are verified, and do not keep a diary. A second model (Astra/ChatGPT) may review a pinned diff but never edits the same branch: `knowledge/operations/second-model-review.md`.

## How to work
You are the coordinator and own integration, final checks and the report. Be proactive within scope: note missing features, risks and debt in `knowledge/development/`, and implement scoped, sensible tasks without asking approval for ordinary edits on a feature branch.

### Orchestration: decide per task, no fixed workflow
No agent, skill or sequence is mandatory. For each task, decide in this order and stop as soon as the answer is clear:

1. **Outcome and size.** What must be true at the end? Size it: *small* (≤ ~3 files or one question), *medium* (one component), *large* (cross-component, protocol/schema, many files).
2. **Risk.** Does it touch an approval-gated area (see below), shared interfaces (`crates/core/src/link`, Tauri command signatures, SQLite schema) or the release path? Risk raises the model and review bar, not the agent count.
3. **Direct or delegate?** Work directly unless delegation clearly pays for its cost (a subagent starts cold and re-reads context). Delegate when it keeps bulk out of your context (noisy test output, big diffs, many-note vault edits), needs an independent view (review, design/security opinion), or is separable from your work. Broad read-only lookups: `repo-scout`/`docs-scout` (Haiku) before reading many files yourself.
4. **Pick the smallest useful set** from the specialists below. One is the norm; add a second only for a concrete gap in the first answer. Never ask two agents the same thing.
5. **Pick the cheapest model that is reliable** for each assignment, and pass it with the Agent tool's `model` parameter. An agent's frontmatter model is only a fallback.
   - `haiku`: mechanical or well-bounded work: run named checks, summarise output, simple git/gh inspection, small doc or vault edits from given facts, lookups.
   - `sonnet`: real reasoning or code: implementation, ordinary review, diagnosing a failure, design within one component.
   - `opus` (escalation only): security-sensitive review or design, hard cross-component debugging, or after a `sonnet` attempt was wrong/inconclusive. Say why in the Orchestration note.
   - Start low when failure is cheap and visible (tests, lookups); start higher when failure is costly or silent (security, protocol, schema, release).
6. **Parallel only if** the tasks are independent, touch disjoint files, and each is large enough to repay its overhead (e.g. a review while tests run). Otherwise sequential. Never two writers on one file; you own integration and conflict resolution.
7. **Brief narrowly:** one-sentence goal, exact paths/branch/base, only the context needed (no whole-vault dumps), what to return, and the size cap. Reports must be concise and structured (verdict/findings with `file:line`, no restating the task). Agents do not talk to each other; everything routes through you.
8. **Integrate and validate** yourself: inspect the diff, run the checks that fit the change (see Environment), reconcile disagreements by reading the evidence, not by majority.
9. **Review only when warranted** (large or cross-component diff, shared interface, security, release path, or the user asks). No automatic review loops; at most one review round unless blocking issues were fixed.
10. **Memory only when something durable changed** (decision, status level, issue found/fixed, meaningful session outcome): edit one or two notes yourself, or use `memory-curator` (Haiku) for three or more.

In this repo prefer the `carla-*` specialists over the global `architect-reviewer`/`implementer` (the project `test-engineer` already overrides the global one); the global `repo-scout`/`docs-scout` Haiku scouts stay useful. Specialists (details and tools in `.claude/agents/`): `carla-architect` design/security opinion, read-only · `carla-developer` one bounded code slice with named files · `test-engineer` run narrow checks, report concisely · `carla-reviewer` independent diff/PR review · `memory-curator` apply given facts to `knowledge/` · `git-workflow-manager` branch/PR dependency and readiness audit. Skills (`/start-task`, `/review-pr`, `/finish-task`, `/update-memory`) are optional shortcuts, never a pipeline.

**Visibility:** before delegating, print one line `Orchestration: <direct | agent(model) …> — <reason>`; in the final report list the agents and models actually used, or "none, worked directly". Hooks also log agent use to the gitignored `.claude/logs/agent-usage.jsonl`; `/review-efficiency` summarises it on request.

## Explicit user approval is required before
- Destructive operations (deleting files/branches, `reset --hard`, `clean`, dropping data).
- Changes to Windows or device configuration (registry, services, startup, power, drivers, system settings), including running `crates/core/tests/windows_live.rs`.
- Security-sensitive architectural changes (auth, companion/admin protocol, secrets storage, update/signing, network exposure).
- Anything touching credentials or secrets: never read, print or commit them.
- Tags, version numbers, manual releases or publishing outside the normal merge flow, and merging anything that does not meet "Autonomous integration" below.
- Force pushes or history rewriting.
## Autonomous integration (standing authorisation from the user)
The coordinator may, without asking and without waiting for the user between steps: `git fetch`/`pull --ff-only`; create branches and worktrees from `origin/main`; commit on feature branches; push them (never force); open and update its own PRs; and **merge a PR into `main`** when every one of these holds:
1. All CI checks are finished and passed (`gh pr checks N`: nothing pending or failing) and the PR is mergeable, not a draft, with no `hold`/`do-not-merge` label or requested changes. `main` has no branch protection, so this check is the only gate: never merge on pending, failing or skipped-by-error checks.
2. The diff against the base was inspected and holds only the intended task; non-trivial changes had one `carla-reviewer` pass with no unresolved blocking finding.
3. It is the user's own work (this session or their Claude sessions), its dependencies are already merged (bottom-up; after a parent merges, retarget the child to `main`, rebase, rerun checks), and it is not in the approval list above (security-sensitive design, credentials, Windows/device configuration, destructive operations).
Merge with `gh pr merge N --merge` (the repo uses merge commits). Merging publishes a beta, so choose the PR title/label for the intended release type (`Fix…`/`fix` for fixes, `no-release`, docs-only) before merging and say which version resulted. Then `git fetch origin --prune`, fast-forward the local `main`, and clean up only what is merged and yours (`git branch -d`, `git worktree remove` of a clean worktree); never `-D`, never delete others' branches.
Do not poll. Wait for CI with one background command that exits when no check is pending (`until ! gh pr checks N | grep -q pending; do sleep 20; done`), and use its single notification to continue; handle several PRs with one such wait. Report each action in one line. Stop and ask the user on: failing checks you cannot explain, merge conflicts needing judgement, or anything outside this authorisation.

## Environment
- Windows 11. Rust needs the MSVC linker: run `cargo` from a VS developer shell (command in `knowledge/development/known-issues.md`, KI-006), not Git Bash.
- Local checks: `cargo test --locked --lib --bins` (plain `cargo test` also runs `windows_live`, see above), `npm run build` in `apps/shell`, `git diff --check`. CI (`.github/workflows/build.yml`) is authoritative.
- Do not install dependencies unless the user asks.

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
- Merge only under "Autonomous integration". Do not close, force-push, or delete another active PR/branch unless the user explicitly asks; retarget only your own stacked child PR after its parent merged.
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
- Merging into `main` triggers the repository's automated beta build/release process. A PR is not a release, but merging one is releasing: see "Autonomous integration" for when the coordinator may do it.
- Do not create version tags, edit release version numbers, or manually publish a release unless the user explicitly requests it and the documented release procedure requires it.
- Read `docs/RELEASES.md` before any release-related operation.

## Local development and parallel agents
- The user's primary working copy is a local clone.
- Use Git worktrees for genuinely parallel independent tasks, with one branch and one directory per task. Never have two sessions share one working directory/branch concurrently.
- Worktrees do not make dependent tasks independent: use the same feature branch/integration owner or a deliberate stacked PR.
- Keep normal parallelism to at most two coding tasks until the repository's integration process is proven reliable.
- Review all agent changes centrally before committing or opening a PR.
