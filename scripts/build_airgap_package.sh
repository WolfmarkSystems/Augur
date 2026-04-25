#!/usr/bin/env bash
# build_airgap_package.sh — Sprint 5 P3
#
# Build a VERIFY air-gap package containing every model weight the
# tool needs at runtime. Run this on an internet-connected machine,
# transfer the resulting tarball via USB, and unpack on the
# air-gapped workstation. See docs/AIRGAP_INSTALL.md.
#
# Usage:
#   bash scripts/build_airgap_package.sh [whisper_preset] [output_dir]
#
# whisper_preset: tiny | base | large-v3   (default: tiny)
# output_dir:     directory for the .tar.gz (default: ./)

set -euo pipefail

PRESET="${1:-tiny}"
OUT_DIR="${2:-.}"

# Egress URLs match the named consts in the Rust source —
# `grep WHISPER_MODEL_URL_` from the workspace root finds the
# Rust-side constants this script mirrors.
LID_MODEL_URL="https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz"
case "$PRESET" in
    tiny)      WHISPER_REPO="openai/whisper-tiny";       WHISPER_REV="main"        ;;
    base)      WHISPER_REPO="openai/whisper-base";       WHISPER_REV="refs/pr/22"  ;;
    large-v3)  WHISPER_REPO="openai/whisper-large-v3";   WHISPER_REV="main"        ;;
    *)         echo "Unsupported preset: $PRESET (expected tiny|base|large-v3)" >&2 ; exit 2 ;;
esac

NLLB_MODEL="facebook/nllb-200-distilled-600M"
DATE_STAMP="$(date +%Y%m%d)"
OUTPUT="$OUT_DIR/verify-airgap-${PRESET}-${DATE_STAMP}.tar.gz"
STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

echo "→ Air-gap staging dir: $STAGING"
mkdir -p "$STAGING/whisper" "$STAGING/nllb"

echo "→ Fetching LID model (lid.176.ftz)..."
curl -fL --silent --show-error --output "$STAGING/lid.176.ftz" "$LID_MODEL_URL"

echo "→ Fetching Whisper preset '$PRESET' (HF: $WHISPER_REPO @ $WHISPER_REV)..."
python3 - <<PY
from huggingface_hub import snapshot_download
snapshot_download(
    repo_id="$WHISPER_REPO",
    revision="$WHISPER_REV",
    local_dir="$STAGING/whisper",
    allow_patterns=["config.json", "tokenizer.json", "model.safetensors"],
)
PY

echo "→ Fetching NLLB-200 ($NLLB_MODEL)..."
python3 - <<PY
from huggingface_hub import snapshot_download
snapshot_download(
    repo_id="$NLLB_MODEL",
    local_dir="$STAGING/nllb",
)
PY

echo "→ Writing install.sh..."
cat > "$STAGING/install.sh" <<'INSTALL'
#!/usr/bin/env bash
# Install VERIFY air-gap models on an offline workstation. Run
# this from the unpacked package directory.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
CACHE="${VERIFY_CACHE_DIR:-$HOME/.cache/verify/models}"
mkdir -p "$CACHE/whisper" "$CACHE/nllb"

cp "$HERE/lid.176.ftz"         "$CACHE/lid.176.ftz"
cp -R "$HERE/whisper/."        "$CACHE/whisper/"
cp -R "$HERE/nllb/."           "$CACHE/nllb/"

echo "✅ VERIFY models installed at $CACHE"
echo "   Set VERIFY_AIRGAP_PATH=$HERE if you want every run to use the package directly"
echo "   instead of the cache copy:"
echo "     export VERIFY_AIRGAP_PATH=$HERE"
echo "   Verify install:"
echo "     verify classify --classifier-backend fasttext --text 'مرحبا' --target en"
INSTALL
chmod +x "$STAGING/install.sh"

echo "→ Packaging $OUTPUT..."
tar -czf "$OUTPUT" -C "$STAGING" .

echo "✅ Air-gap package: $OUTPUT"
echo "   Transfer via USB, then on the air-gapped workstation:"
echo "     mkdir verify-airgap && tar -xzf $(basename "$OUTPUT") -C verify-airgap"
echo "     bash verify-airgap/install.sh"
