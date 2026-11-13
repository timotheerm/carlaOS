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
# A commit GitHub cannot answer for (a pull request's trial merge, say) only
# costs its line, so say so and carry on. "-" stands for "no labels".
git rev-list --max-count=500 "$range" | while read -r commit; do
  gh api "repos/$GITHUB_REPOSITORY/commits/$commit/pulls" \
    --jq '.[] | select(.merged_at != null) | [.number, ([.labels[].name] | join(",") | if . == "" then "-" else . end), .title] | @tsv' \
    || echo "::warning::Could not look up the pull request for commit $commit." >&2
done | sort -t "$(printf '\t')" -k1,1n -u
