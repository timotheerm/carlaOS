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
| `docs` | Architecture and decisions. |

## Building

Every push builds a Windows installer in GitHub Actions. Open the latest
**Build** run and download `carlaos-shell-installer`.

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
