#!/bin/sh
# Packages the Windows side into one zip a person can be handed:
#   Collab.exe, bin/collab.exe, the icon, and an installer they can read.
# The Windows equivalent of dmg.sh.
set -e
cd "$(dirname "$0")"
./build.sh >/dev/null
SRC="dist.noindex/windows"
OUT="dist.noindex/collab-setup.zip"
STAGE="dist.noindex/setup-stage"
[ -f "$SRC/Collab.exe" ] || { echo "collab: no $SRC/Collab.exe — the app did not build" >&2; exit 1; }
[ -f "$SRC/bin/collab.exe" ] || { echo "collab: no $SRC/bin/collab.exe" >&2; exit 1; }
rm -rf "$STAGE" "$OUT"
mkdir -p "$STAGE/collab/bin"
cp "$SRC/Collab.exe"      "$STAGE/collab/"
cp "$SRC/bin/collab.exe"  "$STAGE/collab/bin/"
[ -f "$SRC/collab.png" ] && cp "$SRC/collab.png" "$STAGE/collab/"
cp app/windows/installer/Install.cmd    "$STAGE/collab/"
cp app/windows/installer/Install.ps1    "$STAGE/collab/"
cp app/windows/installer/Uninstall.ps1  "$STAGE/collab/"
( cd "$STAGE" && zip -q -r "../../$OUT" collab )
rm -rf "$STAGE"
echo "built: $OUT ($(du -h "$OUT" | cut -f1))"
echo "  hand this over; they extract it and double-click Install.cmd"
