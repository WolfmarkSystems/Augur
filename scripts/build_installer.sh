#!/bin/bash
# Sprint 18 P1 — build the AUGUR Installer .dmg.
# Run from the workspace root:
#   bash scripts/build_installer.sh
#
# Output:
#   apps/augur-installer/src-tauri/target/release/bundle/dmg/
#       AUGUR Installer_<VERSION>_aarch64.dmg
#   apps/augur-installer/src-tauri/target/release/bundle/macos/
#       AUGUR Installer.app

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/apps/augur-installer"

echo "==> AUGUR Installer build"

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
