# AUGUR — Sprint 4
# Whisper Quality + Speaker Diarization + PDF + ctranslate2 Benchmark

_Date: 2026-04-25_
_Model: claude-opus-4-7_
_Approved by: KR_
_Workspace: ~/Wolfmark/augur/_

---

## Context

Sprint 3 shipped: video pipeline, ctranslate2 backend (auto-select),
batch processing with JSON reports, Strata plugin (feature-gated).
49 tests passing, v1.0 feature-complete.

Sprint 4 adds quality and depth:
1. Whisper temperature-fallback for better accuracy on hard audio
2. Speaker diarization — who said what, when
3. PDF text extraction — common evidence format
4. ctranslate2 benchmark on real hardware
5. whichlang default flip + fasttext experimental label (post-Sprint 1 finding)

---

## Hard rules (always)

- Zero `.unwrap()` in production code paths
- Zero `unsafe{}` without explicit justification
- Zero `println!` in production — use `log::` macros
- All errors handled explicitly
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes after every change
- No TODO/FIXME in committed code

## OFFLINE INVARIANT — NON-NEGOTIABLE

All inference local. No content leaves the machine.
Network only for one-time model downloads via named consts.

## MACHINE TRANSLATION ADVISORY — NON-NEGOTIABLE

Every TranslationResult: `is_machine_translation = true`,
`advisory_notice` non-empty. No exceptions.

---

## PRIORITY 1 — Fix fastText Default + Experimental Label

### The Sprint 1 finding

fasttext 0.8.0 misclassifies Arabic as Esperanto. whichlang
correctly identifies all tested languages. The default was left
as fasttext in Sprint 1 pending Sprint 2 investigation. Sprint 2
confirmed the incompatibility. Time to fix it.

### Changes required

**Step 1 — Flip the default**

In `apps/augur-cli/src/main.rs` line 30:

```rust
// Change:
#[arg(long, default_value = "fasttext")]
classifier_backend: ClassifierBackend,

// To:
#[arg(long, default_value = "whichlang")]
classifier_backend: ClassifierBackend,
```

**Step 2 — Mark fasttext experimental in docs**

In `crates/augur-classifier/src/classifier.rs`, update the
doc comment on the FastText backend:

```rust
/// FastText LID backend — EXPERIMENTAL.
///
/// The `fasttext = "0.8.0"` crate is NOT binary-compatible with
/// Facebook's published `lid.176.ftz` model. It produces
/// systematically wrong classifications (Arabic → Esperanto, etc).
///
/// Use whichlang (the default) for production work.
/// This backend is kept for research evaluation only.
/// Sprint 5 will evaluate `fasttext-pure-rs` as a replacement.
```

**Step 3 — Update CLAUDE.md**

Add under "Known limitations":
```
## fastText backend (experimental — do not use in production)
fasttext 0.8.0 is not binary-compatible with lid.176.ftz.
Systematically wrong classifications confirmed in Sprint 1
diagnostic probe. whichlang is the production default.
Sprint 5: evaluate fasttext-pure-rs for 176-language coverage.
```

**Step 4 — Update --help text**

The `--classifier-backend` flag description should note:
"whichlang (default) — 16 languages, production-ready, offline.
fasttext — EXPERIMENTAL, known classification errors, do not use
for casework."

**Step 5 — Commit the diagnostic example**

`crates/augur-classifier/examples/lid_label_probe.rs` was
written but not committed in Sprint 1. Commit it now as a
non-default `[[example]]` entry in `augur-classifier/Cargo.toml`:

```toml
[[example]]
name = "lid_label_probe"
required-features = ["fasttext-probe"]
```

Gate it behind a feature flag so it doesn't run by default.

### Tests

```rust
#[test]
fn default_backend_is_whichlang() {
    // Parse CLI args with no --classifier-backend flag
    // Verify ClassifierBackend::Whichlang is selected
}
```

### Acceptance criteria — P1

- [ ] Default backend is whichlang
- [ ] fasttext marked experimental in docs and --help
- [ ] lid_label_probe.rs committed as feature-gated example
- [ ] CLAUDE.md updated
- [ ] 1 new test passes

---

## PRIORITY 2 — Whisper Temperature Fallback

### What it is

Whisper's greedy decoder can hallucinate on difficult audio
(background noise, heavy accents, multiple speakers). OpenAI's
recommended approach: if the initial decode produces low confidence
or detects "no speech", retry with higher temperature (more random
sampling) up to a maximum number of retries.

### Implementation

In `crates/augur-stt/src/whisper.rs`:

```rust
pub struct TranscribeOptions {
    pub preset: WhisperPreset,
    pub temperature: f32,           // initial: 0.0 (greedy)
    pub temperature_increment: f32, // retry step: 0.2
    pub max_temperature_retries: u8, // max retries: 5
    pub no_speech_threshold: f32,   // below this prob → retry: 0.6
    pub compression_ratio_threshold: f32, // hallucination guard: 2.4
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        Self {
            preset: WhisperPreset::Fast,
            temperature: 0.0,
            temperature_increment: 0.2,
            max_temperature_retries: 5,
            no_speech_threshold: 0.6,
            compression_ratio_threshold: 2.4,
        }
    }
}
```

The decode loop:

```rust
fn decode_with_fallback(
    &self,
    mel: &Tensor,
    options: &TranscribeOptions,
) -> Result<SttResult, AugurError> {
    let mut temperature = options.temperature;

    for attempt in 0..=options.max_temperature_retries {
        let result = self.decode_at_temperature(mel, temperature)?;

        // Check no-speech probability
        if result.no_speech_prob < options.no_speech_threshold {
            return Ok(result.into_stt_result());
        }

        // Check compression ratio (hallucination guard)
        let ratio = compression_ratio(&result.text);
        if ratio < options.compression_ratio_threshold {
            return Ok(result.into_stt_result());
        }

        if attempt == options.max_temperature_retries {
            log::warn!(
                "Whisper: max temperature retries reached ({}) — \
                 returning best attempt",
                options.max_temperature_retries
            );
            return Ok(result.into_stt_result());
        }

        log::debug!(
            "Whisper: retry {} at temperature {:.1} \
             (no_speech={:.2}, compression={:.2})",
            attempt + 1,
            temperature,
            result.no_speech_prob,
            ratio
        );
        temperature += options.temperature_increment;
    }

    unreachable!()
}

fn compression_ratio(text: &str) -> f32 {
    if text.is_empty() { return 0.0; }
    // Rough approximation: unique chars / total chars
    let unique: std::collections::HashSet<char> = text.chars().collect();
    unique.len() as f32 / text.len() as f32
}
```

### Wire into CLI

Add `--temperature` and `--max-retries` flags to `augur transcribe`
and `augur translate`:

```bash
augur transcribe --input noisy.mp3 --max-retries 5
```

### Tests

```rust
#[test]
fn temperature_fallback_options_default_correctly() {
    let opts = TranscribeOptions::default();
    assert_eq!(opts.temperature, 0.0);
    assert_eq!(opts.max_temperature_retries, 5);
}

#[test]
fn compression_ratio_detects_repetition() {
    // "aaaaaaaaaa" has ratio 0.1 — very repetitive, likely hallucination
    assert!(compression_ratio("aaaaaaaaaa") < 0.5);
    // Normal text has higher ratio
    assert!(compression_ratio("Hello world") > 0.5);
}
```

### Acceptance criteria — P2

- [ ] Temperature fallback implemented
- [ ] No-speech detection gates retry
- [ ] Compression ratio guards against hallucination
- [ ] CLI flags exposed
- [ ] 2 new tests pass
- [ ] Clippy clean

---

## PRIORITY 3 — PDF Text Extraction

### Why

PDF is one of the most common document formats in forensic evidence
— scanned documents, exported reports, communication transcripts.
AUGUR should handle it natively.

### Two approaches

**Approach A — pdf-extract crate (pure Rust)**
```toml
pdf-extract = "0.7"
```
Extracts text from PDF text layers. Fast, no system deps.
Does NOT handle scanned/image PDFs (no embedded text).

**Approach B — pdf-extract + Tesseract fallback**
Try pdf-extract first. If it returns empty text (scanned PDF),
fall back to rasterizing each page with `pdftoppm` (part of
poppler, common on forensic workstations) then running Tesseract.

Use Approach B — forensic PDFs are often scanned documents.

### Implementation

**Step 1 — Add PipelineInput::Pdf**

```rust
pub enum PipelineInput {
    Text(String),
    Audio(PathBuf),
    Image(PathBuf),
    Video(PathBuf),
    Pdf(PathBuf),      // NEW
}
```

Extend `detect_input_kind` to route `.pdf` → `PipelineInput::Pdf`.

**Step 2 — PDF extraction function**

In `crates/augur-ocr/src/lib.rs`:

```rust
pub fn extract_pdf_text(
    pdf_path: &Path,
    scratch_dir: &Path,
) -> Result<String, AugurError> {
    // Try pdf-extract first (text layer)
    let text = pdf_extract::extract_text(pdf_path)
        .map_err(|e| AugurError::Ocr(e.to_string()))?;

    if !text.trim().is_empty() {
        log::debug!("PDF text layer extracted: {} chars", text.len());
        return Ok(text);
    }

    // Scanned PDF — rasterize and OCR each page
    log::debug!("PDF has no text layer — rasterizing for OCR");
    extract_pdf_via_ocr(pdf_path, scratch_dir)
}

fn extract_pdf_via_ocr(
    pdf_path: &Path,
    scratch_dir: &Path,
) -> Result<String, AugurError> {
    // Use pdftoppm to rasterize pages to PNG
    // pdftoppm -png -r 300 input.pdf scratch_dir/page
    // Then run Tesseract on each page image
    // Concatenate results
}
```

**Step 3 — Wire into Pipeline**

```rust
PipelineInput::Pdf(path) => {
    let scratch = std::env::temp_dir().join("verify/pdf-scratch");
    std::fs::create_dir_all(&scratch)?;
    let text = extract_pdf_text(&path, &scratch)?;
    self.process_text(&text, target_language)
}
```

**Step 4 — Wire into CLI and batch**

```bash
augur translate --input document.pdf --target en
augur batch --input /evidence --types audio,video,image,pdf --target en
```

**Step 5 — Tests**

```rust
#[test]
fn pdf_input_detected_by_extension() {
    assert!(matches!(
        detect_input_kind(Path::new("doc.pdf")),
        PipelineInput::Pdf(_)
    ));
}

#[test]
fn pdf_extraction_returns_error_for_missing_file() {
    // Not panic, clear AugurError
}

#[test]
fn scanned_pdf_falls_back_to_ocr() {
    // Unit test with mock — empty text layer → OCR path taken
}
```

### Acceptance criteria — P3

- [ ] PDF input detected and routed through pipeline
- [ ] Text-layer PDFs extracted without Tesseract
- [ ] Scanned PDFs fall back to rasterize + OCR
- [ ] Batch processing includes PDF type
- [ ] Missing pdftoppm returns clear error
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 4 — ctranslate2 Benchmark

### Context

Sprint 3 shipped ctranslate2 as an auto-select backend but
couldn't benchmark it because sentencepiece wasn't installed.
This priority properly provisions the environment and measures
the speedup.

### Steps

**Step 1 — Provision the environment**

```bash
pip3 install ctranslate2 sentencepiece transformers \
    --break-system-packages 2>&1 | tail -5
python3 -c "import ctranslate2, sentencepiece; print('OK')"
```

**Step 2 — Run the benchmark**

Translate a 100-word Arabic paragraph with both backends:

```bash
# Time transformers backend
time python3 crates/augur-translate/scripts/worker_script.py \
    --text "$(cat tests/fixtures/arabic_100_words.txt)" \
    --src ara_Arab --tgt eng_Latn \
    --backend transformers

# Time ctranslate2 backend  
time python3 crates/augur-translate/scripts/worker_script_ct2.py \
    --text "$(cat tests/fixtures/arabic_100_words.txt)" \
    --src ara_Arab --tgt eng_Latn
```

Create `tests/fixtures/arabic_100_words.txt` with a 100-word
Arabic text sample if it doesn't exist.

**Step 3 — Document results**

Add to CLAUDE.md under "Sprint 4 decisions":
```
## ctranslate2 benchmark results (2026-04-25, M1 Max)
transformers backend: Xs
ctranslate2 backend:  Ys
speedup: Zx
recommendation: [which to use as default]
```

**Step 4 — Update auto-select logic if needed**

If ctranslate2 is significantly faster AND quality is equivalent,
update `Backend::Auto` to prefer ct2 more aggressively.

If quality is worse, document the tradeoff and keep transformers
as default with ct2 as opt-in.

**Step 5 — Add a benchmark fixture**

Commit `tests/fixtures/arabic_100_words.txt` so future benchmarks
are reproducible with the same input.

### Acceptance criteria — P4

- [ ] ctranslate2 and sentencepiece installed and working
- [ ] Benchmark run against same 100-word input
- [ ] Results documented in CLAUDE.md
- [ ] Backend recommendation updated based on results
- [ ] Fixture committed

---

## Session log format

```
## AUGUR Sprint 4 — [date]

P1 fastText default fix: PASSED
  - Default now whichlang: yes/no
  - lid_label_probe committed: yes/no

P2 Temperature fallback: PASSED / FAILED
  - Fallback loop implemented: yes/no
  - No-speech detection: yes/no

P3 PDF extraction: PASSED / FAILED
  - Text-layer PDFs: yes/no
  - Scanned PDF fallback: yes/no

P4 ctranslate2 benchmark: PASSED / FAILED
  - transformers time: Xs
  - ctranslate2 time: Ys
  - Speedup: Zx

Final test count: [number]
Clippy: CLEAN
Offline invariant: MAINTAINED
MT advisory: ALWAYS PRESENT
```

---

## Commit format

```
fix: augur-sprint-4-P1 whichlang default + fasttext experimental label
feat: augur-sprint-4-P2 Whisper temperature fallback + hallucination guard
feat: augur-sprint-4-P3 PDF extraction — text layer + OCR fallback
docs: augur-sprint-4-P4 ctranslate2 benchmark results + recommendation
```

---

_Sprint 4 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_After Sprint 4, AUGUR is production-ready for v1.0 release._
