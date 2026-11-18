# 09 · How-to recipes

Step-by-step recipes for the changes people make most often. Each recipe names the files to
touch, in order, and the checks to run. Read [01-architecture-and-runtime.md](01-architecture-and-runtime.md)
first (threads, locks) and [10-pitfalls-and-open-questions.md](10-pitfalls-and-open-questions.md)
before you open a PR. The deep reference for each area is in chapters 02–08.

General rules that apply to every recipe:

- **Never hold `library.store()` across network, process or Windows calls**, and never build a
  `MutexGuard` in a `for`/`if let`/`while let`/`match` header. Read into a local first.
- Errors the player sees are **one plain line** that says what to do. Detail goes to the logs.
- Anything that can change Windows or device configuration, touch credentials, or delete user
  data needs the owner's explicit approval (see `CLAUDE.md`).
- New protocol fields are **optional** (`#[serde(default)]`, often `skip_serializing_if`), so an
  older Ally or desktop keeps working.
- Add tests next to the code (`#[cfg(test)] mod tests`). Shell tests run with
  `cargo test --locked -p carlaos-shell`; core/companion with
  `cargo test --locked -p carla-core -p carla-companion --lib --bins`.
- Files in the working copy are often CRLF (`core.autocrlf=true`). Edit with tools that keep line
  endings; scripted edits must normalise `\r\n` before matching and restore it after.

---

## 1. Add a command the interface can call (Ally)

1. **Write the function** in the module it belongs to under `apps/shell/src-tauri/src/`
   (for example `hosts.rs`, `library.rs`, `details.rs`). Signature pattern:
   ```rust
   #[tauri::command(async)]
   pub fn hosts_do_thing(admin: State<'_, Admin>, library: State<'_, Library>, id: String) -> Result<Thing, String> {
       admin.require()?;                          // only if it changes administrator settings
       let host = library.store().hosts()?.into_iter().find(|h| h.id == id).ok_or("That desktop is no longer set up.")?;
       // network / process work here, with NO store guard alive
       ...
   }
   ```
   - Use `#[tauri::command(async)]` for **anything** that touches the network, a process, the
     store lock or the session slot. A plain `#[tauri::command]` runs on the UI thread; keep
     those for pure, instant work.
   - Return `Result<T, String>` where the `String` is the one-line message for the player.
   - Admin-only actions call `admin.require()?` first (`apps/shell/src-tauri/src/admin.rs`).
2. **Register it** in `tauri::generate_handler![...]` in `apps/shell/src-tauri/src/main.rs`.
3. **Expose it to the interface** in `apps/shell/src/native.ts` (or the state store in
   `apps/shell/src/library/store.ts`), with a browser-preview fallback when `!inTauri`, following
   the existing objects (`boot`, `crash`, `hosts`...).
4. **Call it from a screen** (see recipe 3).
5. Checks: `cargo check --locked -p carlaos-shell --all-targets`, `cargo test --locked -p carlaos-shell`,
   `npm run build` in `apps/shell`, the clippy lock lint (see [08](08-build-test-release.md)).

## 2. Add a background worker (Ally)

1. Write `pub fn watch(library: Library)` (add `app: AppHandle` only if it must emit events) in its
   module that spawns **one** thread
   with a `loop { sleep(INTERVAL); one_round(&library) }`.
2. Put the round in its own function taking what it needs as parameters (for example a `reach`
   function), so a test can drive one round with stand-ins. `hosts::pulse_all` is the model.
3. Inside the round: copy lists out of the store first; do network work with no lock held; write
   results back with short `library.store()` calls.
4. Skip or lighten work while a game runs: check `locked(&library.0.active).is_some()`.
5. Start it in `main.rs` `setup`.
6. Add a test that runs one round and asserts it finishes and leaves the library usable.

## 3. Add a screen or a setting (Ally interface)

1. Create `apps/shell/src/screens/<name>.tsx`. Reuse parts from `apps/shell/src/ui/` and follow an
   existing small screen (for example `screens/crash.tsx`, `screens/startup.tsx`).
2. Make every interactive element reachable by controller: `data-focus` plus a stable
   `data-fid="<area>:<item>"`, the way the other screens do. See `apps/shell/src/nav/focus.ts`.
3. Add the page kind to the page stack in `apps/shell/src/App.tsx` (the `Page` union and the
   `page = ...` routing) and an entry where it is opened (for Settings, the list in
   `apps/shell/src/screens/screens.tsx`).
4. Native calls go through `native.ts` / `store.ts` (recipe 1). Text is rendered as text, never
   as HTML.
5. **Never do anything that can throw before the first render** (`main.tsx` top level). A thrown
   error there is a white screen (that happened in 0.33.0).
6. Check: `npm run build` (TypeScript + Vite). There is no frontend unit-test runner; small node
   scripts under `scripts/` test isolated pieces (`scripts/test-inflight.mjs`).

Details: [05-ally-ui.md](05-ally-ui.md).

## 4. Add a column or table to the Ally's catalogue

1. Tables are created with `CREATE TABLE IF NOT EXISTS` in the schema that
   `Store::prepare` runs (`crates/core/src/library/store.rs`; `Store::open` calls it). New tables
   go there.
2. New columns go in the migration list in `Store::prepare` as `("table", "column", "TYPE ...")`
   tuples; the code adds a column only when it is missing, so it is idempotent. Give it a
   default (`NOT NULL DEFAULT ...`) or make it nullable; old rows must stay valid.
3. Add `Store` methods to read/write it; keep each call a single short statement.
4. Test opening a database **created before** the change (create the old schema in a temp file,
   open it with `Store::open`, use the new column). Several existing tests do exactly this.
5. Document it in [07-data-and-storage.md](07-data-and-storage.md).

## 5. Add a route to the desktop protocol

Both sides change; the protocol must stay compatible with older peers.

1. **Types**: request/response structs in `crates/core/src/link/mod.rs`. Every new field
   `#[serde(default)]`; never rename or remove a field without a migration plan.
2. **Desktop handler**: in `crates/companion/src/server.rs`, add the arm to `Server::signed`
   (`match (req.method, req.path)`), for example
   `("POST", "/v1/thing") => self.thing(device, req.body, now),`. Signed routes are already
   authenticated (the device's key, a nonce, a time window) before they reach this match;
   loopback-only dashboard routes go through the `/local` handler in
   `crates/companion/src/dashboard.rs` with its Host and `x-carla-token` checks.
   Slow routes must be listed among the "long" routes so they get a long slot and a longer body
   deadline (see [03](03-desktop-host.md)).
3. **Route shape in the log**: add the path to `route_shape` in `crates/companion/src/server.rs`
   so ids and keys are masked in `host.log` (`/v1/saves/:folder/write`); it has its own tests.
4. **Client**: a method on `Link` in `crates/core/src/link/client.rs`, using `self.post(path, &body, TIMEOUT)`
   or `self.ask("GET", path, &[], TIMEOUT)` with a timeout constant (`QUICK` 3 s, `SLOW` 40 s). Handle an older desktop
   answering 404 as "not supported", not as an error the player sees.
5. **Tests**: in `server.rs` tests, use the `desk()` / `desk_with()` harness (a real companion on
   a free port) and a real `Link`. Cover: signed success, unsigned/unknown device refused, wrong
   key refused, an older peer (missing field / 404).
6. Document it in [06-link-protocol.md](06-link-protocol.md). Adding a route is a protocol
   change: it needs the owner's approval when it is security-sensitive (auth, admin, deletion).

## 6. Add a `carla-companion` CLI command (desktop)

1. Add the dispatch arm in `crates/companion/src/main.rs` (`match` on the first argument, e.g.
   `Some("describe") => describe::run(&files, args)`). Larger commands get their own module
   (`describe.rs`, `logview.rs`).
2. Parse arguments by hand like the neighbouring commands; print plain sentences; exit non-zero
   on failure. Anything destructive asks for `--yes` (see `saves clean`, `saves accept`).
3. Never print a line meant for pasting into a terminal unless every character is safe to paste
   (see `crates/core/src/library/find.rs` `add_line` / `safe_to_paste`).
4. Add parsing tests next to the command; update the help text (`HELP` in `main.rs`) and the
   CLI reference in [03-desktop-host.md](03-desktop-host.md).

## 7. Add a dashboard section or button (desktop)

1. Data: add fields to the dashboard's overview (`crates/companion/src/dashboard.rs`).
2. Page: `crates/companion/src/dashboard.html`. Build elements with the page's `h()` helper (text
   only, never `innerHTML` with data).
3. Actions: `POST /local/<action>` handled in `dashboard.rs` with the same Host and token checks
   as the others; accept **no paths or arguments from the browser** (use ids the server itself
   produced, re-checked when used).
4. In click handlers, **take `event.currentTarget` and anything derived from it before the first
   `await`**; a browser clears it after dispatch (this broke "Add a game" once).
5. Run `node scripts/test-dashboard.mjs` (also run by a companion test when node is installed)
   and the dashboard refusal tests.

## 8. Start a program (either side)

- Use `std::process::Command` with **null stdio** (`Stdio::null()` for stdin/stdout/stderr) and,
  on Windows, `CREATE_NO_WINDOW` where no window is wanted. On the desktop this is mandatory in
  anything the launcher can reach (see `crates/core/src/library/launch.rs` `quiet()`).
- Never run a program found during discovery; discovery only reads files.
- Never interpolate user-controlled text into a shell command line; pass arguments as separate
  `arg()`s or through environment variables (see `open_program` in `launch.rs`).

## 9. Write to the logs

- Ally: `crate::crash::note("...")` for the crash trail (short, plain), `crate::conn_log::line(...)`
  for the connection log. Desktop: `files.host("...")` for `host.log`, `files.log(...)` for the
  launcher log.
- Never log keys, PINs, tokens, signatures, request bodies, save contents, or personal paths.
  `carla_core::crash::scrub` hides IPv4/IPv6 addresses and the Windows account name; computer
  names are hidden in the Ally's logs (`conn_log::know_name`).

## 10. Ship a change

1. Branch from `origin/main` (`feat/…`, `fix/…`, `docs/…`); one PR per coherent change.
2. Run the checks for what you touched (above) and `git diff --check`.
3. PR body: summary, tests run, limitations, dependencies. Title starting with **Fix** (or label
   `fix`/`bug`) makes the merge a fix release; label `no-release` (or docs-only changes) publishes
   nothing; anything else is a feature release.
4. Merging to `main` publishes a beta automatically. Do not tag or edit versions by hand.
   Details: [08-build-test-release.md](08-build-test-release.md), `docs/RELEASES.md`, `CLAUDE.md`.
5. Do not claim a change works on the Ally or a desktop unless it ran there.

## Open questions / needs investigation

- There is no shared test harness for driving the Ally's Tauri commands end to end; recipes rely on
  extracting pure functions and testing those.
