#!/usr/bin/env bash
# List the merged pull requests behind a range of commits, oldest first, as
#   number <tab> labels (comma separated) <tab> title
#
#   changes.sh <previous tag or commit, may be empty> <commit>
#
# Needs a full clone, `gh` signed in (GH_TOKEN) and GITHUB_REPOSITORY.
set -euo pipefail

previous="${1:-}"
target="${2:?Give the commit to list changes up to.}"
range="$target"
if [ -n "$previous" ]; then
  range="$previous..$target"
fi

# Ask about every commit, not only the merges on main: a pull request that was
# merged into another pull request's branch still deserves its line.
# "-" stands for "no labels".
#
# GitHub links a just-merged pull request to its commits a little late, so a
# lookup may fail or come back empty. It is tried again a few times; a commit
# that still has no pull request is listed as an UNRESOLVED change
# ("?<tab>-<tab>Change <sha>: ..."). It counts as a normal change, so it can
# never make "everything merged since the last release is no-release" true by
# being left out.
tries="${CHANGES_TRIES:-4}"
delay="${CHANGES_DELAY:-4}"
lookup() {
  local commit="$1" attempt=1 rows
  while :; do
    if rows="$(gh api "repos/$GITHUB_REPOSITORY/commits/$commit/pulls" \
      --jq '.[] | select(.merged_at != null) | [.number, ([.labels[].name] | join(",") | if . == "" then "-" else . end), .title] | @tsv')" \
      && [ -n "$rows" ]; then
      printf '%s\n' "$rows"
      return
    fi
    if [ "$attempt" -ge "$tries" ]; then
      echo "::warning::Could not find the pull request for commit $commit; counting it as a normal change." >&2
      printf '?\t-\tChange %s: pull request not found\n' "${commit:0:7}"
      return
    fi
    sleep "$((delay * attempt))"
    attempt=$((attempt + 1))
  done
}
git rev-list --max-count=500 "$range" | while read -r commit; do
  lookup "$commit"
done | sort -t "$(printf '\t')" -k1,1n -u
