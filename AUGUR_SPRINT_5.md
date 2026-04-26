# AUGUR — Sprint 5
# fasttext-pure-rs Evaluation + Speaker Diarization + Air-gap Package

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Workspace: ~/Wolfmark/augur/_

---

## Context

Sprint 4 shipped: whichlang as default, temperature fallback,
PDF extraction, ctranslate2 benchmark (2.85x confirmed). 57 tests.

AUGUR is v1.0 complete. Sprint 5 focuses on three things that
expand its reach and reliability:

1. Evaluate `fasttext-pure-rs` — if it reads Facebook's lid.176.ftz
   correctly, we get 176-language coverage back with no FFI
2. Speaker diarization — who said what and when (critical for
   multi-party recordings in LE/IC work)
3. Air-gap package — a pre-bundled offline installer for
   workstations that can never touch the internet

---

## Hard rules (always)

- Zero `.unwrap()` in production code paths
- Zero `unsafe{}` without explicit justification
- Zero `println!` in production
- All errors handled explicitly
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes after every change

## OFFLINE INVARIANT — NON-NEGOTIABLE
## MACHINE TRANSLATION ADVISORY — NON-NEGOTIABLE

---

## PRIORITY 1 — fasttext-pure-rs Evaluation

### Context

Sprint 1 confirmed fasttext 0.8.0 produces systematically wrong
classifications — Arabic → Esperanto, Chinese → Serbian. The crate
is not binary-compatible with Facebook's lid.176.ftz.

Sprint 4 identified `fasttext-pure-rs` as a candidate — it
explicitly claims Facebook .ftz compatibility. Sprint 5 evaluates it.

### The 30-second test protocol

```bash
# Add the crate
cargo add fasttext-pure-rs -p augur-classifier

# Run the diagnostic probe
cargo run --example lid_label_probe \
    --features augur-classifier/fasttext-probe \
    -p augur-classifier 2>&1
```

**Pass criteria:** Arabic → ar (not eo), Chinese → zh (not sr),
Russian → ru (not ar), Spanish → es (not en).

**If all 4 pass:** fasttext-pure-rs is compatible. Proceed to
Step 2 (wire it as the fasttext backend).

**If any fail:** Document the failure, remove the crate, update
CLAUDE.md noting fasttext-pure-rs also incompatible. Accept
whichlang's 16 languages as the production ceiling until a
compatible pure-Rust fastText reader exists.

### If the evaluation passes — Step 2

Replace the fasttext 0.8.0 backend with fasttext-pure-rs:

```rust
// In classifier.rs, Backend::FastText arm:
// Replace: use fasttext::FastText;
// With: use fasttext_pure_rs::FastText; (or whatever its API is)
```

Run the full classification test suite. All 8 tests must pass.
Then flip `--classifier-backend fasttext` from EXPERIMENTAL
to PRODUCTION-READY in the --help text and CLAUDE.md.

### Tests

```rust
#[test]
fn fasttext_pure_rs_classifies_arabic_correctly() {
    // Only runs if fasttext-pure-rs probe passes
    // Arabic → ar, confidence > 0.8
}

#[test]  
fn fasttext_pure_rs_classifies_forensic_languages() {
    // Farsi, Pashto, Urdu — the high-value LE/IC languages
    // that whichlang doesn't cover
}
```

### Acceptance criteria — P1

- [ ] fasttext-pure-rs probed against lid.176.ftz
- [ ] Probe result documented in CLAUDE.md
- [ ] If compatible: wired as production fasttext backend
- [ ] If incompatible: documented, removed, whichlang remains default
- [ ] All existing classification tests still pass
- [ ] Clippy clean

---

## PRIORITY 2 — Speaker Diarization

### What it is

Speaker diarization answers "who spoke when" in a multi-party
recording. Without it, a transcript of a meeting between three
people is a wall of undifferentiated text. With it, each segment
is labeled: Speaker 1 (0:00-0:15), Speaker 2 (0:15-0:32), etc.

This is critical for LE/IC forensic audio work — an intercepted
call between two subjects needs to distinguish who said what.

### Approach — pyannote.audio via subprocess

pyannote.audio is the standard open-source diarization toolkit.
Like NLLB-200, it has no pure-Rust implementation — subprocess
is the right approach.

```bash
pip3 install pyannote.audio --break-system-packages
```

Note: pyannote requires a HuggingFace token for model download
(free account). This is the first AUGUR feature that requires
an HF token. Handle carefully — the token must be stored in
`~/.cache/augur/hf_token` (not hardcoded, not in env where
it could leak).

### Implementation

**Step 1 — Token management**

```rust
pub struct HfTokenManager {
    token_path: PathBuf,
}

impl HfTokenManager {
    pub fn token_path() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("verify/hf_token")
    }

    pub fn load(&self) -> Result<String, AugurError> {
        // Read token from file
        // Return clear error if missing with instructions
    }

    pub fn is_configured(&self) -> bool {
        self.token_path.exists()
    }
}
```

Add a CLI command for one-time setup:
```bash
augur setup --hf-token hf_xxxxx
# Writes token to ~/.cache/augur/hf_token
# Confirms by attempting a test API call (online check)
```

**Step 2 — Diarization worker script**

`crates/augur-stt/scripts/diarize.py`:

```python
import sys
from pyannote.audio import Pipeline

def diarize(audio_path: str, hf_token: str) -> list[dict]:
    pipeline = Pipeline.from_pretrained(
        "pyannote/speaker-diarization-3.1",
        use_auth_token=hf_token
    )
    diarization = pipeline(audio_path)
    
    segments = []
    for turn, _, speaker in diarization.itertracks(yield_label=True):
        segments.append({
            "start_ms": int(turn.start * 1000),
            "end_ms": int(turn.end * 1000),
            "speaker": speaker,  # "SPEAKER_00", "SPEAKER_01", etc.
        })
    return segments
```

**Step 3 — DiarizationEngine**

```rust
pub struct DiarizationEngine {
    token_manager: HfTokenManager,
}

pub struct DiarizationSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker_id: String,    // "SPEAKER_00", "SPEAKER_01"
    pub speaker_label: Option<String>, // user-assigned label if set
}

impl DiarizationEngine {
    pub fn is_available(&self) -> bool {
        // Check pyannote installed + token configured
    }

    pub fn diarize(
        &self,
        audio_path: &Path,
    ) -> Result<Vec<DiarizationSegment>, AugurError>;
}
```

**Step 4 — Merge diarization with STT segments**

When both are available, merge them:

```rust
pub struct EnrichedSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,          // from STT
    pub speaker_id: String,    // from diarization
    pub translated_text: Option<String>, // from NLLB
}
```

Match STT segments to diarization segments by timestamp overlap.
The result is a fully enriched transcript:

```
[0:00-0:05] SPEAKER_00: مرحبا بالعالم
             → Hello world

[0:05-0:12] SPEAKER_01: كيف حالك اليوم
             → How are you today

[0:12-0:18] SPEAKER_00: بخير شكرا
             → Fine, thank you
```

**Step 5 — Wire into CLI**

```bash
# With diarization (requires HF token + pyannote)
augur translate --input interview.mp3 --target en --diarize

# Without (default — no HF account needed)
augur translate --input interview.mp3 --target en
```

Diarization is opt-in. Default behavior unchanged.

**Step 6 — Tests**

```rust
#[test]
fn diarization_engine_reports_unavailable_without_pyannote() {
    // Mock missing pyannote
    // is_available() returns false, not panic
}

#[test]
fn enriched_segment_merges_stt_and_diarization_by_overlap() {
    // Unit test with synthetic segments
    // STT [0-5000ms] "hello" + diarization [0-4500ms] SPEAKER_00
    // → EnrichedSegment { text: "hello", speaker_id: "SPEAKER_00" }
}

#[test]
fn hf_token_manager_returns_clear_error_when_missing() {
    // No token file → AugurError with setup instructions
}
```

### Acceptance criteria — P2

- [ ] `augur setup --hf-token` stores token securely
- [ ] Diarization worker script runs against real audio when available
- [ ] STT + diarization segments merged by timestamp
- [ ] CLI `--diarize` flag opt-in, default unchanged
- [ ] `is_available()` returns false gracefully when pyannote missing
- [ ] 3 new tests pass
- [ ] Machine translation advisory still present on all translated output
- [ ] Clippy clean

---

## PRIORITY 3 — Air-gap Package

### What it is

Many forensic workstations in LE/IC environments cannot access
the internet at all. AUGUR's download-on-first-run model doesn't
work there. An air-gap package is a pre-bundled archive containing
all model weights that can be transferred via USB and installed
offline.

### Implementation

**Step 1 — Package builder script**

`scripts/build_airgap_package.sh`:

```bash
#!/bin/bash
# Build a AUGUR air-gap package with all model weights
# Run this on an internet-connected machine, transfer to air-gapped workstation

set -e
OUTPUT="augur-airgap-$(date +%Y%m%d).tar.gz"
STAGING=$(mktemp -d)

echo "→ Downloading AUGUR model weights..."

# LID model
curl -fL "$LID_MODEL_URL" -o "$STAGING/lid.176.ftz"

# Whisper models (choose preset)
PRESET="${1:-tiny}"  # tiny, base, or large-v3
curl -fL "$WHISPER_URL_$PRESET" -o "$STAGING/whisper-$PRESET.bin"

# NLLB-200
python3 -c "
from huggingface_hub import snapshot_download
snapshot_download('facebook/nllb-200-distilled-600M',
                  local_dir='$STAGING/nllb-600M')
"

echo "→ Writing install script..."
cat > "$STAGING/install.sh" << 'INSTALL'
#!/bin/bash
# Install AUGUR model weights on air-gapped workstation
CACHE="$HOME/.cache/augur/models"
mkdir -p "$CACHE/whisper"
cp lid.176.ftz "$CACHE/"
cp whisper-*.bin "$CACHE/whisper/"
cp -r nllb-600M "$CACHE/nllb/"
echo "✅ AUGUR models installed. Run: augur classify --text 'test'"
INSTALL
chmod +x "$STAGING/install.sh"

echo "→ Packaging..."
tar -czf "$OUTPUT" -C "$STAGING" .
rm -rf "$STAGING"

echo "✅ Air-gap package: $OUTPUT"
echo "   Transfer to air-gapped workstation and run: tar -xzf $OUTPUT && bash install.sh"
```

**Step 2 — Air-gap detection in ModelManager**

When AUGUR runs and the model isn't in cache, before trying
to download, check for a `AUGUR_AIRGAP_PATH` environment variable:

```rust
pub fn ensure_lid_model(&self) -> Result<PathBuf, AugurError> {
    // Check cache first
    if self.cached_lid_model().exists() {
        return Ok(self.cached_lid_model());
    }

    // Check air-gap override
    if let Ok(airgap) = std::env::var("AUGUR_AIRGAP_PATH") {
        let airgap_model = PathBuf::from(&airgap).join("lid.176.ftz");
        if airgap_model.exists() {
            log::info!("Using air-gap model from {}", airgap);
            std::fs::copy(&airgap_model, self.cached_lid_model())?;
            return Ok(self.cached_lid_model());
        }
    }

    // Download (internet required)
    log::warn!("Downloading LID model — requires internet access");
    self.download_lid_model()
}
```

**Step 3 — Documentation**

Add `docs/AIRGAP_INSTALL.md`:

```markdown
# AUGUR Air-Gap Installation

For workstations without internet access.

## On an internet-connected machine:
bash scripts/build_airgap_package.sh [tiny|base|large-v3]

## Transfer the package to the air-gapped workstation via USB.

## On the air-gapped workstation:
tar -xzf augur-airgap-YYYYMMDD.tar.gz
bash install.sh

## Verify installation:
AUGUR_AIRGAP_PATH=~/.cache/augur/models augur classify \
    --text "مرحبا بالعالم" --target en
```

**Step 4 — Tests**

```rust
#[test]
fn model_manager_uses_airgap_path_when_set() {
    // Set AUGUR_AIRGAP_PATH env var to a temp dir with fake model
    // Verify ModelManager copies from airgap path, no network call
}

#[test]
fn airgap_path_takes_priority_over_download() {
    // Even if download would succeed, airgap path wins
}
```

### Acceptance criteria — P3

- [ ] `build_airgap_package.sh` script written and executable
- [ ] `AUGUR_AIRGAP_PATH` env var respected by ModelManager
- [ ] Air-gap path takes priority over download
- [ ] `docs/AIRGAP_INSTALL.md` written
- [ ] 2 new tests pass
- [ ] Clippy clean

---

## Session log format

```
## AUGUR Sprint 5 — [date]

P1 fasttext-pure-rs: COMPATIBLE / INCOMPATIBLE
  - Arabic classifies correctly: yes/no
  - Farsi/Pashto/Urdu covered: yes/no
  - Backend status: production / still experimental

P2 Speaker diarization: PASSED / FAILED
  - pyannote available: yes/no
  - Segment merging working: yes/no
  - HF token management: yes/no

P3 Air-gap package: PASSED / FAILED
  - Package builder script: yes/no
  - AIRGAP_PATH env var: yes/no
  - Install docs written: yes/no

Final test count: [number]
Clippy: CLEAN
Offline invariant: MAINTAINED
MT advisory: ALWAYS PRESENT
```

---

## Commit format

```
feat: augur-sprint-5-P1 fasttext-pure-rs — [compatible/incompatible]
feat: augur-sprint-5-P2 speaker diarization — pyannote, enriched segments
feat: augur-sprint-5-P3 air-gap package — offline installer + AIRGAP_PATH
```

---

_Sprint 5 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_P1 is a quick probe — 30 seconds determines the path._
_P2 is the highest forensic value feature in this sprint._
_P3 makes AUGUR deployable in classified environments._
