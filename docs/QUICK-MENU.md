# The quick menu

A small menu that opens on top of a running game, local or streamed, for the
things a player wants without leaving it.

**Open it:** hold **View** and **Menu** together (the two small buttons either
side of the middle of the controller) for half a second. Close it the same
way, or with **B**.

| Row | What it does |
| --- | --- |
| Back to the game | Closes the menu. |
| Power | Left and right choose Quiet, Balanced or Turbo, at once and for next time. See [POWER.md](POWER.md). |
| Screen | A or left and right switch between Smooth and 60 Hz. Not shown during a stream. |
| Sound | Left and right change the volume in steps of ten. A mutes. |
| Brightness | Left and right change the built-in screen in steps of ten, never below 5%. |
| End game | Asks once, then closes the game. For a stream, the game is closed on the desktop too and the stream ends. |

The top shows the game, how long it has been running, the time and the
battery. A row that this machine cannot offer (no battery, an external screen
without brightness control) is simply not shown.

## How it works

- While a game is in front, CarlaOS does not receive the controller. A small
  watcher reads it directly from Windows (XInput), **only while a game is
  running**, about twenty times a second. With no game running it does
  nothing.
- The menu is a second, see-through window that stays on top, so the game is
  visible behind it. It is created the first time it is opened and given up
  when the game ends.
- The menu is steered by the watcher, not by the window having the keyboard,
  so it works even when Windows does not hand over the focus.
- Sound uses Windows' own volume of the current output. Brightness uses
  Windows' own backlight control for the built-in screen.
- "End game" closes every program that runs from the game's own folder. It
  does not ask the game nicely, which is why the menu asks first: anything
  the game has not saved is lost. For a stream the Ally asks the desktop
  (signed request `POST /v1/stream/end`), and the companion's launcher, which
  started the game, closes it.

## Why not the Xbox button

On Windows the Xbox button opens the Game Bar, and Windows handles it before
any program sees it. Using it would mean switching that off system-wide. The
two-button hold needs no change to Windows and cannot collide with it. Moving
to the Xbox button later is possible as a reversible setting.

## Limits, and what is not tested

Everything in the menu itself and the shortcut logic is tested, the first in
the browser preview and the second with unit tests. The parts that talk to
Windows are type-checked against the Windows API but **have not been run on
Windows yet**. To check on the laptop that stands in for the Ally:

1. Start a game, hold View + Menu: the menu appears over it.
2. Sound and brightness move, and the Windows values move with them.
3. B returns to the game, and the game takes input again.
4. End game closes it and CarlaOS comes back.

Known limits:

- A game in exclusive fullscreen may minimise when the menu opens. Borderless
  fullscreen, which most games use, is not affected.
- The game also sees View and Menu being pressed. In most games that opens
  the pause menu, which is welcome here.
- A game that reads the controller even when it is not in front will also see
  the presses made in the menu.
- A Steam game that needs a moment to sync its cloud saves gets that moment
  only if it closes itself. Use the game's own exit when saves matter.
