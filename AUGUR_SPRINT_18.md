# AUGUR Sprint 18 — .dmg Packaging + macOS Distribution
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

AUGUR Desktop and the Installer Wizard both exist as Tauri apps
but neither ships as a proper macOS .dmg. This sprint produces
distributable .dmg files for both — what an examiner actually
downloads and installs. This is the last step before AUGUR
can be handed to an agency.

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- `cargo clippy -- -D warnings` clean
- MT advisory present in all user-facing text including About dialog

---

## PRIORITY 1 — macOS Bundle Configuration

### Step 1 — Installer app bundle

Configure `apps/augur-installer/src-tauri/tauri.conf.json` for
proper macOS bundle:

```json
{
  "bundle": {
    "active": true,
    "targets": ["dmg", "macos"],
    "identifier": "systems.wolfmark.augur.installer",
    "icon": ["icons/32x32.png", "icons/128x128.png",
             "icons/128x128@2x.png", "icons/icon.icns"],
    "resources": [
      "resources/ffmpeg",
      "resources/tesseract/",
      "resources/tessdata/"
    ],
    "macOS": {
      "minimumSystemVersion": "12.0",
      "entitlements": "entitlements.plist",
      "signingIdentity": null,
      "dmg": {
        "background": "assets/dmg-background.png",
        "windowSize": { "width": 660, "height": 400 },
        "appPosition": { "x": 180, "y": 170 },
        "applicationFolderPosition": { "x": 480, "y": 170 }
      }
    }
  }
}
```

### Step 2 — Desktop app bundle

Configure `apps/augur-desktop/src-tauri/tauri.conf.json`:

```json
{
  "bundle": {
    "active": true,
    "targets": ["dmg", "macos"],
    "identifier": "systems.wolfmark.augur",
    "icon": ["icons/32x32.png", "icons/128x128.png",
             "icons/128x128@2x.png", "icons/icon.icns"],
    "macOS": {
      "minimumSystemVersion": "12.0",
      "entitlements": "entitlements.plist",
      "signingIdentity": null,
      "dmg": {
        "background": "assets/dmg-background.png",
        "windowSize": { "width": 660, "height": 400 },
        "appPosition": { "x": 180, "y": 170 },
        "applicationFolderPosition": { "x": 480, "y": 170 }
      }
    }
  }
}
```

### Step 3 — entitlements.plist

Both apps need the same entitlements for network access
(model downloads) and file system access (evidence files):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.network.client</key>
  <true/>
  <key>com.apple.security.files.user-selected.read-write</key>
  <true/>
  <key>com.apple.security.files.downloads.read-write</key>
  <true/>
  <key>com.apple.security.temporary-exception.files.absolute-path.read-write</key>
  <array>
    <string>/Users/</string>
  </array>
</dict>
</plist>
```

### Step 4 — DMG background image

Create `assets/dmg-background.png`:
- Size: 660x400 pixels
- Dark background matching AUGUR's teal brand
- AUGUR wordmark centered at the top
- Subtle instruction text: "Drag AUGUR to Applications"
- Two circular zones visible: app icon drop target + Applications alias

Generate a simple SVG and convert to PNG:

```bash
# Create the SVG programmatically in Rust build script
# Or create a simple placeholder 660x400 dark teal PNG
# The exact design can be refined later — functional is the goal
```

### Step 5 — App icons

Both apps need proper icon sets. Generate from the teal "A" mark:

```bash
mkdir -p apps/augur-installer/icons
mkdir -p apps/augur-desktop/icons

# Generate icon sizes from a base SVG/PNG:
# 32x32.png, 128x128.png, 128x128@2x.png (256x256)
# icon.icns (macOS icon format)

# Use sips on macOS to resize:
# sips -z 32 32 icon_1024.png --out icons/32x32.png
# etc.
```

Create a simple script `scripts/generate_icons.sh`:
```bash
#!/bin/bash
# Generates all required icon sizes from icons/icon_1024.png
# Run from the apps/augur-installer/ or apps/augur-desktop/ directory

BASE="icons/icon_1024.png"
if [ ! -f "$BASE" ]; then
    echo "Place a 1024x1024 icon at $BASE first"
    exit 1
fi

sips -z 32 32 "$BASE" --out icons/32x32.png
sips -z 128 128 "$BASE" --out icons/128x128.png
sips -z 256 256 "$BASE" --out "icons/128x128@2x.png"

# Generate .icns
mkdir -p icons/icon.iconset
sips -z 16 16 "$BASE" --out icons/icon.iconset/icon_16x16.png
sips -z 32 32 "$BASE" --out icons/icon.iconset/icon_16x16@2x.png
sips -z 32 32 "$BASE" --out icons/icon.iconset/icon_32x32.png
sips -z 64 64 "$BASE" --out icons/icon.iconset/icon_32x32@2x.png
sips -z 128 128 "$BASE" --out icons/icon.iconset/icon_128x128.png
sips -z 256 256 "$BASE" --out icons/icon.iconset/icon_128x128@2x.png
sips -z 256 256 "$BASE" --out icons/icon.iconset/icon_256x256.png
sips -z 512 512 "$BASE" --out icons/icon.iconset/icon_256x256@2x.png
sips -z 512 512 "$BASE" --out icons/icon.iconset/icon_512x512.png
sips -z 1024 1024 "$BASE" --out icons/icon.iconset/icon_512x512@2x.png
iconutil -c icns icons/icon.iconset -o icons/icon.icns

echo "Icons generated successfully"
```

### Step 6 — Build scripts

Create `scripts/build_installer.sh`:
```bash
#!/bin/bash
set -euo pipefail

echo "Building AUGUR Installer..."
cd apps/augur-installer

# Build frontend
npm install
npm run build

# Build Tauri .dmg
cd src-tauri
cargo tauri build

echo "Installer .dmg: target/release/bundle/dmg/AUGUR Installer_1.0.0_aarch64.dmg"
```

Create `scripts/build_desktop.sh`:
```bash
#!/bin/bash
set -euo pipefail

echo "Building AUGUR Desktop..."
cd apps/augur-desktop

# Build frontend
npm install
npm run build

# Build Tauri .dmg
cd src-tauri
cargo tauri build

echo "Desktop .dmg: target/release/bundle/dmg/AUGUR_1.0.0_aarch64.dmg"
```

### Step 7 — About dialog

Add "About AUGUR" to the Help menu. Shows:

```
AUGUR — Forensic Language Analysis
Version 1.0.0
Wolfmark Systems

Built by operators, for operators.

Models: NLLB-200 (Meta AI), Whisper (OpenAI)
All processing is performed locally.
No evidence leaves your machine.

⚠ Machine Translation Notice
All translations produced by AUGUR are machine-generated
and require verification by a certified human translator
before use in legal proceedings.

© 2026 Wolfmark Systems
```

The MT advisory is in the About dialog. Always. Cannot be removed.

### Tests

```rust
#[test]
fn bundle_identifier_correct() {
    // Read tauri.conf.json, verify bundle identifier
    let conf = include_str!("../tauri.conf.json");
    let json: serde_json::Value = serde_json::from_str(conf).unwrap();
    assert_eq!(
        json["bundle"]["identifier"],
        "systems.wolfmark.augur"
    );
}

#[test]
fn minimum_macos_version_set() {
    let conf = include_str!("../tauri.conf.json");
    let json: serde_json::Value = serde_json::from_str(conf).unwrap();
    assert_eq!(
        json["bundle"]["macOS"]["minimumSystemVersion"],
        "12.0"
    );
}
```

### Acceptance criteria — P1

- [ ] `tauri.conf.json` configured for DMG bundling
- [ ] `entitlements.plist` created for both apps
- [ ] `generate_icons.sh` script created
- [ ] `build_installer.sh` and `build_desktop.sh` scripts created
- [ ] About dialog with MT advisory
- [ ] `cargo tauri build` succeeds for both apps (produces bundle)
- [ ] 2 new tests pass
- [ ] Clippy clean

---

## PRIORITY 2 — Version Management + Release Notes

### Step 1 — Unified version file

Create `VERSION` at the workspace root:
```
1.0.0
```

Both `tauri.conf.json` files read from this. Add a build script
that injects the version:

```rust
// src-tauri/build.rs
fn main() {
    let version = std::fs::read_to_string("../../VERSION")
        .unwrap_or_else(|_| "0.0.0".to_string())
        .trim()
        .to_string();
    println!("cargo:rustc-env=AUGUR_VERSION={}", version);
    tauri_build::build()
}
```

### Step 2 — CHANGELOG.md

Create `CHANGELOG.md` at the augur workspace root:

```markdown
# AUGUR Changelog

## v1.0.0 — 2026-04-26

### First release

**Core capabilities:**
- Foreign language detection (176 languages via fastText,
  200 via NLLB-200)
- Speech-to-text transcription (Whisper, 99 languages)
- Machine translation (NLLB-200-distilled-600M, 200 languages)
- Speaker diarization (pyannote, optional)
- Arabic dialect detection (Egyptian, Gulf, Levantine,
  Moroccan, Iraqi) via CAMeL Tools or lexical analysis
- Dialect-aware translation routing

**Offline capabilities:**
- All processing performed locally — no evidence leaves machine
- Air-gap deployment support (AUGUR_AIRGAP_PATH)
- Tiered model installation (minimal/standard/full)

**Forensic features:**
- Chain of custody logging on all evidence interactions
- Evidence package export (ZIP with MANIFEST + chain of custody)
- SHA-256 integrity on downloaded models
- Machine translation advisory — mandatory, non-suppressible

**Desktop application:**
- AUGUR Installer — one-click setup wizard
- AUGUR Desktop — split-view document and transcript workspace
- 37-language picker with quality tiers
- Live streaming translation during inference
- Batch directory processing
- Human review workflow (segment flagging)
- Export: HTML, JSON, ZIP package

**Machine Translation Advisory:**
All translations produced by AUGUR are machine-generated.
Verify with a certified human translator for legal proceedings.
```

### Step 3 — GitHub release preparation

Create `scripts/prepare_release.sh`:
```bash
#!/bin/bash
set -euo pipefail

VERSION=$(cat VERSION)
echo "Preparing release v$VERSION..."

# Build both apps
bash scripts/build_installer.sh
bash scripts/build_desktop.sh

# Create release directory
mkdir -p releases/v$VERSION

# Copy DMGs
cp apps/augur-installer/src-tauri/target/release/bundle/dmg/*.dmg \
    releases/v$VERSION/
cp apps/augur-desktop/src-tauri/target/release/bundle/dmg/*.dmg \
    releases/v$VERSION/

# Generate SHA-256 checksums
cd releases/v$VERSION
shasum -a 256 *.dmg > SHA256SUMS.txt
cat SHA256SUMS.txt

echo ""
echo "Release v$VERSION ready in releases/v$VERSION/"
echo "Upload to GitHub Releases with CHANGELOG.md release notes"
```

### Acceptance criteria — P2

- [ ] `VERSION` file at workspace root
- [ ] `CHANGELOG.md` written with v1.0.0 notes
- [ ] `prepare_release.sh` script created
- [ ] Version injected via build.rs env var
- [ ] MT advisory in CHANGELOG.md
- [ ] All scripts have executable permissions

---

## After both priorities complete

```bash
cd apps/augur-installer/src-tauri && cargo build 2>&1 | tail -5
cd ../../augur-desktop/src-tauri && cargo build 2>&1 | tail -5
```

Commit:
```bash
git add apps/ scripts/ VERSION CHANGELOG.md
git commit -m "feat: augur-sprint-18 macOS packaging + release scripts + CHANGELOG"
```

Report:
- Whether `cargo tauri build` succeeds for both apps
- Location of generated DMG files
- Any code signing issues (expected without Apple Developer account)
- Any deviations from spec

---

_AUGUR Sprint 18 — macOS Distribution_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_After this sprint, AUGUR ships._
