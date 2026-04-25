# VERIFY — Sprint 3
# Video Pipeline + ctranslate2 NLLB + Batch Processing + Real Strata Plugin

_Date: 2026-04-25_
_Model: claude-opus-4-7_
_Approved by: KR_
_Workspace: ~/Wolfmark/verify/_

---

## Context

Sprint 2 shipped: real Whisper STT (candle, Metal), NLLB-200
translation (Python subprocess), Tesseract OCR, CLI wired end-to-end.
35 tests passing, offline invariant maintained.

Sprint 3 adds the remaining high-value capabilities:
1. Video pipeline — extract audio from video, transcribe, translate
2. ctranslate2 NLLB swap — 3-5x faster CPU inference
3. Batch processing — process a directory of files in one command
4. Real Strata plugin trait wiring — VERIFY appears in Strata's
   plugin grid and surfaces translation artifacts inline

After Sprint 3, VERIFY is feature-complete for v1.0 standalone
release and fully integrated as a Strata plugin.

---

## Hard rules (absolute)

- Zero `.unwrap()` in production code paths
- Zero `unsafe{}` without explicit justification comment
- Zero `println!` in production — use `log::` macros
- All errors handled explicitly
- `cargo clippy --workspace -- -D warnings` must be clean
- `cargo test --workspace` must pass after every change
- No new TODO/FIXME in committed code

## OFFLINE INVARIANT — NON-NEGOTIABLE

All inference runs locally. No content ever leaves the machine.
Permitted network: one-time model weight downloads via named consts.

## MACHINE TRANSLATION ADVISORY — NON-NEGOTIABLE

Every translation output must carry the advisory notice.
Every `TranslationResult` must have `is_machine_translation = true`
and `advisory_notice` non-empty. No exceptions. No suppression.

---

## PRIORITY 1 — Video Pipeline

### What it does

Video files are extremely common in forensic evidence — phone
recordings, surveillance footage, screen captures. VERIFY should
be able to take a video file and produce a translated transcript.

Pipeline: Video file → extract audio (ffmpeg) → STT → classify
→ NLLB-200 → translated transcript with timestamps

### Implementation

**Step 1 — Video format detection**

In `crates/verify-core/src/pipeline.rs`, extend `PipelineInput`:

```rust
pub enum PipelineInput {
    Text(String),
    Audio(PathBuf),
    Image(PathBuf),
    Video(PathBuf),   // NEW
}
```

Video format detection by extension:
```rust
fn detect_input_type(path: &Path) -> PipelineInput {
    match path.extension().and_then(|e| e.to_str()) {
        Some("mp4") | Some("MP4") |
        Some("mov") | Some("MOV") |
        Some("avi") | Some("AVI") |
        Some("mkv") | Some("MKV") |
        Some("m4v") | Some("M4V") |
        Some("wmv") | Some("WMV") |
        Some("webm") | Some("WEBM") |
        Some("3gp") | Some("3GP") => PipelineInput::Video(path.to_path_buf()),
        Some("mp3") | Some("wav") | Some("m4a") |
        Some("ogg") | Some("flac") | Some("aac") => PipelineInput::Audio(path.to_path_buf()),
        Some("png") | Some("jpg") | Some("jpeg") |
        Some("tiff") | Some("bmp") => PipelineInput::Image(path.to_path_buf()),
        _ => PipelineInput::Audio(path.to_path_buf()), // fallback
    }
}
```

**Step 2 — Audio extraction from video**

In `crates/verify-stt/src/whisper.rs`, add video preprocessing:

```rust
/// Extract audio track from video file to a temporary WAV.
/// Requires ffmpeg (same dependency as audio preprocessing).
pub fn extract_audio_from_video(
    video_path: &Path,
    scratch_dir: &Path,
) -> Result<PathBuf, VerifyError> {
    // Use ffmpeg to extract audio:
    // ffmpeg -i input.mp4 -vn -ar 16000 -ac 1 -f wav output.wav
    // -vn = no video
    // -ar 16000 = 16kHz sample rate (Whisper requirement)
    // -ac 1 = mono
    let output = scratch_dir.join("extracted_audio.wav");
    
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-y", "-loglevel", "error",
            "-i", &video_path.to_string_lossy(),
            "-vn", "-ar", "16000", "-ac", "1",
            "-f", "wav",
            &output.to_string_lossy(),
        ])
        .status()
        .map_err(|e| VerifyError::Preprocessing(format!(
            "ffmpeg not found: {e}. Install ffmpeg to process video files."
        )))?;

    if !status.success() {
        return Err(VerifyError::Preprocessing(
            "ffmpeg failed to extract audio from video".to_string()
        ));
    }

    Ok(output)
}
```

**Step 3 — Wire video through Pipeline::run()**

```rust
PipelineInput::Video(path) => {
    let scratch = std::env::temp_dir()
        .join("verify")
        .join("video-scratch");
    std::fs::create_dir_all(&scratch)?;
    
    log::debug!("Extracting audio from video: {}", path.display());
    let audio_path = extract_audio_from_video(&path, &scratch)?;
    
    // Proceed identically to Audio input
    self.process_audio(&audio_path, target_language)
}
```

**Step 4 — Wire into CLI**

`verify translate` should auto-detect video files:

```bash
verify translate --input interview.mp4 --target en
```

Output:
```
[VERIFY] Input type: Video
[VERIFY] Extracting audio track (ffmpeg)...
[VERIFY] Running Whisper (Fast preset)...
[VERIFY] Language detected: ar
[VERIFY] Transcript (Arabic):
  [0:00-0:05] مرحبا بالعالم
  [0:05-0:12] كيف حالك اليوم
[VERIFY] Translating ar → en...
[VERIFY] Translation:
  [0:00-0:05] Hello world
  [0:05-0:12] How are you today
⚠  MACHINE TRANSLATION NOTICE
   ...
```

Note: translated segments preserve timestamps from STT. An examiner
needs to know not just WHAT was said but WHEN.

**Step 5 — Timestamped translation segments**

Extend `TranslationResult` to support segment-level translation:

```rust
pub struct TranslationResult {
    pub source_text: String,
    pub translated_text: String,
    pub source_language: String,
    pub target_language: String,
    pub confidence: f32,
    pub model: String,
    pub is_machine_translation: bool,    // always true
    pub advisory_notice: String,         // always non-empty
    pub segments: Option<Vec<TranslatedSegment>>, // NEW
}

pub struct TranslatedSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
}
```

When STT segments are available, translate each segment
individually and populate `segments`. This gives examiners
timestamp-accurate translated transcripts.

**Step 6 — Tests**

```rust
#[test]
fn video_input_detected_by_extension() {
    assert!(matches!(
        detect_input_type(Path::new("interview.mp4")),
        PipelineInput::Video(_)
    ));
    assert!(matches!(
        detect_input_type(Path::new("recording.mov")),
        PipelineInput::Video(_)
    ));
}

#[test]
fn translated_segments_preserve_timestamps() {
    // Given STT segments with start/end_ms
    // Verify TranslatedSegment has same timestamps
}

#[test]
fn video_without_ffmpeg_returns_clear_error() {
    // Mock missing ffmpeg
    // Verify VerifyError::Preprocessing not panic
}
```

### Acceptance criteria — P1

- [ ] Video files auto-detected by extension
- [ ] Audio extracted from video via ffmpeg
- [ ] STT runs on extracted audio
- [ ] Translation segments preserve timestamps
- [ ] CLI `verify translate --input video.mp4` works
- [ ] Missing ffmpeg returns clear error
- [ ] 3 new tests pass
- [ ] Clippy clean, zero new `.unwrap()`

---

## PRIORITY 2 — ctranslate2 NLLB Swap

### Why

The Python `transformers` NLLB subprocess from Sprint 2 is
correct but slow — 30-60 seconds per paragraph on CPU. CTranslate2
is an optimized inference engine that runs the same NLLB-200 model
3-5x faster on CPU.

### Implementation

**Step 1 — Probe ctranslate2**

```bash
pip3 install ctranslate2 sentencepiece --break-system-packages
python3 -c "import ctranslate2; print(ctranslate2.__version__)"
```

If available → proceed. If not → document and keep Sprint 2's
transformers backend as production, note ctranslate2 as an
optional performance upgrade.

**Step 2 — Convert the model**

CTranslate2 requires a one-time model conversion:

```python
import ctranslate2
ctranslate2.converters.OpusMTConverter(
    "facebook/nllb-200-distilled-600M"
).convert(
    "~/.cache/verify/models/nllb/ct2/",
    quantization="int8"   # further speedup, minimal quality loss
)
```

Add a `ModelManager::ensure_nllb_ct2_model()` method that:
1. Checks if the CT2 model exists in cache
2. If not: downloads the HF model + runs conversion
3. Returns path to CT2 model

**Step 3 — Update worker script**

Replace the `transformers` pipeline in `worker_script.py` with
ctranslate2:

```python
import ctranslate2
import sentencepiece as spm

def translate(text, src_lang, tgt_lang, model_dir, spm_path):
    translator = ctranslate2.Translator(model_dir, device="cpu")
    sp = spm.SentencePieceProcessor()
    sp.load(spm_path)
    
    # Tokenize with source language token
    tokens = sp.encode(text, out_type=str)
    tokens = [f"__{src_lang}__"] + tokens
    
    # Translate
    results = translator.translate_batch(
        [tokens],
        target_prefix=[[f"__{tgt_lang}__"]]
    )
    
    # Decode
    output_tokens = results[0].hypotheses[0][1:]  # strip lang token
    return sp.decode(output_tokens)
```

**Step 4 — Benchmark**

After implementation, run a benchmark:
- Translate a 100-word Arabic paragraph
- Time with transformers backend vs ctranslate2
- Report in CLAUDE.md under "Sprint 3 decisions"

**Step 5 — Graceful fallback**

If ctranslate2 is not available, fall back to transformers:

```rust
fn translate_subprocess(
    text: &str,
    src_lang: &str,
    tgt_lang: &str,
) -> Result<String, VerifyError> {
    // Try ctranslate2 first
    // If ct2 model not available, fall back to transformers
    // Log which backend is being used at log::debug! level
}
```

**Tests**

```rust
#[test]
fn translation_backend_fallback_is_graceful() {
    // If ct2 model missing, transformers used
    // No panic, clear log message
}
```

### Acceptance criteria — P2

- [ ] ctranslate2 installed and probed
- [ ] Model conversion documented or automated
- [ ] Translation speed improvement measured and logged
- [ ] Graceful fallback to transformers if ct2 unavailable
- [ ] Machine translation advisory still present (always)
- [ ] 1 new test passes
- [ ] Clippy clean

---

## PRIORITY 3 — Batch Processing

### What it does

Examiners frequently need to process an entire evidence directory
— hundreds of audio files from a phone extraction, for example.
VERIFY should be able to run against a folder and produce a
consolidated translation report.

### CLI interface

```bash
# Process all files in a directory
verify batch --input /path/to/evidence/folder --target en

# Process only specific types
verify batch --input /path/to/folder --target en --types audio,video

# Output to file
verify batch --input /path/to/folder --target en --output report.json
```

### Implementation

**Step 1 — Batch subcommand**

Add `batch` to the CLI in `apps/verify-cli/src/main.rs`:

```rust
#[derive(Subcommand)]
enum Commands {
    Classify { ... },
    Transcribe { ... },
    Translate { ... },
    Batch {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, default_value = "en")]
        target: String,
        #[arg(long, value_delimiter = ',')]
        types: Option<Vec<String>>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}
```

**Step 2 — Batch engine**

In `crates/verify-core/src/pipeline.rs`:

```rust
pub struct BatchResult {
    pub total_files: u32,
    pub processed: u32,
    pub foreign_language: u32,
    pub translated: u32,
    pub errors: u32,
    pub results: Vec<BatchFileResult>,
    pub generated_at: String,   // ISO 8601
}

pub struct BatchFileResult {
    pub file_path: String,
    pub input_type: String,          // "audio", "video", "image", "text"
    pub detected_language: String,
    pub is_foreign: bool,
    pub translation: Option<TranslationResult>,
    pub error: Option<String>,
}

impl Pipeline {
    pub fn run_batch(
        &self,
        input_dir: &Path,
        target_language: &str,
        file_types: Option<&[&str]>,
    ) -> Result<BatchResult, VerifyError> {
        // Walk input_dir
        // For each file: detect type, run pipeline
        // Collect results
        // Return BatchResult
    }
}
```

**Step 3 — Output formats**

When `--output report.json` is specified, write:

```json
{
  "generated_at": "2026-04-25T14:30:00Z",
  "total_files": 47,
  "processed": 47,
  "foreign_language": 12,
  "translated": 12,
  "errors": 0,
  "target_language": "en",
  "machine_translation_notice": "All translations produced by VERIFY are machine-generated. Verify with a certified human translator for legal proceedings.",
  "results": [
    {
      "file": "recording_001.mp3",
      "type": "audio",
      "language": "ar",
      "is_foreign": true,
      "transcript": "...",
      "translation": "...",
      "segments": [...]
    }
  ]
}
```

The `machine_translation_notice` field at the top level of the
JSON is mandatory — it applies to the entire report.

**Step 4 — Progress reporting**

For large directories, show progress:

```
[VERIFY] Batch processing: /evidence/audio/
[VERIFY] Found 47 files (23 audio, 8 video, 16 image)
[VERIFY] Processing... [████████░░] 35/47
[VERIFY] Foreign language detected: 12 files
[VERIFY] Translating... [████████░░] 10/12
[VERIFY] Complete. Report: report.json
```

**Step 5 — Tests**

```rust
#[test]
fn batch_result_machine_translation_notice_present() {
    // BatchResult serialized JSON contains machine_translation_notice
}

#[test]
fn batch_skips_unsupported_file_types_gracefully() {
    // .pdf, .doc, unknown extensions don't panic
}

#[test]
fn batch_result_counts_are_accurate() {
    // total_files = processed + errors
}
```

### Acceptance criteria — P3

- [ ] `verify batch --input /folder --target en` works
- [ ] Processes audio, video, image files in directory
- [ ] JSON output contains machine_translation_notice at top level
- [ ] Progress shown during long batch runs
- [ ] Errors per file captured without stopping batch
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 4 — Real Strata Plugin Trait Wiring

**Only if P1-P3 complete. This makes VERIFY a first-class Strata plugin.**

### The problem

Sprint 2 shipped the adapter shape — `VerifyStrataPlugin` has the
right structure but doesn't implement the real `StrataPlugin` trait
from the Strata SDK because it's not vendored.

### Solution

Vendor the Strata plugin SDK into VERIFY:

```bash
# From ~/Wolfmark/verify/
mkdir -p vendor
cp -r ~/Wolfmark/strata/crates/strata-plugin-sdk vendor/
```

Add to workspace `Cargo.toml`:
```toml
[patch.crates-io]
strata-plugin-sdk = { path = "vendor/strata-plugin-sdk" }
```

Then implement the real trait in `crates/verify-plugin-sdk/src/lib.rs`.

### Plugin behavior

When VERIFY runs as a Strata plugin:
1. Receives `PluginContext` with `root_path` pointing to materialized evidence
2. Walks `root_path` for audio, video, and image files
3. Classifies language of each file
4. For foreign-language files: runs STT/OCR + translation
5. Returns `ArtifactRecord` per translation

Each translation artifact:
```rust
ArtifactRecord {
    name: format!("VERIFY Translation: {}", file_name),
    artifact_type: "verify_translation",
    category: "Communications",    // translations are communications
    value: translated_text,
    source_file: file_path,
    confidence: Confidence::Medium, // MT is always Medium
    is_advisory: true,             // always advisory
    advisory_notice: MT_NOTICE,    // always present
    mitre_technique: String::new(), // not a MITRE technique
    forensic_value: ForensicValue::High, // foreign language evidence is high value
    ..Default::default()
}
```

### Acceptance criteria — P4

- [ ] `strata-plugin-sdk` vendored into VERIFY
- [ ] `VerifyStrataPlugin` implements real `StrataPlugin` trait
- [ ] Plugin compiles with Strata workspace
- [ ] Translation artifacts have `is_advisory = true`
- [ ] `advisory_notice` always non-empty
- [ ] `confidence = Medium` on all translation artifacts
- [ ] 2 tests pass

---

## What Sprint 3 does NOT touch

- Whisper temperature-fallback (quality improvement, Sprint 4)
- Speaker diarization (who said what — Sprint 4)
- PDF text extraction (Sprint 4)
- VERIFY web UI (post-v1.0)
- Real-time transcription (post-v1.0)

---

## Session log format

```
## VERIFY Sprint 3 — [date]

P1 Video pipeline: PASSED / FAILED
  - Video formats detected: yes/no
  - Audio extraction working: yes/no
  - Timestamped segments: yes/no

P2 ctranslate2: PASSED / SKIPPED
  - Available on build host: yes/no
  - Speed improvement: [x]x faster
  - Fallback works: yes/no

P3 Batch processing: PASSED / FAILED
  - Directory walk working: yes/no
  - JSON output with MT notice: yes/no
  - Error handling per file: yes/no

P4 Strata plugin: PASSED / SKIPPED
  - SDK vendored: yes/no
  - Real trait impl: yes/no

Final test count: [number]
Clippy: CLEAN
Offline invariant: MAINTAINED
MT advisory: ALWAYS PRESENT
```

---

## Commit format

```
feat: verify-sprint-3-P1 video pipeline — extract audio, timestamped segments
feat: verify-sprint-3-P2 ctranslate2 — 3-5x faster NLLB inference
feat: verify-sprint-3-P3 batch processing — directory scan, JSON report
feat: verify-sprint-3-P4 Strata plugin — real SDK trait, translation artifacts
```

---

_Sprint 3 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/verify/_
_After Sprint 3, VERIFY is v1.0 complete._
_Video + batch + Strata plugin = the full forensic workflow._
