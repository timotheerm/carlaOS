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

## What the player sees

The games are in My Games with the console's name on the badge and the Offline
badge. Names are tidied: "Tin Knight (USA) (Rev 1).iso" shows as "Tin Knight".
A disc image with a track list (`.cue`, `.m3u`) is one game. Playtime,
milestones, favourites and search work as for any game. CarlaOS comes back by
itself when the emulator closes, so the emulator should be set to close when
the game is quit.

Removing a console removes its cards. No file is deleted.

## Limits for now

- Consoles are set up on the Ally. Emulated games on a desktop can be streamed
  by adding them there one by one with `carla-companion add`.
- No cover pictures yet for emulated games.
- Not tried with real emulators: the suggested options come from each
  emulator's documentation and may need adjusting for a particular version.
