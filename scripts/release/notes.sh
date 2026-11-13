#!/usr/bin/env bash
# Write the text of a GitHub release to standard output: which file is for
# which machine, how to install or go back, and what is new.
#
#   notes.sh <version> <commit> <previous tag or commit, may be empty> <host: true|false>
#
# Needs a full clone, `gh` signed in (GH_TOKEN) and GITHUB_REPOSITORY.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

version="${1:?version}"
target="${2:?commit}"
previous="${3:-}"
host="${4:-true}"

site="https://github.com/$GITHUB_REPOSITORY"
files="$site/releases/download/v$version"
ally="CarlaOS-Ally-$version-setup.exe"
desktop="CarlaOS-Desktop-Host-$version-setup.exe"
full="CarlaOS-Full-$version-setup.exe"

cat <<EOF
> **Beta.** CarlaOS is still being tested and has not been tried on a real Ally yet. Expect rough edges.

## Which file do I need?

| For | Download |
| --- | --- |
| **The Ally**, or a laptop standing in for it | [$ally]($files/$ally) |
EOF

if [ "$host" = true ]; then
  cat <<EOF
| **A desktop PC** that your games run on | [$desktop]($files/$desktop) |
| **Both on one machine**, for testing | [$full]($files/$full) |

The full installer has a tick box for each half, and installs exactly what the two separate files do.
EOF
else
  cat <<EOF

This version is from before the desktop host existed, so there is only the Ally interface.
EOF
fi

cat <<EOF

- **Windows will warn about an unknown publisher**, because beta builds are not signed. Choose *More info*, then *Run anyway*.
- **Updating or going back:** run the installer of the version you want, on top of what is there. If it asks whether to remove the other version first, either answer is fine. Your library, settings and linked desktops stay on the machine.
- Every version stays available on the [releases page]($site/releases). More in [docs/RELEASES.md]($site/blob/main/docs/RELEASES.md).

## What is new

EOF

changes="$("$here/changes.sh" "$previous" "$target")"
if [ -n "$changes" ]; then
  # Titles are other people's text: keep them from being read as Markdown or mentions.
  printf '%s\n' "$changes" | while IFS="$(printf '\t')" read -r number _labels title; do
    title="$(printf '%s' "$title" | sed -e 's/[<>]//g' -e 's/@/@ /g')"
    echo "- $title ([#$number]($site/pull/$number))"
  done
else
  echo "- Changes made directly, without a pull request."
fi

if [ -n "$previous" ]; then
  echo
  echo "[Every change since the version before]($site/compare/$previous...v$version)"
fi
