# 08. Build, test and release

How to build, test and ship CarlaOS: the local Windows toolchain, every check and what it covers, the GitHub Actions workflow job by job, the automatic release pipeline and its numbering rules, the three installers, the updater at a high level, and the pitfalls that have already cost time.

Audience: a developer or AI that has never seen the repository. Paths are repo-relative. Claims I could not confirm from code or config are marked **UNVERIFIED:** with what would confirm them.

Authoritative sources, in order of trust: the workflow `.github/workflows/build.yml` and the scripts it calls (they are what actually runs), then `docs/RELEASES.md`, `docs/LOCAL_SETUP_WINDOWS.md`, `docs/GIT_WORKFLOW.md`, and `CLAUDE.md` for process rules. When this chapter and CI disagree, CI is right (`CLAUDE.md`: "CI is authoritative").

Related chapters: storage and what survives an update in `docs/dev/07-data-and-storage.md`.

---

## 1. Workspace layout and crate names

Cargo workspace at the repository root (`Cargo.toml`, `resolver = "2"`):

| Package name (Cargo) | Directory | Kind | What it is | Platform |
| --- | --- | --- | --- | --- |
| `carla-core` | `crates/core` | library | Shared logic: catalogue, scanners, link protocol, crash/log helpers, secret sealing, updater, clean-up engine | Portable (Windows-only parts are `cfg(windows)`) |
| `carla-companion` | `crates/companion` | binary `carla-companion` | The desktop host: HTTP server on the desktop, dashboard, launcher | Portable to build/test; behaviour is Windows |
| `carlaos-shell` | `apps/shell/src-tauri` | binary (Tauri 2) | The Ally interface: Rust side of the Tauri app | Needs WebView2 (Windows) or webkit2gtk (Linux) to build |

The React/TypeScript front end is `apps/shell` (npm project `carlaos-shell`, `apps/shell/package.json`); its Rust half is `apps/shell/src-tauri`. Do not confuse the Cargo package `carlaos-shell` (the Rust crate) with the npm package of the same name.

Key workspace facts (`Cargo.toml`):

- `default-members = ["crates/core", "crates/companion"]`. **A plain `cargo build` or `cargo test` at the root does not build the shell.** The shell needs a web view toolchain, so you must name it: `-p carlaos-shell`.
- `[workspace.package] version = "0.1.0"` is a **placeholder**. Every crate inherits it. Real numbers are stamped at release build time (section 7.4). Do not edit it by hand to "bump" a version (`CLAUDE.md`: never edit version numbers).
- `Cargo.lock` is committed and CI uses `--locked`, so a build fails if `Cargo.lock` is out of step with the `Cargo.toml` files. After changing a dependency, update and commit `Cargo.lock`. (Adding dependencies needs the user's approval, `CLAUDE.md`.)
- `[profile.release]`: `lto = true`, `codegen-units = 1`, `strip = true`, `panic = "abort"`, `opt-level = "s"`. Consequences: release builds are slow; a panic **aborts** the process (the crash reporter's panic hook is the last code that runs, see chapter 07); debug/test builds unwind normally.
- No `rust-toolchain` file: CI uses `dtolnay/rust-toolchain@stable`; locally use stable MSVC (`rustup default stable-msvc`, `docs/LOCAL_SETUP_WINDOWS.md`). The Rust edition is 2021, which matters for temporary lifetimes in `for`/`if let`/`match` headers (section 6.4).
- License `MIT`, repository `https://github.com/timotheerm/carlaOS`.

Other top-level directories relevant here: `scripts/` (release scripts and small Node tests), `packaging/` (NSIS installers and smoke test), `.github/workflows/build.yml`.

Shell build configuration, `apps/shell/src-tauri/tauri.conf.json`:

| Key | Value | Meaning |
| --- | --- | --- |
| `productName` | `CarlaOS` | Installer and install folder name |
| `identifier` | `dev.carlaos.shell` | App identifier; also the data folder name (chapter 07). **Never change it casually** |
| `version` | `0.1.0` | Placeholder, stamped at release build |
| `build.beforeBuildCommand` | `npm run build` | `npx tauri build` builds the UI first |
| `build.beforeDevCommand`, `devUrl` | `npm run dev`, `http://localhost:1420` | `npx tauri dev` starts Vite on 1420 |
| `build.frontendDist` | `../dist` | `apps/shell/dist`, gitignored |
| window | label `main`, fullscreen, no decorations, not resizable, 1920x1080, WebView2 flags `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required` | A console-style full-screen window (see known issue KI-017 in `knowledge/development/known-issues.md` about SmartScreen) |
| `security.csp` | `default-src 'self'; img-src 'self' asset: http://asset.localhost data:; style-src 'self' 'unsafe-inline'` | Pictures from disk are served through the asset protocol |
| `assetProtocol` | enabled, scope `[]` | Folders are allowed at run time (`allow_pictures`, chapter 07) |
| `bundle.targets` | `["nsis"]` | Only an NSIS installer |
| `bundle.windows.webviewInstallMode` | `skip` | The installer does **not** bundle or install WebView2 |
| `bundle.windows.nsis.installMode` | `currentUser` | Per-user install, no administrator |

---

## 2. Prerequisites on Windows 11

Follow `docs/LOCAL_SETUP_WINDOWS.md` for the click-by-click version. Summary and the reasons behind each item:

| Need | Version / source | Why |
| --- | --- | --- |
| Git | `winget install --id Git.Git -e` | Also provides Git Bash, needed for the `.sh` scripts |
| GitHub CLI | `winget install --id GitHub.cli -e`, then `gh auth login` | PRs, checks, the autonomous integration flow in `CLAUDE.md` |
| Node.js | **22.x** (`winget install --id OpenJS.NodeJS.22 -e`) | CI uses Node 22 (`actions/setup-node@v4`, `node-version: 22`); the front end and `scripts/*.mjs` assume it. `scripts/test-inflight.mjs` uses `stripTypeScriptTypes` from `node:module`, which needs a recent 22.x. **UNVERIFIED:** the minimum 22.x minor; confirm with `node scripts/test-inflight.mjs` on the oldest Node you intend to support |
| Rust | `rustup` with the **stable MSVC** toolchain (`rustup default stable-msvc`) | The workspace builds with stable |
| MSVC build tools | Visual Studio 2022 **Build Tools**, workload "Desktop development with C++" (MSVC compiler plus Windows 10/11 SDK) | Rust's MSVC target needs `link.exe` and the SDK libraries to link anything |
| WebView2 runtime | Normally included with Windows 11 | The Tauri shell renders in it; the installer does not install it (`webviewInstallMode: skip`) |
| NSIS | Not needed for normal development | Only `packaging/package.mjs` needs `makensis`; CI installs it. `npx tauri build` downloads Tauri's own NSIS to `%LOCALAPPDATA%\tauri\NSIS` |

### 2.1 The developer shell requirement (vcvars)

Rust on Windows links with the MSVC linker. `link.exe` is **not on the normal PATH**; Visual Studio's developer environment puts it there (vcvars, "Developer PowerShell"). Without it, `cargo build` fails at the link step.

PowerShell, from `knowledge/development/known-issues.md` (KI-006), with the Build Tools in their default place:

```powershell
$vs = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
Import-Module "$vs\Common7\Tools\Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation -DevCmdArguments "-arch=x64"
```

Run `cargo` from that shell, then check `where.exe link` lists the Visual Studio `link.exe` first.

**Why Git Bash breaks it.** Git for Windows ships GNU coreutils with its own `link` command (`/usr/bin/link`, which makes hard links). In Git Bash `link` resolves to that, shadowing MSVC's `link.exe`, so Rust's linker invocation does the wrong thing and fails with confusing errors. KI-006 records this: "Not usable from Git Bash (GNU `link` shadows it)". So:

- Run `cargo` and `npx tauri ...` from the VS developer shell (PowerShell or "Developer PowerShell for VS 2022"), **not** Git Bash.
- Use Git Bash only for the shell scripts (`scripts/check-windows.sh`, `scripts/release/*.sh`), invoked as `& 'C:\Program Files\Git\bin\bash.exe' scripts/check-windows.sh` from PowerShell.
- The Claude Code Bash tool on Windows is Git Bash; do not call `cargo` through it unless the VS environment was imported into that process.

**UNVERIFIED:** that the install path above exists on a given machine (a Community or Professional edition installs elsewhere); confirm with `vswhere.exe -latest -property installationPath`.

### 2.2 First-time checks

```powershell
git --version; gh --version; node --version; npm --version; rustc --version; cargo --version; rustup show
```

Expect `stable-x86_64-pc-windows-msvc` as the default. The first Rust build is slow (it compiles bundled SQLite, `ureq`, Tauri, and so on).

---

## 3. Every build and test command

Run Rust commands from the VS developer shell, at the repository root unless a directory is given.

### 3.1 Rust

| Command | Covers | Notes |
| --- | --- | --- |
| `cargo test --locked --lib --bins` | Unit tests of `carla-core` and `carla-companion` (default members): everything compiled into the libraries and binaries. **The standard local check** (`CLAUDE.md`, "Environment") | Excludes integration tests, therefore excludes `windows_live` |
| `cargo test --locked` (or plain `cargo test`) | The above **plus integration tests**: on Windows this includes `crates/core/tests/windows_live.rs` | **Do not run on a real machine without approval.** `windows_live.rs` is `#![cfg(windows)]` and runs the real clean-up helper script against the machine (scan, apply, undo, check restored). It changes Windows settings and then undoes them. CI runs it on a throwaway Windows runner. `CLAUDE.md` makes running it require explicit user approval |
| `cargo test --locked -p carla-core --lib` | One crate's unit tests | Narrowest useful check for core changes |
| `cargo test --locked -p carla-companion --lib --bins` | The companion's tests | Includes server tests using a temp state folder and the dashboard test that runs Node (3.3) |
| `cargo test --locked -p carlaos-shell --lib --bins` | The Tauri crate's tests (library state, hosts watch round, crash/boot glue) | **Needs the UI built first** (`npm run build` in `apps/shell`, 3.2) because `tauri::generate_context!` reads `frontendDist`. CI comment: "they need the interface built above". Also needs the Windows toolchain and WebView2 headers |
| `cargo test --locked store` | A name filter: tests whose path contains `store` | Faster iteration; add `-p` to limit the crate |
| `cargo build --release --locked -p carla-companion` | Release companion binary at `target\release\carla-companion.exe` | Input of the host installer |
| `cargo clippy --locked -p carlaos-shell -- -A clippy::all -D clippy::significant_drop_in_scrutinee` | The one enforced lint, see 3.4 | Run by CI on Windows only |
| `cargo fmt --all -- --check` | Formatting | **Not enforced and will report differences**, see 3.5 |

`--locked` means "do not touch `Cargo.lock`; fail if it needs changes". Use it always, as CI does.

### 3.2 Front end (`apps/shell`)

| Command | Covers |
| --- | --- |
| `npm ci` | Installs exactly `package-lock.json` (CI uses this; needs network the first time) |
| `npm run build` | `tsc --noEmit && vite build`: type-checks the TypeScript, then builds `apps/shell/dist` |
| `npm run dev` | Vite dev server on port 1420 (what `tauri dev` starts) |
| `npx tauri dev` | Runs the whole app with hot reload (see section 4) |
| `npx tauri build` | Builds the release installer (`target\release\bundle\nsis\*.exe`); runs `npm run build` first through `beforeBuildCommand` |

There is **no unit-test script for the front end** (`package.json` has only `dev`, `build`, `preview`, `tauri`; known issue KI-005). The type check is the front end's only automated gate besides the Node script below. Stack (from `package.json`): React 19, Vite 8, TypeScript 7, `@tauri-apps/api` 2 and `@tauri-apps/cli` 2.

### 3.3 Node script tests (`scripts/`)

| Script | Run by | What it tests |
| --- | --- | --- |
| `node scripts/test-inflight.mjs` | CI (Linux job) | `apps/shell/src/inflight.ts` with a fake window and timers: a command running longer than 8 s is noted once while running and once when it ends; quick, ignored and failing commands behave; installing the watcher never throws even if `__TAURI_INTERNALS__` is frozen or missing. Prints `ok ...` lines and `all passed` |
| `node scripts/test-dashboard.mjs [path to dashboard.html]` | **Not by CI directly:** by `cargo test` in `crates/companion/src/dashboard.rs` (test `the_add_button_of_the_page_works_when_clicked`), skipped with a message if `node` is not on PATH | Extracts `h()` and `offered()` from `crates/companion/src/dashboard.html`, runs them with a tiny fake DOM, simulates a click whose `event.currentTarget` becomes null after the handler returns (as in a browser) and checks the row is removed and a toast shown. Note it splits the HTML on `\r\n`, so CRLF working copies work (section 9) |
| `bash scripts/release/test-changes.sh` | CI (Linux job) | `scripts/release/changes.sh` with a fake `gh`: resolved commits collapse to one row; a late answer is found by retrying; commits that never resolve (empty answer, failure) become "unresolved" rows; an unresolved row can never make "everything is `no-release`" true |
| `bash scripts/check-windows.sh` | You, locally; not CI | Type-checks the **shell for Windows from Linux/macOS** with `cargo check --target x86_64-pc-windows-gnu`, with fake C compiler stand-ins; nothing it produces can run. Needs `rustup target add x86_64-pc-windows-gnu`. Creates `apps/shell/dist` if missing. Extra arguments go to `cargo check`. `docs/LOCAL_SETUP_WINDOWS.md` also suggests it as part of a pre-PR check |

### 3.4 Clippy and the enforced lint

The only lint CI enforces:

```
cargo clippy --locked -p carlaos-shell -- -A clippy::all -D clippy::significant_drop_in_scrutinee
```

`-A clippy::all` silences every other clippy lint; `-D clippy::significant_drop_in_scrutinee` makes this one an error. Reason (commit message of "Fix the Ally freezing: the library lock was taken twice on one thread"): in Rust 2021 a temporary in the header of `for`, `if let` or `match` lives until the end of the whole statement. So

```rust
for host in library.store().hosts().unwrap_or_default() { ... library.store() ... }
```

keeps the library `MutexGuard` for the entire loop, and a nested `library.store()` on the same thread then deadlocks on a non-reentrant `std::sync::Mutex`. Every other command then waits behind that lock and the Ally freezes. This shipped from 0.20 to 0.34. The fix pattern: **copy the data out into a local first, then loop**, and never call `library.store()` while holding another guard on it. The lint flags exactly this shape; it is run only on the shell crate, where the library lock lives. Local equivalent: run the same command from the VS shell.

### 3.5 Formatting policy

The repository is **not rustfmt-formatted**: the code deliberately uses a long-line, compact style, and `cargo fmt --all -- --check` reports differences in many files (known issue KI-018: "A whole-repo reformat would bury real diffs, so it is not done"). The decision (normalise in one dedicated PR and add a CI check, or document that fmt is not part of the contract) is still open. Rules for you:

- Do not run `cargo fmt` over files you are not otherwise changing.
- Match the surrounding style in the lines you write.
- Do not add a format check to CI as part of a feature change.

### 3.6 Whitespace check

`git diff --check` before every commit/PR (`CLAUDE.md`). It flags trailing whitespace and conflict markers. On a CRLF working copy with `core.autocrlf=true` Git normalises line endings first (section 9), so a plain run is clean.

### 3.7 Recommended order for a change

1. Narrow test for the crate you touched (`cargo test --locked -p <crate> --lib`).
2. `cargo test --locked --lib --bins` (portable crates).
3. If you touched the shell or UI: `npm run build` in `apps/shell`, then `cargo test --locked -p carlaos-shell --lib --bins`, then the clippy command in 3.4.
4. `node scripts/test-inflight.mjs` if you touched `apps/shell/src/inflight.ts` or the Tauri invocation layer.
5. `git diff --check`.
6. Push and let CI run the rest. A local build "is not the same as validating a real Ally" (`docs/LOCAL_SETUP_WINDOWS.md`); hardware behaviour needs a real ASUS ROG Xbox Ally.

---

## 4. Running things locally

### 4.1 The shell (Ally interface)

From the VS developer shell:

```powershell
cd apps\shell
npm ci
npx tauri dev
```

- Starts Vite on `http://localhost:1420`, compiles the Rust side, opens the window. First compile is long; keep the terminal open.
- The window is **fullscreen and undecorated** by configuration (`tauri.conf.json`). Close it with Alt+F4 or the app's own exit; do not expect a title bar.
- Data goes to the real `%LOCALAPPDATA%\dev.carlaos.shell\` (chapter 07). A dev run therefore shares library, PIN and linked desktops with an installed CarlaOS on the same account. Be careful when testing migrations: back up that folder first.
- Passing `--boot` as an argument emulates Windows starting it (start-up guard); do not do this casually (chapter 07, `boot.json`).
- Anything that changes Windows (start with Windows, clean-up, power) is gated by the administrator PIN and writes to the real machine. Per `CLAUDE.md`, Windows or device configuration changes need explicit approval.

### 4.2 The companion (desktop host)

```powershell
cargo run -p carla-companion -- info          # prints version/role JSON, harmless
cargo run -p carla-companion -- help          # command list
$env:CARLA_COMPANION_DIR = "$env:TEMP\carla-dev-companion"   # isolated state folder
cargo run -p carla-companion -- serve         # run the server (needs config; see below)
```

- `carla-companion` with no arguments does first-time setup (asks for a name, looks for Sunshine), opens a linking code and runs. `setup`, `dashboard`, `code`, `allies`, `devices`, `forget`, `games`, `add`, `drop`, `find`, `consoles`, `console add|drop`, `saves ...`, `log [--follow] [--lines N] [--launcher]`, `update`, `remove`, `info`, `firewall`, `sunshine`, `serve` exist (see `HELP` in `crates/companion/src/main.rs`).
- **Set `CARLA_COMPANION_DIR`** to a scratch folder when developing: otherwise state goes to the real `%LOCALAPPDATA%\CarlaOS\companion` and may overwrite a working host's config and linked devices. Tests use this variable for the same reason.
- Some subcommands change the machine: `firewall` (Windows Firewall rules), setup and `remove` (the `Run` registry entry `CarlaOS Companion`). These are Windows configuration; ask before running them on a real machine.
- The companion's HTTP server listens on `link::PORT` = 47600 by default (`crates/core/src/link/mod.rs`; the port is stored in the companion's `config.json`).

### 4.3 Running the Linux type check

`bash scripts/check-windows.sh` after `rustup target add x86_64-pc-windows-gnu` (see 3.3). From Windows use Git Bash or `& 'C:\Program Files\Git\bin\bash.exe' scripts/check-windows.sh`; it expects a Unix-like environment and a GNU Windows target, which on a Windows host with MSVC is an unusual combination. **UNVERIFIED:** that it works on a Windows host; it is documented as "from Linux or macOS". Confirm by running it once.

---

## 5. The CI workflow, job by job

File: `.github/workflows/build.yml`, name `Build`. One workflow does everything: build and test every PR, and turn every change that reaches `main` into a numbered beta release.

### 5.1 Triggers

| Trigger | Condition | What happens |
| --- | --- | --- |
| `push` | branches `[main]`, **`paths-ignore: ["**.md", "docs/**"]`** | A merge to `main` builds, tests and (normally) publishes. A push that only changes Markdown or `docs/**` does not run the workflow at all, so a docs-only merge creates no release and no run |
| `pull_request` | any | Build and test; `release` job only prints a preview, nothing is published |
| `workflow_dispatch` | inputs `version`, `ref`, `previous`, `host` | Manual publishing (section 7.7) |

`permissions` default to `contents: read`; the `release` job raises it to `contents: write` and the `plan` and `release` jobs get `pull-requests: read` (they query merged PRs).

### 5.2 Concurrency group and cancelled runs

```yaml
concurrency:
  group: build-${{ github.ref }}-${{ inputs.version || 'auto' }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

- The group is per git ref (and per manual version). For a PR the ref is `refs/pull/<n>/merge`, so a **new push to a PR cancels the older run of that PR** (you will see runs marked *Cancelled*; that is normal, not a failure).
- For pushes to `main` `cancel-in-progress` is false: runs queue **one at a time**, "so two merges cannot claim the same number". GitHub keeps one running and one pending run per group; a still newer one replaces the pending one. That is why `docs/RELEASES.md` says "If several pull requests are merged while one build is running, the ones in the middle may share a release with the last; its text lists all of them."
- A manual run with an explicit version has its own group (`...-0.5.0`), so it can run beside the normal one.
- There is no branch protection on `main` (`CLAUDE.md`): CI status is the only gate, so check it yourself before merging (section 8).

### 5.3 Job `plan` ("Version"), Linux

Runs `scripts/release/plan.sh` after a **full-depth** checkout (`fetch-depth: 0`; version numbers come from tags). Outputs: `version`, `release`, `republish`, `ref`, `target`, `previous`, `host`, `tests`. All later jobs consume these. Details of the logic are in section 7.

### 5.4 Job `check` ("Tests and UI build"), Linux

Runs only if `needs.plan.outputs.tests == 'true'` (an older commit that is only being published again was tested when it was new). Steps:

1. Checkout `needs.plan.outputs.ref`.
2. `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`.
3. `cargo test --locked`: the default members (`carla-core`, `carla-companion`). On Linux `windows_live` does not compile (`#![cfg(windows)]`), `secret` is the identity function, Windows-only code is compiled out. The dashboard test shells out to `node`. **UNVERIFIED:** which Node runs it, because `setup-node` comes *after* this step; the Ubuntu runner image normally has a Node preinstalled. Confirm by reading the log of `the_add_button_of_the_page_works_when_clicked` (a skip prints "skipped: node is not available").
4. `bash scripts/release/test-changes.sh`.
5. `actions/setup-node@v4` (Node 22, npm cache keyed on `apps/shell/package-lock.json`), `npm ci`, `npm run build` (both in `apps/shell`).
6. `node scripts/test-inflight.mjs`.

The shell crate is **not** built on Linux (no webkit2gtk setup in this job): its Rust code is only compiled and tested in the Windows job. A change to `apps/shell/src-tauri` that is not Windows-clean will only fail there.

### 5.5 Job `windows` ("Windows installers"), `windows-latest`

Does not wait for `check`; both need only `plan`. Environment `VERSION` = planned version. Steps, in order:

1. **Checkout** the planned `ref` (the commit being built).
2. **Checkout again** into `.release-tools` with `sparse-checkout` of `packaging` and `scripts/release` from the commit the workflow itself runs from. Purpose: "the packaging always comes from the commit this workflow runs from, so an older commit can be rebuilt with today's installers". Consequence: when you change `packaging/` or `scripts/release/` the *tooling* version used is the workflow's own commit, while the *source* built is `ref`.
3. Rust stable with the `clippy` component; rust-cache; Node 22 with npm cache; `npm ci` in `apps/shell`.
4. **Stamp version** `node .release-tools/scripts/release/stamp-version.mjs $env:VERSION .` (section 7.4). It runs **after** the cache step on purpose: "the cache is keyed on Cargo.lock as committed" (stamping changes `Cargo.lock`, which would otherwise change the cache key).
5. **Test the core on Windows** (if `tests`): `cargo test --locked -p carla-core -- --nocapture`. This is the plain crate test run, so it **includes `windows_live`**, which "runs the Windows clean-up helper for real on this throwaway machine: scan, apply, undo, and check everything is back as it was".
6. **Test the companion on Windows**: `cargo test --locked -p carla-companion`. Comment: "Linking, signing and key sealing, with Windows data protection for real."
7. **Build the shell installer**: `npx tauri build` in `apps/shell` (runs `npm run build`, compiles the shell in release mode, produces `target\release\bundle\nsis\CarlaOS_<version>_x64-setup.exe`; the exact file name pattern is Tauri's, `package.mjs` only requires exactly one `.exe` there whose name contains the version).
8. **Test the shell** (if `tests`): `cargo test --locked -p carlaos-shell`. Needs the UI built, which step 7 just did. First ran in CI with the lock-deadlock fix.
9. **Lint for lock guards**: the clippy command in 3.4.
10. **Build the companion**: `cargo build --release --locked -p carla-companion`.
11. **Find NSIS**: looks for `makensis` on PATH, in `Program Files (x86)\NSIS`, and in `%LOCALAPPDATA%\tauri\NSIS` (where Tauri downloaded its own); if none, `choco install nsis`. Exports `MAKENSIS`.
12. **Make the installers**: `node .release-tools/packaging/package.mjs --version $VERSION --source . --out installers [--no-host]`.
13. **Try the installers**: `packaging/smoke-test.ps1 -Version ... -Dir installers [-NoHost]` (section 6.4).
14. **Upload artifact** `carlaos-installers` from `installers/*` (`if-no-files-found: error`). Every PR run therefore produces downloadable installers as an artifact, as the README says.

Note that the Windows job builds with the **stamped** version, so test runs see the release number, not `0.1.0`.

### 5.6 Job `release` ("Publish the release" or "Release preview"), Linux

```yaml
needs: [plan, check, windows]
if: always() && needs.windows.result == 'success' && (needs.check.result == 'success' || needs.check.result == 'skipped')
```

It runs after a skipped test job (an old commit re-published) but never after a failed one. The job title is "Publish the release" when `plan.release == 'true'`, else "Release preview". Steps:

1. Checkout (full depth), download the `carlaos-installers` artifact into `installers/`.
2. **Write the release text:** `scripts/release/notes.sh "$VERSION" "$TARGET" "$PREVIOUS" "$HOST" > notes.md`, append it to the run summary, then `(cd installers && sha256sum --check SHA256SUMS.txt)` to verify the checksum file against the files.
3. **Publish** (only if `release == 'true'`): tag `v<version>`, title `CarlaOS <version> (beta)`, flagged **pre-release**. Assets, with display labels: Ally installer always; host and full installers if `HOST` is true; `SHA256SUMS.txt`. If `republish` is true: `gh release edit` (title, notes) and `gh release upload --clobber`; otherwise `gh release create "$tag" --target "$TARGET" ...`. **The tag is created by this command at `TARGET`**; there is no manual tagging.
4. On a pull request the job stops after step 2: the notes in the run summary are what the release would say.

Failure behaviour worth knowing: if any test or build step fails, no release is created. The change stays untagged, and the *next* successful build's "what is new" and its version computation include it (the range is "latest tag..target").

---

## 6. The three installers

Made by `packaging/package.mjs` (arguments `--version`, `--source`, `--out`, `--no-host`; needs NSIS; fails if the version is not three numbers, if there is not exactly one Tauri installer, or if the Tauri installer name lacks the version, i.e. "Stamp the version before building"). It writes `SHA256SUMS.txt` with one SHA-256 per installer.

| Installer file | Built from | Installs | Where |
| --- | --- | --- | --- |
| `CarlaOS-Ally-<v>-setup.exe` | Tauri NSIS bundle (the shell), renamed | The Ally interface (`carlaos-shell`) | `%LOCALAPPDATA%\CarlaOS` (per `docs/RELEASES.md`), per user. Uninstall key `HKCU\...\Uninstall\CarlaOS` (the smoke test reads it) |
| `CarlaOS-Desktop-Host-<v>-setup.exe` | `packaging/host.nsi` wrapping `target\release\carla-companion.exe` | The companion | `%LOCALAPPDATA%\Programs\CarlaOS Desktop Host` |
| `CarlaOS-Full-<v>-setup.exe` | `packaging/full.nsi` carrying the two above | Either or both, with a tick box each | As the two above; **it installs nothing itself** |
| `SHA256SUMS.txt` | `package.mjs` | n/a | Updater and humans verify downloads against it |

`--no-host` makes only the Ally installer (versions before the desktop host existed: 0.1.0 and 0.2.0).

All three install **for the signed-in account only** and never ask for administrator rights (`RequestExecutionLevel user` in the `.nsi` files, `installMode: currentUser` for Tauri). Beta builds are **unsigned**, so Windows SmartScreen shows "Windows protected your PC" (known issue KI-004).

### 6.1 Ally installer (Tauri NSIS)

Standard Tauri 2 NSIS bundle: no custom NSIS template in the repository. The updater runs it with `/P /R` (passive progress window, then reopen CarlaOS; code comment: "shows a small progress window and opens CarlaOS again when it is done"). It may ask whether to remove an installed version first; either answer is fine (`docs/RELEASES.md`). WebView2 is not bundled.

**UNVERIFIED:** the exact NSIS behaviours inherited from Tauri's default template (uninstall page options, handling of the application data folder). Confirm by running the installer and uninstaller interactively on a throwaway machine.

### 6.2 Desktop host installer (`packaging/host.nsi`)

- Defines: `PRODUCT "CarlaOS Desktop Host"`, `EXE "carla-companion.exe"`, uninstall key `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\CarlaOS Desktop Host`, start-up value `HKCU\...\Run` name `CarlaOS Companion`. Requires `VERSION`, `COMPANION`, `ICON`, `OUTFILE` defines (passed by `package.mjs`).
- Install section: decides `Updating` = a `Run` value already exists (the companion was set up before); `taskkill /F /IM carla-companion.exe` (a running companion keeps its file locked; its settings are on disk; a stream in progress keeps running); copies `carla-companion.exe` and `carlaos.ico`; writes `uninstall.exe`; creates shortcuts (Start menu "CarlaOS Desktop Host", "CarlaOS Dashboard" in the Start menu and on the Desktop, "Update CarlaOS Desktop Host" which runs `carla-companion update`); writes the Uninstall registry values (`DisplayName`, `DisplayVersion`, `Publisher`, `InstallLocation`, `DisplayIcon`, `UninstallString`, `QuietUninstallString`, `NoModify`, `NoRepair`).
- When updating: rewrites the `Run` value to `conhost.exe --headless "<install dir>\carla-companion.exe" serve` and starts the companion in the background right away so the Ally "does not lose the desktop until the next sign-in".
- A fresh install **does not** register anything to start with Windows (the smoke test checks this); setup does that when the administrator runs the companion.
- Finish page: a tick box "Set up this desktop and show a code for linking the Ally" (unticked after an update).
- Uninstall: runs `carla-companion remove` (stop starting with Windows), deletes the `Run` value, kills the process, deletes program files, shortcuts and the Uninstall key. **It leaves the companion's data folder alone** (chapter 07).
- Silent: `/S` (used by the updater and by `full.nsi`).

### 6.3 Full installer (`packaging/full.nsi`)

Carries both installers inside, extracts the chosen ones to `$PLUGINSDIR` and runs each with `/S`; aborts with a message box if one returns non-zero. Two sections (`CarlaOS for the Ally`, `CarlaOS Desktop Host`), both ticked by default. The finish page's two boxes are "Set up this machine as a desktop host and show a code for the Ally" (runs `carla-companion.exe` from the host's recorded `InstallLocation`) and "Open CarlaOS" (runs the program recorded as the Ally's `DisplayIcon`, which is why the smoke test insists that value is the program). The result is identical to using the separate installers, "and updates either way".

### 6.4 The installer smoke test (`packaging/smoke-test.ps1`)

Run by CI on the throwaway runner; "safe on a real one too, but it does install CarlaOS". Parameters: `-Version`, `-Dir`, `-NoHost`. Steps:

1. **`-NoHost`:** silently install the Ally installer; assert the Uninstall entry `CarlaOS` exists with `DisplayVersion == Version` and `DisplayIcon` points at an existing program. Done.
2. Otherwise: silently install the **full** installer; assert both halves are listed with the right version; run `carla-companion.exe info` and check it reports the stamped version; assert a fresh install created **no** `Run\CarlaOS Companion` value.
3. **Update path:** create the `Run` key if the runner lacks it; set `Run\CarlaOS Companion` to a bogus path (`C:\elsewhere\...`); run the host installer silently over the install; assert the value now points at the installed copy with `conhost.exe --headless "<exe>" serve`.
4. **Uninstall:** run the host's `uninstall.exe /S _?=<folder>` (`_?=` makes NSIS wait instead of handing over to a copy); assert the program, the Uninstall entry and the `Run` value are gone.

What it does **not** prove (stated in `docs/RELEASES.md`, "Not yet tried by a person"): nobody has clicked through the installers on a real PC, and no real machine has updated itself through the updater.

---

## 7. The release pipeline in detail

### 7.1 Principles

- **Every merge to `main` publishes a beta**, automatically. There is nothing to run by hand. Older versions are never removed.
- **The git tag is the only source of a version number.** The `0.1.0` in `Cargo.toml`, `Cargo.lock` and `tauri.conf.json` is a placeholder; the build stamps the real number in. *Do not edit version numbers and do not create tags* (`docs/RELEASES.md`, `CLAUDE.md`).
- Version format: `0.<feature>.<fix>` (three numbers). `1.0.0` is reserved for the day CarlaOS stops being a beta and is published by hand.
- A merge to `main` is a release. Choose the PR title and labels with that in mind before merging (7.3).

### 7.2 `scripts/release/plan.sh`: how a run decides

Inputs from the workflow: `GITHUB_EVENT_NAME`, `GITHUB_REF`, `GITHUB_SHA`, `GH_TOKEN`, `IN_VERSION`, `IN_REF`, `IN_PREVIOUS`, `IN_HOST`. Outputs (to `GITHUB_OUTPUT`): `version`, `release`, `republish`, `ref`, `target`, `previous`, `host`, `tests`.

Common steps:

1. `latest` = the highest tag matching `v[0-9]*.[0-9]*.[0-9]*` by version sort (`git tag -l ... --sort=-v:refname`). Note: tags are not required to be reachable from the current commit.
2. `next_version <commit>`:
   - If there is no tag: `0.1.0`.
   - Else list the merged PRs behind the commits in `latest..<commit>` with `changes.sh` (7.5): one row per PR `number <tab> labels <tab> title`.
   - **fix** = labels contain `fix` or `bug` (comma-separated, case-insensitive) **or** the title, lower-cased, starts with `fix` (the awk test is `^fix`, so a title such as "Fixture ..." would also count; **UNVERIFIED:** whether that matters in practice).
   - **skipped** = labels contain `no-release`.
   - If there is at least one change and **all** are fixes: patch + 1 (`0.5.0` to `0.5.1`). Otherwise minor + 1 and patch 0 (`0.4.3` to `0.5.0`). A single non-fix PR makes the whole batch a feature version.
   - If there is at least one change and **all** are `no-release`: `skip=true`.
   - **Unresolved commits** (a commit whose PR could not be found, shown as `?<tab>-<tab>Change <sha>: pull request not found`) are counted as normal changes: neither fix nor no-release. That is deliberate (7.5).
3. Per event:

| Event | Behaviour |
| --- | --- |
| `pull_request` (and anything else) | Computes the version the PR would get. `release=false`. Nothing is published |
| `push` to `main` | `ref = target = GITHUB_SHA`. If the commit already has a `v*` tag: "This commit is already released", `release=false`. Else if `skip`: builds without releasing. Else `release=true` |
| `workflow_dispatch`, no `version` | Release `main` now under the next number (only if `GITHUB_REF` is `refs/heads/main` and the commit is untagged); otherwise just a build |
| `workflow_dispatch`, with `version` | Publish exactly that version: checks it is `N.N.N`; if `ref` is given it must resolve (commit, tag or branch) and then `tests=false`; the commit must be an ancestor of `origin/main` ("Releases only ever come from what is on main"); if the tag already exists it must point at the same commit and then `republish=true`; `previous` defaults to the tag just below it, or `IN_PREVIOUS`; `host=false` if unticked; `release=true` |

### 7.3 What an author must do (PR title and label rules)

| Intended release | What to set on the PR before merging | Result |
| --- | --- | --- |
| New feature (normal case) | Any title | Minor + 1, patch reset (`0.4.1` to `0.5.0`) |
| Fix only | Title starts with `Fix`, or label `fix` or `bug` | Patch + 1 (`0.5.0` to `0.5.1`), **only if every PR merged since the last release is a fix** |
| No release (the change ships with the next release) | Label `no-release` | If every PR since the last release has it, the run still builds and tests but publishes nothing |
| Documentation only | Change only `*.md` and/or `docs/**` | The push does not trigger the workflow at all (paths-ignore); ships with the next release |

Details that bite:

- The decision uses **all** PRs merged since the last tag, not just yours. If a feature PR merged earlier is still unreleased (for example the build failed), your `Fix...` PR is released as a *feature* version together with it.
- A `no-release` PR merged together with an unreleased normal PR is released with it.
- The title is public: `notes.sh` prints one line per PR using the PR title ("say what the player or administrator gets, in plain words"), stripping `<`, `>` and spacing out `@`.
- Merging publishes. Say in your report which version resulted (`CLAUDE.md`, "Autonomous integration").

### 7.4 `scripts/release/stamp-version.mjs`

`node stamp-version.mjs <version> [repo root]` rewrites the placeholder in exactly three places, and exits non-zero (with a message) if the version is not `^\d+\.\d+\.\d+$` or a target has nothing to replace:

1. `Cargo.toml`: `version` under `[workspace.package]` (every crate inherits it).
2. `Cargo.lock`: for each `[[package]]` entry **without** a `source` line (our own crates), set `version`. They must match `Cargo.toml` "or `cargo build --locked` refuses to build".
3. `apps/shell/src-tauri/tauri.conf.json`: the top-level `"version"` (installer and window version).

`apps/shell/package.json` also says `0.1.0` but is **not** stamped. The program's reported version is `carla_core::VERSION = env!("CARGO_PKG_VERSION")` (`crates/core/src/lib.rs`), so Settings, `carla-companion info`, the crash reports and the updater all see the stamped number. Because stamping edits tracked files **in the CI workspace only**, the repository never contains a release number.

Running it locally changes three tracked files: use a scratch clone or `git checkout -- Cargo.toml Cargo.lock apps/shell/src-tauri/tauri.conf.json` afterwards (that restores files you changed yourself; do it only if you have no other edits to them).

### 7.5 `scripts/release/changes.sh`: which PRs are in a release

`changes.sh <previous tag or commit, may be empty> <commit>` lists `git rev-list --max-count=500 <previous>..<commit>`, and for each commit asks GitHub `repos/<repo>/commits/<sha>/pulls` for merged PRs, printing `number<tab>labels<tab>title` (labels joined by comma, `-` for none), de-duplicated by PR number (`sort -u`), oldest first. It asks about **every commit, not only merge commits**, so a PR merged into another PR's branch still gets its line.

The lookup retries (`CHANGES_TRIES`, default 4; `CHANGES_DELAY`, default 4 s times the attempt number) because **GitHub links a just-merged PR to its commits a little late**. A commit that still has no PR is printed as an **unresolved** change and counted as a normal change. History: known issue KI-016 (PR #52): the planner could skip a release (treating a `no-release` batch as all `no-release`) when the lookup of a just-merged PR came back empty. `scripts/release/test-changes.sh` exists to keep that fixed.

### 7.6 `scripts/release/notes.sh`: the release text

Writes Markdown: a beta warning; "Which file do I need?" table with links to the three installers (or only the Ally installer when `host` is false, with a note that this version predates the desktop host); notes about the unsigned warning, updating or going back, and the releases page; "What is new" with one bullet per PR from `changes.sh` (`- <title> ([#n](.../pull/n))`; unresolved rows are bullets without a link; `- Changes made directly, without a pull request.` if none); and a compare link `previous...v<version>`.

### 7.7 Publishing by hand (rare)

Actions, Build, Run workflow (permission settings deny `gh release create/edit/upload` to agents in `.claude/settings.json`, so this is a human action):

- Nothing filled in, on `main`: release what is on `main` now under the next number.
- Version and ref: build that commit of `main` and publish it under that number (used for 0.1.0 to 0.4.0). `previous` says where its change list starts; untick "Include the desktop host" for versions that had none.
- A version that already exists on the same commit: rebuild and replace its files and text (repair).

A release can only be built from a commit on `main`.

### 7.8 What a release contains and where it goes

See section 6 for the files. Each is attached to a GitHub **pre-release** with the tag `v<version>`, title `CarlaOS <version> (beta)`, and labels "For the Ally: ...", "For a desktop PC: ...", "Both, for testing: ...", "Checksums: SHA256SUMS.txt".

---

## 8. Updates from inside CarlaOS (high level)

Code: `crates/core/src/update.rs` (shared), `apps/shell/src-tauri/src/updates.rs` (Ally), `crates/companion/src/main.rs` and `server.rs` (host). Documented for people in `docs/RELEASES.md` ("Updating from inside CarlaOS").

| Aspect | Behaviour |
| --- | --- |
| Source | Only this repository's releases: `REPOSITORY = "timotheerm/carlaOS"`. Downloads must start with `https://github.com/<repo>/releases/download/` and contain no `..` (`ours()`), compared case-insensitively |
| Choosing | Parses the releases list; ignores drafts; takes the **highest version above the program's own that has an installer for its half** (`CarlaOS-Ally-<v>-setup.exe` or `CarlaOS-Desktop-Host-<v>-setup.exe`). Betas count (until 1.0 everything is one). A version it cannot parse (a local build) counts as older than everything, so a developer's build is always offered an update |
| Verifying | Downloads the installer and `SHA256SUMS.txt`, and runs the installer only if its SHA-256 matches. Installer size must be at most `LARGEST_INSTALLER` (400 MB). Beta builds are unsigned, so the checksum "protects against a broken download, not against someone who controls the GitHub repository" |
| Where | `%TEMP%\CarlaOS-update` (`download_dir`) |
| Installing | Ally: runs the installer with `/P /R` (passive, then relaunch). Host: `/S` (silent; it restarts the companion itself). Detached (`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`) so stopping the caller does not kill the installer |
| Never | Never goes back to an older version: for that, run an older installer by hand |
| Who triggers | Ally: Settings, Updates (PIN-gated: `update_install` calls `admin.require()`), re-checking GitHub server-side so the page cannot request anything but our releases. On a machine with both halves the host is updated first (its installer works unseen) and the Ally's last (it ends the running program). A linked desktop can be told to update from the Ally; it refuses while a game runs there. On a desktop: Start menu "Update CarlaOS Desktop Host" or `carla-companion update` |
| `desktop_host_here` | Finds a desktop host installed on the same machine through its Uninstall registry key (`DisplayVersion`, `InstallLocation`), which is how the Ally interface notices a full install |

Things to remember when changing release assets: the asset **names** are part of the updater's contract (`Part::installer`) and `notes.sh`, `package.mjs`, `build.yml`'s upload list and `smoke-test.ps1` all spell them. Rename one and you break updating for installed copies until they are reinstalled by hand. A desktop host from 0.6.0 or older has no updater and must be updated once by hand.

---

## 9. Branch, PR and merge rules (summary)

`CLAUDE.md` is authoritative; read its "Non-negotiable Git rules", "Autonomous integration" and "Explicit user approval" sections before acting. Highlights an implementer must not violate:

- Never commit to `main`; every task has an explicit branch (`feat/...`, `fix/...`, `docs/...`) from the latest `origin/main` (`git fetch origin --prune` first) and a known base commit.
- One PR per coherent feature or fix. Inspect all open PRs and their head/base before opening one. Dependent work goes on the same branch, or a deliberately **stacked PR** whose base is the parent branch; never a `main`-targeted PR that depends on an unmerged PR. Merge bottom-up; after a parent merges, retarget the child to `main`, rebase, re-run checks.
- Use a **worktree per parallel task**; never two sessions in one working directory or two writers on one file (`docs/GIT_WORKFLOW.md`). See 10.2 about `CARGO_TARGET_DIR`.
- Before a PR: `git status`, `git diff --check`, inspect the diff, run the relevant checks. PR body: summary, tests run, limitations, dependencies. Do not claim Windows hardware validation unless it ran on Windows hardware.
- Merge only when every CI check has finished and passed (`gh pr checks N`), the PR is mergeable and not a draft, the diff contains only the task, and it is not in an approval-gated area. The repository uses **merge commits**: `gh pr merge N --merge`. Do not poll CI; wait with one background command.
- Needs explicit user approval: destructive operations, Windows/device configuration changes (including running `windows_live`), security-sensitive changes (auth, companion/admin protocol, secrets storage, update/signing, network exposure), anything touching credentials, tags, version numbers, manual releases, force pushes or history rewriting.
- Shared interfaces: the link protocol (`crates/core/src/link`), Tauri command signatures and the SQLite schema. A change to any of them must be agreed first.
- Inconsistency to be aware of: `docs/LOCAL_SETUP_WINDOWS.md` says "merge with the repository's normal squash strategy", whereas `CLAUDE.md` says the repo uses merge commits (`gh pr merge N --merge`). Follow `CLAUDE.md`. `changes.sh` inspects every commit, so it works with either, but a squash would lose the per-commit view. **UNVERIFIED:** the repository's actual merge settings; confirm with `gh repo view --json mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed`.

---

## 10. Known build and test pitfalls

### 10.1 Toolchain

- **No MSVC / wrong shell:** link errors in Git Bash or a plain PowerShell (section 2.1, KI-006). Fix: VS developer shell. Check with `where.exe link`.
- **`cargo test` at the root runs `windows_live`** on Windows. Use `--lib --bins`.
- **Shell tests fail to compile without `apps/shell/dist`:** build the UI first. `scripts/check-windows.sh` creates an empty `dist` for its own purpose; CI gets it from `npx tauri build`.
- **`--locked` failures:** `Cargo.lock` out of step with the manifests; or you stamped a version but not the lock (`stamp-version.mjs` does both).
- **Node too old** for `stripTypeScriptTypes` in `scripts/test-inflight.mjs` (section 2).

### 10.2 Shared `CARGO_TARGET_DIR` between worktrees

If several worktrees (one per parallel task, `docs/GIT_WORKFLOW.md`) are pointed at one shared `CARGO_TARGET_DIR`, Cargo's fingerprints can treat a build made from another worktree's sources as up to date, so you can get **stale results**: tests that ran another branch's code, or a pass that is not yours. Rules of thumb: give each worktree its own target directory (the default `target/` inside the worktree) or run `cargo clean -p <crate>` when switching; always confirm a surprising pass or failure by re-running in a clean target before believing it. **UNVERIFIED:** this repository contains no documentation of such an incident; it is reported by the project's coordinators. Confirm by comparing results of the same commit in a worktree with its own `target/`.

Related: the first build in a new worktree recompiles everything (long); that cost is the price of isolation.

### 10.3 Line endings

- `git config core.autocrlf` is `true` on the developer machine (from `C:/Program Files/Git/etc/gitconfig`, i.e. the Git for Windows default). There is **no `.gitattributes`** in the repository. Result: the index (and CI's Linux checkout) hold **LF**, the Windows working copy holds **CRLF** (`git ls-files --eol` shows `i/lf w/crlf` for `Cargo.toml`, `scripts/release/plan.sh`, `packaging/host.nsi` and `crates/companion/src/dashboard.html`).
- Consequence for scripts: a CRLF `scripts/release/*.sh` run by Git Bash from a Windows working copy sees `\r` at line ends and typically fails with errors such as `$'\r': command not found`. **UNVERIFIED:** that Git Bash here fails on `plan.sh` exactly so; confirm by running `bash scripts/release/test-changes.sh` in a Windows working copy. CI is unaffected (Linux checkout is LF). Workaround: run such scripts from a Linux/WSL checkout, or `dos2unix` a scratch copy, or `git -c core.autocrlf=input` clone; do not commit CRLF versions.
- Code that reads repo files must tolerate both: `scripts/test-dashboard.mjs` does `html.split('\r\n').join('\n')`.
- `git diff --check` after editing, and never convert whole files' line endings in a feature PR.
- Never write `.sh` files with CRLF into the index: Linux CI would fail on them. Editors that preserve the existing ending are safe.

### 10.4 Flaky tests and fixes (history)

| Date and ref | Symptom | Root cause | Fix |
| --- | --- | --- | --- |
| KI-009, PR #45 | `server::tests::a_link_that_is_refused_at_the_desktop_links_nothing` failed once | Two tests used `desk("refuse")`; the helper's state folder was shared and wiped in parallel | Test-only: unique state folder per test (the tests build a folder name from the process id plus a counter, e.g. `DESKS.fetch_add(1, ...)`). 10/10 passes after |
| `82da4fd` | The silent-client server test failed once on the Linux runner | Without socket timeouts (everywhere but Windows) each silent client keeps a connection thread of the HTTP library on the server | Fewer silent clients on non-Windows and a generous bound for the second client's answer |
| KI-016, PR #52 | A release was skipped | GitHub had not linked a just-merged PR to its commit yet, so the planner saw "no PR" rows and counted a `no-release` batch as all `no-release` | `changes.sh` retries and counts an unresolved commit as a normal change; `test-changes.sh` covers it |
| `3fbf01d` (after #34 in 0.20) | The Ally froze every 45 s after a desktop answered | `hosts::watch` iterated over `library.store().hosts()`; the temporary guard lived for the whole `for`, and an inner call locked the same `Mutex` again | Copy the list out first; five similar `if let`/`match` headers fixed; CI runs the shell's tests and the clippy lint (3.4) |

General lessons:

- Tests that use `std::env::temp_dir()` must make folder names unique **within** the process (process id alone is not enough, because tests run in parallel threads of one process). Existing tests add a counter or a test name.
- Tests that rely on timing (sockets, silent clients) must leave generous bounds; shared CI runners are slow.
- A `CARLA_COMPANION_DIR`-style override is how a test isolates a state folder; do not write to the real data directories in tests.
- Anything touching Windows (`windows_live`, registry, DPAPI) is exercised only on Windows CI. Linux CI is green even if Windows-only code is broken; check `windows` job results, not just `check`.

### 10.5 Release pipeline traps

- Builds of `main` are serialised and the middle ones can be superseded (5.2).
- A docs-only push does not run the workflow, so it cannot repair a broken release; but a *PR* of docs still runs the PR checks.
- If the `windows` job fails after `check` passed, nothing is published. Re-running the workflow on the same `main` commit (Actions, Re-run) re-plans: the version is recomputed from the tags at that moment.
- Do not push a tag by hand; `plan.sh` considers the highest tag as the last release, so a stray tag will shift all later numbers.
- `workflow_dispatch` with a version already tagged on a different commit stops with an error.

---

## 11. Quick reference

| I want to... | Do |
| --- | --- |
| Check my Rust change fast | `cargo test --locked -p carla-core --lib` (VS shell) |
| Run the standard local checks | `cargo test --locked --lib --bins`, then `git diff --check` |
| Check the UI | `cd apps\shell; npm ci; npm run build` |
| Run the app | `cd apps\shell; npx tauri dev` |
| Test the shell crate | `npm run build` (in `apps/shell`), then `cargo test --locked -p carlaos-shell --lib --bins` |
| Run the enforced lint | `cargo clippy --locked -p carlaos-shell -- -A clippy::all -D clippy::significant_drop_in_scrutinee` |
| Type-check Windows code from Linux | `bash scripts/check-windows.sh` |
| Test the release scripts | `bash scripts/release/test-changes.sh` (LF checkout) |
| Make a fix release | Merge PR(s) all titled `Fix...` or labelled `fix`/`bug` |
| Make a change that does not release | Label `no-release` on every unreleased PR, or change only `*.md`/`docs/**` |
| Get installers from a PR | Download the `carlaos-installers` artifact of the PR's Build run |
| Never | Edit versions, create tags, run `windows_live` without approval, force-push, commit to `main`, change `identifier` |

---

## 12. Open questions / needs investigation

1. **Node on the Linux `check` job:** `cargo test` (which runs the dashboard script test) runs before `setup-node`; which Node does it use? (5.4)
2. **Minimum Node 22 minor** for `scripts/test-inflight.mjs` (2).
3. Whether the companion's port can be changed in practice (it is a `config.json` field, default 47600).
4. **`scripts/check-windows.sh` on a Windows host:** documented for Linux/macOS; does it work from Git Bash with MSVC installed? (4.3)
5. **Fix detection regex** `^fix` matches any title starting with "fix" (for example "Fixture"); acceptable? (7.2)
6. **Merge strategy mismatch** between `docs/LOCAL_SETUP_WINDOWS.md` (squash) and `CLAUDE.md` (merge commits); check the repository settings and fix the stale doc. (9)
7. **Shared `CARGO_TARGET_DIR` stale results:** no in-repo record; document the exact failing scenario once reproduced. (10.2)
8. **CRLF working copies running `scripts/release/*.sh`:** confirm failure mode and decide whether to add a `.gitattributes` (`*.sh text eol=lf`) to protect scripts. (10.3)
9. **Tauri NSIS defaults** (uninstall options, application data handling, `%LOCALAPPDATA%\CarlaOS` as install folder) are inherited, not read from this repository. (6.1)
10. **`cargo fmt` policy** (KI-018) is still undecided.
11. **Frontend tests** do not exist (KI-005); only `tsc` and `vite build` plus one Node script.
12. **Unsigned installers** (KI-004) and the real-machine updater run are still outstanding (`docs/RELEASES.md`, "Not yet tried by a person").
13. **Updater cleanup:** are downloaded installers removed from `%TEMP%\CarlaOS-update` after install? (also in chapter 07)
