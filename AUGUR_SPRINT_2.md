# AUGUR — Sprint 2
# Real Whisper STT + NLLB-200 Translation + Tesseract OCR

_Date: 2026-04-25_
_Model: claude-opus-4-7_
_Approved by: KR_
_Workspace: ~/Wolfmark/augur/_

---

## Context

Sprint 1 shipped: workspace scaffold, fastText/whichlang dual
classifier, Whisper STT interface (stub backend — whisper-rs
deferred due to cmake FFI), NLLB-200 translation stub, CLI with
classify/transcribe/translate subcommands.

Sprint 2 ships real inference. Two stubs become real:
1. Whisper STT — real audio transcription
2. NLLB-200 — real translation

Plus one new capability:
3. Tesseract OCR — image/document text extraction

After Sprint 2, AUGUR can take an audio file in Arabic and
produce an English transcript. That's the core forensic value.

---

## Hard rules (absolute)

- Zero `.unwrap()` in production code paths
- Zero `unsafe{}` without explicit justification comment
- Zero `println!` in production — use `log::` macros
- All errors handled explicitly — no silent failures
- `cargo clippy --workspace -- -D warnings` must be clean
- `cargo test --workspace` must pass after every change
- No new TODO/FIXME in committed code

## OFFLINE INVARIANT — NON-NEGOTIABLE

All inference runs locally. No audio, no text, no image, no
translation content ever leaves the examiner's machine.

The ONLY permitted network calls are one-time model weight
downloads. Every download URL must be a named `pub const`.
All downloads log via `log::warn!` before any network egress.

---

## PRIORITY 1 — Real Whisper STT (replace the stub)

### The Sprint 1 problem

`whisper-rs` requires cmake + C++ toolchain. Rejected. The stub
backend currently returns a clean error explaining Sprint 2
will ship real inference.

### Solution — candle-whisper (pure Rust)

Hugging Face's `candle` framework ships a pure-Rust Whisper
implementation that reads the same GGML model weights as the
C++ whisper.cpp. No cmake. No FFI. M1 Max has Metal acceleration
support via candle's metal backend.

**Step 1 — Probe candle-whisper**

```bash
cd ~/Wolfmark/augur
cargo add candle-core --features metal
cargo add candle-nn
cargo add candle-transformers
cargo add hf-hub
cargo build -p augur-stt 2>&1 | tail -20
```

If candle builds cleanly on ARM64 → proceed.
If build fails → evaluate `whisper-rs` with the cmake dep
accepted as a known system requirement (document it clearly).
If both fail → implement a subprocess-based approach calling
the `whisper` CLI binary if present, same pattern as ffmpeg.

Spend max 20 minutes on build probing before deciding.
Document the chosen approach in CLAUDE.md under "Sprint 2 decisions."

**Step 2 — Implement real SttEngine**

Replace the stub in `crates/augur-stt/src/whisper.rs`:

```rust
pub struct SttEngine {
    model: WhisperModel,  // candle-whisper or equivalent
    preset: WhisperPreset,
}

impl SttEngine {
    pub fn load(model_path: &Path, preset: WhisperPreset) 
        -> Result<Self, AugurError> 
    {
        // Load the GGML model weights
        // Initialize candle device (Metal on M1, CPU fallback)
        // Return loaded engine
    }

    pub fn transcribe(&self, audio_path: &Path) 
        -> Result<SttResult, AugurError> 
    {
        // 1. Validate audio file exists
        // 2. Preprocess to 16kHz mono WAV (existing preprocess_audio)
        // 3. Run Whisper inference
        // 4. Return SttResult with transcript + segments + language
    }
}
```

**Step 3 — SttResult populated correctly**

```rust
pub struct SttResult {
    pub transcript: String,
    pub detected_language: String,   // ISO 639-1
    pub confidence: f32,
    pub segments: Vec<SttSegment>,   // timestamped segments
}

pub struct SttSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}
```

Segments are essential for forensic use — examiners need to know
exactly when in an audio file each phrase was spoken.

**Step 4 — Wire into CLI**

The `augur transcribe` subcommand currently surfaces the stub
error. After this fix it should produce real output:

```
[AUGUR] Preprocessing audio: recording.mp3
[AUGUR] Running Whisper (Fast preset — ggml-tiny.bin)
[AUGUR] Language detected: ar
[AUGUR] Transcript:
  [0:00 - 0:03] مرحبا بالعالم
  [0:03 - 0:07] كيف حالك اليوم
[AUGUR] Complete. 2 segments, 6 words.
```

**Step 5 — Update integration tests**

The two `#[ignore]` integration tests in `tests/whisper_integration.rs`
need updating:
- `stub_transcribe_returns_structured_error_on_valid_wav` → flip to
  assert real transcript content (requires model download)
- Keep the `#[ignore]` gate on `AUGUR_RUN_INTEGRATION_TESTS=1`

Add one new unit test that doesn't require model download:
```rust
#[test]
fn stt_result_has_all_required_fields() {
    // Construct a synthetic SttResult, verify all fields present
}
```

### Acceptance criteria — P1

- [ ] Real Whisper inference replaces the stub
- [ ] Arabic audio file produces Arabic transcript
- [ ] Segments have correct start/end timestamps
- [ ] Detected language populated from Whisper's own detection
- [ ] CLI `augur transcribe` shows real output
- [ ] Unit tests pass without model download
- [ ] Integration tests pass with `AUGUR_RUN_INTEGRATION_TESTS=1`
- [ ] Zero `.unwrap()` in production paths
- [ ] Clippy clean

---

## PRIORITY 2 — NLLB-200 Translation (replace the stub)

### What NLLB-200 is

Meta's No Language Left Behind model. 200 languages. The 600M
distilled variant is 2.4GB and runs on CPU. The 1.3B variant is
better quality but 5.1GB. For Sprint 2, ship the 600M variant.

### Options for Rust inference

**Option A — candle-transformers (Hugging Face)**
If candle built cleanly in P1, NLLB-200 is available through
the same framework. `candle_transformers::models::t5` covers the
encoder-decoder architecture NLLB uses.

**Option B — Python subprocess (pragmatic)**
Call a minimal Python script via subprocess:
```bash
python3 -c "
from transformers import pipeline
translator = pipeline('translation', model='facebook/nllb-200-distilled-600M')
result = translator('مرحبا', src_lang='ara_Arab', tgt_lang='eng_Latn')
print(result[0]['translation_text'])
"
```
Most forensic workstations with Python 3.8+ and pip can install
transformers. Document the dependency clearly.

**Option C — ctranslate2 subprocess**
CTranslate2 is a C++ inference engine with Python bindings that
is significantly faster than raw transformers for CPU inference.
`pip install ctranslate2 sentencepiece` — no GPU required.

**Decision rule:**
- If candle NLLB works from P1 → Option A
- If forensic workstations typically have Python → Option B first,
  note Option C as a performance upgrade
- Document the choice and rationale in CLAUDE.md

**Step 1 — Probe chosen option**

Test translation of "مرحبا بالعالم" (Arabic → English).
Expected output: "Hello world" or equivalent.
If output is reasonable → proceed.

**Step 2 — Implement TranslationEngine**

In `crates/augur-translate/src/lib.rs`:

```rust
pub struct TranslationEngine {
    // model handle — candle model or subprocess config
    src_lang: String,
    tgt_lang: String,
}

pub struct TranslationResult {
    pub source_text: String,
    pub translated_text: String,
    pub source_language: String,    // ISO 639-1
    pub target_language: String,    // ISO 639-1
    pub confidence: f32,
    pub model: String,              // "nllb-200-distilled-600M"
    pub is_machine_translation: bool, // always true — for examiner clarity
    pub advisory_notice: String,    // "Machine translation — verify with human translator for legal proceedings"
}
```

The `advisory_notice` is mandatory. Machine translation in a
forensic context must be labeled as such. An examiner presenting
AUGUR output in court must know it is MT output, not certified
human translation.

**Step 3 — Language code mapping**

NLLB uses its own language codes (e.g. `ara_Arab` for Arabic,
`eng_Latn` for English). Map from ISO 639-1:

```rust
fn iso_to_nllb(iso: &str) -> Result<&'static str, AugurError> {
    match iso {
        "ar" => Ok("ara_Arab"),
        "zh" => Ok("zho_Hans"),
        "ru" => Ok("rus_Cyrl"),
        "es" => Ok("spa_Latn"),
        "fr" => Ok("fra_Latn"),
        "de" => Ok("deu_Latn"),
        "fa" => Ok("pes_Arab"),   // Farsi/Persian — high LE value
        "ps" => Ok("pbt_Arab"),   // Pashto — high LE value
        "ur" => Ok("urd_Arab"),   // Urdu — high LE value
        "ko" => Ok("kor_Hang"),
        "ja" => Ok("jpn_Jpan"),
        "vi" => Ok("vie_Latn"),
        "tr" => Ok("tur_Latn"),
        "pt" => Ok("por_Latn"),
        "it" => Ok("ita_Latn"),
        "nl" => Ok("nld_Latn"),
        "en" => Ok("eng_Latn"),
        other => Err(AugurError::UnsupportedLanguage(other.to_string())),
    }
}
```

Note the forensically important languages: Farsi, Pashto, Urdu.
These are high-value for LE/IC work and should be explicitly
tested.

**Step 4 — Wire into Pipeline**

In `crates/augur-core/src/pipeline.rs`, implement the full
end-to-end pipeline:

```rust
pub struct Pipeline {
    classifier: LanguageClassifier,
    stt: Option<SttEngine>,
    translator: Option<TranslationEngine>,
}

pub struct PipelineResult {
    pub source_language: String,
    pub source_text: String,        // original (transcript or text input)
    pub translated_text: String,    // translated output
    pub translation_result: TranslationResult,
    pub stt_segments: Option<Vec<SttSegment>>, // if audio input
}

impl Pipeline {
    /// Full pipeline: classify → STT (if audio) → translate
    pub fn run(
        &self,
        input: PipelineInput,
        target_language: &str,
    ) -> Result<PipelineResult, AugurError>;
}

pub enum PipelineInput {
    Text(String),
    Audio(PathBuf),
    // Image(PathBuf) — Sprint 2 P3
}
```

**Step 5 — Wire into CLI**

`augur translate` should now produce real output:

```
[AUGUR] Classifying input language...
[AUGUR] Language: ar (Arabic) — confidence: 0.99 — is_foreign: true
[AUGUR] Running Whisper (Fast preset)...
[AUGUR] Transcript (Arabic):
  [0:00-0:03] مرحبا بالعالم

[AUGUR] Translating ar → en via NLLB-200-distilled-600M...
[AUGUR] Translation (English):
  Hello world

⚠  MACHINE TRANSLATION NOTICE
   This output was produced by an automated translation model.
   For legal proceedings, verify with a certified human translator.
   Model: facebook/nllb-200-distilled-600M
   Source language: Arabic (ara_Arab)
   Target language: English (eng_Latn)
```

The machine translation notice must always print. It is not
optional. It is not suppressible via a flag.

**Step 6 — Tests**

```rust
#[test]
fn advisory_notice_always_present_in_translation_result() {
    // Verify is_machine_translation = true and advisory_notice
    // is non-empty on every TranslationResult
    // This is a load-bearing test — mirrors Strata's advisory pattern
}

#[test]
fn iso_to_nllb_maps_forensic_languages_correctly() {
    // ar → ara_Arab
    // fa → pes_Arab  (Farsi)
    // ps → pbt_Arab  (Pashto)
    // ur → urd_Arab  (Urdu)
}

#[test]
fn unsupported_language_returns_clear_error() {
    // iso_to_nllb("xx") → Err(UnsupportedLanguage)
    // Not panic, not silent
}
```

### Acceptance criteria — P2

- [ ] Real NLLB-200 translation replaces TRANSLATION_STUB
- [ ] Arabic → English produces correct output
- [ ] Farsi, Pashto, Urdu language codes mapped correctly
- [ ] Machine translation notice always present in output
- [ ] advisory_notice always present in TranslationResult
- [ ] Full pipeline: text input → classify → translate works
- [ ] Full pipeline: audio input → classify → STT → translate works
- [ ] 3 new tests pass (including the load-bearing advisory test)
- [ ] Zero `.unwrap()` in production paths
- [ ] Clippy clean

---

## PRIORITY 3 — Tesseract OCR

### What it does

Extracts text from images (screenshots, photographs of documents,
scanned pages). Common in forensic work — phones contain screenshots
of conversations, vehicles contain photographed documents.

### Rust binding

`tesseract` crate (pure Rust bindings to libtesseract).
Alternative: `leptess` crate.

Both require `libtesseract` system library:
```bash
brew install tesseract
```

This is an acceptable system dependency — Tesseract is standard
on forensic analyst workstations. Document it clearly.

**Step 1 — Probe**

```bash
brew install tesseract
cargo add tesseract -p augur-ocr
cargo build -p augur-ocr 2>&1 | tail -10
```

If build fails without brew install → document the requirement
and implement a subprocess-based approach calling `tesseract`
CLI binary directly (same pattern as ffmpeg for audio).

**Step 2 — Implement OcrEngine**

In `crates/augur-ocr/src/lib.rs`:

```rust
pub struct OcrEngine {
    language: String,    // tesseract language code e.g. "ara", "eng"
}

pub struct OcrResult {
    pub text: String,
    pub confidence: f32,          // 0.0 - 1.0
    pub detected_language: String, // from classifier after OCR
    pub page_count: u32,
    pub words: Vec<OcrWord>,
}

pub struct OcrWord {
    pub text: String,
    pub confidence: f32,
    pub bounding_box: Option<BoundingBox>,
}

impl OcrEngine {
    pub fn new(language: &str) -> Result<Self, AugurError>;

    /// Extract text from an image file.
    /// Supported: PNG, JPG, TIFF, BMP, PDF (single page)
    pub fn extract_text(&self, image_path: &Path) 
        -> Result<OcrResult, AugurError>;
}
```

**Step 3 — Language selection**

Tesseract uses its own language codes (`ara`, `eng`, `rus`, etc.)
and requires the corresponding language data files.

Language data install:
```bash
brew install tesseract-lang  # installs all language packs
```

Or targeted:
```bash
brew install tesseract
# Language data files go in /usr/local/share/tessdata/
```

For air-gapped workstations: document how to pre-download
tessdata files and place them in the correct path.

**Step 4 — Add Image input to Pipeline**

Extend `PipelineInput` to handle images:

```rust
pub enum PipelineInput {
    Text(String),
    Audio(PathBuf),
    Image(PathBuf),    // NEW
}
```

In `Pipeline::run()`, when input is `Image`:
1. Run OcrEngine to extract text
2. Run classifier on extracted text
3. If foreign → run TranslationEngine
4. Return PipelineResult with OCR text + translation

**Step 5 — Add to CLI**

```bash
# Extract and translate text from an image
augur translate --input screenshot.png --target en

# Output:
[AUGUR] Input type: Image
[AUGUR] Running OCR (Arabic)...
[AUGUR] Extracted text: مرحبا بالعالم
[AUGUR] Classifying language...
[AUGUR] Language: ar — translating to en...
[AUGUR] Translation: Hello world
⚠  MACHINE TRANSLATION NOTICE ...
```

**Step 6 — Tests**

```rust
#[test]
fn ocr_engine_initializes_for_english() {
    // No image needed — just verify initialization
}

#[test]
fn ocr_returns_error_for_missing_file() {
    // Not panic, clear error
}

#[test]
fn image_input_routes_through_pipeline() {
    // Unit test with mock OCR result
}
```

### Acceptance criteria — P3

- [ ] Tesseract OCR extracts text from image files
- [ ] Image input wired into Pipeline
- [ ] CLI `augur translate --input image.png` works
- [ ] Missing tessdata returns clear error, not panic
- [ ] 3 new tests pass
- [ ] Zero `.unwrap()` in production paths
- [ ] Clippy clean

---

## PRIORITY 4 — Strata Plugin SDK Wiring

**Only if P1-P3 complete with time remaining.**

### Goal

Wire `augur-plugin-sdk` to the Strata plugin API so AUGUR can
run as a Strata plugin — artifacts surface inline in Strata's UI.

### Implementation

In `crates/augur-plugin-sdk/src/lib.rs`, implement the
`StrataPlugin` trait:

```rust
impl StrataPlugin for AugurStrataPlugin {
    fn name(&self) -> &str { "AUGUR" }
    fn version(&self) -> &str { "0.2.0" }
    fn description(&self) -> &str {
        "Foreign language detection and translation — \
         surfaces translated content as Strata artifacts"
    }

    fn execute(&self, ctx: PluginContext) 
        -> Result<Vec<ArtifactRecord>, PluginError> 
    {
        // 1. Walk ctx.root_path for audio/image files
        // 2. For each file: classify language
        // 3. If foreign: run STT/OCR + translate
        // 4. Return translation artifacts
    }
}
```

Each translation becomes an artifact:
```
artifact_type: "augur_translation"
value: translated_text
source_plugin: "AUGUR"
confidence: Confidence::Medium  // MT output is always Medium
is_advisory: true               // MT is advisory — not verified
advisory_notice: "Machine translation — verify with human translator"
mitre_technique: ""             // translation is not a MITRE technique
```

### Acceptance criteria — P4

- [ ] `AugurStrataPlugin` implements `StrataPlugin` trait
- [ ] Plugin compiles against Strata plugin SDK
- [ ] Translation artifacts have is_advisory = true
- [ ] Machine translation advisory_notice always present
- [ ] 2 tests pass

---

## Session log format

```
## AUGUR Sprint 2 — [date]

P1 Whisper STT: PASSED / FAILED
  - Backend chosen: candle / whisper-rs / subprocess
  - Arabic audio transcribed: yes/no
  - Segments with timestamps: yes/no

P2 NLLB-200: PASSED / FAILED
  - Backend chosen: candle / subprocess / ctranslate2
  - Arabic → English: correct/incorrect
  - Farsi/Pashto/Urdu codes wired: yes/no
  - advisory_notice always present: yes/no

P3 Tesseract OCR: PASSED / FAILED
  - Image text extraction: yes/no
  - Pipeline image input wired: yes/no

P4 Strata Plugin: PASSED / SKIPPED

Final test count: [number]
Clippy: CLEAN
Offline invariant: MAINTAINED / VIOLATED
Machine translation notice: ALWAYS PRESENT / [issue]
```

---

## Commit format

```
feat: augur-sprint-2-P1 real Whisper STT — [backend chosen], segments
feat: augur-sprint-2-P2 NLLB-200 translation — advisory notice mandatory
feat: augur-sprint-2-P3 Tesseract OCR — image pipeline wired
feat: augur-sprint-2-P4 Strata plugin SDK — AUGUR as Strata plugin
```

---

## Load-bearing test — AUGUR equivalent of Strata's advisory tests

This test is Sprint 2's load-bearing invariant. It must pass for
the sprint to be considered complete:

```rust
#[test]
fn machine_translation_advisory_always_present() {
    // Every TranslationResult must have:
    // - is_machine_translation = true
    // - advisory_notice.is_empty() == false
    // This is non-negotiable for forensic use.
    // A tool that produces MT output without labeling it
    // is dangerous in a legal context.
}
```

---

_Sprint 2 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_The machine translation advisory notice is the hard requirement._
_Every translation output must be labeled as machine-generated._
_No exceptions. No suppression flags._
