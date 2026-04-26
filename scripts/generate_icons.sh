#!/bin/bash
# Sprint 18 P1 — generate the macOS icon set from `icons/icon_1024.png`.
# Run from `apps/augur-installer/` or `apps/augur-desktop/`.
#
# Produces:
#   icons/32x32.png
#   icons/128x128.png
#   icons/128x128@2x.png   (256x256)
#   icons/icon.icns
#   icons/icon.png         (kept for backward compat with existing
#                           tauri.conf.json `icon: ["icons/icon.png"]`)

set -euo pipefail

BASE="src-tauri/icons/icon_1024.png"
OUT="src-tauri/icons"

if [ ! -f "$BASE" ]; then
    echo "Place a 1024x1024 PNG at $BASE first." >&2
    exit 1
fi

sips -z 32 32   "$BASE" --out "$OUT/32x32.png"           >/dev/null
sips -z 128 128 "$BASE" --out "$OUT/128x128.png"         >/dev/null
sips -z 256 256 "$BASE" --out "$OUT/128x128@2x.png"      >/dev/null
# Keep the bare icon.png so existing `icon: ["icons/icon.png"]`
# entries still resolve while the .icns is being adopted.
sips -z 1024 1024 "$BASE" --out "$OUT/icon.png"          >/dev/null

ICONSET="$OUT/icon.iconset"
mkdir -p "$ICONSET"
sips -z 16 16     "$BASE" --out "$ICONSET/icon_16x16.png"        >/dev/null
sips -z 32 32     "$BASE" --out "$ICONSET/icon_16x16@2x.png"     >/dev/null
sips -z 32 32     "$BASE" --out "$ICONSET/icon_32x32.png"        >/dev/null
sips -z 64 64     "$BASE" --out "$ICONSET/icon_32x32@2x.png"     >/dev/null
sips -z 128 128   "$BASE" --out "$ICONSET/icon_128x128.png"      >/dev/null
sips -z 256 256   "$BASE" --out "$ICONSET/icon_128x128@2x.png"   >/dev/null
sips -z 256 256   "$BASE" --out "$ICONSET/icon_256x256.png"      >/dev/null
sips -z 512 512   "$BASE" --out "$ICONSET/icon_256x256@2x.png"   >/dev/null
sips -z 512 512   "$BASE" --out "$ICONSET/icon_512x512.png"      >/dev/null
sips -z 1024 1024 "$BASE" --out "$ICONSET/icon_512x512@2x.png"   >/dev/null

iconutil -c icns "$ICONSET" -o "$OUT/icon.icns"
rm -rf "$ICONSET"

echo "Icons generated under $OUT/"
ls -1 "$OUT"
