# Releases

CarlaOS is in beta. Every change that reaches `main` is built, numbered and
published on the [releases page](https://github.com/timotheerm/carlaOS/releases)
as a pre-release, without anyone doing anything. Older versions are never
removed, so there is always something to go back to.

## What a release contains

| File | For | What it is |
| --- | --- | --- |
| `CarlaOS-Ally-<version>-setup.exe` | The Ally, or a laptop standing in for it | The pink console interface. |
| `CarlaOS-Desktop-Host-<version>-setup.exe` | A desktop PC that your games run on | The companion that shows the Ally this PC's games and starts them for a stream. |
| `CarlaOS-Full-<version>-setup.exe` | Both on one machine | One installer with a tick box for each half. It runs the two installers above, so the result is identical. |
| `SHA256SUMS.txt` | Checking a download | One checksum per file. |

All three install for the signed-in Windows account only and never ask for an
administrator. The Ally interface goes to `%LOCALAPPDATA%\CarlaOS`, the desktop
host to `%LOCALAPPDATA%\Programs\CarlaOS Desktop Host`. Both appear under
*Installed apps* in Windows Settings and can be removed from there.

Beta builds are not signed, so Windows shows "Windows protected your PC" the
first time: choose *More info*, then *Run anyway*.

## Version numbers

Versions are three numbers, `0.<feature>.<fix>`, and the git tag `v0.5.0` is
the only place a number is decided. The `0.1.0` written in `Cargo.toml` and
`tauri.conf.json` is a placeholder for local builds; the build stamps the real
number into the programs and installers (`scripts/release/stamp-version.mjs`),
which is also what Settings shows on the Ally.

When something is merged into `main`:

- **A new feature version** (`0.4.0` to `0.5.0`) is the normal case.
- **A fix version** (`0.5.0` to `0.5.1`) when every pull request merged since
  the last release is a fix: its title starts with "Fix", or it has the label
  `fix` or `bug`.
- **No release** when every pull request merged since the last release has the
  label `no-release`, or when only documentation changed (`docs/` and `.md`
  files). The change simply ships with the next release.

`1.0.0` is for the day CarlaOS stops being a beta, and is published by hand.

The text of a release is written by `scripts/release/notes.sh`: the table of
downloads, then one line per merged pull request, using the pull request's
title. So a pull request title should say what the player or administrator
gets, in plain words.

For people and assistants working on CarlaOS, that is the whole procedure:
merge into `main`. Do not edit version numbers, do not create tags.

## Updating and going back

Run the installer of the version you want on top of what is installed. Going
back to an older version works the same way as updating. The Ally installer
may ask whether to remove the version that is there first; either answer is
fine, and the desktop host installer does not ask.

What is kept: the library, settings and linked desktops on the Ally, and the
name, Sunshine sign-in and linked devices on a desktop. They are stored apart
from the programs, and installing, updating and uninstalling leave them alone.

One caution about going back: the older version then reads data that a newer
one wrote. Whether every older version copes with that has not been tried. If
one misbehaves after going back, say which two versions were involved.

Updating a desktop host stops it and starts it again in the background. A game
that is being streamed at that moment keeps running, but the stream ends and
has to be started again.

## Updating from inside CarlaOS

Nothing has to be downloaded by hand after the first install.

- **On the Ally (or any machine with the interface):** Settings, Updates
  (administrator PIN). The page says which version is installed and whether a
  newer one is out. *Update this machine* downloads it, checks it and installs
  it; CarlaOS closes for a moment and opens again as the new version. On a
  machine that has both halves (the full install), the same press updates the
  desktop host too.
- **Linked desktops, from the Ally:** the same page lists every linked desktop
  with its version. Pressing one that is behind tells it to update itself. It
  is away for a moment while it restarts. A desktop has to be on, and it
  refuses while a game is running on it.
- **On a desktop itself:** *Update CarlaOS Desktop Host* in the Start menu, or
  `carla-companion update`.

How it works (`crates/core/src/update.rs`): the program asks GitHub for this
repository's releases, takes the highest version above its own that has an
installer for its half, downloads that installer and the release's
`SHA256SUMS.txt`, and only runs the installer when its checksum matches. Files
are only ever taken from this repository's release downloads. The update is
the same installer a person would run, started quietly, so everything under
"Updating and going back" applies. Updating never goes back to an older
version; for that, run the older installer by hand.

A desktop host from before the updater existed (0.6.0 and older) does not know
how to update itself: install a
newer Desktop Host on it once by hand, and it can from then on.

Beta builds are not signed, so the checksum protects against a broken
download, not against someone who controls the GitHub repository.

## Publishing by hand

Rarely needed. *Actions*, *Build*, *Run workflow*:

- **Nothing filled in, on `main`:** release what is on `main` now under the
  next number. Useful after a release was skipped.
- **Version and ref filled in:** build that commit of `main` and publish it
  under that number. This is how the versions from before releases existed
  (`0.1.0` to `0.4.0`) were published. *Previous* says where its list of
  changes starts when the release before it does not exist yet. Untick
  *Include the desktop host* for `0.1.0` and `0.2.0`, which had none.
- **A version that already exists, same commit:** build it again and replace
  its files and text. Use this to repair a release.

A release can only be built from a commit that is on `main`.

## How it is built

`.github/workflows/build.yml`, one workflow for everything:

1. **Version** (`scripts/release/plan.sh`) decides the commit, the number and
   whether this run publishes.
2. **Tests and UI build** on Linux.
3. **Windows installers** on a Windows machine: tests, the Tauri build, the
   companion, then `packaging/package.mjs` makes the three installers and
   `packaging/smoke-test.ps1` installs them on that machine, checks the
   version each program reports, updates the desktop host over itself and
   uninstalls it.
4. **Publish the release** creates the tag and the pre-release. On a pull
   request this last job only prints the text the release would have, in the
   run's summary.

Builds of `main` run one at a time. If several pull requests are merged while
one build is running, the ones in the middle may share a release with the
last; its text lists all of them.

## Not yet tried by a person

The updater's rules (which release to take, checksums, refusing files from
elsewhere, a linked Ally asking a desktop to update) are covered by tests, but
no real machine has updated itself yet: the download from GitHub, the
interface closing and reopening, and a desktop updating on request.

The installers are installed, checked and removed on GitHub's Windows machine
at every build, but nobody has yet clicked through them on a real PC: the
pages of the desktop host and full installers, the set-up window that opens at
the end, and the Windows warning for unsigned programs.
