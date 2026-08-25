#!/bin/sh
# Builds Collab.app — the menu bar app and its window.
# Needs Xcode's Swift; nothing from the network.
set -e
cd "$(dirname "$0")"
OUT="${1:-$PWD/../../dist.noindex/macos/Collab.app}"
ICONS="$PWD"

# The icon is generated from icon.swift, which is the source of truth for it.
if [ ! -f "$ICONS/collab.icns" ] || [ "$ICONS/icon.swift" -nt "$ICONS/collab.icns" ]; then
  ( cd "$ICONS"
    swiftc -O -o ./.mkicon icon.swift
    ./.mkicon collab.iconset >/dev/null
    iconutil -c icns collab.iconset -o collab.icns
    cp collab.iconset/icon_256x256.png ../windows/collab.png
    rm -rf collab.iconset ./.mkicon )
fi

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp Info.plist "$OUT/Contents/Info.plist"
cp "$ICONS/collab.icns" "$OUT/Contents/Resources/collab.icns"
# The block font travels with the app. Info.plist points ATSApplicationFontsPath
# at this directory, so it is available to the process without being installed
# on the machine — the same file the Windows build carries, so a diagram drawn
# on one and read on the other is the same shape.
mkdir -p "$OUT/Contents/Resources/Fonts"
cp Fonts/JetBrainsMono-Regular.ttf "$OUT/Contents/Resources/Fonts/"
cp Fonts/JetBrainsMono-Bold.ttf    "$OUT/Contents/Resources/Fonts/"

swiftc -O -target arm64-apple-macos14 -parse-as-library \
  Sources/Theme.swift Sources/Core.swift Sources/Channels.swift Sources/Views.swift Sources/Update.swift Sources/CollabApp.swift \
  -o "$OUT/Contents/MacOS/Collab"

# Ad-hoc signature with a stable identifier, so the notification permission you
# grant survives every rebuild instead of resetting each time.
# The command-line half travels inside the app. Dragging Collab.app to
# Applications has to be enough on its own, and everything else here — the
# server, the watcher, the MCP server — is that binary.
CORE="$PWD/../../dist.noindex/macos/collab"
[ -f "$CORE" ] || CORE="$PWD/../../core/target/release/collab"
cp "$CORE" "$OUT/Contents/Resources/collab"
chmod +x "$OUT/Contents/Resources/collab"

codesign --force --sign - --identifier com.tankun.collab.app "$OUT"
codesign --verify --strict "$OUT"
echo "built and signed: $OUT"
