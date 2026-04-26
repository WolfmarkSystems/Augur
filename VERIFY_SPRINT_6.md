# VERIFY Overnight Sprint — Production Polish + Real-World Testing
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/verify/_

---

## Before starting

1. Read CLAUDE.md completely
2. Run `cargo test --workspace 2>&1 | tail -5`
3. Run `cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3`
4. Both must pass before any code changes

Current state:
- Version: v1.0
- Tests: 61 passing (57 unit + 4 integration #[ignore])
- Sprints 1-5 complete
- Capabilities: classify, transcribe, translate, video, batch, PDF, OCR,
  speaker diarization (graceful fallback), air-gap package

---

## Hard rules (absolute)

- Zero `.unwrap()` in production code paths
- Zero `unsafe{}` without explicit justification
- Zero `println!` in production — use `log::` macros
- All errors handled explicitly
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes after every change
- No TODO/FIXME in committed code

## OFFLINE INVARIANT — NON-NEGOTIABLE
All inference runs locally. No content leaves the machine.
Network only for one-time model weight downloads via named consts.

## MACHINE TRANSLATION ADVISORY — NON-NEGOTIABLE  
Every TranslationResult: `is_machine_translation = true`,
`advisory_notice` non-empty. No exceptions.

---

## PRIORITY 1 — Batch Report Improvements

### Context

Sprint 3 shipped batch processing with JSON output. In real
forensic use, examiners need the batch output to be more
structured and actionable.

### Improvements

**1a — Add CSV output format**

```bash
verify batch --input /evidence --target en --output report.csv
```

CSV format:
```
file_path,input_type,detected_language,is_foreign,transcript,translation,error
recording_001.mp3,audio,ar,true,"مرحبا بالعالم","Hello world",
doc_002.pdf,pdf,zh,true,"你好世界","Hello world",
clean_003.mp3,audio,en,false,"This is English",,
```

Add `OutputFormat::Csv` to the batch subcommand.

**1b — Summary statistics at end of JSON report**

Add a `summary` field to `BatchResult`:

```rust
pub struct BatchSummary {
    pub total_files: u32,
    pub processed: u32,
    pub foreign_language_files: u32,
    pub translated_files: u32,
    pub errors: u32,
    pub languages_detected: HashMap<String, u32>, // {"ar": 5, "zh": 3}
    pub processing_time_seconds: f64,
    pub machine_translation_notice: String,        // always present
}
```

**1c — Progress file for long batch runs**

When `--output` is specified, write a `<output>.progress.json`
file that updates after each file. Examiners can tail this file
to monitor progress on large directories without waiting for
the full run to complete.

### Tests

```rust
#[test]
fn batch_csv_output_has_correct_headers() {
    // Verify CSV header row matches spec
}

#[test]
fn batch_summary_languages_counts_correctly() {
    // 3 Arabic + 2 Chinese files
    // Verify languages_detected = {"ar": 3, "zh": 2}
}

#[test]
fn batch_summary_machine_translation_notice_present() {
    // BatchSummary.machine_translation_notice is non-empty
}
```

### Acceptance criteria — P1

- [ ] CSV output format working
- [ ] BatchSummary with language breakdown in JSON output
- [ ] machine_translation_notice in BatchSummary (always)
- [ ] Progress file written during long batches
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 2 — Language Detection Confidence Reporting

### Context

whichlang returns high confidence on short text but may be less
reliable on very short inputs (< 10 words). Examiners need to
know when a language detection is high-confidence vs uncertain.

### Improvements

**2a — Confidence tiers**

Add to `ClassificationResult`:

```rust
pub enum ConfidenceTier {
    High,      // > 0.85 — reliable, use for casework
    Medium,    // 0.60-0.85 — likely correct, verify if critical
    Low,       // < 0.60 — uncertain, human review recommended
}

pub struct ClassificationResult {
    // existing fields...
    pub confidence_tier: ConfidenceTier,
    pub input_word_count: usize,        // so examiner knows why tier is low
    pub advisory: Option<String>,       // human-readable note if Low/Medium
}
```

**2b — Short input warning**

If input has < 10 words, add advisory:
"Short input (N words) — language detection may be unreliable.
Verify with a human linguist if this evidence is critical."

**2c — CLI display**

```
[VERIFY] Language detected: ar (Arabic)
         Confidence: HIGH (0.97)
         Input: 47 words

[VERIFY] Language detected: fa (Farsi) 
         Confidence: MEDIUM (0.73)
         ⚠ Short input (6 words) — verify with human linguist
           if this evidence is critical to your case
```

**2d — Update batch JSON to include confidence tier per file**

### Tests

```rust
#[test]
fn high_confidence_long_arabic_text() {
    // 50-word Arabic text → High tier
}

#[test]
fn low_confidence_very_short_input() {
    // 3-word input → Low tier with advisory
}

#[test]
fn medium_confidence_includes_advisory_text() {
    // Medium tier → advisory is non-empty
}
```

### Acceptance criteria — P2

- [ ] ConfidenceTier enum with three levels
- [ ] Short input triggers advisory notice
- [ ] CLI displays tier and advisory
- [ ] Batch JSON includes confidence tier per file
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 3 — VERIFY Self-Test Command

### Context

Examiners deploying VERIFY need a way to verify it's working
correctly before using it on real evidence. A self-test command
runs a known input through the full pipeline and confirms the
output matches expected values.

### Implementation

```bash
verify self-test
```

Output:
```
[VERIFY] Running self-test...

✓ Classification: Arabic text → ar (confidence: HIGH)
✓ Classification: English text → en (not foreign)
✓ Classification: Empty input → handled gracefully
✓ Audio preprocessing: ffmpeg available
✓ STT: Whisper model cached (tiny preset)
  ⚠ STT inference: model not downloaded (run: verify self-test --full)
✓ Translation: NLLB model cached
  ⚠ Translation inference: requires Python + transformers
✓ OCR: Tesseract available
✓ Air-gap: VERIFY_AIRGAP_PATH not set (online mode)
✓ Offline invariant: no unexpected network calls detected

Self-test PASSED (5 checks passed, 2 skipped — run --full to test all)

[VERIFY] This installation is ready for casework.
```

The `--full` flag actually downloads and runs inference on a
built-in test sample (hardcoded Arabic text → expected English
translation). This requires model downloads.

**Implementation:**

```rust
pub struct SelfTestResult {
    pub checks: Vec<SelfTestCheck>,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub ready_for_casework: bool,
}

pub struct SelfTestCheck {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

pub enum CheckStatus {
    Pass,
    Fail,
    Skip,     // requires --full or model not cached
    Warning,  // works but with caveats
}
```

### Tests

```rust
#[test]
fn self_test_classification_check_passes() {
    // Run just the classification check
    // Verify it passes without network
}

#[test]
fn self_test_reports_ffmpeg_availability() {
    // Check detects whether ffmpeg is installed
    // Returns Pass or Warning, not panic
}

#[test]
fn self_test_result_ready_for_casework_requires_no_failures() {
    // If any check is Fail, ready_for_casework = false
}
```

### Acceptance criteria — P3

- [ ] `verify self-test` command runs and outputs results
- [ ] Classification, STT availability, OCR availability all checked
- [ ] Air-gap detection working
- [ ] `--full` flag triggers inference tests
- [ ] `ready_for_casework` correctly reflects pass/fail state
- [ ] 3 new tests pass
- [ ] Clippy clean
- [ ] Offline invariant maintained (basic self-test has zero network)

---

## PRIORITY 4 — Pashto/Persian Disambiguation Documentation

### Context

Sprint 5 confirmed that Pashto (ps) is classified as Persian/Farsi
(fa) by both whichlang and fasttext-pure-rs. This is a model-level
limitation — both Arabic-script languages are very similar and the
models confuse them.

This is a known limitation that affects LE/IC users in Afghanistan,
Pakistan, and related casework. It must be documented clearly.

### Implementation

**4a — Update classification output for fa/ps ambiguity**

When whichlang returns `fa` (Persian/Farsi) with confidence below
0.90, add an advisory:

```
⚠ LANGUAGE ADVISORY: Farsi (fa) and Pashto (ps) are linguistically 
  similar Arabic-script languages. When this classification is 
  critical to your case, verify with a certified human linguist 
  fluent in both languages.
  
  Context: Both whichlang and fasttext models frequently confuse 
  Farsi and Pashto. This is a known limitation of current automated
  language identification for these languages.
```

**4b — Add to docs/LANGUAGE_LIMITATIONS.md**

Create `docs/LANGUAGE_LIMITATIONS.md`:

```markdown
# VERIFY — Known Language Detection Limitations

## Pashto / Persian (Farsi) Confusion

**Languages affected:** Pashto (ps), Persian/Farsi (fa)
**Severity:** High for LE/IC casework in Afghanistan/Pakistan region
**Root cause:** Both languages use Arabic script with similar
  character distributions. Current models (whichlang, fasttext)
  were not specifically optimized for this distinction.

**Behavior:** VERIFY may classify Pashto text as Farsi (fa).
  Confidence scores do not reliably indicate when this confusion
  has occurred.

**Mitigation:** When Farsi is detected in evidence from a context
  where Pashto is plausible, always verify with a certified human
  linguist. VERIFY will surface an advisory notice when Farsi is
  detected with confidence below 0.90.

**Sprint reference:** VERIFY Sprint 5 (2026-04-25) confirmed this
  limitation via the lid_label_probe diagnostic tool.

## Short Text Classification

**Input length:** < 10 words
**Severity:** Medium
**Behavior:** All language classifiers are less reliable on very
  short inputs. VERIFY surfaces a warning when input is < 10 words.

## Additional Forensically Important Language Pairs

The following language pairs may also be confused by automated
classification:
- Serbian/Croatian/Bosnian (sr/hr/bs) — South Slavic, Cyrillic/Latin
- Malay/Indonesian (ms/id) — very similar, different contexts
- Hindi/Urdu (hi/ur) — same language, different scripts

When any of these languages are detected in high-stakes casework,
verify with a human linguist.
```

**4c — Add to CLAUDE.md under Known Limitations**

Document the Pashto/Persian issue in CLAUDE.md so future
developers know not to treat this as a bug to fix quickly.

**4d — Advisory in TranslationResult when fa detected**

When source language is `fa` (Farsi), add to `advisory_notice`:
"Note: Automated tools may confuse Farsi (fa) with Pashto (ps).
Verify language identification if this is critical evidence."

### Tests

```rust
#[test]
fn farsi_detection_includes_disambiguation_advisory() {
    // When source_language == "fa", advisory_notice contains 
    // disambiguation warning
}

#[test]
fn language_limitations_doc_exists() {
    // docs/LANGUAGE_LIMITATIONS.md exists and is non-empty
}
```

### Acceptance criteria — P4

- [ ] Farsi detection triggers disambiguation advisory in output
- [ ] `docs/LANGUAGE_LIMITATIONS.md` written and committed
- [ ] CLAUDE.md updated with Pashto/Persian limitation
- [ ] Advisory present in TranslationResult when fa detected
- [ ] 2 new tests pass
- [ ] Clippy clean
- [ ] MT advisory still always present (not replaced by language advisory)

---

## After all priorities complete

```bash
cargo test --workspace 2>&1 | grep "test result" | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

All must pass. Then commit:

```bash
git add -A
git commit -m "feat: verify-overnight batch improvements + confidence tiers + self-test + Pashto advisory"
```

Report:
- Which priorities passed
- Test count before (61) and after
- Output of `verify self-test` command
- Any deviations from spec

---

## What this sprint does NOT touch

- Strata code (separate repo)
- Whisper model weights (already handled)
- The core pipeline architecture (proven in Sprints 1-5)
- fasttext-pure-rs (confirmed working in Sprint 5)

---

_VERIFY Overnight Sprint authored by: Claude (architect) + KR (approved)_
_Execute autonomously. Only stop for hard rule violations_
_or architectural decisions not covered by this spec._
_This is an overnight run. Go deep._
