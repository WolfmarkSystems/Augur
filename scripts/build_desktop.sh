#!/bin/bash
# Sprint 18 P1 — build the AUGUR Desktop .dmg.
# Run from the workspace root:
#   bash scripts/build_desktop.sh
#
# Output:
#   apps/augur-desktop/src-tauri/target/release/bundle/dmg/
#       AUGUR_<VERSION>_aarch64.dmg
#   apps/augur-desktop/src-tauri/target/release/bundle/macos/
#       AUGUR.app

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/apps/augur-desktop"

echo "==> AUGUR Desktop build"

echo "--> Installing frontend dependencies"
cd "$APP"
npm install --silent

echo "--> Building frontend"
npm run build

echo "--> Building Tauri bundle (.app + .dmg)"
cd "$APP/src-tauri"
cargo tauri build

echo ""
echo "Bundle artefacts:"
find "$APP/src-tauri/target/release/bundle" -name '*.dmg' -o -name '*.app' 2>/dev/null \
    | sed "s|$ROOT/||"
