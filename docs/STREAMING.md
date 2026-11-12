# Streaming from a desktop

Status: M3, plus waking and Tailscale addresses from M4. Written and tested as far as possible without the hardware. See
"Not yet tried on real machines" at the end.

## What the player sees

A game that is installed on a linked desktop has a **Stream from HOME PC**
button next to (or instead of) **Play**. One press: the Ally asks the desktop
for that game, the picture appears full screen, and CarlaOS is back when the
game is closed. Nothing else is asked.

## What the administrator does, once

On the desktop:

1. Install [Sunshine](https://github.com/LizardByte/Sunshine), open
   `https://localhost:47990` and choose its user name and password.
2. Run `CarlaOS-Desktop-Host-<version>-setup.exe` from the
   [releases page](https://github.com/timotheerm/carlaOS/releases) and leave
   the box on its last page ticked. A set-up window opens. It asks for a name ("HOME PC") and the
   Sunshine user name and password, adds one application called "CarlaOS" to
   Sunshine, sets itself to start with Windows without a window, and shows a
   six-digit code. Windows asks once whether the companion may accept
   connections on private networks: allow it.

On the Ally:

1. Install CarlaOS (`CarlaOS-Ally-<version>-setup.exe`) and
   [Moonlight](https://github.com/moonlight-stream/moonlight-qt).
2. Settings, Desktops (administrator PIN), Add a desktop. The desktop is found
   by itself; choose it and enter the code.

That is all. The Ally and Sunshine are paired without anyone typing a PIN, and
the desktop's Steam games appear in the library with their covers.

To get a new code later, open **CarlaOS Desktop Host** from the Start menu or
run `carla-companion code` (the program is in
`%LOCALAPPDATA%\Programs\CarlaOS Desktop Host`). `carla-companion devices` and `forget ID` list and
remove linked devices; `remove` stops it starting with Windows.

## How it fits together

```
Ally                                   Desktop
shell ── signed HTTP, port 47600 ───▶  companion ── https://localhost:47990 ──▶ Sunshine
  │                                        ▲                                      │
  └─ Moonlight ── GameStream protocol ─────┼──────────────────────────────────────┘
                                           └── "carla-companion launch" (run by Sunshine)
```

- **One Sunshine application.** Sunshine only knows "CarlaOS", whose command is
  `carla-companion launch`. Just before starting Moonlight the Ally tells the
  companion which game it wants (`POST /v1/stream/prepare`). When Sunshine runs
  the launcher, it takes that note, starts the game and stays alive until the
  game's programs are gone. Sunshine then ends the stream, Moonlight exits and
  CarlaOS comes back to the front. No list of games ever has to be kept in step
  inside Sunshine.
- **Leaving a stream does not close the game.** The player may have unsaved
  progress. Streaming the same game again picks it up; asking for a different
  one is refused with "X is still running on HOME PC".
- **The desktop's games** are read with the same Steam scanner the Ally uses,
  so a game has the same id (`steam:<app id>`) everywhere and shows as one card
  with several places to play.

## Waking a desktop and reaching it from elsewhere

- **Addresses.** Each time a desktop answers, it lists every address it has:
  home network and, when Tailscale is installed, its Tailscale address
  (100.64.0.0/10). The Ally asks all known addresses at once and uses the one
  that answers, preferring the home network. So the same link works at home
  and away, and survives the router handing out a new address. Away from home
  both machines need Tailscale signed in to the same account; nothing else is
  set up. A desktop can also be linked for the first time by typing its
  Tailscale address.
- **Waking.** The desktop also reports the hardware addresses of its
  home-network adapters. When it does not answer, the Ally can send a
  Wake-on-LAN packet: from the desktop's page ("Wake HOME PC"), or by itself
  when the player presses Stream. It then waits up to 90 seconds for the
  desktop and for Sunshine, showing "Waking the desktop", and starts the game.
- **What waking needs.** A wired network adapter with Wake-on-LAN turned on in
  the firmware (BIOS) and in the adapter's Windows power settings, the desktop
  asleep or shut down but still powered, and the Ally on the same home
  network. Wi-Fi adapters usually cannot be woken. Waking from another location
  needs an always-on device at home to send the packet; that is not built yet.
- After waking, Windows must be signed in for games to start. A desktop that
  sleeps keeps its session; one that was shut down stops at the sign-in screen
  unless it signs in automatically.

## Trust

| Step | What protects it |
| --- | --- |
| Finding a desktop | Nothing, on purpose. A UDP broadcast only saves typing an address. |
| Linking | The six-digit code feeds a password-authenticated key exchange (SPAKE2). The code never travels. A code works for 10 minutes, for 5 attempts, for one device. Both sides must prove they derived the same key. |
| Every later request | Signed with the device key (HMAC-SHA256) over method, path, time, a one-time nonce and the body. Old or repeated requests are refused. |
| Every reply | Signed too, bound to the request's nonce, so the Ally only believes the desktop it linked. |
| Keys on disk | Sealed with Windows data protection for the Windows account, on both machines. So is the Sunshine password. |
| Sunshine's web API | Only ever contacted on the same machine. Its certificate is self-made and cannot be checked, which is why the companion refuses any address but localhost. |

Requests are signed, not encrypted: what travels is game names and status. The
one sensitive value, the four-digit Moonlight pairing PIN, is useless without
the pairing that is in progress with it. Between locations (M4) everything
goes through Tailscale, which encrypts it.

The companion listens on TCP and UDP 47600. Do not forward that port on a
router; nothing in CarlaOS needs it.

## Code

| Where | What |
| --- | --- |
| `crates/core/src/link/` | Protocol types, signing, linking, discovery, the Ally's client, the Moonlight driver |
| `crates/core/src/secret.rs` | Sealing keys with Windows data protection |
| `crates/companion/src/server.rs` | The companion API and its end-to-end tests |
| `crates/companion/src/sunshine.rs` | Sunshine's web API, old and new pairing flows |
| `crates/companion/src/launcher.rs` | What Sunshine runs |
| `apps/shell/src-tauri/src/hosts.rs` | Linking, status, mirroring games, starting a stream |
| `apps/shell/src/screens/link.tsx` | The administrator's Desktops page |

## Not yet tried on real machines

Everything below is written against Sunshine's and Moonlight's current source
and documentation, but has only run against stand-ins:

- Pairing Moonlight with Sunshine through the companion (both Sunshine's older
  PIN call and the newer one that names the pairing request are handled).
- Sunshine running the launcher, and the stream ending when the game closes.
- Moonlight's window behaviour when it is started and stopped by CarlaOS.
- Windows data protection and start-up with Windows (covered by the Windows
  build's tests, not by a person).

- Waking a real desktop, and reaching one through Tailscale.

Known limits: Steam games only on desktops; waking only from the same network
as the desktop.

Before the first real stream from HOME PC, check the AMD driver: Sunshine has
open reports of crashes with the RX 9070 XT on Adrenalin 26.6.x, with 25.9.1
reported as working.
