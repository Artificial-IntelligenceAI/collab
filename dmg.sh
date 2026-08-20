#!/bin/sh
# Makes the disk image people actually install from: open it, drag the app
# across, done. Everything collab needs — the server, the watcher, the MCP
# server — is the one binary carried inside Collab.app, so nothing else has to
# be installed and nothing is left behind if it is dragged to the bin.
set -e
cd "$(dirname "$0")"
[ -d dist.noindex/macos/Collab.app ] || ./build.sh >/dev/null

VOL="collab"
OUT="dist.noindex/collab.dmg"
STAGE="dist.noindex/dmg-stage"
BG="app/mac/dmg-background.png"
# Note: the window layout below works — size, positions, no toolbar — but the
# background picture does not render on macOS 26 despite being set without
# error and recorded in the .DS_Store. Left in because it is harmless and may
# work elsewhere; the image is a perfectly ordinary drag-to-Applications
# installer without it.

[ -f "$BG" ] || { cd app/mac && swiftc -O -o ./.mkbg dmg-background.swift && ./.mkbg dmg-background.png >/dev/null && rm -f ./.mkbg; cd ../..; }

# Any earlier image of the same name has to go first. Two volumes called
# "collab" and Finder picks one of them for you — which is how a layout can be
# applied, successfully, to entirely the wrong disk.
while [ -d "/Volumes/$VOL" ]; do
  hdiutil detach "/Volumes/$VOL" -force >/dev/null 2>&1 || break
done

rm -rf "$STAGE" "$OUT" dist.noindex/rw.dmg
mkdir -p "$STAGE/bg"
cp -R dist.noindex/macos/Collab.app "$STAGE/Collab.app"
cp "$BG" "$STAGE/bg/backdrop.png"
ln -s /Applications "$STAGE/Applications"

# Read-write first, so Finder can be told how to lay the window out; the layout
# is what makes it read as "drag this there" rather than "here are two icons".
hdiutil create -srcfolder "$STAGE" -volname "$VOL" -fs HFS+ \
  -format UDRW -ov dist.noindex/rw.dmg >/dev/null
DEV=$(hdiutil attach -readwrite -noverify -noautoopen dist.noindex/rw.dmg | grep '/dev/disk' | head -1 | awk '{print $1}')
sleep 2

# The layout is what makes this read as "drag that there" rather than "here are
# two icons". Finder will not answer questions about the background afterwards,
# so this reports whether the script ran, not whether it looks right; that is
# checked by opening the image and looking at it.
LAYOUT=$(osascript <<APPLESCRIPT 2>&1 || true
tell application "Finder"
  activate
  set d to disk "$VOL"
  open d
  delay 1
  set w to container window of d
  set current view of w to icon view
  set toolbar visible of w to false
  set statusbar visible of w to false
  set the bounds of w to {200, 120, 860, 540}
  set o to the icon view options of w
  set arrangement of o to not arranged
  set icon size of o to 108
  -- Relative to the disk rather than a POSIX path: an absolute alias records the
  -- volume mounted while building and resolves to nothing on a later mount.
  -- The folder is visible at this point on purpose. Finder does not enumerate
  -- dot-directories, so a relative reference into one can never resolve; it is
  -- hidden after this runs instead.
  set bgRef to (file "backdrop.png" of folder "bg" of d)
  set background picture of o to bgRef
  set position of item "Collab.app" of w to {180, 236}
  set position of item "Applications" of w to {480, 236}
  delay 2
  close w
  return "ok"
end tell
APPLESCRIPT
)
# Finder will not answer questions about the background it has just been given
# — asking throws where setting did not — so this reports only whether the
# layout ran. Whether it looks right is checked by opening the image and looking.
case "$LAYOUT" in
  ok) echo "  layout applied" ;;
  *) echo "  WARNING: layout step failed ($LAYOUT) — the image still works, it just looks plain" ;;
esac

# Hidden only now: Finder had to be able to see it to reference it at all.
chflags hidden "/Volumes/$VOL/bg" 2>/dev/null || true

sync
hdiutil detach "$DEV" >/dev/null
hdiutil convert dist.noindex/rw.dmg -format UDZO -imagekey zlib-level=9 -o "$OUT" >/dev/null
rm -rf dist.noindex/rw.dmg "$STAGE"
echo "built $OUT ($(du -h "$OUT" | cut -f1))"
