# Game library

## What it does

- Finds installed Steam games by reading Steam's own files. No sign-in, no network.
- Lets the administrator add any program as a game, with a name, a section, launch options and artwork.
- Starts a game, waits for it to finish, records the time played and brings CarlaOS back to the front.

## Where things live

| Part | Path |
| --- | --- |
| Catalogue (SQLite) | `crates/core/src/library/store.rs` |
| Steam scanner | `crates/core/src/library/steam.rs`, `vdf.rs` |
| Starting and watching a game | `crates/core/src/library/launch.rs` |
| Tauri commands | `apps/shell/src-tauri/src/library.rs` |
| Interface state | `apps/shell/src/library/store.ts` |

On the Ally the catalogue is `%LOCALAPPDATA%\dev.carlaos.shell\library.sqlite`, and copied artwork sits in `art` next to it.

## Catalogue

`games` is one row per title. `installs` is one row per place the title can be started from; `place` is `local` for this machine and will be a desktop's id once streaming exists. `play_sessions` holds one row per time a game was played.

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
