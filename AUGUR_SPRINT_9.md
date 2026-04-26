# AUGUR Sprint 9 — Pashto Disambiguation + Batch Parallel + Evidence Integration
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Before starting

1. Read CLAUDE.md completely
2. Run `cargo test --workspace 2>&1 | tail -5`
3. Confirm 113 tests passing (default build)

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

## PRIORITY 1 — Pashto Disambiguation via Script Analysis

### Context

Sprint 5 confirmed that automated tools confuse Pashto (ps) with
Farsi/Persian (fa). Sprint 6 added an advisory when Farsi is detected.
This sprint attempts to actually distinguish them using script-level
analysis — not perfect, but better than nothing.

### The linguistic distinction

Pashto and Farsi both use Arabic script but have different character
distributions:

**Pashto-specific characters** (not in standard Farsi):
- ټ (U+0679) — ARABIC LETTER TTEH
- ډ (U+0688) — ARABIC LETTER DDAL
- ړ (U+0693) — ARABIC LETTER REH WITH RING BELOW
- ږ (U+0696) — ARABIC LETTER REH WITH DOT BELOW AND DOT ABOVE
- ژ (U+0698) — ARABIC LETTER JEH (also in Farsi but rare)
- ښ (U+069A) — ARABIC LETTER SEEN WITH DOT BELOW AND DOT ABOVE
- ګ (U+06AB) — ARABIC LETTER KAF WITH RING
- ڼ (U+06BC) — ARABIC LETTER NOON WITH RING
- ۍ (U+06CD) — ARABIC LETTER YEH WITH TAIL
- ې (U+06D0) — ARABIC LETTER E

**Farsi-specific characters** (not in standard Pashto):
- پ (U+067E) — ARABIC LETTER PEH (common in Farsi)
- چ (U+0686) — ARABIC LETTER TCHEH
- ژ (U+0698) — ARABIC LETTER JEH
- گ (U+06AF) — ARABIC LETTER GAF

### Implementation

**Step 1 — Script analyzer**

Create `crates/augur-classifier/src/script.rs`:

```rust
/// Analyze Arabic-script text to distinguish Pashto from Farsi
/// Returns a score: positive = more likely Pashto, negative = more likely Farsi
pub fn pashto_farsi_score(text: &str) -> PashtoFarsiAnalysis {
    // Count Pashto-specific characters
    // Count Farsi-specific characters
    // Return analysis with character counts and recommendation
}

pub struct PashtoFarsiAnalysis {
    pub pashto_char_count: u32,
    pub farsi_char_count: u32,
    pub pashto_specific_chars: Vec<char>,  // which ones found
    pub farsi_specific_chars: Vec<char>,
    pub recommendation: ScriptRecommendation,
    pub confidence: f32,
}

pub enum ScriptRecommendation {
    LikelyPashto,
    LikelyFarsi,
    Ambiguous,   // both or neither specific chars found
}
```

**Step 2 — Wire into classifier**

When `whichlang` returns `fa` (Farsi):
1. Run `pashto_farsi_score()` on the input text
2. If `recommendation == LikelyPashto` with confidence > 0.7:
   - Change language code to `ps`
   - Add note: "Reclassified from Farsi to Pashto based on
     script analysis (found Pashto-specific characters: ټ, ډ, ...)"
3. If `Ambiguous`:
   - Keep `fa` but add enhanced advisory noting both are possible
4. Update `ClassificationResult` with the disambiguation result

**Step 3 — Update CLI output**

When disambiguation runs:
```
[AUGUR] Language detected: fa (Farsi/Persian)
         Script analysis: Found Pashto-specific characters (ټ, ډ, ړ)
         Reclassified: ps (Pashto) — confidence: 0.82
         ⚠ Verify with human linguist fluent in both Farsi and Pashto
```

**Step 4 — Update self-test**

Add a disambiguation check to `augur self-test`:
- Test with text containing known Pashto-specific characters
- Verify reclassification occurs

**Step 5 — Tests**

```rust
#[test]
fn pashto_specific_chars_trigger_reclassification() {
    // Text with ټ, ډ, ړ → recommendation = LikelyPashto
}

#[test]
fn farsi_text_without_pashto_chars_stays_farsi() {
    // Standard Farsi text → recommendation = LikelyFarsi or Ambiguous
    // No false reclassification
}

#[test]
fn ambiguous_text_keeps_advisory() {
    // Text with no script-specific chars → Ambiguous
    // Advisory still present
}

#[test]
fn disambiguation_note_in_classification_result() {
    // When reclassification occurs → note field non-empty
}
```

### Acceptance criteria — P1

- [ ] Pashto-specific character detection implemented
- [ ] Reclassification from fa → ps when confidence > 0.7
- [ ] Advisory updated with specific characters found
- [ ] self-test includes disambiguation check
- [ ] 4 new tests pass
- [ ] Clippy clean
- [ ] MT advisory still present on any translation output

---

## PRIORITY 2 — Parallel Batch Processing

### Context

Large evidence directories (500+ files) run slowly because batch
processing is sequential. Multi-core M1 Max has 10 cores sitting
idle. Parallel processing with a configurable thread pool would
dramatically speed up large batch runs.

### Implementation

**Step 1 — Add rayon dependency**

```toml
[dependencies]
rayon = "1.8"
```

rayon is the standard Rust data-parallelism library. Pure Rust,
no unsafe in user code required.

**Step 2 — Parallel batch execution**

In `crates/augur-core/src/pipeline.rs`, replace the sequential
file loop with rayon parallel iterator:

```rust
use rayon::prelude::*;

pub fn run_batch_parallel(
    &self,
    input_dir: &Path,
    target_language: &str,
    file_types: Option<&[&str]>,
    num_threads: Option<usize>,
) -> Result<BatchResult, AugurError> {
    // Configure thread pool
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads.unwrap_or(
            num_cpus::get().min(8)  // cap at 8 to avoid OOM
        ))
        .build()
        .map_err(|e| AugurError::Batch(e.to_string()))?;
    
    // Collect files first
    let files = self.collect_files(input_dir, file_types)?;
    
    // Process in parallel
    let results: Vec<BatchFileResult> = pool.install(|| {
        files.par_iter()
            .map(|path| self.process_single_file(path, target_language))
            .collect()
    });
    
    Ok(self.build_batch_result(results))
}
```

**Important constraints:**
- The classifier (whichlang) is stateless → safe for parallel use
- The STT engine (candle) is NOT thread-safe → use per-thread instances
  OR process audio files sequentially, text/image in parallel
- NLLB subprocess → each subprocess is independent → safe to parallelize

**Step 3 — CLI flag**

```bash
augur batch --input /evidence --target en --threads 4
augur batch --input /evidence --target en --threads auto  # default
```

`--threads auto` uses `min(num_cpus, 8)`.

**Step 4 — Progress reporting with parallel**

Update the progress file writer to be thread-safe:
```rust
use std::sync::{Arc, Mutex};
let progress = Arc::new(Mutex::new(BatchProgress::new(total)));
```

**Step 5 — Benchmark**

After implementation, run a benchmark:
- 20 text files of varying sizes
- Sequential vs parallel (4 threads)
- Document speedup in CLAUDE.md

**Step 6 — Tests**

```rust
#[test]
fn parallel_batch_produces_same_results_as_sequential() {
    // Run same directory sequentially and in parallel
    // Verify results are equivalent (order may differ)
}

#[test]
fn parallel_batch_thread_count_respected() {
    // Configure 2 threads, verify rayon pool size
}

#[test]
fn parallel_batch_progress_file_is_consistent() {
    // Run parallel batch, verify progress file has valid JSON
    // at end of run (no torn writes)
}
```

### Acceptance criteria — P2

- [ ] `rayon` parallel batch implementation
- [ ] `--threads` CLI flag with auto default
- [ ] STT/audio handled safely (no data races)
- [ ] Progress file thread-safe
- [ ] Results equivalent to sequential
- [ ] Speedup measured and documented
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 3 — Evidence Package Export

### Context

After running AUGUR on an evidence directory, examiners need
to package the results for sharing with prosecutors, other agencies,
or archival. An evidence package contains the original files,
AUGUR translations, the batch report, and a manifest.

### Implementation

**Step 1 — Evidence package structure**

```bash
augur package --input /evidence --output case-001-verify.zip
```

Creates a zip containing:
```
case-001-verify/
├── MANIFEST.json          ← package metadata + integrity hashes
├── REPORT.html            ← the batch HTML report
├── REPORT.json            ← the batch JSON report
├── translations/          ← one .txt file per translated file
│   ├── recording_001.mp3.en.txt
│   ├── doc_002.pdf.en.txt
│   └── ...
├── original/              ← symlinks or copies of source files
│   ├── recording_001.mp3
│   └── doc_002.pdf
└── CHAIN_OF_CUSTODY.txt   ← who ran AUGUR, when, on what system
```

**Step 2 — MANIFEST.json**

```json
{
  "package_version": "1.0",
  "created_at": "2026-04-26T10:00:00Z",
  "examiner": "D. Examiner",
  "agency": "Wolfmark Systems",
  "case_number": "2026-001",
  "augur_version": "1.0.0",
  "source_directory": "/evidence",
  "file_count": 47,
  "translated_count": 12,
  "machine_translation_notice": "...",  // always present
  "files": [
    {
      "original_name": "recording_001.mp3",
      "sha256": "a3f4b2...",
      "language": "ar",
      "translated": true,
      "translation_file": "translations/recording_001.mp3.en.txt"
    }
  ]
}
```

**Step 3 — Chain of custody text**

```
AUGUR Evidence Package — Chain of Custody
==========================================
Package created: 2026-04-26 10:00:00 UTC
Examiner: D. Examiner
System: MacBook Pro M1 Max (darwin arm64)
AUGUR version: 1.0.0

Source directory: /evidence
Files processed: 47
Foreign language files: 12
Languages detected: Arabic (8), Chinese (3), Russian (1)

MACHINE TRANSLATION NOTICE:
All translations in this package were produced by AUGUR,
an automated machine translation system. They have not been
reviewed by a certified human translator. For legal proceedings,
verify all translations with a qualified human linguist.

Original files: SHA-256 hashes listed in MANIFEST.json
Translation files: Generated by NLLB-200-distilled-600M
```

**Step 4 — Original file handling**

For large evidence directories, copying originals into the zip
is impractical. Add a `--no-originals` flag that omits the
`original/` directory but still includes SHA-256 hashes in
the manifest for integrity verification.

Default: `--no-originals` (safer for large evidence)
With flag: `--include-originals` copies source files into zip

**Step 5 — Tests**

```rust
#[test]
fn package_manifest_includes_mt_notice() {
    // Create package, parse MANIFEST.json
    // Verify machine_translation_notice present and non-empty
}

#[test]
fn package_manifest_sha256_correct() {
    // Create a file with known content
    // Package it, verify SHA-256 in manifest matches
}

#[test]
fn package_chain_of_custody_present() {
    // Create package, verify CHAIN_OF_CUSTODY.txt exists
    // and contains examiner and timestamp
}
```

### Acceptance criteria — P3

- [ ] `augur package` command creates zip
- [ ] MANIFEST.json with all required fields
- [ ] machine_translation_notice in MANIFEST (always)
- [ ] CHAIN_OF_CUSTODY.txt with examiner/timestamp
- [ ] `--no-originals` default, `--include-originals` opt-in
- [ ] Per-translation .txt files in `translations/`
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
git commit -m "feat: augur-sprint-9 Pashto disambiguation + parallel batch + evidence package"
```

Report:
- Which priorities passed
- Test count before (113) and after
- Output of parallel batch benchmark (P2)
- Any deviations from spec

---

_AUGUR Sprint 9 authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_P1 is forensically critical — better Pashto detection._
_P2 makes large evidence runs practical._
_P3 closes the examiner workflow loop._
