#!/usr/bin/env bash
# Decide what this run of the Build workflow is: which commit it builds, which
# version number that gets, and whether it ends in a GitHub release.
# The rules are explained for people in docs/RELEASES.md.
#
# Reads (all set by the workflow):
#   GITHUB_EVENT_NAME, GITHUB_REF, GITHUB_SHA, GITHUB_REPOSITORY, GH_TOKEN
#   IN_VERSION   version asked for by hand (workflow_dispatch), or empty
#   IN_REF       commit, tag or branch to build instead of the current one
#   IN_PREVIOUS  where "what is new" starts, when the tag before does not exist yet
#   IN_HOST      "false" to leave the desktop host out (versions from before it existed)
# Writes to GITHUB_OUTPUT:
#   version, release, republish, ref, target, previous, host, tests
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"

out() {
  echo "$1=$2" >> "${GITHUB_OUTPUT:-/dev/stdout}"
  echo "  $1: ${2:-(empty)}"
}
stop() {
  echo "::error::$1"
  exit 1
}

# Highest version tag there is, e.g. v0.4.0. Empty before the first release.
latest="$(git tag -l 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n 1)"

# The tag just below a version, so "what is new" starts in the right place
# even when an older version is published after a newer one.
tag_below() {
  { git tag -l 'v[0-9]*.[0-9]*.[0-9]*'; echo "v$1"; } | sort -V -u | awk -v tag="v$1" '$0 == tag { print before; exit } { before = $0 }'
}

# Next number after the latest tag. Every merged pull request is a new
# feature version (0.4.0 -> 0.5.0) unless everything merged since the last
# release is a fix (0.4.0 -> 0.4.1). Sets `version` and `skip`.
next_version() {
  skip=false
  if [ -z "$latest" ]; then
    version="0.1.0"
    return
  fi
  local major minor patch changes total fixes skipped
  IFS=. read -r major minor patch <<< "${latest#v}"
  changes="$("$here/changes.sh" "$latest" "$1")"
  total="$(printf '%s' "$changes" | grep -c . || true)"
  # A fix is a pull request labelled "fix" or "bug", or whose title starts with "Fix".
  fixes="$(printf '%s' "$changes" | awk -F '\t' 'tolower($3) ~ /^fix/ || ("," tolower($2) ",") ~ /,(fix|bug),/' | grep -c . || true)"
  skipped="$(printf '%s' "$changes" | awk -F '\t' '("," tolower($2) ",") ~ /,no-release,/' | grep -c . || true)"
  if [ "$total" -gt 0 ] && [ "$fixes" -eq "$total" ]; then
    version="$major.$minor.$((patch + 1))"
  else
    version="$major.$((minor + 1)).0"
  fi
  if [ "$total" -gt 0 ] && [ "$skipped" -eq "$total" ]; then
    skip=true
  fi
}

release=false
republish=false
ref=""
target="$GITHUB_SHA"
previous="$latest"
host=true
tests=true

echo "Latest release so far: ${latest:-none}"

case "$GITHUB_EVENT_NAME" in
  workflow_dispatch)
    if [ -n "${IN_VERSION:-}" ]; then
      # A version published by hand: an older commit, or a release done again.
      version="${IN_VERSION#v}"
      [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || stop "\"$IN_VERSION\" is not a version. Use three numbers, such as 0.5.0."
      if [ -n "${IN_REF:-}" ]; then
        target="$(git rev-parse --verify --quiet "$IN_REF^{commit}" || git rev-parse --verify --quiet "origin/$IN_REF^{commit}")" \
          || stop "\"$IN_REF\" is not a commit, tag or branch in this repository."
        tests=false # an older commit was tested when it was new
      fi
      # Releases only ever come from what is on main.
      git merge-base --is-ancestor "$target" origin/main || stop "Commit $target is not on main, so it cannot be released."
      if tagged="$(git rev-parse --verify --quiet "refs/tags/v$version^{commit}")"; then
        [ "$tagged" = "$target" ] || stop "v$version already exists and is a different commit ($tagged)."
        republish=true
      fi
      previous="$(tag_below "$version")"
      if [ -n "${IN_PREVIOUS:-}" ]; then
        previous="$(git rev-parse --verify --quiet "$IN_PREVIOUS^{commit}")" || stop "\"$IN_PREVIOUS\" is not a commit or tag in this repository."
      fi
      [ "${IN_HOST:-true}" = "false" ] && host=false
      release=true
      ref="$target"
    else
      # "Run workflow" with nothing filled in: release main now, or just build a branch.
      next_version "$target"
      ref="$target"
      if [ "$GITHUB_REF" = "refs/heads/main" ] && [ -z "$(git tag --points-at "$target" 'v[0-9]*')" ]; then
        release=true
      fi
    fi
    ;;
  push)
    next_version "$target"
    ref="$target"
    if [ -n "$(git tag --points-at "$target" 'v[0-9]*')" ]; then
      echo "This commit is already released."
    elif [ "$skip" = true ]; then
      echo "Everything merged since $latest is labelled no-release: building without releasing."
    else
      release=true
    fi
    ;;
  *)
    # Pull requests: build and test what the next version would be. Nothing is published.
    next_version "$target"
    ;;
esac

echo "Plan:"
out version "$version"
out release "$release"
out republish "$republish"
out ref "$ref"
out target "$target"
out previous "$previous"
out host "$host"
out tests "$tests"
