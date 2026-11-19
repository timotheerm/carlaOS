# 05 - The Ally interface (React / TypeScript)

Source: `apps/shell/src` (entry `index.html` -> `src/main.tsx`), built by Vite, shown inside the Tauri web view
described in chapter 04. This chapter covers the page structure, the single shared state store, the bridge to
Rust, controller/touch/keyboard navigation, every screen, shared components, theming, the browser preview, and
step-by-step recipes for the common changes.

All facts come from reading `apps/shell/src/**`, `apps/shell/package.json`, `vite.config.ts`, `tsconfig.json`
and `index.html`. Items that code alone cannot prove are marked **UNVERIFIED:** with what would confirm them.
Nothing in this chapter was run on the device.

---

## 1. Toolchain and ground rules

| Fact | Detail |
| --- | --- |
| Stack | React 19, TypeScript (`strict`, `noUnusedLocals`, `noUnusedParameters`), Vite, `@tauri-apps/api` v2. No router, no state library, no CSS framework, no UI kit |
| Build | `npm run build` in `apps/shell` = `tsc --noEmit && vite build` (output `apps/shell/dist`, which `tauri.conf.json` serves as `frontendDist`). **This is the only automated check of the UI code**: type errors and unused locals/params fail it |
| Dev | `npm run dev` = Vite on `http://localhost:1420` (`strictPort`). Opened in a normal browser the app runs in **preview mode** (section 9): no Tauri, sample data |
| Tests | **There is no frontend test runner** (no Jest/Vitest/Playwright dependency). The one automated JS check is `node scripts/test-inflight.mjs` (CI runs it) which exercises `src/inflight.ts` behaviour. Anything else must be checked by building and by looking at it (browser preview, then the Ally) |
| Language | User-facing text is plain English, short sentences, British spelling in places ("favourite"), no emoji in the interface; errors from Rust are already sentences and are shown as they come |
| Units | `html { font-size: calc(100vw / 120) }`: 1 rem = 16 px on the Ally's 1920 px screen and scales with the window. Use `rem` for sizes |
| Screen | The target is a 1920x1080, 7-inch handheld used by controller, touch or keyboard. Everything focusable must work with all three |

Two windows run this same bundle: the console (`<App />`) and the quick menu over a game (`<QuickMenu />`,
selected by `location.hash === "#quick"`). See section 12.

---

## 2. Boot and render (`main.tsx`)

```
createRoot(#root).render(<StrictMode> { #quick ? <QuickMenu/> : <App/> } </StrictMode>)
```

then, after render is *scheduled* (these lines run right after `render(...)` returns):

1. `window.addEventListener("error", ...)` and `"unhandledrejection"`: `note(kind, what)` sends one line to the Rust crash
   trail (`crash.note`) - the message only, URLs cut at `?`/`#`, clipped to 200 characters, wrapped in `try/catch` so it can
   never throw.
2. Heartbeat: `crash.alive(document.hidden)` immediately, every 3 s, and on `visibilitychange`. Rust's watchdog judges the
   window from it (chapter 04, section 15). The page reports whether it is hidden so a covered window is not mistaken for a
   frozen one.
3. `window.setTimeout(() => watchCommands(), 2000)`: installs the command watcher (section 5) **only after 2 s**.

CSS imports (`tokens.css`, `app.css`) are bundled; there is no other global setup.

### 2.1 The white-screen lesson (read before touching `main.tsx` or `App.tsx` start-up)

Release 0.33.0 shipped a white screen: a diagnostic patched `window.__TAURI_INTERNALS__.invoke` at the top of `main.tsx`, before
the first render, and a failure there stopped the page from starting. Rules that follow:

* **Nothing may be able to throw before the first render.** Do not add code above `createRoot(...).render(...)` that touches
  Tauri internals, localStorage parsing, or anything that can fail. `App.tsx::load` and `sound.ts` wrap `localStorage` in
  `try/catch` for this reason.
* **Diagnostics and monkey-patches are installed after the first screen** (hence the 2 s timer), inside `try/catch`, and must hand every call to the original unchanged.
  `inflight.ts` follows this to the letter: every code path that could fail falls through to the original `invoke`.
* A module that throws at **import time** also gives a white screen (React never mounts). Keep top-level module code in the
  UI side-effect free: building constants is fine, calling Tauri or parsing stored JSON is not unless guarded.
* There is no error boundary component in the app. A render-time exception in `App` or any screen unmounts the whole tree.
  See the hooks hazard in section 8.6.
* Because there is no test runner, a `tsc` pass is not proof that the page boots. Open the built app (or `npm run dev` in a
  browser) and watch the console.

**UNVERIFIED:** the exact exception behind 0.33.0 was never captured (no device logs, see `docs/dev/10-pitfalls-and-open-questions.md` if
present on your branch); the fix made every failure mode harmless rather than proving the cause.

---

## 3. App shell, routing and page stack (`App.tsx`)

There is no router. The app has **sections** (top-level tabs) and a **page stack** (pages pushed on top of a section).

### 3.1 Sections

`SECTIONS` in `ui/chrome.tsx`: `home` ("Home"), `games` ("My Library"), `desktops` ("My Desktops"), `cloud` ("Cloud"),
`settings` ("Settings"). `section` state picks which list screen is the base. LB/RB (or Q/E) cycle through them with wrap-around
(`go()`), and the top bar buttons select them (`data-fid="rail:<id>"`).

### 3.2 Page stack

```ts
type Page =
  | { kind: "game"; id: string } | { kind: "host"; id: string } | { kind: "remove"; id: string } | { kind: "cleanup"; id: CleanupView }
  | { kind: "manage" | "add-games" | "desktops" | "search" | "updates" | "startup" | "crash" | "log" | "storage" | "downloads"; id: "home" };
```

`stack: Page[]`. `top = stack[stack.length-1]`. Rendering is a long `if/else` chain choosing a page by `top?.kind`, falling back to the
section screen. Mapping:

| Page kind | Component (file) | Opened from |
| --- | --- | --- |
| `game` | `GameDetail` (`screens/pregame.tsx`) | any catalogue card/hero (`ctx.openGame`); can add/remove My Library membership |
| `host` | `HostView` (`screens/screens.tsx`) | a `HostCard` (`ctx.openHost`) |
| `remove` | `RemoveGame` (`screens/remove-game.tsx`) | Game details, "Remove from My Library"; local Steam uses PIN + Steam confirmation |
| `search` | `Search` (`screens/search.tsx`) | Y / `/` (`action === "search"`); searches My Library |
| `add-games` | `AddGames` (`screens/add-games.tsx`) | My Library, "Add games"; selects from the full catalogue without installing |

The My Library screen (`Games`) keeps its selection filters and counts visible,
and sorts the same personal collection by last played, name, date added or
playtime. The CSS grid uses an auto-fit minimum card width, so cover art stays
legible when the shell is shown in a narrower window.

Add Games filters the unselected catalogue before applying the selected view.
Its on-screen controller keyboard is collapsible; physical/touch keyboard input
still goes directly to the search field. After an item is added, focus moves to
the next result when one exists, rather than jumping back to the page heading.
| `manage` | `Manage` (`screens/manage.tsx`) | Settings, "Manage games"; registers standalone programs on this Ally |
| `desktops` | `LinkDesktops` (`screens/link.tsx`) | Settings, "Desktops" |
| `updates` | `Updates` (`screens/updates.tsx`) | Settings, "Updates" |
| `startup` | `Startup` (`screens/startup.tsx`) | Settings, "Starting up" |
| `crash` | `Crash` (`screens/crash.tsx`) | Settings, "Crash reports" |
| `log` | `ActivityLog` (`screens/log.tsx`) | Settings, "Activity log" |
| `storage` | `Storage` (`screens/storage.tsx`) | Settings, "Storage" |
| `downloads` | `Downloads` (`screens/downloads.tsx`) | top bar download chip, Home "See every step", Settings |
| `cleanup` | `Cleanup` (`screens/cleanup.tsx`), `id` = `home\|review\|changes\|kept` | Settings, "Clean up Windows"; sub-pages pushed by `open(view)` |

Stack operations:

* `push(page)`: remembers the focused element's `data-fid` in `returnFocus.current` (a parallel array), then appends.
* `back()`: if the stack is empty, goes to the Home section; else pops and sets `pendingFocus` to the remembered fid.
* `go(section)`: clears both the stack and `returnFocus`, sets the section.
* `pageKey` = `"playing"` when a session exists, else `"<top.kind>:<top.id>"`, else the section id. The `<main>` element has
  `key={pageKey}`, so every page change **remounts the whole page tree** (state of the page is lost; state that must survive lives in
  `App`, in the store, or in a module singleton).
* The search text `query` lives in `App` ("so the words are still there when the player comes back from looking at a game").

### 3.3 Special overlays and modes

* **Playing mode:** when `session !== null` (from the store), `App` renders only `Ambient` + `Playing` (+ toast) and ignores all input
  except what the `Playing` screen's two buttons get (`handler` returns early on `playing`). The library, top bar and hints are not
  mounted.
* **Splash** (`ui/splash.tsx`): `splash` state true at start; plays 2.6 s (0.9 s with reduced motion), any key/pointer/pad button skips.
  `endSplash` sets `splash=false`, calls `boot.ready()` (tells Rust this start worked; toasts "CarlaOS failed to start twice, so it left the
  desktop alone once." when `stood_down`), `crash.reports()` (toasts if a **new `panic` or `hang`** report exists; an `unexpected` one
  waits in Settings), and focuses the first element of `<main>`. Input is ignored while the splash is up.
* **Side menu** (`screens/sidemenu.tsx`): `menu` boolean. While open it takes every press: only B or Menu close it.
* **Toast:** `toast` string, cleared 2.6 s after being set. The single feedback channel for errors and confirmations: every `catch`
  calls `setToast(message(e))`. Pass `notify` down for this.

### 3.4 Persisted preferences (localStorage)

| Key | Value | Where |
| --- | --- | --- |
| `carla.theme` | `"light"` or `"dark"` (JSON) | `App`; also read by the quick window |
| `carla.backdrop` | `"full" \| "soft" \| "off"` (JSON) | `App` |
| `carla.sounds` | `"on"`/`"off"` (plain) | `ui/sound.ts` |
| `carla.favourites` | JSON array of ids | **browser preview only** (`store.ts::setFavourite` when not in Tauri) |

`<html>` gets `data-theme` and `data-backdrop` attributes (CSS keys off them). Real favourites are in the Rust catalogue.

### 3.5 The `Ctx` object

Screens receive a `Ctx` (`screens/screens.tsx`) assembled in `App`: `games` (playable personal-library games),
`library` (the complete personal library, including not-installed titles), `catalogue` (all known games, for Add Games and details),
`downloads`, `hosts`, `favourites` (Set), and callbacks including `addToLibrary`, `openAddGames`, `removeGame`,
`install`, `openGame`, `openHost`, `notify`, `play`, `wakeHost`, `openCloud`, `openDownloads`, `openManage`,
`refreshHosts`. Callbacks wrap the store with cue sounds and error toasts. A new
cross-screen action is added here (recipe in section 14.3).

---

## 4. The state store (`library/store.ts`)

A module-level singleton, not React state. `useLibrary()` is `useSyncExternalStore` over it.

### 4.1 Shape and update model

```ts
type State = { games: Game[]; myLibrary: Game[]; hosts: Host[]; session: Session | null; scanning: boolean; downloads: Download[] };
const previewGames = sample();
let state: State = inTauri ? { games: [], myLibrary: [], hosts: [], session: null, scanning: false, downloads: [] }
                           : { games: previewGames, myLibrary: previewGames.filter(g => g.platform === "Steam" && g.places.includes("local")), hosts: sampleHosts, session: null, scanning: false, downloads: previewDownloads() };
function set(patch) { state = { ...state, ...patch }; listeners.forEach(l => l()); }   // immutable replace + notify
```

`useLibrary()` returns the whole `State` object, so every consumer re-renders on any change; derive with `useMemo` (as `App` does for
`games` and `favourites`). Never mutate `state` in place (the store compares by reference).

### 4.2 Data conversion

Rust sends snake_case views; the store converts at the boundary and the rest of the UI sees camelCase types from `data/types.ts`:
`GameView -> Game` (`fromView`: `playtime_minutes -> playtimeMinutes`, `last_played` seconds -> ISO string, `cover`/`hero` file paths
-> `convertFileSrc(path)` URLs for the asset protocol, `hue` computed from the title so every game without art gets a steady colour),
`SessionView -> Session` (`fromSession`), `HostView -> Host` (`fromHost`: `lastSeen` is `"now"` when awake else `when(last_seen)`),
`DownloadView -> Download` (`fromDownload`: `game_id -> gameId`). A few payload types are used raw with snake_case (`Health`,
`HostPicture`, `SaveSync`, `Storage`): follow the existing type, do not rename.

### 4.3 `library.start()` (called once from `App`'s mount effect, idempotent via `started`)

In order:

1. `listen("library://session")`: `set({ session: fromSession(payload) })`; a `null` payload (session over) triggers `reload()` so playtime and
   "recently played" refresh.
2. `listen("library://downloads")`: `set({ downloads })`; then initial `installs_list`.
3. `listen("library://changed")`: `reload()` (administrator changed the library from a desktop, covers arrived, scan finished in the background...).
4. `library_session` (state at start; a session may already be running if the page reloads), `reload()` = `library_list` plus
   `library_personal_list` in parallel. It keeps the full catalogue and My Library as separate views of the same store.
5. `hosts_list` (instant, from the database), then `refreshHosts()` (network, may take 10 s).
6. A **30 s interval** (`HOSTS_EVERY`) that calls `refreshHosts()` only when `session === null`, there are hosts, `document.visibilityState === "visible"`
   and `document.hasFocus()` ("nothing is asked while a game is in front or the screen is off"). The interval is never cleared (app lifetime).
7. `library.scan()` = `library_scan` (full library refresh; may take a long time).

`listen` unsubscribe functions are not kept for the store (singleton for the lifetime of the window).

### 4.4 Timeouts and single-flight

| Mechanism | Value | Why |
| --- | --- | --- |
| `within(promise, ms)` | rejects with `TimedOut` if no answer | "A native call that never answers must not leave a busy flag set for ever" |
| `refreshHosts` | `HOSTS_TIMEOUT` = 15 s around `hosts_refresh` and again around `reload()` | Rust gives up on slow desktops at 10 s (`REFRESH_LIMIT`); the page's 15 s is for when Rust does not answer at all |
| `askingHosts` | single flight | A second `refreshHosts` while one runs returns immediately |
| on `TimedOut` | `set({ hosts: unreachable(state.hosts) })` (every desktop shown as unreachable, "did not answer in time", games stay) and a one-time (`hostsTimedOut`) crash-trail note | "The desktops are not connected any more just because nothing said otherwise" |
| `scan` | 120 s via `within`, `scanning` flag cleared in `finally` | "The spinner must not stay for ever" |
| `wake(id)` | after `hosts_wake`, re-`refreshHosts()` at +8, +16, +28, +45, +70 s while the host is not ready/busy | Waking takes seconds to a minute |
| `sleepHost` | `refreshHosts()` after 8 s | |

### 4.5 Methods of `library` (every wrapper, the command it calls, preview behaviour)

All methods start `if (inTauri) ... else <preview>`. Arguments are passed camelCase (`{ gameId, place }`), Rust maps them to snake_case.

| Method | Command(s) | Preview behaviour |
| --- | --- | --- |
| `start()` | see 4.3 | no-op |
| `refreshHosts()` | `hosts_refresh` then `library_list` | no-op |
| `resume()` | `session_resume` | no-op |
| `wake(id)` | `hosts_wake` + timed refreshes | after 1.5 s marks the sample host ready |
| `discover()` | `hosts_discover` | after 0.4 s returns a fake "HOME PC" |
| `linkAsk(address)` / `linkWait(address)` / `linkCancel()` | `hosts_link_ask` / `hosts_link_wait` (then `library_list`) / `hosts_link_cancel` | fake number `4821`; after 2.5 s adds the sample host |
| `link(address, code)` | `hosts_link` (then `library_list`) | code `000000` rejects, otherwise adds the sample host |
| `measureHost(id)` | `hosts_measure` | after 1.5 s returns a made-up `Health` |
| `removeHost(id)` | `hosts_remove` | removes from state |
| `rescanHost(id)` | `hosts_rescan` | counts sample games |
| `hostAdmin(id)` / `setHostAdmin(id, allowed)` | `hosts_admin` / `hosts_set_admin` | in-memory set |
| `power(gameId, place)` / `setPower(gameId, place, power, screen)` | `power_get` / `power_set` | in-memory map |
| `hostPicture(id)` / `setHostPicture(id, setting)` | `hosts_picture` / `hosts_set_picture` | in-memory map |
| `checkHost(id)` / `repairHost(id, fix)` | `hosts_check` / `hosts_repair` (then `refreshHosts`) | canned 8-step report; `pair` and `wake` fixes change the fake state |
| `sleepHost(id)` | `hosts_sleep` | marks asleep after 0.8 s |
| `install(gameId, place)` | `installs_start` (resolves to `asked_store`) | animates a fake download for 13 s then adds the place |
| `storage()` / `uninstall(gameId)` / `room()` | `storage_overview` / `storage_uninstall` / `installs_room` | fake disk + sizes |
| `scan()` | `library_scan` (120 s guard, `scanning` flag), then reload both catalogue and My Library | no-op |
| `addToMyLibrary(id)` / `removeFromMyLibrary(id)` | `library_personal_add` / `library_personal_remove`, then reload | update selected list in memory |
| `removeAndUninstallFromMyLibrary(id)` | `installs_remove_from_my_library` (Steam uninstall confirmation; admin PIN), then reload | simulated uninstall |
| `setFavourite(id, fav)` | `library_set_favourite` (updates both catalogue and personal view optimistically first) | localStorage |
| `launch(gameId, place)` | `library_launch` (sets `session` from the returned view) | fake waking/starting/running phases |
| `openCloud(service, name)` | `cloud_open` | fake running session |
| `inspectCloud(link)` / `addCloud(link, title)` | `cloud_inspect` / `cloud_add` (then reload) | URL pattern check for the four services |
| `endSession()` | `library_end_session` | clears the fake session |
| `pick(what)` | `library_pick` (`program\|image\|emulator\|folder`) | fixed fake paths |
| `consoles()` / `suggestConsole(emulator)` / `saveConsole(console)` / `removeConsole(id)` | `consoles_list` / `consoles_suggest` / `consoles_save` / `consoles_remove` (then reload) | in-memory list |
| `addManual(game)` / `removeManual(id)` | `library_add_manual` / `library_remove_manual` (then reload) | local state |
| `details(gameId)` | `library_details` | `sampleDetails` |
| `updates(gameId)` | `library_updates` (also patches `games[].updates` in state so cards show it) | stored updates |
| `syncSaves(gameId)` | `saves_sync` | sample sync lines |
| `backupSaves` / `restoreSaves` / `pickSaveFolder` / `forgetSaveFolder` | `saves_backup` / `saves_restore` / `saves_pick_folder` / `saves_forget_folder` | in-memory saves |
| `inspect(path)` | `library_inspect` | heuristic fake |
| `findArt(gameId, term)` / `useArt(gameId, choice)` / `pickArt(gameId, wide)` / `clearArt(gameId)` | `art_find` (previews turned into asset URLs) / `art_use` / `art_pick` / `art_clear` (each then reload) | `sampleArt` |
| `artAuto()` / `setArtAuto(on)` / `artAutoAgain()` | `art_auto` / `art_set_auto` / `art_auto_again` | flag |
| `discoverGames()` | `discover_scan` | canned |
| `artKey()` / `setArtKey(key)` | `art_key` / `art_set_key` | flag, shape check |
| `findOnline(term)` / `fetchOnline(id)` | `meta_search` / `meta_fetch` (cover path -> asset URL) | `sampleMatches` |

Exports besides `library`: `useLibrary`, `message(e)` (string -> itself, `Error` -> `.message`, otherwise "Something went wrong."; Tauri rejects with the
Rust `String`), `within`.

---

## 5. The bridge to Rust (`native.ts` and the other call sites)

### 5.1 `native.ts`

* `export const inTauri = "__TAURI_INTERNALS__" in window;` - the switch for every preview fallback. Always branch on it; never assume Tauri.
* `appInfo()` -> `app_info` (preview: `{role:"shell", version:"dev", ...}`); `quit()` -> `quit` (no-op in preview).
* `boot.ready()` -> `boot_ready` (preview `{enabled: previewBoot, stood_down:false}`), `boot.set(enabled)` -> `boot_set`.
* `crash.reports()`, `crash.text(name)`, `crash.seen()`, `crash.alive(hidden)` (never throws), `crash.note(text)` (never throws), `crash.openFolder()` ->
  `crash_reports`, `crash_report_text`, `crash_seen`, `crash_alive`, `crash_note`, `crash_open_folder`. Preview: empty/no-op.
* `activity.tail(count)`, `activity.text()`, `activity.openFolder()` -> `log_tail`, `log_text`, `log_open_folder`. Preview: two made-up lines.

### 5.2 Other places that call `invoke` directly (all are guarded by `inTauri` or a preview `Backend`)

| File | Calls |
| --- | --- |
| `library/store.ts` | all of section 4.5 |
| `cleanup/backend.ts` | `admin_status`, `admin_set_pin`, `admin_unlock`, `admin_lock`, `cleanup_scan`, `cleanup_apply`, `cleanup_undo`; preview = a stateful in-memory fake with ~25 sample items |
| `updates/backend.ts` | `update_check`, `update_install` (+ listens `update-progress`), `hosts_update`; preview = fake "0.5.0 -> 0.6.0" |
| `screens/quick.tsx` | `quick_state`, `quick_close`, `quick_end_game`, `quick_show_desktop`, `quick_set_sound`, `quick_set_power`, `quick_set_brightness` |
| `screens/sidemenu.tsx` | `system_state`, `system_set_sound`, `system_set_brightness`, `system_power` |
| `App.tsx` | listens `system://woke`, `system://notice`; calls `appInfo`, `quit` |

So "the bridge" is a convention, not one file: `native.ts` holds app/boot/crash/log; `library/store.ts` is the large one; admin and update flows have
their own `backend.ts` with the `Backend` type pattern (a `tauri` implementation and a `pretend()` implementation chosen by `inTauri`).

### 5.3 Conventions for any new wrapper

* Return the Rust payload; convert at the boundary if you want camelCase (like `details`'s `achievements_from -> achievementsFrom`).
* Never throw synchronously; return a promise. Rejections are the Rust `String` (use `message(e)`).
* Provide a preview fallback so the page is explorable in a browser (and so `npm run dev` does not crash).
* Argument keys camelCase (`gameId`), values must be JSON-serialisable and match the Rust parameter types exactly (e.g. `ids: string[]` for `Vec<String>`).
* Add the type to `data/types.ts` (or the local `backend.ts` for admin/update).

---

## 6. The command watcher (`inflight.ts`)

* **Purpose:** a frozen Ally once left no trace of which native call the page was waiting on. `watchCommands()` wraps `window.__TAURI_INTERNALS__.invoke`
  (the function every `invoke` from `@tauri-apps/api` goes through) so that a call still running after `STUCK_AFTER_MS` = 8000 ms writes
  `command <name> still running after 8 s` to the crash trail, and `command <name> finished|failed after N s` when it ends. Only the command name and
  time are written, never arguments.
* **Installed after 2 s** (`main.tsx`): "a diagnostic must not be able to stop the page from starting" (section 2.1).
* **Safety rules in the code:** returns early if not in Tauri or if `invoke` is not a function; calls the original first and returns exactly its result;
  all bookkeeping is inside `try/catch`; attaches a *separate* `.then` branch so the caller's promise is untouched; if assigning `internals.invoke` fails,
  nothing is watched and nothing breaks.
* **Ignored commands** (`IGNORED` regex): names beginning with `crash_`, `log_`, `update_`, `plugin:`, and exactly `hosts_link_wait`, `hosts_discover`,
  `library_scan`, `installs_start`. These legitimately wait for a person or a long job, or are the calls that write the notes themselves.
  Anything else that takes more than 8 s is "stuck" by this definition (for example `library_launch` while a desktop wakes - chapter 04, open question 9).
* **Limits:** it sees only page-initiated calls and cannot see a native thread blocked on a lock; it did not catch the real 0.20-0.34 freeze (that was a Rust deadlock).
  It is a hint for the trail, not a watchdog (the watchdog is on the Rust side).
* **Test:** `node scripts/test-inflight.mjs` (run by CI) is the only automated UI test.

---

## 7. Navigation: controller, touch and keyboard

### 7.1 Principle

Every focusable thing is a real DOM element carrying `data-focus`. Moving "focus" means moving real DOM focus (`el.focus()`), so the keyboard, controller
and touch all end up driving the same focused element, and the browser's own behaviour (Enter/Space on a button, `click`) does the activation.

### 7.2 Attributes (conventions - follow them exactly)

| Attribute | Meaning |
| --- | --- |
| `data-focus` | Element is a focus target for directional movement. Without it, D-pad/arrows skip the element. Disabled (`disabled` attribute) or invisible (`offsetParent === null`) elements are not candidates |
| `data-fid="<area>:<name>"` | A **stable, unique** focus id. Used to put focus back after a page is popped (`App.returnFocus`), after a view changes (`focusById`, `keepFocus`). Must be unique on the page at any moment; use a prefix per area (`rail:`, `game:`, `play:`, `desk:`, `set:`, `manage:` ...). Include the entity id for list items (`game:${id}`, `saves:restore:${b.id}`) |
| `data-autofocus` | Preferred first focus when a page opens (`focusFirst` looks for `[data-focus][data-autofocus]` before any `[data-focus]`). Pass `data-autofocus=""` conditionally (`undefined` to omit) |
| `data-game="<gameId>"` | Marks a game's card/hero/page. The ambient backdrop and the bottom "focus line" follow the focused element's nearest `[data-game]`; the X/F favourite action targets `document.activeElement.closest("[data-game]")` |
| `data-quiet` | Suppresses the "select" click sound for that element (`ui/sound.ts`), used for Sounds toggle and Stop buttons |
| `data-level="sound"\|"brightness"` | Side-menu rows that react to Left/Right on the keyboard |

### 7.3 `nav/focus.ts`

* `move(dir)`: from the focused element's rectangle, scores every candidate on the requested side (primary distance must be >= -8 px), takes the nearest band
  (within 48 px of the closest), then the best aligned (smallest orthogonal gap; left edge breaks ties so leaving a full-width item lands on the first card of
  the next row). **Sideways moves stay inside the current row** (orthogonal gap must be 0). If nothing is focused it focuses the first target.
* `focusFirst(root)`, `focusById(id)` (returns whether found; uses `CSS.escape`), `currentFocusId()`, `keepFocus(...prefer)` (if the focused element vanished,
  try the preferred fids, then the first in `<main>` - used where a button disappears when state changes so that A always does something).
* `installScrollIntoView()`: on every `focusin` of a `[data-focus]`, `scrollIntoView({block:"center", inline:"center", behavior:"smooth"})`.

### 7.4 `nav/input.ts` (keyboard and gamepad to actions)

* **Actions:** `back`, `prev`, `next`, `favourite`, `options`, `search`. Directions go straight to `move()`; they are not actions.
* **Keyboard:** arrows move; `Escape`/`Backspace` = back; `q`/`e` = prev/next section; `f` = favourite; `m` = options (side menu); `/` = search. While an
  `<input>` is focused, only ArrowUp/ArrowDown/Escape leave it (the rest types).
* **Gamepad** (browser Gamepad API, standard mapping, polled in `requestAnimationFrame`): D-pad (12-15) or left stick (threshold 0.55) move; held direction repeats
  after 380 ms then every 130 ms. **A (0) = `document.activeElement.click()`**. B (1) back, X (2) favourite, Y (3) search, LB (4) prev, RB (5) next, Menu (9) options.
  (View is not mapped here; View+Menu is read by Rust for the quick menu.)
* **Repeat rules:** `prev`/`next` repeat slowly (first after 600 ms, then every 400 ms); every other action fires once per press even if held.
  `heldFires` is exported for this; no test covers it.
* **Only while CarlaOS is in front:** polling stops when the document is hidden or without focus (`onVisibility`), sets `data-away` on `<html>` (CSS pauses animations),
  and marks all buttons as held (`wasDown`) so nothing fires by itself on the way back from a game. "The same controller is playing the game while one runs in front of us."
* **Held state lives at module level** (`heldDir`, `wasDown`, ...), so installing the listener again never turns a held button into a new press.
* `App` installs it once and routes actions to a ref'd handler (`handler.current`), so state changes do not reinstall the listener.

### 7.5 What the app does with each action (`App.handler`)

Ignored while the splash is up. While the side menu is open: only `back`/`options` close it. While a game runs (`playing`): ignored. Otherwise:
`options` opens the side menu; `back` pops the stack (or goes Home); `prev`/`next` change section; `favourite` toggles the favourite of the focused game
(`[data-game]`); `search` pushes the Search page unless already there (and clears the query).

### 7.6 How focus is restored and placed

1. On `push`: remember `currentFocusId()` (the `data-fid` of the focused element).
2. On `back`: the remembered fid becomes `pendingFocus`.
3. `useLayoutEffect([pageKey])`: if `pendingFocus` is set and `focusById` finds it, done. Otherwise scroll `<main>` to the top and `focusFirst(main)` (honouring `data-autofocus`).
4. Screens with internal views (Manage, LinkDesktops, Updates, Storage, Startup, Crash, Log, HostView) have their own `useLayoutEffect(... focusFirst(main) ...)` keyed
   on their internal step, and `keepFocus(...)` where a button can vanish. **If you add an internal view to a screen you must re-place focus when it changes**,
   otherwise A presses nothing (the first thing a user notices on a controller).
5. The side menu focuses its first `[data-focus]` on mount; closing it does not explicitly restore focus (DOM focus returns to `body` unless another effect
   re-places it; **UNVERIFIED:** whether focus lands back on the opener - check on a controller).

### 7.7 Touch

Everything is a `button`/`input` with real `onClick`, so touch works with no extra code. Sizes follow `--touch-min: 4rem`. The `Search` page also uses the Windows touch
keyboard through its text `<input>` (and an on-screen letter grid for controllers).

---

## 8. Screens (every file in `screens/`)

"Entry" is how the player reaches it. "Native" lists the `invoke` commands behind it (through the wrappers of section 4.5/5).
Admin pages show a PIN pad first (section 8.3).

### 8.1 Section screens (`screens/screens.tsx`)

| Component | Entry | Content | Native |
| --- | --- | --- | --- |
| `Home` | Section "Home" | "Continue playing" hero (most recently played), "My desktops" row, "Installing now" row (+ "See every step" card), "Recently played", "Favourites", "Recently added". Empty state texts when no games | via `Ctx` |
| `Games` | Section "My Games" | Filter chips (All, On this Ally, Streamed, Cloud, Favourites, Not installed) and a grid of `GameCard`s sorted by title | via `Ctx` |
| `Desktops` | Section "My Desktops" | Large `HostCard`s; empty state points to Settings | via `Ctx` |
| `HostView` | push `host` | One desktop: status line by state, **Wake** and **Check again** buttons when not awake, grid of its games | `wakeHost`, `refreshHosts` (Ctx) |
| `Cloud` | Section "Cloud" | The four services from `data/sample.ts::cloud` (static, UI-side data: ids `gfn`, `xcloud`, `boosteroid`, `luna` must match Rust's `carla_core::library::cloud` service ids) and a row of cloud games | `openCloud` -> `cloud_open` |
| `Settings` | Section "Settings" | A list of `setting` buttons: Appearance (theme), Game pictures behind the screen (backdrop), Sounds, Refresh library, Downloads, Manage games, Desktops, Clean up Windows, Updates, Storage, Starting up, Crash reports, Activity log, Exit to Windows, version line. Props are callbacks from `App` | `library.scan` (refresh), `quit` |

### 8.2 Game and play screens

| Component | File | Purpose / behaviour | Native |
| --- | --- | --- | --- |
| `GameDetail` | `pregame.tsx` | The game's page: cover, badges, state, description, store-update lines, stats (played, last played, achievements/milestones, last saved), install/play actions per place (`data-fid="play:<place>"`, `install:<place>`, `fav`), "Where it is" panel, Achievements and CarlaOS milestones rows, Power panel (Quiet/Balanced/Turbo + screen speed), Saves panel | On open: `library_details`, `saves_sync` (then details again if any sync line), `library_updates`; `installs_room` if installable here; `power_get`/`power_set`; `saves_backup`; play/install/favourite via Ctx |
| `Playing` | `playing.tsx` | Shown instead of the library during a session: cover, phase text (waking / starting / reconnecting / playing), note, "Back to the game" (adopted sessions), "I have finished playing"/"Stop" (`data-fid="playing:end"`) | `session_resume`, `library_end_session` |
| `Search` | `search.tsx` | Search by name+platform with accent/case-insensitive "every word must begin a word" matching; text input + on-screen letter grid (`data-fid="key:<letter>"`) for controllers | none (filters `ctx.games`) |
| `Downloads` | `downloads.tsx` | Every in-progress/queued/paused install (here and on desktops), steps, speed, time left; anyone may look | none (store state) |

### 8.3 Administrator pages and the PIN pattern

Rust holds a single global "unlocked" flag (no timeout, chapter 04). The UI pattern every admin page follows:

```tsx
const [admin, setAdmin] = useState<AdminStatus | null>(null);
useEffect(() => {
  void backend.status().then(setAdmin, () => setAdmin({ pin_set: false, unlocked: false }));
  return () => void backend.lock().catch(() => undefined);      // locked again on leaving
}, []);
useLayoutEffect(() => { const main = document.querySelector("main"); if (main) focusFirst(main); }, [admin?.unlocked, ...]);
if (!admin) return null;
if (!admin.unlocked) return <PinPad pinSet={admin.pin_set} lede="..." unlock={...backend.unlock} setPin={...backend.setPin} />;
```

`PinPad` (`ui/pin.tsx`) is a 4-digit pad (also takes physical digits); first time it asks to choose a PIN and confirm; results `unlocked | wrong | wait(seconds)` map to
messages. `CodePad` is the six-digit variant for the desktop link code (digits visible). **The page check is cosmetic; the real gate is `admin.require()` in
Rust.** Clean-up pages use the `cleanup` store which also calls `backend.lock()` via `cleanup.leave()` when the last clean-up page leaves the stack (`App` effect on `inCleanup`).

| Component | File | Purpose | Native |
| --- | --- | --- | --- |
| `Manage` | `manage.tsx` | Menu: add a game by hand (choose a program with the file dialog, inspect, form `AddForm` with title/kind/origin/args/cover and "find details online"), add a cloud game (`CloudForm`), games' pictures (`ArtList`/`ArtPicker` in `artwork.tsx`), automatic pictures toggle and "look again", consoles (`Consoles` in `consoles.tsx`), game saves folders (`SavesAdmin`: pick/restore/forget), "found on this Ally" (`FoundList`: discovered games and emulators), remove manual games | `library_pick`, `library_inspect`, `library_add_manual`, `library_remove_manual`, `meta_search`, `meta_fetch`, `cloud_inspect`, `cloud_add`, `art_*`, `consoles_*`, `saves_*`, `discover_scan`, `art_auto*` |
| `Consoles` | `consoles.tsx` | Add/edit/remove a console (emulator exe, name, game folder, file endings, launch options with `{game}`) | `consoles_list/suggest/save/remove`, `library_pick` |
| `ArtList` / `ArtPicker` | `artwork.tsx` | Which games lack pictures; for one game choose from found pictures, pick a file, clear, set SteamGridDB key | `art_find`, `art_use`, `art_pick`, `art_clear`, `art_key`, `art_set_key` |
| `LinkDesktops` | `link.tsx` | List linked desktops, add (search LAN -> allow at the desktop with a matching number, or the six-digit code), per-desktop page: picture setting, "allow it to administer this Ally", measure connection (`HealthCard`), check (`CheckDesktop` with per-step fix buttons: wake/pair/update/moonlight/relink), rescan games, put to sleep, remove (with confirmation) | `hosts_discover`, `hosts_link_ask/wait/cancel`, `hosts_link`, `hosts_measure`, `hosts_check`, `hosts_repair`, `hosts_rescan`, `hosts_sleep`, `hosts_remove`, `hosts_picture`, `hosts_set_picture`, `hosts_admin`, `hosts_set_admin` |
| `Cleanup` | `cleanup.tsx` | Windows clean-up: home (summary) -> review (checklist of recommended/optional items) / changes (undo) / kept (explanations); results banner | via `cleanup` store: `cleanup_scan/apply/undo` |
| `Updates` | `updates.tsx` | Check for newer CarlaOS (this Ally, host on this machine, linked desktops), install with progress, update a desktop | `update_check`, `update_install`, `hosts_update` |
| `Startup` | `startup.tsx` | Toggle start-with-Ally; explains the ways back to Windows (Exit to Windows, hold View while starting, auto stand-down after 2 failed starts) | `boot_ready`, `boot_set` |
| `Storage` | `storage.tsx` | Disk bars and installed Steam games by size; uninstall (needs a second press in the page, then Steam asks to confirm); polls every 8 s for completion | `storage_overview`, `storage_uninstall` |

### 8.4 Read-only diagnostic pages

| Component | File | Purpose | Native |
| --- | --- | --- | --- |
| `Crash` | `crash.tsx` | List crash reports, view one, copy to clipboard, open folder; marks reports seen | `crash_reports`, `crash_report_text`, `crash_seen`, `crash_open_folder` |
| `ActivityLog` | `log.tsx` | Last 200 lines of connection log + crash trail by time, refreshed every 3 s while open, stays at bottom unless scrolled up, page up/down for controller, copy, open folder | `log_tail`, `log_text`, `log_open_folder` |

### 8.5 Overlay screens

| Component | File | Purpose |
| --- | --- | --- |
| `SideMenu` | `sidemenu.tsx` | Menu button anywhere in the library: sound, brightness (-/+ buttons; Left/Right on keyboard via `data-level`), Wi-Fi name+signal, battery, Sleep/Restart/Turn off (needs a second press: label becomes "X? Press again"). `system_state`, `system_set_sound`, `system_set_brightness`, `system_power` |
| `QuickMenu` | `quick.tsx` | The window over a running game; see section 12 |
| `Splash` | `ui/splash.tsx` | Start-up animation |

### 8.6 Hazard noticed: hooks after an early return in `GameDetail`

In `pregame.tsx`, `GameDetail` returns `<Empty>This game is no longer in the library.</Empty>` when `game` is undefined **before** later hooks
(`useState` for `room`, `useEffect` for `offersHere`). If a game that was shown is removed from the library while its page is open (a catalogue change event, a
remote removal), the hook count changes between renders and React throws "Rendered fewer hooks than expected", unmounting the tree (a white screen in this app, which has no
error boundary). **UNVERIFIED:** whether this can be triggered in practice (`App` keeps the page on the stack while the game is gone); confirm by removing a manual game while
its page is open in the browser preview. Fix pattern: move those hooks above the early return.

---

## 9. Shared UI parts (`ui/`)

| File | Exports | Notes |
| --- | --- | --- |
| `ui/parts.tsx` | `playtime`, `amount`, `when`, `day`, `size`, `placeLabel`, `fetching`, `Progress`, `Cover`, `Badges`, `updateLine`, `availability`, `GameCard`, `Row`, `HostCard`, `Grid`, `Empty` | `GameCard` (a `<button data-focus data-fid="game:<id>" data-game="<id>">`), `Cover` (real art or a hue gradient with the title; tall vs wide; console band), `Badges` (source, Offline/Cloud/Stream), formatting helpers used everywhere (`size` in decimal units, `when` "today, 21:05") |
| `ui/chrome.tsx` | `SECTIONS`, `Section`, `TopBar`, `FocusLine`, `Hints` | Top bar (rail of sections, LB/RB hints, downloads chip with overall %, Online/Offline, browser-reported battery, 15 s clock). `FocusLine` shows the focused game's name and state at the foot (90 ms debounce). `Hints` is the footer key legend (hard-coded in `App`) |
| `ui/install.tsx` | `StateKey`, `InstallState`, `percentOf`, `timeLeft`, `speed`, `stateOfDownload`, `downloadOf`, `installState`, `stateAt`, `StateLine`, `Ring`, `Steps`, `placeName` | **One answer for "where a game stands"** so a card, the game page and the downloads page never disagree. Use it rather than recomputing |
| `ui/platform.ts` | `banner(game)`, `consoleColour(name)` | The band that names the console of emulated/port/homebrew games; whole-word match on the console name; colours are hard-coded |
| `ui/ambient.tsx` | `Ambient` | Two cross-fading layers behind everything that take the focused game's picture/colour (`fixed` forces one game for the Playing page; `backdrop` `full|soft|off`). Focus-driven via `focusin` + 140 ms debounce |
| `ui/pin.tsx` | `PinPad`, `CodePad` | See 8.3 |
| `ui/sound.ts` | `play(cue)`, `installSounds`, `setSounds`, `soundsOn`, `Cue` | WebAudio sine tones made in code, nothing shipped; cues `move`, `select`, `back`, `section`, `launch`, `favourite`. `installSounds` ticks on real focus moves and chimes on clicks of `[data-focus]` (unless `data-quiet`). Disabled when `document.hidden`. The autoplay flag in `tauri.conf.json` lets it start without a click |
| `ui/icons.tsx` | `OnDevice`, `Streamed`, `Heart`, `Trophy`, `Lock`, `Sparkle`, `Clock`, `SaveIcon`, `CloudIcon`, `Source`, `Download` | Inline SVG components (`aria-hidden`) |
| `ui/splash.tsx` | `Splash` | Start-up animation (CSS-driven) |

Components take plain props (no context), set `aria-label`s on cards, and use `aria-hidden` on decorative parts. Keep screen-reader text meaningful (`FocusLine` and `Hints` are `aria-hidden`; section
pages have an `sr-only` `<h1>`).

---

## 10. Types and sample data (`data/`)

* `data/types.ts`: the shapes the UI works with (`Game`, `Host`, `Download`, `Session`, `Details`, `Saves`, `SaveSync`, `Health`, `HostPicture`, `CheckReport`, `Console`,
  `Power`/`POWERS`/`POWER_TEXT`, ...). The file says it mirrors the Rust-side catalogue; where a field comes straight from Rust it keeps the Rust (snake_case) name.
  `Session` has no `since` although Rust sends it. `HostState` includes `asleep` and `off`, which only the preview data uses.
* `data/sample.ts` (also imported in **production code**): `cloud` (the four cloud services shown on the Cloud screen), plus preview-only `hosts`, `games`, `sampleDetails(game)`,
  `sampleMatches(term)`, `sampleArt(game, term)`. Sample titles are **invented on purpose** ("the prototype must not ship or imply real artwork"); do not add real game titles or art.
  Because `App.tsx` imports `cloud` from here, this file is part of the shipped bundle; changing a service id here must be matched in `crates/core/src/library/cloud.rs`.

---

## 11. Browser preview mode

`npm run dev` (or `vite preview` after a build) in an ordinary browser:

* `inTauri` is false, so every wrapper takes its preview branch; `store.ts` starts with `sample()` games (favourites from `localStorage.carla.favourites`, default three), sample hosts and fake
  downloads; admin has a fake in-memory PIN flow; updates show a fake available release; the quick menu renders `SAMPLE` data with a painted game background (`.quick--preview`).
* Use it for layout and flow work and for screenshots; **it cannot show real behaviour** (focus quirks of the Windows web view, timing, the asset protocol, the controller via XInput, real errors).
* Keyboard stands in for the controller (arrows, Enter, Esc, q/e, f, m, /). The Gamepad API works in the browser if a pad is connected.
* Preview fallbacks are not tested; keep them simple and make them throw the same kinds of sentences the Rust side would (`throw "That is not the code on the desktop."`).

---

## 12. The quick menu window (`screens/quick.tsx`)

* Rendered when the URL hash is `#quick`; a separate web view window (chapter 04, 2.2 and 14.1). It has its own React tree, its own `QuickMenu` state, and shares localStorage with the main
  window (reads `carla.theme` to match).
* Adds the class `quick-window` to `<html>` (CSS makes the page transparent; `.quick__panel` is the visible card) and applies the saved theme.
* **Input is events, not DOM focus:** Rust sends `quick://press` (`up|down|left|right|select|back`) while a game owns the controller; the page maps each press through `act.current(press)` (a ref updated on every render so the single
  listener always sees the latest state). The keyboard (`ArrowKeys`, Enter, Escape, Backspace) is also handled so the menu can be tried in a browser. There are no `data-focus` elements here; the "current row" is state (`row`).
* Rows (built dynamically): Back to the game, Power (when known), Screen (when the profile sets the screen), Sound, Brightness, Show the desktop (streams only), End game (with a two-answer confirmation: "Keep playing" / "End game").
* `quick://open` resets to the top row, clears confirmation and note, refreshes the clock and calls `quick_state` for fresh levels.
* Levels step in tens (`step()` mirrors `carla_core::quick::step`); brightness is clamped to >= 5.
* Failures show in the footer as a note (`message(e)`), not a toast.

---

## 13. Design tokens, theming and CSS organisation

### 13.1 `tokens.css`

Defines everything tunable on the handheld in one place:

* Colours (light): `--bg` `#fbf5f3`, `--surface`, `--accent` `#d98ba0` (pink), `--accent-soft`, `--on-accent`, `--text`, `--text-quiet`, `--ok`, `--warn`, `--off`, `--shadow`.
* Dark theme: `:root[data-theme="dark"]` overrides the same variables.
* Shape/size: `--radius-card`, `--radius-pill`, `--gap`, `--touch-min` (4 rem).
* Type: `--font` (Nunito if installed, else Segoe UI Variable/Segoe UI), `--text-body` 1.375 rem, `--text-title`, `--text-display`.
* Motion: `--ease`, `--fast` 150 ms, `--slow` 200 ms.
* Global rules: `html { font-size: calc(100vw / 120) }`, `body` is `overflow: hidden; user-select: none`, `:root[data-away]` pauses all animations (set by `nav/input.ts` when the window is not in front),
  `prefers-reduced-motion` zeroes durations, `.sr-only`.

**Rule:** colours, sizes and durations come from variables. Do not hard-code colours in new CSS (the console-band colours in `ui/platform.ts` and the cloud tile hues are the existing exceptions).

### 13.2 `app.css` (about 3000 lines, one file, plain CSS, BEM-style `block__element--modifier` names)

Sections in file order (search for the `/* ---------- ... ---------- */` banners): frame (`.shell`, buttons reset, **the single focus look** `[data-focus]:focus` = rose ring + glow), top bar/rail, main (`.main` scrolls, no
scrollbar, `page` entry animation), covers and cards, source/badges, hero, desktops, controls (`.pill`, `.chip`, `.setting`, `.field`), steps of a check, game detail, toast, administrator PIN and clean-up, activity log, real artwork,
playing, forms, the page of one game (pregame), startup animation, search, quick menu (own transparent window), update note, ambient backdrop, console band, art picker, "a game on its way in", cloud, power choice, installing, storage, side
menu, "the immersive look", state dots, rings, Home hero, foot line, top-bar downloads chip, downloads page, a game's places, connection figures, link match number.
New features append a banner section near related ones. Dark-mode specifics are written as `:root[data-theme="dark"] .x` overrides next to the light rule.

Layout notes: `.shell` is a 3-row grid (top bar, `main`, hints) with a radial pink wash; `.main` is the only scroller; `.row__track` is a horizontal scroller (scrollbars hidden); focus visuals are shadows (not outlines) so they do not shift layout.

### 13.3 Adding styles

Add classes next to related ones with a banner comment; use tokens; keep touch targets >= `--touch-min`; do not rely on `:hover` (handheld); respect `data-away` (do not start animation timelines via JS); add a `prefers-reduced-motion` guard for new animation.

---

## 14. Recipes

### 14.1 Add a screen (a new page reachable from Settings)

1. **Types:** if it shows Rust data, add the payload type to `data/types.ts` (or a local `backend.ts` for admin-style features).
2. **Native side:** add the command(s) in Rust following chapter 04, section 17 (async vs sync, `admin.require()` for admin pages, one-sentence errors, register in `main.rs`).
3. **Bridge:** add wrapper(s) to `library/store.ts` (`library.someThing`) with an `inTauri` branch and a preview fallback; or, for an admin feature with several calls, a `backend.ts` + store like `cleanup/` or `updates/`.
4. **Component:** create `screens/<name>.tsx` exporting `<Name notify=... />`. Make every interactive element a `button`/`input` with `data-focus` and a unique `data-fid`, set `data-autofocus` on the first button.
   If it needs admin, copy the PIN pattern of section 8.3 (including `backend.lock()` on unmount and the `focusFirst` layout effect keyed on `admin?.unlocked`).
5. **Route:** in `App.tsx` add the kind to the `Page` union (`{ kind: "name"; id: "home" }`), import the component, add an `else if (top?.kind === "name") page = <Name .../>` line in the chain, and pass `onName={() => push({ kind: "name", id: "home" })}` to `Settings`.
6. **Entry:** add a `<button className="setting" data-focus data-fid="set:name" onClick={onName}>` to `Settings` in `screens/screens.tsx` and the prop to its type.
7. **Focus:** `App` restores focus on Back automatically (it remembers `set:name`). Inside the screen, re-place focus whenever an internal view changes (`useLayoutEffect(() => focusFirst(main), [view])`; `keepFocus(...)` for buttons that disappear).
8. **Styling:** reuse `.settings`, `.setting`, `.pagehead`, `.pill`, `.lede`, `.notice`, `.working`; add CSS to `app.css` only if needed (tokens only).
9. **Check:** `npm run build` in `apps/shell`; open it in the browser preview; walk the flow with arrows + Enter + Esc; check the "stuck at nothing focused" case; then on the Ally with the controller (hardware validation is a separate, explicit step).

### 14.2 Add a setting

*Purely visual/local preference* (like theme): add state + a `localStorage` key in `App` (use `load()`, with a default, wrapped in try/catch), apply it to `<html>` via `dataset` in an effect, pass `value` and an `onX` cycle callback to `Settings` and add a
`setting` button (`data-fid="set:x"`); add CSS keyed on `:root[data-x="..."]`. Remember the quick window reads only `carla.theme`; share any other key explicitly if it must apply there.

*Native setting* (persisted in the catalogue/settings): Rust side `store.set_setting(KEY, Some(bytes))` in a command (admin-gated if it affects the machine; seal anything secret with `secret::seal`), a `library.getX/setX` wrapper pair with a preview variable, and a row in the screen that owns it
(for example `Manage` for automatic pictures: `library.artAuto()` on unlock, `library.setArtAuto(!on)` on press, then `notify(...)`).

### 14.3 Add an action to the game page

1. **Rust command** (chapter 04, 17) and its **wrapper** in `library/store.ts`.
2. If the action must be reusable from several screens (card, host page), add a callback to `Ctx` (type in `screens/screens.tsx`) and implement it in the `ctx` object in `App.tsx` (wrap with a cue sound and `.catch((e) => setToast(message(e)))`). Otherwise call `library.x` directly in `pregame.tsx`.
3. Add a `<button className="pill" data-focus data-fid="x:..." onClick=...>` inside `.detail__actions` in `GameDetail` (decide whether it is `pill--primary`, and who gets `data-autofocus` - the first primary action does). Respect the existing conditions (`game.places`, `game.installable`, `details`).
4. If it changes data the page shows, refresh it: call `library.details(gameId)` again and `setDetails`, or rely on the `library://changed` event (`reload()`), or update the store optimistically as `setFavourite` does.
5. If the action is only for the administrator, **do not put it on the player's game page**; put it under Manage (`SavesAdmin`, `ArtPicker`...) behind the PIN, as saves folder selection and artwork choice are.

### 14.4 React to something that happens on a Rust thread

Add `app.emit("area://what", payload)` in Rust (document it in chapter 04, section 16), then `listen<Payload>("area://what", ...)` in `store.ts::start` (store-level data) or in `App`'s `useEffect` (toasts), always inside `if (!inTauri) return;` and with
the unlisten promise cleaned up when it is inside a component effect (see the `system://` listeners in `App.tsx`).

---

## 15. Frontend pitfalls (collected)

* **White screen:** never let start-up code throw before first render; install diagnostics late and inside `try/catch` (section 2.1). There is no error boundary: a render exception blanks everything.
* **No frontend tests:** `tsc --noEmit` + a working build are the gate; `scripts/test-inflight.mjs` is the only script test. Always load the page after changes, in the browser preview and on the Ally for anything touching focus or Tauri calls.
* **`inTauri` branches:** a new `invoke` without a preview fallback breaks `npm run dev` in a browser (`invoke` is undefined there).
* **Stale closures with listeners:** `installInput` is installed once and calls `handler.current`; `quick.tsx` uses an `act` ref for the same reason; copy this pattern for any global listener.
* **`<main key={pageKey}>` remounts** on page changes; page-local state is lost. Hold shared state in `App`/store.
* **`event.currentTarget` is null after an `await`** (a known bug class elsewhere in this repo): read what you need from the event before awaiting.
* **Single-flight and timeouts:** wrap any native call that could never answer in `within(...)` if it sets a busy flag; clear flags in `finally` (as `scan` and `refreshHosts` do).
* **Do not trust optimistic UI for the Rust side:** after a failed command, the state must come back (`setFavourite` updates optimistically; check how failures are handled where you copy it - it rejects to the caller which toasts but does not roll the star back).
  **UNVERIFIED:** whether the favourite star reverts on failure; confirm by making `library_set_favourite` fail.
* **Admin flag is global and never expires in Rust:** a page that forgets `backend.lock()` on unmount leaves every admin command usable until the app restarts or another page locks it.
* **Focus must always land somewhere:** after any change that removes the focused element, call `keepFocus(...)`/`focusFirst(...)`; otherwise A (click on `activeElement`) does nothing.
* **Both windows run `main.tsx`:** the quick window also starts the heartbeat and error listeners (chapter 04, open question 3). Avoid side effects at module load that make sense only in the console window.
* **Asset URLs:** only `convertFileSrc(path)` of files under directories Rust allowed (`allow_pictures`) will display; a new picture folder needs a Rust change (chapter 04, 2.3). The CSP also forbids remote images/fetches.
* **`StrictMode` double effects** in dev: effects mount/unmount/mount; keep effects idempotent (the store's `started` flag does this; component effects must clean up).
* **Sounds and autoplay:** `AudioContext` must be created/resumed lazily (done in `play`); a first user gesture is not required in the Tauri window only because of the web view autoplay flag.
* **Text:** keep error/status messages plain and one sentence; do not show raw Rust `Debug`. No emoji in UI text (the code uses a few Unicode symbols such as the lightning mark for charging, and arrows in hints).

---

## Open questions / needs investigation

1. **`GameDetail` hooks after an early return** (section 8.6): confirm whether removing a game while its page is open can crash the tree, and add an error boundary or hoist the hooks.
2. **Optimistic favourite on failure** (section 15): confirm that a failed `library_set_favourite` leaves a wrong star.
3. **Side menu focus restore** (section 7.6 point 5): where focus lands after the side menu closes.
4. **`data/sample.ts::cloud` is shipped production data** and duplicated knowledge of the Rust service list; consider moving to a Rust command so one list exists. Confirm the ids match `carla_core::library::cloud::SERVICES` after any change.
5. **`Session` type lacks `since`** (Rust sends it; the quick menu gets its own `since`). Confirm nothing in the page needs it.
6. **Typing and `Escape` in inputs:** `installInput` lets Escape leave an input but `back` is not fired while typing; confirm this is the intended behaviour on the Windows touch keyboard (the controller/touch-keyboard interplay is a hardware question).
7. **No `error` boundary and no UI test runner:** a decision for the maintainers (the inflight node script shows the pattern for small node-testable modules such as `nav/input.ts::heldFires`, `search.tsx::matches`, `ui/parts.tsx` formatters, `updates/backend.ts::older`).
8. **Controller inside the quick window** relies entirely on Rust events (`quick://press`); the keyboard path exists for browsers. Confirm that touching the quick window with a finger is meaningfully handled (only the "Back to the game" row has an `onClick`).
9. **`tsc` version** in `package.json` is `^7.0.2` (the native TypeScript compiler line); confirm the CI and local toolchains actually resolve to that version before relying on newer language features.
10. **Heartbeat from a hidden quick window** may influence the Rust watchdog (chapter 04, open question 3).
