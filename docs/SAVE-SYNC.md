# Saves that follow the player

Play a game on the Ally, continue it on the desktop (or streamed from it), and
come back to the Ally: the saves go along by themselves. Nobody presses a
button.

This is for games that have no cloud saves of their own. Steam games with
Steam Cloud are left to Steam.

## Turning it on for a game (administrator, once)

Both machines must be told where the game keeps its saves. A folder is only
ever named at the machine it is on: the Ally can never point a desktop at a
folder, and a desktop can never point the Ally at one.

1. **On the Ally:** Settings, Manage games, the game, "Choose the save folder".
2. **On the desktop**, in a terminal:

   ```
   carla-companion saves set "Name of the game" "C:\Users\You\Saved Games\The Game"
   ```

   Use the name the game has in CarlaOS. A game from Steam, Epic or GOG is
   then matched by its store id; a game added by hand is matched by its name,
   so give it the same name on both machines.

`carla-companion saves` lists what is set, `saves clear KEY` takes one back.
Nothing is ever deleted by either.

## When saves move

| Moment | What happens |
| --- | --- |
| Before a game starts on the Ally | Newer saves are fetched from every desktop that answers. |
| After a game was played on the Ally | The new saves go to every desktop that answers. |
| Before a stream starts | The Ally's newer saves go to that desktop first. |
| After a stream | The desktop's new saves come to the Ally, once the game has closed there (tried for about two minutes). |
| When the game's page is opened | Checked once more, in the background. |

A desktop that is off simply takes part the next time. The game's page shows
one line per desktop: in step, sent, fetched, or what went wrong.

## Which side wins

CarlaOS remembers what both machines held the last time they agreed.

- Only one side changed since: that side is copied to the other. Clocks do not
  matter for this.
- A side with no saves at all never wipes the other.
- Both changed (for example the Ally was played offline, then the desktop):
  the saves written last win, and the page says so.

**Nothing is lost either way.** Before a machine's saves are replaced it keeps
a safe copy of the whole folder (the last five). On the Ally they are restored
from Manage games. On the desktop:

```
carla-companion saves copies KEY
carla-companion saves restore KEY COPY
```

## Safety

- Saves are only touched while the game is closed on that machine. The desktop
  refuses while its game runs.
- Every file is checked against its fingerprint after travelling. A damaged
  file stops the copy before any old save is removed.
- Files are written beside the old one and swapped in, so a game never finds
  half a save.
- Only plain files inside the named folder can be read or written. Paths that
  climb out, drive names and links are refused on both machines.
- Limits: 2000 files, 32 MB per file, 512 MB per game. A larger folder is not
  a save folder and is refused.
- Only a linked Ally can ask, with the same signed requests as everything
  else. Saves travel signed but not encrypted on the home network; away from
  home they go through Tailscale, which encrypts them.

## Finding where a game keeps its saves

If you do not know the folder, the desktop can suggest one. It only suggests;
you confirm with `saves set`, and nothing is changed or deleted.

```
carla-companion saves guess "Name of the game"
carla-companion saves find "Name of the game"
```

- `guess` looks for folders named like the game under Saved Games, Documents
  (and My Games), AppData (Local, LocalLow, Roaming), ProgramData and the
  game's own install folder. Nothing needs to be played.
- `find` takes a snapshot of those places, asks you to start the game, play
  briefly, save and quit, then press Enter. It takes a second snapshot and
  ranks the folders where files appeared or changed (save-like names, small
  files and plausible places rank higher; logs, caches and settings lower).

Each suggestion comes with the exact `saves set` line to run. The search is
bounded (depth, number of files, time), skips noisy folders (Microsoft,
Packages, Temp, caches, crash dumps, browser profiles, Steam, Epic, Unity,
NVIDIA, AMD, Discord) and never follows links. `guess` looks at most three
folders deep. Skipping Steam also hides Steam's `userdata` folder (Steam Cloud
saves), and so does skipping Epic for Epic's own folders. Each place has its
own budget: when a walk is cut short the command says so, and that place is
left out of the comparison, so the answer may be incomplete.

It cannot find saves of a game that writes only when it quits and was left
running, that keeps them in the cloud, or that keeps them in the registry.

## Not covered yet

- Emulator memory cards that an emulator shares between its games are kept in
  step per console, not per game: see below.
- Two desktops are kept in step through the Ally, not with each other.
- Tested with stand-ins only: two folders, and an Ally and a companion talking
  over the real protocol on one machine. Not yet run with a real game on
  Windows.

## Emulated games

An emulator keeps the saves of all its games together, so an emulated game has
one save folder per **console**, not per game. Choosing "Choose the save
folder" on any game of a console sets it for every game of that console. Its
saves are kept in step under the console's name (for example "GameCube and
Wii"), so the desktop names its folder the same way:

```
carla-companion saves set "GameCube and Wii" "C:\Users\You\Documents\Dolphin Emulator\GC"
```

Steam, Epic and GOG games are unchanged.
