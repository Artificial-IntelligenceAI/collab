#!/bin/sh
# Builds collab.ico from the Mac's collab.icns, so both platforms wear the same
# face and there is one source of truth for it — app/mac/icon.swift.
# macOS has no .ico writer; the format is a small directory plus embedded PNGs.
set -e
cd "$(dirname "$0")"
ICNS="../mac/collab.icns"
OUT="collab.ico"
[ -f "$ICNS" ] || { echo "collab: $ICNS missing — run app/mac/build.sh first" >&2; exit 1; }
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
iconutil -c iconset "$ICNS" -o "$TMP/i.iconset"
# Windows wants these; 48 and 64 have no macOS equivalent so they are resampled.
for s in 16 24 32 48 64 128 256; do
  src="$TMP/i.iconset/icon_${s}x${s}.png"
  [ -f "$src" ] || src="$TMP/i.iconset/icon_512x512.png"
  sips -z "$s" "$s" "$src" --out "$TMP/$s.png" >/dev/null 2>&1
done
python3 - "$TMP" "$OUT" <<'PY'
import struct, sys, os
tmp, out = sys.argv[1], sys.argv[2]
sizes = [16, 24, 32, 48, 64, 128, 256]
blobs = [(s, open(os.path.join(tmp, f"{s}.png"), "rb").read()) for s in sizes]
# ICONDIR: reserved, type=1 (icon), image count
head = struct.pack("<HHH", 0, 1, len(blobs))
offset = len(head) + 16 * len(blobs)
entries, data = b"", b""
for s, blob in blobs:
    # 0 means 256 in a directory entry; there is no room for the real number.
    entries += struct.pack("<BBBBHHII", s if s < 256 else 0, s if s < 256 else 0,
                           0, 0, 1, 32, len(blob), offset)
    data += blob
    offset += len(blob)
open(out, "wb").write(head + entries + data)
print(f"  {out}: {len(blobs)} sizes, {len(head+entries+data)} bytes")
PY
