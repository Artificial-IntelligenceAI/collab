#!/bin/sh
# Builds everything into dist/.
#
#   dist/macos/    collab (Rust core) + Collab.app (menu bar app + window)
#   dist/windows/  collab.exe + collab-notify.exe + collab.png
#
# The Mac half needs Rust and Xcode's Swift. The Windows half needs the
# x86_64-pc-windows-gnu Rust target and the .NET SDK, and is skipped with a
# warning if either is missing rather than shipping a half-built Windows folder.
set -e
cd "$(dirname "$0")"
mkdir -p dist/macos dist/windows

echo "→ core (Rust)"
( cd core && cargo build --release --quiet )
cp core/target/release/collab dist/macos/collab

echo "→ Collab.app (Swift)"
app/mac/build.sh "$PWD/dist/macos/Collab.app" >/dev/null && echo "  built dist/macos/Collab.app"

echo "→ windows"
if rustup target list --installed 2>/dev/null | grep -q x86_64-pc-windows-gnu; then
  ( cd core && cargo build --release --quiet --target x86_64-pc-windows-gnu )
  cp core/target/x86_64-pc-windows-gnu/release/collab.exe dist/windows/collab.exe
  echo "  built dist/windows/collab.exe"
else
  echo "  SKIPPED collab.exe — run: rustup target add x86_64-pc-windows-gnu && brew install mingw-w64" >&2
fi
if notify/windows/build.sh "$PWD/dist/windows" >/dev/null 2>&1; then
  echo "  built dist/windows/collab-notify.exe"
else
  echo "  SKIPPED collab-notify.exe — needs: brew install dotnet" >&2
fi

echo
du -sh dist/macos dist/windows 2>/dev/null
