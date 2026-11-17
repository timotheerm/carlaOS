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

## A second desktop for the same game

A folder is still only ever confirmed at the machine it is on. But once one
desktop has a save folder below a well-known place (Saved Games, Documents,
Documents\My Games, AppData Local, LocalLow or Roaming, ProgramData, or the
game's own install folder), it also records the folder as a **place**: the
well-known root and the path below it, for example "Saved Games / Wolf". The
Ally remembers the place the first time a signed answer carries one, and
sends it to every desktop it asks about that game.

A desktop that has no folder for the game turns the place into a folder of its
own profile (the same Saved Games, a different user name). Names that could
lead out of the root (`..`, drive letters, network paths, links) are refused.
When that folder, or the folder it would be made in, exists, the desktop notes
a **suggestion** in `save-suggestions.json` (at most 200, one per game) and
answers "no folder", as before. Nothing is created, read or synced.

```
carla-companion saves suggestions     list them
carla-companion saves accept KEY      use the folder (it must exist)
carla-companion saves dismiss KEY     forget it
```

`accept` runs the same checks as `saves set`. After that the ordinary rules
apply: the Ally's newer save goes to the desktop before a stream, and the
desktop's new save comes back afterwards. A desktop that already has a folder
for the game ignores any place it is sent.

## Keeping a desktop clean

Optional, and off unless you turn it on. For a game you only ever want to
play from the Ally, the desktop can empty its own live save folder after a
stream, so no second copy lingers there. The next stream puts the saves back
from the Ally before the game starts.

**Opt-in, at the desktop only:**

```
carla-companion saves clean KEY on --yes
carla-companion saves clean KEY off
```

**Read this before turning it on.** It is for one Ally only. Do not play this
game directly on the desktop: it would start with no saves, and a later sync
could take those new saves for the real ones. With it on, the desktop's safe
copies and the Ally's folder are the only copies of the saves. Without
`--yes`, `saves clean KEY on` prints this warning, changes nothing and exits
with a non-zero status.

The flag lives in the desktop's own `saves.json`. It cannot be set from the
Ally or over the protocol, and `saves set` / `saves accept` always start with
it off, so naming a different folder never inherits it. `carla-companion saves`
marks the games that have it.

**Exactly when it happens.** Only after a stream, from the Ally, and only when
every step before it went right: the sync that follows the stream ended with
both machines holding the same saves, and the Ally's folder is not empty. It
is never done after a failed or partial sync, for a game played on the Ally,
for a cloud game or while a game runs on either machine. It is tried once and
never retried: if the desktop declines, its files stay.

**What is checked.** The Ally describes its own folder afresh (every file,
size and SHA-256) and sends that to the desktop. This is a check, not a
cryptographic proof: the description must match the desktop's live files
exactly, and any linked device could have obtained that description from the
desktop's own answers. The desktop removes anything only if all of this
holds, otherwise it answers and keeps everything:

1. the game is flagged here (otherwise refused);
2. no game is running on the desktop;
3. the Ally's description is plausible, with at least one file;
4. the desktop's own files, read again right now, match it exactly: the same
   paths, sizes and fingerprints, no extra and no missing file;
5. a full safe copy was made and read back to match;
6. nothing changed while the copy was made.

Then it removes exactly the files that were listed, each checked once more
just before (a file that changed since is left in place and counted), inside
the save folder and not behind a link. The folder itself stays. All changes
to a game's saves on the desktop wait for each other, so another Ally's
write cannot land in between. What happened goes to the Ally's crash trail;
the game's page shows only a real failure.

**A stream waits for the saves.** Because the desktop is empty after a clean,
the next stream is held back until the Ally has put its saves on the desktop
and read them back there (the Ally says "The saves could not be put on the
desktop yet. Try again in a moment."). Otherwise the game would start from
nothing and its new saves would later replace the real ones. A game with no
save on the Ally yet, or a desktop that is not set to keep clean, is never
held back. If a send is interrupted and the desktop holds only some of the
Ally's files, the Ally's saves are never replaced by that part: the desktop is
topped up instead (only for a desktop set to keep clean, and only when the
desktop has no extra or changed file).

The Ally remembers, in its database, what each desktop last said about
cleaning a game. So if the desktop cannot be asked at all (it is off, the
network dropped, it answered with an error), the stream is held back too, as
long as the Ally has saves for the game, and this also holds after the Ally
was restarted. A desktop the Ally has never heard from about the game is not
held back.

**The accepted limit.** This is for one Ally only. Do not play the game
directly on that desktop. `saves clean KEY on` therefore needs `--yes`. Another
Ally, or direct play, can still start from an empty or older desktop; the
desktop's safe copies are then the only way back.

**Restoring.** The copy taken before a clean is kept longer than the ordinary
ones (the last 15, counted apart from the last 5 ordinary copies). It may be
the only other copy: if the Ally loses its saves, restore from the desktop:

```
carla-companion saves copies KEY
carla-companion saves restore KEY COPY
```

**What is never done.** Nothing outside the named save folder is touched.
Nothing is removed on the Ally. Nothing is removed on a desktop whose
administrator did not turn it on, or when the two copies differ by one byte.
Files that are not part of the description (links, names the Ally could not
hold) are left alone. A game started directly on the desktop in between finds
no saves there until the next stream or the next sync from the Ally.

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
