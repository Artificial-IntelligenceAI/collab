#!/bin/sh
# Builds a release and signs it.
#
#   ./release.sh 3.1.0 "what changed"
#
# The private key is read from stdin, not from an argument: an argument shows up
# in the process list while it runs, and in shell history for ever. Paste it
# when asked — it comes out of your password manager and goes nowhere else.
set -e
cd "$(dirname "$0")"
VERSION="$1"; NOTES="$2"
[ -n "$VERSION" ] || { echo "usage: ./release.sh <version> \"notes\"" >&2; exit 2; }

./build.sh >/dev/null
OUT="release.noindex"
rm -rf "$OUT"; mkdir -p "$OUT/macos-arm64" "$OUT/windows-x64"

cp dist.noindex/macos/collab "$OUT/macos-arm64/collab"
tar -czf "$OUT/macos-arm64/Collab.app.tar.gz" -C dist.noindex/macos Collab.app
for f in collab.exe collab-notify.exe collab.png; do
  [ -f "dist.noindex/windows/$f" ] && cp "dist.noindex/windows/$f" "$OUT/windows-x64/$f"
done
rmdir "$OUT/windows-x64" 2>/dev/null || true

printf 'paste the release private key (it will not be echoed to history): '
core/target/release/collab release sign "$OUT" -version "$VERSION" -notes "$NOTES" -key -

echo
echo "upload the contents of $OUT/ to the release, keeping the folder layout."
echo "collab-release.json and its .sig must sit at the top."
