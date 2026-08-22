#!/bin/sh
# Builds Collab.exe — the Windows app. Cross-builds from a Mac; needs dotnet.
set -e
cd "$(dirname "$0")"
OUT="${1:-$PWD/../../dist.noindex/windows}"
DOTNET="$(command -v dotnet || echo /opt/homebrew/opt/dotnet/bin/dotnet)"
[ -x "$DOTNET" ] || { echo "collab: no dotnet — run: brew install dotnet" >&2; exit 1; }
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
./make-icon.sh
mkdir -p "$OUT"
"$DOTNET" publish -c Release -o "$OUT"
# The command line lives in bin\ beside the app, and build.sh puts it there
# directly. It must never be moved from $OUT: macOS is case-insensitive too, so
# `mv $OUT/collab.exe $OUT/bin/` moves *Collab.exe*, the app, on top of the
# command line — which is exactly what it did.
mkdir -p "$OUT/bin"
echo "built: $OUT/Collab.exe ($(du -h "$OUT/Collab.exe" | cut -f1))"
