#!/bin/sh
# Builds everything, for both machines, into dist/.
#
#   dist/macos/    collab          + collab.app   (the notifier)
#   dist/windows/  collab.exe      + collab-notify.exe + collab.png
#
# The Go binaries need nothing but Go. collab.app needs Xcode's Swift.
# collab-notify.exe needs the .NET SDK (brew install dotnet) and is skipped
# with a warning if it is missing, rather than silently shipping a Windows
# build with no popups in it.
set -e
cd "$(dirname "$0")"

rm -rf dist
mkdir -p dist/macos dist/windows

echo "→ go"
GOOS=darwin  GOARCH=arm64 go build -trimpath -ldflags="-s -w" -o dist/macos/collab   .
GOOS=windows GOARCH=amd64 go build -trimpath -ldflags="-s -w" -o dist/windows/collab.exe .
cp dist/macos/collab ./collab            # for running out of the source tree

echo "→ collab.app (Swift)"
notify/mac/build.sh "$PWD/dist/macos/collab.app" >/dev/null && echo "  built dist/macos/collab.app"
cp -R dist/macos/collab.app ./collab.app 2>/dev/null || true

echo "→ collab-notify.exe (C#)"
if notify/windows/build.sh "$PWD/dist/windows" >/dev/null 2>&1; then
  echo "  built dist/windows/collab-notify.exe"
else
  echo "  SKIPPED — no .NET SDK. Windows gets no popups until you run:" >&2
  echo "           brew install dotnet && ./build.sh" >&2
fi

echo
du -sh dist/macos dist/windows
