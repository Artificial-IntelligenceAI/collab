#!/bin/sh
# Builds Collab.exe — the Windows app. Cross-builds from a Mac; needs dotnet.
set -e
cd "$(dirname "$0")"
OUT="${1:-$PWD/../../dist.noindex/windows}"
DOTNET="$(command -v dotnet || echo /opt/homebrew/opt/dotnet/bin/dotnet)"
[ -x "$DOTNET" ] || { echo "collab: no dotnet — run: brew install dotnet" >&2; exit 1; }
export DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1
mkdir -p "$OUT"
"$DOTNET" publish -c Release -o "$OUT" >/dev/null
# The command line lives in bin\ beside the app. Windows filenames are
# case-insensitive, so collab.exe and Collab.exe cannot share a folder —
# copying one over the other destroys it, which is how this was found.
mkdir -p "$OUT/bin"
[ -f "$OUT/collab.exe" ] && mv "$OUT/collab.exe" "$OUT/bin/collab.exe"
echo "built: $OUT/Collab.exe ($(du -h "$OUT/Collab.exe" | cut -f1))"
