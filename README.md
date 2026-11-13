# CarlaOS

A pink, console-style gaming shell for the ASUS ROG Xbox Ally (RC73YA) on Windows 11.

The person holding the Ally should only ever have to turn it on and pick a game.
Everything else (libraries, emulators, desktops to stream from, tuning) is set up
beforehand or remotely by an administrator.

## Goals, in order

1. **Zero setup for the player.** No sign-in screens, pairing codes or settings in the way of a game.
2. **One library.** Store games, standalone executables, emulated games and games on other PCs appear together.
3. **Streaming from several desktops** through Sunshine, Moonlight and Tailscale, hidden behind one tap.
4. **Lean.** The shell gets out of the way while a game runs, and Windows is trimmed reversibly.

## Layout

| Path | What it is |
| --- | --- |
| `apps/shell` | The handheld interface: Tauri 2, React, TypeScript. |
| `crates/core` | Rust code shared by both ends: catalogue model, scanners, protocol. |
| `crates/companion` | The background program for each desktop that streams games. |
| `packaging` | The desktop host and full installers, and the check that tries them. |
| `scripts/release` | Version numbering and release notes, run by the Build workflow. |
| `docs` | Architecture and decisions. |

## Downloads

Every feature that is merged becomes a numbered beta on the
[releases page](https://github.com/timotheerm/carlaOS/releases), and every
earlier version stays there to go back to. Each release has three installers:

| File | For |
| --- | --- |
| `CarlaOS-Ally-<version>-setup.exe` | The Ally, or a laptop standing in for it. |
| `CarlaOS-Desktop-Host-<version>-setup.exe` | A desktop PC that your games run on. |
| `CarlaOS-Full-<version>-setup.exe` | Both on one machine, with a tick box for each. For testing. |

How versions are numbered, how to go back to an older one and how to publish
one by hand: [docs/RELEASES.md](docs/RELEASES.md).

## Building

Every pull request is built and tested in GitHub Actions, installers included
(the `carlaos-installers` artifact of its **Build** run).

To work on it locally on Windows:

```
cd apps/shell
npm install
npx tauri dev
```

`cargo test` at the repository root covers the portable crates.

## Boundaries

CarlaOS launches games and emulators that are already installed. It does not
download games, ROMs, firmware or keys, and it does not bypass DRM or anti-cheat.
