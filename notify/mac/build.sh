#!/bin/sh
# Builds collab.app — the tiny bundle that raises macOS notifications.
# Needs Xcode's Swift; nothing else, and nothing from the network.
set -e
cd "$(dirname "$0")"
OUT="${1:-$PWD/../../collab.app}"

# The icon is generated from icon.swift, which is the source of truth for it.
if [ ! -f collab.icns ] || [ icon.swift -nt collab.icns ]; then
  swiftc -O -o ./.mkicon icon.swift
  ./.mkicon collab.iconset >/dev/null
  iconutil -c icns collab.iconset -o collab.icns
  cp collab.iconset/icon_256x256.png ../windows/collab.png   # the Windows toast icon
  rm -rf collab.iconset ./.mkicon
fi

rm -rf "$OUT"
mkdir -p "$OUT/Contents/MacOS" "$OUT/Contents/Resources"
cp Info.plist "$OUT/Contents/Info.plist"
cp collab.icns "$OUT/Contents/Resources/collab.icns"

swiftc -O -target arm64-apple-macos11 -o "$OUT/Contents/MacOS/collab-notify" main.swift

# Ad-hoc signature. UserNotifications refuses to deliver for an unsigned bundle,
# and a stable identifier is what keeps macOS treating this as the same app
# across rebuilds — otherwise the permission you granted would reset each time.
codesign --force --sign - --identifier com.tankun.collab "$OUT"
codesign --verify --strict "$OUT" && echo "built and signed: $OUT"
