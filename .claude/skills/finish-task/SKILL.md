---
name: finish-task
description: Wrap up CarlaOS work - self-inspect the diff, run the relevant checks, update memory only if warranted, then commit, push, open the PR and merge it when the CLAUDE.md "Autonomous integration" conditions hold.
disable-model-invocation: true
---

# Finish

Never tag, edit versions, force-push or merge outside CLAUDE.md "Autonomous integration".

1. `git status --short --branch` (must not be `main`); `git diff <base>...HEAD --stat`, then read the diff yourself. Remove stray files and unrelated changes. `git diff --check`.
2. Run only the checks the change needs (CLAUDE.md "Environment"): Rust changes → `cargo test --locked --lib --bins -p <crate>`; shell changes → `npm run build`; docs/config only → none. Use `test-engineer` only for long or noisy runs. Say plainly what ran and what could not.
3. Independent review: only for non-trivial changes, one `carla-reviewer` call. Skip for small diffs.
4. Docs: update `docs/` if user-visible behaviour changed. Run `/update-memory` only if a decision, status change or issue arose (not after every task).
5. If PRs are open, ask `git-workflow-manager` to confirm base/dependencies/release type; otherwise check yourself (`Fix…`/`fix` label, `no-release`, docs-only).
6. Commit (attribution line from the environment), push the branch, open the PR (summary, tests run, limitations, dependencies; no hardware claims unless true; title/label chosen for the intended release type).
7. Wait for CI with one background command, then merge under "Autonomous integration" if every condition holds; otherwise report what blocks it. Update local `main` and clean up merged branches/worktrees as described there.
