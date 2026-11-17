# The immersive look, install states and finding games

## The picture behind the screen

The wide picture of the game in focus fills the screen behind everything, the way a console does, and changes as focus moves. The page's own colour comes back by the middle of the screen, so words are as readable as on a plain page; the bar at the top gets a ground of its own.

- A game with only a cover gets a soft wash of that cover's colours. A game with no picture gets its own colour.
- Settings, "Game pictures behind the screen" steps through Full, Soft (colours only, as before) and Off.
- The picture drifts very slowly; not at all when Windows is set to reduce motion, and never while a game is in front.

The line at the foot of the screen names the game in focus and how it stands.

## How a game stands

Every card says it in the same place, with a dot whose shape differs as well as its colour (`apps/shell/src/ui/install.tsx`):

| On the card | Meaning |
| --- | --- |
| Installed | On this Ally. Plays offline. |
| Ready to stream / Ready in the cloud | Installed on a desktop, or on a cloud service. |
| Not installed | In the store library, installed nowhere. |
| Queued | The store knows and has not begun. |
| Installing · 42 % | With a ring on the picture, and the time left under it. |
| Paused at 42 % | Paused in the store. |
| Finishing up | Everything is fetched; the store is putting files in place. |
| Update needed / Updating · 42 % | The store has to update it before it starts. |

A game's page has "Where it is": one tile for each place (this Ally, each desktop, the cloud) with its state there. An install shows its four steps (Queued, Downloading, Finishing, Ready), sizes, speed and time left.

Downloads (the pill in the top bar while something is fetched, or Settings, Downloads) lists everything being installed or updated here and on the desktops. Speed and time left are worked out from how far each download got between two looks, so they appear a few seconds after it starts.

## Pictures without asking

After each look through the library, installed games with no cover or no wide picture are looked up in the background (`crates/core/src/library/autoart.rs`):

- a Steam game by its Steam number: its library cover and wide picture, then what the shop says its pictures are (newer games), then SteamGridDB when a key is kept;
- anything else by its name, in the places the picture chooser asks. A picture is only taken when that place calls the game by the same name ("Portal" is not "Portal 2"), so a wrong picture is rarer than no picture.

What is sent: the game's name or Steam number, to Steam, Wikipedia, the libretro box art collection, and SteamGridDB if a key was entered. Pictures chosen by hand always win. A game nothing was found for is left alone for a week. It never runs while a game is being played. Settings, Manage games has "Find pictures by itself" (on by default) and "Look for missing pictures now".

## Finding games, ports and emulators

Settings, Manage games, "Look for games on this Ally" (`crates/core/src/library/discover.rs`) looks in:

- folders called Games, Ports, Emulators or Emulation on every disk and in the user's folder: each folder inside is one game, and its main program is picked (not the uninstaller, the crash reporter or an Unreal starter);
- shortcuts on the desktop and in the Start menu, when they lead to a port or emulator CarlaOS knows, or into one of those folders;
- where programs install themselves, only for the ports and emulators CarlaOS knows by name.

Nothing is added until the administrator chooses it: one press for a game, "Add all", or an emulator's line to pair it with a folder of game files.

Recognition itself is better too: about 75 ports and rebuilt classics are known by name (`ports.rs`), a program is asked for the product name Windows shows under Properties (`pe.rs`), build names such as `TinKnight-Win64-Shipping` are read as "Tin Knight", and more emulators are known (Ryujinx, Eden, Citron, Xenia, RMG, ares and others).

## Not checked on real hardware

Nothing here ran on Windows or reached the picture places from where it was built: the reading of their answers, of program files and of folders is tested against made-up examples. The preview in a browser shows invented games with drawn pictures.
