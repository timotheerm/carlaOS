#!/usr/bin/env bash
# Tests scripts/release/changes.sh with a fake `gh` that answers at once, answers late, answers empty or fails.
# Run: bash scripts/release/test-changes.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"
git init -q .
git config user.email t@t
git config user.name t
for n in 1 2 3; do echo "$n" > f; git add f; git commit -q -m "c$n"; done
mapfile -t shas < <(git rev-list --reverse HEAD)
mkdir bin
cat > bin/gh <<'FAKE'
#!/usr/bin/env bash
# Behaviour per commit comes from $FAKE/mode.<sha>: ok | empty | fail | late (answers from the 3rd try)
sha="$(printf '%s' "$2" | sed 's#.*/commits/\([0-9a-f]*\)/pulls#\1#')"
mode="$(cat "$FAKE/mode.$sha")"
count_file="$FAKE/count.$sha"
n=$(( $(cat "$count_file" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$count_file"
row="$(printf '7\tno-release\tA docs change')"
case "$mode" in
  ok) printf '%s\n' "$row" ;;
  empty) : ;;
  fail) exit 1 ;;
  late) if [ "$n" -ge 3 ]; then printf '%s\n' "$row"; fi ;;
esac
FAKE
chmod +x bin/gh
export PATH="$work/bin:$PATH" FAKE="$work" GITHUB_REPOSITORY=o/r CHANGES_TRIES=4 CHANGES_DELAY=0

run() { bash "$here/changes.sh" "" HEAD 2>/dev/null; }
unresolved=$'^\\?\t-\tChange [0-9a-f]{7}: pull request not found$'

# 1. every commit resolves: one row for the one pull request
for s in "${shas[@]}"; do echo ok > "mode.$s"; done
[ "$(run | wc -l)" = 1 ] || { echo "FAIL resolved commits"; run; exit 1; }
echo "ok   resolved commits collapse to one row"

# 2. a lookup that is empty at first and answers later is found by retrying
echo late > "mode.${shas[2]}"; rm -f count.*
if run | grep -q '^?'; then echo "FAIL a late answer was reported unresolved"; exit 1; fi
echo "ok   a late answer is found by retrying"

# 3. a commit that never resolves is an unresolved change, never dropped
for mode in empty fail; do
  echo "$mode" > "mode.${shas[2]}"; rm -f count.*
  run | grep -Eq "$unresolved" || { echo "FAIL $mode lookup not reported"; run; exit 1; }
  echo "ok   a $mode lookup becomes an unresolved change"
done

# 4. the unresolved row can never make "everything is no-release" true
rows="$(run)"
total="$(printf '%s' "$rows" | grep -c .)"
skipped="$(printf '%s' "$rows" | awk -F '\t' '("," tolower($2) ",") ~ /,no-release,/' | grep -c . || true)"
[ "$skipped" -lt "$total" ] || { echo "FAIL a no-release skip would not be blocked"; exit 1; }
echo "ok   an unresolved change blocks a no-release skip ($skipped of $total are no-release)"
echo "all passed"
