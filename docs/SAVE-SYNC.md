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

## Not covered yet

- Emulator saves (memory cards belong to the emulator, not to one game).
- Two desktops are kept in step through the Ally, not with each other.
- Tested with stand-ins only: two folders, and an Ally and a companion talking
  over the real protocol on one machine. Not yet run with a real game on
  Windows.
