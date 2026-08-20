#!/bin/sh
# Installs collab on this Mac: the core, the app, and the LaunchAgent that
# keeps the server running. Safe to re-run; this is also how you upgrade.
set -e
cd "$(dirname "$0")"
[ -f dist/macos/collab ] || ./build.sh

BIN="$HOME/.local/bin"
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
mkdir -p "$BIN" "$HOME/Applications" "$HOME/Library/LaunchAgents"

# Delete before copying, every time. Writing over a signed binary in place
# leaves macOS holding a stale code signature, and the kernel then kills the
# result on exec with no error message at all — a miserable thing to diagnose.
rm -f  "$BIN/collab"
cp     dist/macos/collab "$BIN/collab"

osascript -e 'quit app "collab"' 2>/dev/null || true
rm -rf "$HOME/Applications/Collab.app"
cp -R  dist/macos/Collab.app "$HOME/Applications/Collab.app"
"$LSREG" -f "$HOME/Applications/Collab.app"

# The server, and the app. The app matters as much: it is what raises the
# popups, so a machine where only the server came back at login is a machine
# that receives messages silently.
for label in com.tankun.collab com.tankun.collab.app; do
  cp "$label.plist" "$HOME/Library/LaunchAgents/"
  launchctl bootout   "gui/$(id -u)/$label" 2>/dev/null || true
  launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/$label.plist"
done

echo "installed:"
echo "  $BIN/collab"
echo "  $HOME/Applications/Collab.app"
echo "  server and app both start at login (launchd)"
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) echo; echo "note: $BIN is not on your PATH — add it, or use the full path" ;;
esac
echo
"$BIN/collab" who || true
