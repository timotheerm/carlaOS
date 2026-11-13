# Only the game, at the Ally's size

Status: written and tested with stand-ins. The Windows parts have been
type-checked but never run: see "Not yet tried on real machines".

## What the player sees

Press **Stream from HOME PC**. Instead of the desktop's wallpaper and icons
there is a dark CarlaOS screen that says "Starting Hades", then the game,
filling the Ally's screen without black bars. When the game closes it says
"See you soon" and CarlaOS is back. The desktop itself is never in the picture.

When Steam has to update the game first, the same screen says so and how far
it is ("Steam is updating it first · 43% · 1.1 GB to go"), and the game starts
when the update is done. The game's page on the Ally says it beforehand, and
its card has an **Update** badge.

## The three parts

### 1. The picture is made for the Ally's screen

The Ally used to ask for 1920x1080 whatever its screen was, and Moonlight
switched the Ally's screen to fit. Now:

- the Ally asks for a picture of its own screen's shape and, up to
  1920x1200, its own size (`display::fit`, `quality::Level::picture_on`). A
  16:10 laptop gets 1920x1200, a 1366x768 one gets exactly that, the Ally
  gets 1920x1080 as before. The lighter pictures are the same shape, smaller.
- Moonlight fills the screen without changing its mode (`--display-mode
  borderless`).
- 120 pictures a second are only asked for when the Ally's screen shows them.

### 2. The desktop's screen follows

When the stream starts, the launcher switches the desktop's main screen to
that size (`crates/core/src/display.rs`, `crates/companion/src/screen.rs`):

1. the exact size if the screen has it, at a refresh rate that shows every
   picture evenly (60 for 60, else 120, else the next faster one);
2. else the smallest size of the same shape that is at least as large;
3. else the same width with the nearest height;
4. else it is left alone.

It is put back when the game closes. Because Sunshine ends the launcher
without warning when a stream is quit, three things stand behind that:

- the switch is the temporary kind: never written to the Windows settings,
  gone after signing out;
- a note (`screen.json`) is written before switching, and the companion in
  the background puts the screen back within seconds of the stream's last
  sign of life, also after a crash or restart;
- `carla-companion stream restore` does it by hand.

The size comes from what Sunshine tells the program it runs
(`SUNSHINE_CLIENT_WIDTH`, `_HEIGHT`, `_FPS`), so a stream started from
Moonlight itself is matched too. The Ally also says it when it asks for a
game, for a Sunshine that does not.

### 3. The curtain

Sunshine can only send a whole screen, not one window. So the launcher puts
its own plain full-screen window over the desktop (`crates/companion/src/stage.rs`):

| What is on the desktop | What the stream shows |
| --- | --- |
| No game window yet | "Starting …", and the update's progress if there is one |
| A window of Steam, Epic, GOG, EA, Ubisoft or a crash report that is not their main window | That window, in front of the curtain: it may need an answer |
| A game window that fills the screen | The game alone. The curtain is hidden, nothing of CarlaOS is in its way |
| A game in a window, or its own launcher | That window on a plain backdrop |
| The game has closed | "See you soon" until Sunshine ends the stream |

A window is "the game" when its program is inside the game's folder, the
same rule that decides when a game has finished.

It never stays in front of something the player needs:

- with no game after 75 seconds (not counting an update) it lifts by itself;
- the Ally's quick menu (hold View and Menu) has **Show the desktop** during
  a stream, which lifts it for the rest of that stream.

A stream started from Moonlight's own list has no game and no curtain: it
shows the desktop, at the Ally's size.

## Updates

- **Steam.** Read from each game's manifest: the update flags, the version it
  has against the one Steam wants, and the bytes downloaded. A game that is
  being updated now stays in the library (it used to vanish until done).
  Steam only notes a new version while it runs: a desktop whose Steam has
  been closed for days shows nothing until Steam starts.
- **Epic.** Only "being updated now" (the launcher's `Pending` manifests).
  The launcher keeps nothing on disk about a version it has not started on.
- **GOG.** Not known: games installed without Galaxy do not update, and
  Galaxy's own records are not read.

The Ally learns it with the desktop's game list (every five minutes) and asks
again when a game's page is opened (`GET /v1/games/update/<id>`). A game on
the Ally itself is waited for in the same way, with the progress on the
"Starting" screen.

## What the administrator can change

On the desktop:

```
carla-companion stream                    what is on, the screen's sizes, what it would take for the Ally
carla-companion stream screen off         leave the screen alone during streams
carla-companion stream curtain off        show the desktop as it is
carla-companion stream test               ten seconds of both, without an Ally (1920x1080 at 60)
carla-companion stream test 1920x1200 60
carla-companion stream restore            put the screen back now
```

What happened during a stream is in `%LOCALAPPDATA%\CarlaOS\companion\launcher.log`.

## Limits

- **One screen.** The main screen is switched and covered. A game that opens
  on a second monitor is not in the picture.
- **A shape the monitor does not have.** A 16:9 monitor has no 16:10 mode, so
  a 16:10 laptop gets 1920x1080 with thin bars. Only a virtual screen made
  for the stream fixes that; that needs a display driver and is the next step.
- **Windows notifications** are drawn above everything and can still show.
- **Games that run with administrator rights** cannot be put in front of the
  curtain by a program without them; a game that fills the screen is not
  affected.
- **The other machine keeps playing.** Leaving a stream without ending the
  game keeps the desktop's screen at the Ally's size until the game closes.

## Not yet tried on real machines

- Switching a real screen and putting it back, and Sunshine picking up the
  new size mid-start.
- The curtain window itself: its look, its place above the taskbar, windows
  being put in front of it, the game getting the keyboard.
- Moonlight with a size that is not 1080p or 720p, and in borderless mode.
- Steam's and Epic's files are read as documented by the community; the
  update tests use files written to match.

`carla-companion stream test` is the quick way to try the first two at the
desktop.
