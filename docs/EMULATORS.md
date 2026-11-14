# Consoles and emulators

CarlaOS is a launcher here and nothing more. It brings **no emulator, no
system files (firmware, BIOS, keys) and no games**, and it downloads none. The
administrator installs an emulator, sets it up as its own instructions say, and
puts game files they own in a folder. CarlaOS then gives every game file a card
that starts the emulator with it.

## Adding a console

Settings, Manage games, Consoles and emulators, Add a console (behind the PIN):

1. **Emulator**: choose its program. If CarlaOS recognises it by name (Dolphin,
   PCSX2, DuckStation, PPSSPP, mGBA, melonDS, Snes9x, Mesen, Flycast, xemu,
   Cemu, RPCS3, Lime3DS, Azahar) it fills in a name, the file endings and the
   options that start a game full screen.
2. **Name on the badge**: what the cards say, for example "GameCube".
3. **Games folder**: the folder with the game files. Folders inside it are
   looked through too, three levels deep.
4. **Game file endings**: which files are games, for example `iso, chd`.
   Programs (`exe`, `bat` and the like) are refused.
5. **Emulator options**: `{game}` is where the game file goes.

Any other emulator works the same way: fill the fields in by hand.

## Consoles on a desktop

A desktop can run heavier emulators than the Ally (Switch, PlayStation 3,
Wii U at full speed). Set them up there once, at the desktop, and every game
file is streamed to the Ally like any other game:

```
carla-companion console add "GameCube" "C:\Emulators\Dolphin\Dolphin.exe" "D:\Games\GameCube"
carla-companion consoles
carla-companion console drop ID
```

For the emulators listed above, the endings and options are filled in by
themselves; `--endings iso,chd` and `--options "..."` change them. The games
appear on the Ally within five minutes, or at once after "Look for games
again" on the desktop's page there, with the console's name on the badge.
When a stream starts, the desktop starts the emulator with that game file, and
the stream ends when the emulator closes, so set the emulator to close when
the game is quit. The same rules hold as on the Ally: no emulator, system
files or games are brought or downloaded by CarlaOS.

## What the player sees

The games are in My Games with the console's name on the badge and the Offline
badge. Names are tidied: "Tin Knight (USA) (Rev 1).iso" shows as "Tin Knight".
A disc image with a track list (`.cue`, `.m3u`) is one game. Playtime,
milestones, favourites and search work as for any game. CarlaOS comes back by
itself when the emulator closes, so the emulator should be set to close when
the game is quit.

Removing a console removes its cards. No file is deleted.

## Limits for now

- A game set up both on the Ally and on a desktop shows as two cards, one
  played on the Ally and one streamed, because the two machines keep their
  game files in different places.
- No cover pictures yet for emulated games.
- Not tried with real emulators: the suggested options come from each
  emulator's documentation and may need adjusting for a particular version.
