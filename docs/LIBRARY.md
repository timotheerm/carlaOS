# Game library

## What it does

- Discovers games installed through Steam, Epic, GOG, supported emulators and registered standalone programs.
- Keeps a separate personal **My Library** on the Ally: local Steam installs appear automatically; other games are added by choice.
- Lets the administrator register a standalone program with its name, launch options and artwork.
- Starts a game, waits for it to finish, records the time played and brings CarlaOS back to the front.

## My Library

The catalogue is everything CarlaOS knows about. **My Library is the smaller list the player has chosen to keep on the Ally.** It is local to the Ally; there are no user accounts or library profiles.

- A Steam game installed on this Ally is included automatically.
- Games on linked desktops, Steam games that are not installed, and other catalogue entries must be added with **Add games**.
- Adding a game only selects it. It does not install or copy anything.
- Removing a locally installed Steam game from My Library asks Steam to uninstall it. The selection is removed only after the Steam install disappears; cancelling Steam's confirmation keeps the game in My Library.
- Uninstalling from **Settings, Storage** makes space but keeps the My Library entry, so the game can be installed again later.
- Removing a desktop game only removes the Ally's selection. The desktop's files are not touched.
- Removing a local non-Steam game removes the selection only. CarlaOS does not uninstall Epic, GOG, emulator or arbitrary program files.

When this feature is first used, existing non-Steam and desktop catalogue entries are not automatically selected. Local Steam games are included by the normal default rule.

## Where things live

| Part | Path |
| --- | --- |
| Catalogue (SQLite) | `crates/core/src/library/store.rs` |
| Steam scanner | `crates/core/src/library/steam.rs`, `vdf.rs` |
| Starting and watching a game | `crates/core/src/library/launch.rs` |
| Tauri commands | `apps/shell/src-tauri/src/library.rs` |
| Personal membership | `library_entries` in the catalogue; Store methods in `crates/core/src/library/store.rs` |
| Interface state | `apps/shell/src/library/store.ts` |

On the Ally the catalogue is `%LOCALAPPDATA%\dev.carlaos.shell\library.sqlite`, and copied artwork sits in `art` next to it.

## Catalogue

`games` is one row per title. `installs` is one row per place the title can be started from; `place` is `local` for this machine and will be a desktop's id. `available` records where a store can install a game. `library_entries` records the games the player explicitly selected for My Library; local Steam installs are included by the personal-list query even without a membership row. `play_sessions` holds one row per time a game was played.

A rescan replaces the installs of one store on one machine and leaves everything else alone, so favourites and playtime survive, and a game that is uninstalled and reinstalled gets its history back.

Game ids are `steam:<app id>` and `manual:<hash of the program path>`. The same title from two stores is two games for now; merging them comes with the other stores.

## Knowing when a game has finished

Asking Steam to start a game returns immediately, and many games start through a launcher that exits straight away. So a game counts as running while any program whose file is inside the game's folder is running. It counts as finished once none has been seen on two checks in a row, two seconds apart.

Two things follow from that:

- A game that has not appeared after three minutes is given up on and counts as not played.
- A hand-added program that sits in a folder shared with unrelated running programs never looks finished. The playing screen therefore always offers "I have finished playing".

## Controller input while a game runs

The shell ignores the controller whenever its window is not the focused one, and forgets held buttons, so playing a game never moves things around in CarlaOS behind it.

## Checking the Windows build from Linux

`scripts/check-windows.sh` type-checks the shell for Windows without a Windows toolchain.
