#!/bin/sh
# Builds everything into dist.noindex/.
#
# The .noindex suffix is not decoration: Spotlight skips directories named that
# way, and without it the built Collab.app shows up in search beside the
# installed one — same name, same identifier, two hits, no way to tell which is
# which. A file called .metadata_never_index does not help; that only works at
# the root of a volume.
#
#   dist.noindex/macos/    collab (Rust core) + Collab.app (menu bar app + window)
#   dist.noindex/windows/  collab.exe + collab-notify.exe + collab.png
#
# The Mac half needs Rust and Xcode's Swift. The Windows half needs the
# x86_64-pc-windows-gnu Rust target and the .NET SDK, and is skipped with a
# warning if either is missing rather than shipping a half-built Windows folder.
set -e
cd "$(dirname "$0")"
mkdir -p dist.noindex/macos dist.noindex/windows

echo "→ core (Rust)"
( cd core && cargo build --release --quiet )
cp core/target/release/collab dist.noindex/macos/collab

echo "→ Collab.app (Swift)"
app/mac/build.sh "$PWD/dist.noindex/macos/Collab.app" >/dev/null && echo "  built dist.noindex/macos/Collab.app"

echo "→ windows"
if rustup target list --installed 2>/dev/null | grep -q x86_64-pc-windows-gnu; then
  ( cd core && cargo build --release --quiet --target x86_64-pc-windows-gnu )
  cp core/target/x86_64-pc-windows-gnu/release/collab.exe dist.noindex/windows/collab.exe
  echo "  built dist.noindex/windows/collab.exe"
else
  echo "  SKIPPED collab.exe — run: rustup target add x86_64-pc-windows-gnu && brew install mingw-w64" >&2
fi
if app/windows/build.sh "$PWD/dist.noindex/windows" >/dev/null 2>&1; then
  echo "  built dist.noindex/windows/Collab.exe"
else
  echo "  SKIPPED Collab.exe — needs: brew install dotnet" >&2
fi
if notify/windows/build.sh "$PWD/dist.noindex/windows" >/dev/null 2>&1; then
  echo "  built dist.noindex/windows/collab-notify.exe"
else
  echo "  SKIPPED collab-notify.exe — needs: brew install dotnet" >&2
fi

echo
du -sh dist.noindex/macos dist.noindex/windows 2>/dev/null
