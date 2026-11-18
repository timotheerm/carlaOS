# CarlaOS developer guide

For an AI agent or a developer who has never seen this repository and needs to change it safely.
It describes the **code as it is** (checked against `main` at the commit it was written from).
Where the authors were not sure after reading the code, the text says **UNVERIFIED** and what
would settle it. Product behaviour for players and administrators is in the documents one level
up ([`docs/`](../)); project status, decisions and known issues are in
[`knowledge/`](../../knowledge/START-HERE.md); process rules (branches, PRs, releases, what needs
approval) are in [`CLAUDE.md`](../../CLAUDE.md) and [`AGENTS.md`](../../AGENTS.md).

## Read in this order

1. **[01 · Architecture and runtime](01-architecture-and-runtime.md)**: what runs where, every
   background thread, every lock, three request flows, and the post-mortem of the freeze that
   shaped today's rules. Read this even if you only change one screen.
2. **[10 · Pitfalls and open questions](10-pitfalls-and-open-questions.md)**: the mistakes that
   already cost days, and what is still unknown.
3. **[09 · How-to recipes](09-how-to-recipes.md)**: step-by-step for adding a command, a screen, a
   database column, a protocol route, a CLI command, a dashboard button.
4. The chapter for the area you are changing:

| Chapter | Area | Paths |
| --- | --- | --- |
| [02 · Core crate](02-core-crate.md) | Shared library: catalogue, scanners, launching, saves sync, link client, Windows tuning, crash/log files | `crates/core` |
| [03 · Desktop host](03-desktop-host.md) | `carla-companion`: HTTP server, dashboard, launcher, Sunshine, screens, CLI, files | `crates/companion` |
| [04 · Ally native side](04-ally-native.md) | Tauri app: start-up, watchers, every command, sessions, hosts, save-sync orchestration | `apps/shell/src-tauri` |
| [05 · Ally interface](05-ally-ui.md) | React: page stack, state store, native bridge, controller navigation, screens, styling | `apps/shell/src` |
| [06 · Link protocol](06-link-protocol.md) | Linking, signing, every route, compatibility rules, measuring | `crates/core/src/link`, `crates/companion/src/server.rs` |
| [07 · Data and storage](07-data-and-storage.md) | SQLite schema and migrations, every file on both machines, secrets, privacy | `crates/core/src/library/store.rs`, `crates/companion/src/files.rs` |
| [08 · Build, test, release](08-build-test-release.md) | Toolchain, commands, CI jobs, versions, installers, updates | `Cargo.toml`, `apps/shell/package.json`, `.github/workflows`, `scripts`, `packaging` |

## Five rules that apply everywhere

1. **No lock across I/O, no guard in a header.** Never hold `library.store()` (or any mutex)
   across a network call, a process, a Windows setting, or a call that takes the same lock; never
   create a guard in a `for`/`if let`/`while let`/`match` header. This froze every Ally from v0.20.0
   to v0.34.0.
2. **Children get null stdio.** Every program CarlaOS starts gets `Stdio::null()` for all three
   streams (and no window where none is wanted).
3. **Compatible protocol.** The Ally and each desktop update separately: new fields are optional,
   unknown values never fail a message, a 404 from an older peer means "not supported".
4. **Saves are sacred, boundaries are fixed.** Never delete or overwrite saves without an exact
   match and a verified backup. Never download games/ROMs/firmware/keys or bypass DRM.
5. **Evidence, not claims.** Run the checks in [08](08-build-test-release.md); say what ran and what
   could not; never claim something works on an Ally or a desktop unless it ran there.

## Glossary

| Term | Meaning |
| --- | --- |
| Ally | The handheld (ASUS ROG Xbox Ally RC73YA), or any Windows PC running the CarlaOS app in its place (often a laptop during development). |
| Desktop / host / companion | A Windows PC with "CarlaOS Desktop Host" (`carla-companion`) and Sunshine, which games are streamed from. |
| Shell | The Ally app (`carlaos-shell`, Tauri). |
| Link / linked | A desktop and an Ally that share a key after enrolment (approved on the desktop). |
| Session / "now playing" slot | The one running game or stream, `Library.0.active`. |
| Place | Where a game runs: `local`, a desktop's id, or `cloud`. |
| Pulse | The Ally telling a desktop how it is (every 45 s, and with each measurement). |
| Launcher | `carla-companion launch`, the program Sunshine starts for a stream. |
| Keep-clean / retire | Opt-in removal of a desktop's live save after it is safely on the Ally (ADR-009). |
| Crash trail | `breadcrumbs.log` on the Ally: short notes written before a crash or freeze. |

## Keeping this guide true

When you change behaviour described here, update the chapter in the same PR. When you find a
statement that is wrong, fix it and remove the **UNVERIFIED** marker only when you checked the
code. Each chapter ends with "Open questions / needs investigation": resolve items there and move
them into the text.
