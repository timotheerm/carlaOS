# The picture of a stream

Status: written and tested with stand-ins. See "Not yet tried on real
machines" at the end.

## What the player sees

Nothing new. A stream from a desktop simply looks as good as the connection
allows: full sharpness at home, a lighter picture on a hotel or phone
connection, without a setting to find.

## How the picture is chosen

Just before Moonlight starts, the Ally measures the connection to the desktop
over the same path the stream will take (home network or Tailscale):

1. Four empty signed questions give the delay there and back, and how steady
   it is.
2. One question that brings back 128 KB gives a first idea of the speed.
3. When that was quick, a second one brings back up to 2 MB, sized to take
   under a second, because a short download says little about a fast
   connection.

The whole measurement takes between a fraction of a second and about two
seconds. It then picks the sharpest picture that needs no more than a little
over half of what was measured:

| Name | Picture | Video | Connection needed |
| --- | --- | --- | --- |
| smooth | 1080p, 120 a second | 40 Mbit/s | 72 Mbit/s |
| sharp | 1080p, 60 a second | 25 Mbit/s | 45 Mbit/s |
| balanced | 1080p, 60 a second | 12 Mbit/s | 21.6 Mbit/s |
| light | 720p, 60 a second | 7 Mbit/s | 12.6 Mbit/s |
| lightest | 720p, 30 a second | 3 Mbit/s | anything |

The sizes are for a 1080p screen like the Ally's. A screen of another shape
or a smaller one gets a picture made for it, see
[STREAM-SCREEN.md](STREAM-SCREEN.md).

Two more rules:

- An unsteady connection (answers more than 40 ms apart) gets "balanced" at
  most, however fast it is.
- "smooth" is only chosen when the administrator allowed it for that desktop,
  at home, with a delay of 15 ms or less. It only makes sense when the
  desktop's own screen runs at 120.

When a desktop cannot be measured (its companion is older than this feature),
the stream gets what it always had: "sharp" at home, "balanced" through
Tailscale.

The measurement is cautious on purpose. A picture that is a little soft plays;
one that is too heavy stutters.

## What the administrator can change

On the Ally: Settings, Desktops, a desktop, **Picture**. Each press steps to
the next choice: Automatic, Automatic up to 120 a second, or one fixed
picture. Under the row is what the last stream got and what was measured.

From an allowed desktop:

```
carla-companion ally picture "HOME PC"            what is set and what the last stream got
carla-companion ally picture "HOME PC" auto120
carla-companion ally picture "HOME PC" light
```

A fixed picture is never measured or second-guessed.

## How it is built

- `crates/core/src/link/quality.rs`: the levels, the measurement and the
  choice, all tested against invented connections.
- The companion answers `GET /v1/probe/<bytes>` (at most 2 MB) for linked
  devices only, signed like every other reply.
- The shell measures in `hosts::picture_for`, keeps the setting and the last
  result per desktop in the catalogue (`hosts.quality`, `hosts.streamed`) and
  hands Moonlight `--resolution`, `--fps` and `--bitrate`.

## Not yet tried on real machines

- The test measures over the loopback of one machine. Real Wi-Fi and
  Tailscale connections have not been measured, so the thresholds are
  reasoned, not tuned.
- Moonlight's `--bitrate` option is used as its documentation describes it; a
  stream with it has not been started here.
- The picture is chosen once, before the stream. It does not change while a
  game is being played.
