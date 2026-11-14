# Power for each game

A handheld lives on its battery. A small game, a stream or a cloud game needs
little of the processor, and a heavy game wants all of it. So each game has a
power level and a screen speed, chosen once and used every time it starts.

## What the player sees

- On a game's page, under **Power**, three choices:
  - **Quiet**: the fan stays down and the battery lasts longest.
  - **Balanced**: most games.
  - **Turbo**: the most demanding games, best when plugged in.

  Below them is **Screen**: Smooth (as fast as the screen goes, 120 Hz on the
  Ally), or 60 Hz to save battery.
- In the quick menu (hold View and Menu during a game), **Power** and
  **Screen** rows change them while playing. The change takes effect at once
  and is remembered for that game.
- Until someone chooses, a game played on the Ally is Balanced and Smooth, and
  a cloud game or a stream is Quiet. A stream picks its own screen speed, so
  it has no Screen choice.
- Between games, CarlaOS rests in Quiet: its own pages need almost nothing.
  When CarlaOS closes, the Ally goes back to Balanced.

From the desktop:
`carla-companion ally power ID [quiet|balanced|turbo] [smooth|saver] [--stream|--cloud] [--forget]`.

## How it works

- The power level goes to ASUS's own power mode, which is the one Armoury Crate's
  Silent, Performance and Turbo set. It is sent through the ASUS ACPI device
  (`\\.\ATKACPI`, device 0x00120075). The matching Windows power mode is also set:
  best power efficiency, balanced, or best performance. On a machine without
  the ASUS device, such as the laptop stand-in, only the Windows power mode
  changes.
- The screen speed is changed for the moment only and is never saved. CarlaOS
  puts Windows' own setting back when the game ends. If anything goes wrong,
  restarting the Ally does the same.
- Choices are kept per game and per way of playing (on the Ally, streamed, in
  the cloud), in the `settings` table as `power:<local|stream|cloud>:<game id>`.

## Untested

- Nothing ran on the Ally yet. That includes:
  - whether the ASUS device takes the request as other tools send it, and the
    exact watts each mode gives on the Xbox Ally;
  - whether Armoury Crate later sets its own mode over ours.
- 60 Hz assumes the screen offers it at its current size. Where it does not,
  the screen is left as it is.
