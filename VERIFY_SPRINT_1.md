# VERIFY — Foundation Sprint 1
# Scaffold + fastText Classifier + Whisper STT

_Date: 2026-04-24_
_Model: claude-opus-4-7_
_Approved by: KR_
_Workspace: ~/Wolfmark/verify/ (greenfield — does not exist yet)_

---

## What VERIFY is

VERIFY is a forensic translation and transcription tool. It surfaces
foreign-language content inside digital evidence — text, audio, video,
and images — translating it into the examiner's working language without
requiring an internet connection.

Two shipping modes, one codebase:
- Standalone binary: `verify translate --input evidence.mp4 --target en`
- Strata plugin: loaded via the Strata plugin SDK, artifacts surface in UI

This sprint builds the foundation: workspace scaffold, the fastText
language classifier, and Whisper STT integration.

---

## Hard rules (absolute — same as Strata)

- Zero `.unwrap()` in production code paths
- Zero `unsafe{}` without explicit justification comment
- Zero `println!` in production — use `log::debug!` / `log::warn!` / `log::error!`
- All errors handled explicitly — no silent failures
- Every error path either propagates with `?`, logs, or surfaces to caller
- No new TODO/FIXME in committed code
- Plan before code — read and understand before touching anything
- `cargo clippy --workspace -- -D warnings` must be clean
- `cargo test --workspace` must pass after every change

---

## OFFLINE INVARIANT — HARD ARCHITECTURAL REQUIREMENT

VERIFY is offline-first by design. This is non-negotiable.

**No translation request, no audio file, no image, and no classified
content ever leaves the examiner's machine.**

Every feature that requires a network call must be:
1. Optional — not in the default code path
2. Clearly labeled in the API
3. Gated behind an explicit `--online` flag in the CLI

Before shipping any code, confirm: does this function make a network
call? If yes, it is not in the default path.

---

## PRIORITY 1 — Workspace Scaffold

### Goal
Create the VERIFY Rust workspace at `~/Wolfmark/verify/` with the
correct crate structure. No logic yet — just the scaffold that
compiles clean.

### Workspace structure to create

```
~/Wolfmark/verify/
├── Cargo.toml                    ← workspace root
├── CLAUDE.md                     ← hard rules (copy from this sprint)
├── .gitignore                    ← standard Rust + model weights
├── crates/
│   ├── verify-core/              ← pipeline orchestrator
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pipeline.rs       ← main pipeline entry point
│   │       └── error.rs          ← unified error type
│   ├── verify-classifier/        ← fastText language ID
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── classifier.rs
│   ├── verify-stt/               ← Whisper STT
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       └── whisper.rs
│   ├── verify-translate/         ← NLLB-200 (stub in Sprint 1)
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── verify-ocr/               ← Tesseract (stub in Sprint 1)
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   └── verify-plugin-sdk/        ← Strata plugin adapter (stub in Sprint 1)
│       ├── Cargo.toml
│       └── src/lib.rs
├── apps/
│   └── verify-cli/               ← standalone binary
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
└── tests/
    └── fixtures/                 ← test samples (empty for now)
```

### CLAUDE.md content for VERIFY workspace

Write `~/Wolfmark/verify/CLAUDE.md` with:
- Hard rules (same as above)
- Offline invariant (explicitly stated)
- Crate responsibilities (one paragraph per crate)
- Key architectural decisions (weights download-on-first-run,
  two shipping modes, pipeline order)

### .gitignore

Must include:
```
/target/
*.weights
*.bin
*.gguf
*.ggml
models/
weights/
.cache/
```
Model weights must never be committed to git.

### Acceptance criteria — P1

- [ ] `cargo build --workspace` succeeds from `~/Wolfmark/verify/`
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `cargo test --workspace` passes (0 tests, 0 failures)
- [ ] All 6 crates + 1 app present with correct structure
- [ ] CLAUDE.md written and complete
- [ ] .gitignore excludes model weights
- [ ] Git initialized, first commit: `chore: scaffold VERIFY workspace`

---

## PRIORITY 2 — fastText Language Classifier

### What it does

The fastText classifier is the router. It runs on every input before
any heavy pipeline. It identifies the language of the content and
decides whether translation is needed.

- Model: `lid.176.ftz` (176 languages, 900KB compressed)
- Speed: milliseconds per input
- Output: detected language code + confidence score
- Role: routes to the correct pipeline (text, audio, video, image)
  and writes language metadata even without full translation

**This is the ML feature** — a lightweight classifier sitting in front
of the expensive pipeline. An examiner with a 500GB image doesn't wait
for VERIFY to process everything — the classifier scans fast, flags
what's foreign, and queues only those for full translation.

### Implementation

**Step 1 — Add fasttext-rs dependency**

In `crates/verify-classifier/Cargo.toml`:
```toml
[dependencies]
fasttext = "0.1"          # or whichever version is current on crates.io
log = "0.4"
thiserror = "1"
```

Check crates.io for the current fasttext Rust binding. If `fasttext-rs`
or `fasttext` has issues, evaluate `whichlang` (pure Rust, no FFI,
covers major languages) as a fallback. Document the choice.

**Step 2 — Model download helper**

In `verify-classifier/src/classifier.rs`, write a `ModelManager` struct:

```rust
pub struct ModelManager {
    cache_dir: PathBuf,
}

impl ModelManager {
    /// Returns the path to the fastText LID model, downloading if needed.
    /// Model URL: https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz
    /// This is the ONLY network call VERIFY makes, and only on first run.
    pub fn ensure_lid_model(&self) -> Result<PathBuf, VerifyError> {
        // Check if cached
        // If not: download to cache_dir/lid.176.ftz
        // Verify file size (expected: ~900KB)
        // Return path
    }
}
```

Cache directory: `~/.cache/verify/models/` (XDG-compliant)

The download is the ONLY permitted network call. Document this
explicitly in a comment at the call site.

**Step 3 — Classifier implementation**

```rust
pub struct LanguageClassifier {
    model: FastText,  // or whichlang equivalent
}

pub struct ClassificationResult {
    pub language: String,        // ISO 639-1 code e.g. "ar", "zh", "ru"
    pub confidence: f32,         // 0.0 - 1.0
    pub is_foreign: bool,        // true if language != target_language
    pub target_language: String, // what the examiner wants output in
}

impl LanguageClassifier {
    pub fn load(model_path: &Path) -> Result<Self, VerifyError>;

    /// Classify a text sample. Takes first 512 chars for speed.
    pub fn classify(
        &self,
        text: &str,
        target_language: &str,
    ) -> Result<ClassificationResult, VerifyError>;
}
```

**Step 4 — Tests**

Minimum 3 tests in `#[cfg(test)]`:

```rust
#[test]
fn classifies_arabic_correctly() {
    // Use a hardcoded Arabic string — no network needed
    let text = "مرحبا بالعالم";  // "Hello World" in Arabic
    // Assert language == "ar", confidence > 0.8
}

#[test]
fn classifies_english_as_not_foreign() {
    let text = "The quick brown fox jumps over the lazy dog";
    // Assert is_foreign == false when target_language == "en"
}

#[test]
fn handles_empty_input_gracefully() {
    // Assert returns Ok() with low confidence, does not panic
}
```

**Important:** these tests must work WITHOUT downloading the model.
Use a mock or the `whichlang` pure-Rust library for unit tests.
The real fastText model is only needed for integration tests.

### Acceptance criteria — P2

- [ ] `LanguageClassifier` compiles and clippy-clean
- [ ] `ModelManager::ensure_lid_model()` downloads on first call,
  returns cached path on subsequent calls
- [ ] Arabic, Chinese, Russian, Spanish correctly classified in tests
- [ ] English classified as not-foreign when target is English
- [ ] Empty input handled without panic
- [ ] All tests pass without network access
- [ ] Zero `.unwrap()` in production code paths

---

## PRIORITY 3 — Whisper STT Integration

### What it does

Whisper converts audio content to text. VERIFY then passes the
transcript to NLLB-200 for translation.

- Model: OpenAI Whisper (open source, offline)
- Rust binding: `whisper-rs` crate
- Model presets:
  - Fast: `ggml-tiny.bin` (~75MB)
  - Balanced: `ggml-base.bin` (~142MB)
  - Accurate: `ggml-large-v3.bin` (~2.9GB)
- Languages: 99
- Input: audio file path (MP3, WAV, M4A, MP4 audio, OGG, FLAC)
- Output: `SttResult` with transcript + detected language + segments

### Implementation

**Step 1 — Dependencies**

In `crates/verify-stt/Cargo.toml`:
```toml
[dependencies]
whisper-rs = "0.10"      # check crates.io for current version
hound = "3.5"            # WAV decoding
log = "0.4"
thiserror = "1"
```

whisper-rs requires `libwhisper.a` or builds from source. Check
whether it builds cleanly on macOS ARM64 (M1 Max). If there are
build issues, document them clearly and try the `whisper-rs` fork
that uses pre-built binaries.

**Step 2 — Model preset enum**

```rust
#[derive(Debug, Clone, Copy)]
pub enum WhisperPreset {
    Fast,      // ggml-tiny.bin    ~75MB
    Balanced,  // ggml-base.bin    ~142MB
    Accurate,  // ggml-large-v3.bin ~2.9GB
}

impl WhisperPreset {
    pub fn model_filename(&self) -> &'static str;
    pub fn download_url(&self) -> &'static str;
    pub fn expected_size_bytes(&self) -> u64;
}
```

**Step 3 — Model manager extension**

Extend `ModelManager` (or create a parallel one in verify-stt)
to handle Whisper model downloads. Same pattern as fastText:
check cache, download if missing, verify size, return path.

Cache path: `~/.cache/verify/models/whisper/<preset>/`

**Step 4 — STT engine**

```rust
pub struct SttEngine {
    ctx: WhisperContext,
    preset: WhisperPreset,
}

pub struct SttResult {
    pub transcript: String,           // full transcript
    pub detected_language: String,    // ISO 639-1 code
    pub confidence: f32,
    pub segments: Vec<SttSegment>,    // timestamped segments
}

pub struct SttSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl SttEngine {
    pub fn load(model_path: &Path, preset: WhisperPreset)
        -> Result<Self, VerifyError>;

    /// Transcribe an audio file. Converts to 16kHz mono WAV internally.
    pub fn transcribe(
        &self,
        audio_path: &Path,
    ) -> Result<SttResult, VerifyError>;
}
```

**Step 5 — Audio preprocessing**

Whisper requires 16kHz mono f32 PCM. Add a preprocessing step:

```rust
fn preprocess_audio(input: &Path, output: &Path) -> Result<(), VerifyError>
```

Use `ffmpeg` as a subprocess if available (most forensic workstations
have it). If not available, fall back to `hound` for WAV files only
and return a clear error for other formats.

**Step 6 — Tests**

```rust
#[test]
fn preset_model_filenames_are_correct() {
    // No network, no model needed — just test the enum methods
    assert_eq!(WhisperPreset::Fast.model_filename(), "ggml-tiny.bin");
}

#[test]
fn stt_result_segments_are_chronological() {
    // Unit test with mock segments — verify ordering
}

#[test]
fn handles_missing_audio_file_gracefully() {
    // Assert returns Err, not panic
}
```

Whisper integration tests (requiring the actual model) go in
`tests/` directory and are marked `#[ignore]` unless
`VERIFY_RUN_INTEGRATION_TESTS=1` is set.

### Acceptance criteria — P3

- [ ] `whisper-rs` builds cleanly on macOS ARM64
- [ ] `WhisperPreset` enum complete with all three presets
- [ ] `ModelManager` handles Whisper model download + cache
- [ ] `SttEngine::transcribe()` compiles and returns `SttResult`
- [ ] Audio preprocessing handles WAV directly, ffmpeg for others
- [ ] Unit tests pass without model download
- [ ] Integration test structure in place (marked `#[ignore]`)
- [ ] Zero `.unwrap()` in production paths
- [ ] Clippy clean

---

## PRIORITY 4 — CLI Wiring

**Only proceed here if P1, P2, and P3 are complete.**

### Goal

Wire the scaffold into a working CLI that can classify a text input
and transcribe an audio file. NLLB translation is a stub ("translation
coming in Sprint 2") — but the pipeline should run end-to-end.

### CLI interface

```bash
# Classify a text string
verify classify --text "مرحبا بالعالم" --target en

# Transcribe an audio file (STT only, no translation yet)  
verify transcribe --input recording.wav --preset fast

# Full translate (stubs NLLB in Sprint 1, shows "TRANSLATION_STUB")
verify translate --input audio.mp3 --target en --preset balanced
```

### Implementation

In `apps/verify-cli/src/main.rs`, use `clap` for argument parsing:

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
verify-core = { path = "../../crates/verify-core" }
log = "0.4"
env_logger = "0.11"
```

Implement the three subcommands. For `translate`, print:
```
[VERIFY] Language detected: ar (confidence: 0.97)
[VERIFY] Transcript: مرحبا بالعالم
[VERIFY] Translation: STUB — NLLB-200 integration coming in Sprint 2
```

### Acceptance criteria — P4

- [ ] `cargo build --release -p verify-cli` produces a binary
- [ ] `verify classify --text "مرحبا بالعالم" --target en` runs
- [ ] `verify transcribe --input <wav> --preset fast` runs
- [ ] `verify --help` shows clean usage
- [ ] Binary is fully offline in default mode
- [ ] Zero `.unwrap()` in production paths

---

## Session log format

```
## VERIFY Sprint 1 — [date]

P1 Scaffold: PASSED / FAILED
  - Workspace created: yes/no
  - cargo build clean: yes/no
  - Git initialized: yes/no

P2 Classifier: PASSED / FAILED
  - fastText binding chosen: [which crate]
  - Classification tests: pass/fail
  - Arabic test: [result]

P3 Whisper STT: PASSED / FAILED
  - whisper-rs builds on ARM64: yes/no
  - All three presets defined: yes/no
  - Unit tests: pass/fail

P4 CLI: PASSED / SKIPPED

Final test count: [number]
Clippy: CLEAN / [issues]
Offline invariant: MAINTAINED / VIOLATED
```

---

## Commit message format

```
chore: scaffold VERIFY workspace — 6 crates, CLI app, CLAUDE.md
feat: verify-classifier — fastText LID, ModelManager, 3 unit tests
feat: verify-stt — Whisper STT engine, 3 presets, audio preprocessing
feat: verify-cli — classify + transcribe + translate stub subcommands
```

---

## What Sprint 2 will cover

- NLLB-200 translation integration (replace STUB)
- Tesseract OCR for images
- Video pipeline (audio extraction → STT → NLLB)
- verify-plugin-sdk wired to Strata plugin API
- Integration tests against real audio/image fixtures

---

_Sprint 1 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/verify/ (create it first)_
_Offline invariant is the hard architectural requirement._
_Every network call must be documented and justified._
