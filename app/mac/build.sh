#!/bin/sh
# Builds Collab.app — the menu bar app and its window.
# Needs Xcode's Swift; nothing from the network.
set -e
cd "$(dirname "$0")"
OUT="${1:-$PWD/../../dist/macos/Collab.app}"
ICONS="$PWD"

# The icon is generated from icon.swift, which is the source of truth for it.
if [ ! -f "$ICONS/collab.icns" ] || [ "$ICONS/icon.swift" -nt "$ICONS/collab.icns" ]; then
  ( cd "$ICONS"
    swiftc -O -o ./.mkicon icon.swift
    ./.mkicon collab.iconset >/dev/null
    iconutil -c icns collab.iconset -o collab.icns
    cp collab.iconset/icon_256x256.png ../../notify/windows/collab.png
    rm -rf collab.iconset ./.mkicon )
fi

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp Info.plist "$OUT/Contents/Info.plist"
cp "$ICONS/collab.icns" "$OUT/Contents/Resources/collab.icns"

swiftc -O -target arm64-apple-macos14 -parse-as-library \
  Sources/Theme.swift Sources/Core.swift Sources/Views.swift Sources/CollabApp.swift \
  -o "$OUT/Contents/MacOS/Collab"

# Ad-hoc signature with a stable identifier, so the notification permission you
# grant survives every rebuild instead of resetting each time.
codesign --force --sign - --identifier com.tankun.collab.app "$OUT"
codesign --verify --strict "$OUT"
echo "built and signed: $OUT"
