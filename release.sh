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
# The Windows side, in the layout it installs to: the app at the top, the
# command line in bin/. Shipping collab.exe alone — which this did — would send
# an update carrying no app at all.
mkdir -p "$OUT/windows-x64/bin"
[ -f dist.noindex/windows/Collab.exe ]     && cp dist.noindex/windows/Collab.exe     "$OUT/windows-x64/Collab.exe"
[ -f dist.noindex/windows/bin/collab.exe ] && cp dist.noindex/windows/bin/collab.exe "$OUT/windows-x64/bin/collab.exe"
[ -f dist.noindex/windows/collab.png ]     && cp dist.noindex/windows/collab.png     "$OUT/windows-x64/collab.png"
[ -f "$OUT/windows-x64/Collab.exe" ] || { echo "collab: the Windows app is missing from dist — refusing to cut a release without it" >&2; exit 1; }

printf 'paste the release private key (it will not be echoed to history): '
core/target/release/collab release sign "$OUT" -version "$VERSION" -notes "$NOTES" -key -

echo
# The signed manifest names paths; a release host wants flat asset names. Stage
# both: the tree is what was signed, upload/ is what gets uploaded.
UP="$OUT/upload"
mkdir -p "$UP"
cp "$OUT/collab-release.json" "$OUT/collab-release.json.sig" "$UP/"
( cd "$OUT" && find macos-arm64 windows-x64 -type f 2>/dev/null | while read -r f; do
    cp "$f" "upload/$(echo "$f" | tr '/' '-')"
  done )
echo
echo "signed. upload every file in $UP/ as release assets — the names are flat"
echo "on purpose, and collab-release.json and its .sig must be among them."
ls -1 "$UP" | sed 's/^/  /'
