#!/usr/bin/env sh
# Builds the Windows exe from WSL with cargo-xwin and opens it in Windows.
# Usage: scripts/run-windows.sh [--release] [file.pdf]
# The target dir sits on C: so Windows loads pdfium.dll from a local disk.
set -eu
cd "$(dirname "$0")/.."

profile=debug
if [ "${1:-}" = "--release" ]; then
    profile=release
    shift
fi

target_dir=${LEME_WIN_TARGET:-/mnt/c/leme-target}
flags=""
[ "$profile" = release ] && flags="--release"

# shellcheck disable=SC2086
cargo xwin build --target x86_64-pc-windows-msvc --target-dir "$target_dir" $flags

exe="$target_dir/x86_64-pc-windows-msvc/$profile/leme.exe"
if [ $# -gt 0 ]; then
    exec "$exe" "$(wslpath -w "$1")"
fi
exec "$exe"
