#!/bin/sh
# Installs collab on this Mac: the binary, the notifier, and the LaunchAgent
# that keeps the server running. Safe to re-run.
set -e
cd "$(dirname "$0")"
[ -d dist/macos ] || ./build.sh

BIN="$HOME/.local/bin"
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
mkdir -p "$BIN" "$HOME/Applications" "$HOME/Library/LaunchAgents"

# Delete before copying, every time. Writing over a Mach-O binary in place
# leaves macOS holding a stale code signature for that file, and the kernel
# then kills it on sight — with no error message at all, which makes it a
# miserable thing to diagnose. A fresh file gets a fresh signature.
rm -f  "$BIN/collab"
cp     dist/macos/collab "$BIN/collab"

rm -rf "$HOME/Applications/collab.app"
cp -R  dist/macos/collab.app "$HOME/Applications/collab.app"
"$LSREG" -f "$HOME/Applications/collab.app"   # so a click can wake it

cp com.tankun.collab.plist "$HOME/Library/LaunchAgents/"
launchctl bootout   "gui/$(id -u)/com.tankun.collab" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.tankun.collab.plist"

echo "installed:"
echo "  $BIN/collab"
echo "  $HOME/Applications/collab.app"
echo "  server running under launchd (starts at login, restarts if it dies)"
case ":$PATH:" in
  *":$BIN:"*) ;;
  *) echo; echo "note: $BIN is not on your PATH — add it, or use the full path" ;;
esac
echo
echo "check it:  collab test-notify"
