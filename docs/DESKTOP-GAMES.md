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
