#!/usr/bin/env bash
# Type-check the shell for Windows from Linux or macOS, without a Windows
# toolchain. It only checks Rust types: C sources are "compiled" by a stand-in
# that writes empty objects, so nothing this produces can be run.
#
# Needs: rustup target add x86_64-pc-windows-gnu
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
fake="$(mktemp -d)"
trap 'rm -rf "$fake"' EXIT

cat > "$fake/fake-compile" <<'SH'
#!/usr/bin/env bash
case "$*" in *detect_compiler_family*) echo gcc; echo gcc >&2; exit 0;; esac
prev=""
for a in "$@"; do
  if [ "$prev" = "-o" ]; then : > "$a"; fi
  case "$a" in -Fo?*) : > "${a#-Fo}";; -o?*) : > "${a#-o}";; esac
  prev="$a"
done
SH
chmod +x "$fake/fake-compile"
cp "$fake/fake-compile" "$fake/x86_64-w64-mingw32-gcc"
cp "$fake/fake-compile" "$fake/x86_64-w64-mingw32-windres"

mkdir -p "$root/apps/shell/dist"
cd "$root/apps/shell/src-tauri"
# The stand-ins go last on PATH so the host's own compiler is never replaced.
PATH="$PATH:$fake" CC_x86_64_pc_windows_gnu="$fake/fake-compile" AR_x86_64_pc_windows_gnu=ar \
  cargo check --target x86_64-pc-windows-gnu "$@"
