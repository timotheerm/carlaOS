# CarlaOS — shared instructions for coding agents

Read this file before editing the repository. Then read `knowledge/START-HERE.md`, the dated `knowledge/operations/current-state.md`, and the relevant feature documents.

## Source of truth

- Code and tests describe implemented behaviour.
- Git history, current pull requests and CI describe integration state.
- `docs/` contains product and implementation specifications.
- `knowledge/` is the durable, provider-neutral context and decision layer.
- `CLAUDE.md` and `.claude/` contain Claude Code-specific instructions and workflows.
- An open PR is proposed work, never shipped behaviour.

## Git and integration

- Never commit directly to `main`.
- Fetch and inspect the working tree and every relevant open PR before starting.
- Every task must have an explicit branch, base commit and validation plan.
- Keep one PR per coherent feature/fix, not per agent.
- Dependent work must be on its parent branch or use a deliberate stacked PR targeting that parent.
- Use separate Git worktrees for independent concurrent writing sessions. Never share a writable worktree or branch between sessions.
- One coordinator owns integration and final validation. Review every agent diff before integrating it.
- Never overwrite user changes, reset another session's work, force-push, or change the state of an existing PR/branch without explicit approval.
- Do not merge just because GitHub reports a PR as mergeable. Inspect the diff, dependencies and required checks.

## CarlaOS engineering rules

- Keep shared cross-platform logic in `crates/core`; Tauri/Windows integration belongs in `apps/shell/src-tauri`; player UI belongs in `apps/shell/src`.
- Enforce administrator permissions in backend commands, not only in the UI.
- Treat Windows filesystem, process, registry, sleep/resume and installer behaviour as platform-specific risks.
- Consider handheld battery, CPU, memory, disk and network use for background work.
- State what hardware, live services and edge cases were not tested. Never claim physical Ally validation unless it happened on the device.
- Do not introduce dependencies, persistent services, external data sharing or machine-level changes without a clear product reason and explicit documentation.
- Keep game libraries, artwork, saves and user data safe. Do not download game ROMs, firmware or keys, bypass DRM or anti-cheat, or execute discovered programs during discovery.

## Commands and validation

- Rust workspace: `cargo test --locked` from the repository root.
- Windows checks: `scripts/check-windows.sh` through Git Bash when applicable.
- Frontend: `npm ci`, then `npm run build` from `apps/shell`.
- Native Tauri app: `npx tauri dev` from `apps/shell`.
- Git hygiene: `git status --short`, `git diff --check`, and inspect the complete diff.

Use `docs/LOCAL_SETUP_WINDOWS.md` for environment setup and `docs/GIT_WORKFLOW.md` for PR and worktree procedures.

## Release policy

Merging to `main` can trigger the automated beta build/release pipeline. Read `docs/RELEASES.md`; do not manually tag, version or publish in ordinary feature work.

## Memory

Update Markdown memory only after verified behaviour or an approved decision changes. Use ADRs for major architecture/product trade-offs. Do not create a diary, duplicate detailed feature docs, or store secrets, tokens, private data or local configuration.
