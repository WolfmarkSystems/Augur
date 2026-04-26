#!/bin/bash
# Sprint 18 P2 — assemble a release directory.
#
# Reads the version from `VERSION`, builds both apps via the
# per-app build scripts, copies the resulting .dmg files into
# `releases/v<VERSION>/`, and writes `SHA256SUMS.txt`.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(tr -d '[:space:]' < VERSION)"
if [ -z "$VERSION" ]; then
    echo "VERSION file is empty" >&2
    exit 1
fi

REL="releases/v${VERSION}"
echo "==> Preparing release v${VERSION} → ${REL}/"

bash scripts/build_installer.sh
bash scripts/build_desktop.sh

mkdir -p "${REL}"

cp apps/augur-installer/src-tauri/target/release/bundle/dmg/*.dmg "${REL}/"
cp apps/augur-desktop/src-tauri/target/release/bundle/dmg/*.dmg "${REL}/"

cd "${REL}"
shasum -a 256 *.dmg > SHA256SUMS.txt

echo ""
echo "Artefacts in ${REL}/:"
ls -lh *.dmg
echo ""
echo "Checksums:"
cat SHA256SUMS.txt
echo ""
echo "Upload to GitHub Releases with CHANGELOG.md as release notes."
