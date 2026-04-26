# AUGUR Sprint 8 — Strata Plugin Wiring + Multi-Language Evidence + Video Diarization
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Before starting

1. Read CLAUDE.md completely
2. Run `cargo test --workspace 2>&1 | tail -5`
3. Confirm 105 tests passing before any changes

---

## Hard rules (absolute)

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- All errors handled explicitly
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes after every change
- Offline invariant maintained
- MT advisory always present on all translated output

---

## PRIORITY 1 — Real Strata Plugin Trait Wiring

### Context

Sprint 3 shipped the Strata plugin feature-gated behind
`augur-plugin-sdk/strata` with a placeholder implementation.
Sprint 5 documented the decision not to vendor the full SDK
to avoid dragging in filesystem dependencies.

The right approach: vendor only the plugin SDK crate (not strata-fs),
implement the real trait, and wire AUGUR as a first-class Strata plugin.

### Implementation

**Step 1 — Vendor strata-plugin-sdk only**

```bash
mkdir -p vendor
cp -r ~/Wolfmark/strata/crates/strata-plugin-sdk vendor/strata-plugin-sdk
```

Check what strata-plugin-sdk depends on:
```bash
cat vendor/strata-plugin-sdk/Cargo.toml
```

If it only depends on serde and common crates → proceed.
If it depends on strata-fs or strata-core → stub those out or
use the path dep approach with `[patch.crates-io]`.

**Step 2 — Implement real StrataPlugin trait**

In `crates/augur-plugin-sdk/src/strata_impl.rs`:

```rust
use strata_plugin_sdk::{
    StrataPlugin, PluginContext, ArtifactRecord,
    Confidence, ForensicValue, PluginError,
};

pub struct AugurStrataPlugin {
    pipeline: Pipeline,
    target_language: String,
}

impl StrataPlugin for AugurStrataPlugin {
    fn name(&self) -> &str { "AUGUR" }
    fn version(&self) -> &str { env!("CARGO_PKG_VERSION") }
    fn category(&self) -> &str { "Analyzer" }
    fn description(&self) -> &str {
        "Foreign language detection and translation. \
         Surfaces translated content as advisory artifacts."
    }

    fn execute(
        &self,
        ctx: PluginContext,
    ) -> Result<Vec<ArtifactRecord>, PluginError> {
        let mut artifacts = Vec::new();
        
        // Walk root_path for audio, video, image files
        // For each: classify language
        // If foreign: run STT/OCR + translate
        // Return translation artifacts
        
        self.walk_and_translate(&ctx.root_path, &mut artifacts)?;
        Ok(artifacts)
    }
}
```

**Step 3 — Translation artifact format**

Each translation becomes a Strata artifact:
```rust
ArtifactRecord {
    name: format!("[AUGUR] {}: {} → {}",
        file_name, source_lang, target_lang),
    artifact_type: "augur_translation".to_string(),
    category: "Communications".to_string(),
    subcategory: "Foreign Language".to_string(),
    value: translated_text,
    source_file: file_path.to_string_lossy().to_string(),
    source_plugin: "AUGUR".to_string(),
    confidence: Confidence::Medium,  // MT is always Medium
    forensic_value: ForensicValue::High,
    is_advisory: true,               // always true for MT
    advisory_notice: format!(
        "[MT — review by certified human translator] {}",
        translated_text
    ),
    mitre_technique: String::new(),
    ..Default::default()
}
```

**Step 4 — Register in Strata**

Document in CLAUDE.md how to add AUGUR to Strata's PLUGIN_NAMES.
The actual registration happens in Strata's codebase — AUGUR
just needs to compile as a valid StrataPlugin implementor.

**Step 5 — Integration test**

```rust
#[test]
#[cfg(feature = "strata")]
fn strata_plugin_execute_returns_advisory_artifacts() {
    // Mock PluginContext with a temp dir containing a text file
    // Write Arabic text to the temp dir
    // Run execute()
    // Verify: artifacts returned, all is_advisory = true,
    //         all advisory_notice non-empty
}

#[test]
#[cfg(feature = "strata")]
fn strata_plugin_skips_non_foreign_files() {
    // English-only text file
    // Verify: no translation artifacts returned (is_foreign = false)
}
```

### Acceptance criteria — P1

- [ ] `strata-plugin-sdk` vendored (SDK only, not strata-fs)
- [ ] `AugurStrataPlugin` implements real `StrataPlugin` trait
- [ ] `cargo build --features augur-plugin-sdk/strata` succeeds
- [ ] Translation artifacts have `is_advisory = true`
- [ ] `advisory_notice` always non-empty
- [ ] `confidence = Medium` on all translation artifacts
- [ ] 2 feature-gated tests pass
- [ ] Clippy clean both configs

---

## PRIORITY 2 — Multi-Language Evidence Batch

### Context

Real forensic evidence often contains multiple languages in the
same directory — an iPhone backup might have Arabic iMessages,
English emails, and Chinese WeChat exports. AUGUR's batch processor
should handle this gracefully and produce a structured multi-language
report.

### Implementation

**Step 1 — Language grouping in batch results**

Extend `BatchResult` to group files by detected language:

```rust
pub struct LanguageGroup {
    pub language_code: String,      // "ar", "zh", "ru"
    pub language_name: String,      // "Arabic", "Chinese", "Russian"
    pub file_count: u32,
    pub files: Vec<BatchFileResult>,
    pub total_words: u32,           // approximate
}

pub struct BatchResult {
    // existing fields...
    pub language_groups: Vec<LanguageGroup>,
    pub dominant_language: Option<String>,  // most common foreign language
}
```

**Step 2 — Multi-target translation**

Add `--targets` flag (plural) to batch:

```bash
# Translate everything to English
augur batch --input /evidence --target en

# Translate Arabic to English AND Russian to English
augur batch --input /evidence --targets ar:en,ru:en

# Auto-detect all foreign languages and translate to English
augur batch --input /evidence --target en --all-foreign
```

When `--all-foreign` is used:
1. Classify every file
2. Group by language
3. Translate each non-English file to English
4. Report groups separately

**Step 3 — Language summary in report**

JSON report gets a `language_summary` section:

```json
{
  "language_summary": {
    "total_foreign_files": 12,
    "languages_detected": {
      "ar": { "name": "Arabic", "files": 8, "words": 1247 },
      "zh": { "name": "Chinese", "files": 3, "words": 445 },
      "ru": { "name": "Russian", "files": 1, "words": 89 }
    },
    "dominant_foreign_language": "ar"
  }
}
```

**Step 4 — HTML report language sections**

When generating HTML reports with multiple languages, group
findings by language with a language header:

```html
<h2>Arabic Evidence (8 files)</h2>
  [file results...]
<h2>Chinese Evidence (3 files)</h2>
  [file results...]
```

**Step 5 — Tests**

```rust
#[test]
fn language_groups_correctly_populated() {
    // BatchResult with 3 Arabic + 2 Chinese BatchFileResults
    // Verify language_groups has 2 groups with correct counts
}

#[test]
fn dominant_language_is_most_frequent() {
    // 5 Arabic, 2 Chinese, 1 Russian
    // dominant_language == "ar"
}

#[test]
fn all_foreign_flag_skips_english_files() {
    // Mix of English and Arabic files
    // With --all-foreign, English files skipped for translation
    // Arabic files translated
}
```

### Acceptance criteria — P2

- [ ] `LanguageGroup` struct in `BatchResult`
- [ ] `--all-foreign` flag classifies and translates all foreign files
- [ ] Language summary section in JSON/HTML reports
- [ ] HTML report groups findings by language
- [ ] MT advisory present per-group in HTML
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 3 — Video Diarization Pipeline

### Context

Sprint 3 shipped video audio extraction → STT → translation.
Sprint 5 shipped speaker diarization for audio (pyannote, opt-in).
These two should be combined: video → extract audio → STT +
diarization → per-speaker translated transcript.

This is the highest forensic value feature for intercepted
communications — a video of a meeting between two subjects
should produce a labeled transcript showing who said what.

### Implementation

**Step 1 — Wire diarization into video pipeline**

In `crates/augur-core/src/pipeline.rs`, when processing video:

```rust
PipelineInput::Video(path) => {
    let audio = extract_audio_from_video(&path, &scratch)?;
    
    let stt_result = self.stt.transcribe(&audio)?;
    
    // If diarization available and --diarize flag set
    let enriched = if self.diarization.is_available() && options.diarize {
        let diar = self.diarization.diarize(&audio)?;
        merge_stt_with_diarization(stt_result.segments, diar)
    } else {
        stt_result.segments.into_iter()
            .map(|s| EnrichedSegment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: s.text,
                speaker_id: "UNKNOWN".to_string(),
                translated_text: None,
            })
            .collect()
    };
    
    // Translate each segment
    let translated = self.translate_segments(enriched, target_language)?;
    Ok(PipelineResult::from_video_segments(translated))
}
```

**Step 2 — Per-speaker translation**

Translate each speaker's segments separately and label:

```
[0:00-0:05] SPEAKER_00 (Arabic):
  Original: مرحبا بالعالم
  Translation: Hello world

[0:05-0:12] SPEAKER_01 (Arabic):  
  Original: كيف حالك
  Translation: How are you

[0:12-0:18] SPEAKER_00 (Arabic):
  Original: بخير شكرا
  Translation: Fine, thank you
```

**Step 3 — CLI output**

```bash
augur translate --input interview.mp4 --target en --diarize
```

Output:
```
[AUGUR] Input: interview.mp4 (video)
[AUGUR] Extracting audio...
[AUGUR] Running STT (Whisper Fast)...
[AUGUR] Language detected: ar (Arabic)
[AUGUR] Running speaker diarization...
[AUGUR] Speakers detected: 2 (SPEAKER_00, SPEAKER_01)
[AUGUR] Translating ar → en...

[0:00-0:05] SPEAKER_00: Hello world
[0:05-0:12] SPEAKER_01: How are you
[0:12-0:18] SPEAKER_00: Fine, thank you

⚠ MACHINE TRANSLATION NOTICE
  All translations are machine-generated...
  Speaker labels are automated — verify with human analyst
  if speaker identity is material to your case.
```

**Step 4 — Speaker advisory**

Add speaker diarization advisory alongside MT advisory:
"Speaker labels (SPEAKER_00, SPEAKER_01) are generated by
automated voice segmentation. Do not use automated speaker
labels as definitive identification of individuals without
human expert verification."

This advisory is separate from but appears alongside the MT advisory.

**Step 5 — Tests**

```rust
#[test]
fn video_diarization_pipeline_produces_enriched_segments() {
    // Unit test with mock STT + mock diarization results
    // Verify merged output has speaker labels + text + timestamps
}

#[test]
fn speaker_advisory_always_present_when_diarization_used() {
    // PipelineResult with diarization → speaker advisory non-empty
}

#[test]
fn video_without_diarization_still_produces_transcript() {
    // --diarize not set → segments still returned, UNKNOWN speaker
}
```

### Acceptance criteria — P3

- [ ] Video → STT + diarization pipeline works end-to-end
- [ ] Per-speaker segment translation
- [ ] Speaker labels in CLI output
- [ ] Speaker diarization advisory present when diarization used
- [ ] Works gracefully without pyannote (UNKNOWN speaker fallback)
- [ ] MT advisory still present alongside speaker advisory
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## After all priorities complete

```bash
cargo test --workspace 2>&1 | grep "test result" | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
```

Commit:
```bash
git add -A
git commit -m "feat: augur-sprint-8 Strata plugin wiring + multi-language batch + video diarization"
```

Report:
- Which priorities passed
- Test count before (105) and after
- Whether `cargo build --features augur-plugin-sdk/strata` succeeds
- Any deviations from spec

---

_AUGUR Sprint 8 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_P1 closes the Strata integration loop._
_P3 is the highest forensic value feature — labeled video transcripts._
