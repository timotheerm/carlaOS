# Any game on a desktop, and looking after a desktop from the Ally

## Games that Steam does not list

Steam games on a desktop are found by themselves. Any other game is added once
on the desktop, by the administrator, in a terminal there (or over remote
desktop):

    carla-companion add "Paper Lanterns" "D:\Games\Lanterns\Lanterns.exe"
    carla-companion add "Open Quarry" "D:\Games\Quarry\quarry.exe" --args "-fullscreen" --cover "D:\Pictures\quarry.jpg"
    carla-companion games        (lists them, with the key of each)
    carla-companion drop KEY     (takes one off the list; nothing is deleted)

The game then appears on the Ally with the Stream badge, within five minutes or
at once after "Look for games again". Starting it works like any other stream:
the companion starts the program and the stream ends when it closes.

Programs are only ever added at the desktop itself. A linked Ally can ask for a
game on the list to be started; it cannot name a program of its own. A game
whose program has been moved or deleted is hidden until it is back.

CarlaOS starts the program you point it at, as it is. It does not download
games and does not get around copy protection.

### Finding the program: `find` and the dashboard

You do not have to hunt for the right program. At the desktop:

    carla-companion find                       (the usual game folders)
    carla-companion find "D:\Games\My Game"    (one game's folder, or a folder of games)

It lists the likely programs, best first, each with a line to paste. The real
game's program is preferred over a launcher: an Unreal game's
`*-Win64-Shipping.exe`, a Unity game's player next to its `*_Data` folder. Uninstallers,
redistributables and crash handlers are left out. A `.bat`, `.cmd` or shortcut
can be added too. Nothing is started while looking, and nothing is sent anywhere.

The dashboard (`carla-companion dashboard`, on this PC only) has the same under
"Add a game": **Look for games on this PC**, then **Add** next to one. The page
can only add something the companion itself just found; it never names a
program or options of its own.

### Launchers that hand over to another program

Some games start through a small launcher that exits once the real game runs,
often from another folder. CarlaOS counts a game as running while any program
inside the added program's folder runs, so such a game would look finished at
once, or never started. Say which program is the real game:

    carla-companion add "Tin Knight" "D:\Games\Tin Knight\TinKnight.exe" --watch "TinKnight-Win64-Shipping.exe"

The game then counts as running while any program with that file name runs
(case does not matter, the folder does not), and "End game" closes it too. `find`
fills in `--watch` when it can tell. `games` shows it. Adding the real game's
program directly, when it starts fine on its own, needs no `--watch`.

Rules for the watched name, because the game runs for as long as such a program
does and "End game" closes it:

- an `.exe` file name only (no folder), of letters, digits, spaces and `. _ ( ) + -`;
- not a program in the Windows folders, and not Windows', a store's or a
  browser's own (explorer, svchost, steam, chrome and the like): these are refused;
- **End game closes every program with that name**, including other copies you
  started yourself; programs under the Windows folders are never closed.

`find` only prints a line to paste when the title and path hold no quote,
backtick, `$`, `%`, `;`, `&`, `|`, `<`, `>` or `^`, because a terminal could read
those as commands. Otherwise it says so: add that game from the dashboard, or
rename its folder. Folders that Steam, Epic or GOG Galaxy manage, and games they
already list, are not offered. The dashboard's list expires after 10 minutes, and
a program that changed after it was found is refused: look again.

## From the Ally: Settings, Desktops, then a desktop

Behind the administrator PIN, each linked desktop has:

- **Look for games again**: reads the desktop's list now.
- **Put to sleep**: sends the desktop to sleep (sleep, not hibernate). The
  desktop refuses while a game is running there.
- **Remove**: as before.

## Sleeping again after a stream

When CarlaOS had to wake a desktop to start a stream, it lets it sleep again
about twenty seconds after the stream ends. It never does this to a desktop
that was already awake, and the desktop refuses if the game is still open
there.

`carla-companion sleep-test` on the desktop sends it to sleep the same way, to
check once that the Ally can wake it afterwards.

## For the record

- `POST /v1/power/sleep` is signed like every other request, so only linked
  devices can send it.
- Games added by hand have the id `manual:<key>`, where the key comes from the
  program's path, so the same program has the same id everywhere.
- Not tried on real machines yet: the sleep call itself (`SetSuspendState`),
  and starting hand-added programs through Sunshine.
