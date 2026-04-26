# AUGUR — Super Sprint: Production Hardening + New Capabilities + Full Integration
# This is a comprehensive multi-session sprint. Execute autonomously across all priorities.
# Report after each priority group completes. Do not stop between priorities unless blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Before starting

1. Read CLAUDE.md completely
2. Run `cargo test --workspace 2>&1 | tail -5`
3. Confirm 131 tests passing (default build) before any changes
4. This sprint covers 8 priorities across 4 groups. Execute all.

---

## Hard rules (absolute — no exceptions)

- Zero `.unwrap()` in production code paths
- Zero `unsafe{}` without explicit justification comment
- Zero `println!` in production — use `log::` macros
- All errors handled explicitly — no silent failures
- `cargo clippy --workspace -- -D warnings` clean
- `cargo test --workspace` passes after every priority
- Offline invariant: no content leaves the machine
- MT advisory: every TranslationResult has `is_machine_translation: true`
  and `advisory_notice` non-empty. Always. No exceptions.

---

# GROUP A — ARABIC DIALECT DETECTION

## PRIORITY 1 — Arabic Dialect Awareness

### Context

Arabic has 25+ regional dialects (Egyptian, Levantine, Gulf, Moroccan,
Iraqi, Sudanese, etc.) that differ significantly in vocabulary,
grammar, and pronunciation. Standard NLLB-200 translates Modern
Standard Arabic (MSA) well but may produce poor results on heavy
dialect content.

For LE/IC casework, knowing the dialect matters for geolocation —
Egyptian Arabic suggests a subject from Egypt, Gulf Arabic from
the Gulf states, Moroccan Darija from North Africa.

### Implementation

**Step 1 — Dialect indicators**

Create `crates/augur-classifier/src/arabic_dialect.rs`:

```rust
pub enum ArabicDialect {
    ModernStandard,    // MSA — most formal, used in media
    Egyptian,          // Masri — most widely understood
    Levantine,         // Syrian, Lebanese, Palestinian, Jordanian
    Gulf,              // Saudi, Emirati, Kuwaiti, Qatari, Bahraini
    Iraqi,
    Moroccan,          // Darija — heavy French influence
    Yemeni,
    Sudanese,
    Unknown,           // Can't determine with confidence
}

pub struct DialectAnalysis {
    pub detected_dialect: ArabicDialect,
    pub confidence: f32,
    pub indicator_words: Vec<String>,  // which words triggered detection
    pub advisory: String,              // "Dialect detection is approximate"
}

pub fn detect_arabic_dialect(text: &str) -> DialectAnalysis
```

**Dialect detection approach — lexical markers:**

Each dialect has distinctive vocabulary not found in others.
Use a hardcoded lexicon of the most reliable markers:

```rust
// Egyptian markers
const EGYPTIAN_MARKERS: &[&str] = &[
    "إيه",    // "what" (Masri)
    "كده",    // "like this"
    "مش",     // "not" (Egyptian)
    "عايز",   // "want" (m)
    "عايزة",  // "want" (f)
    "ازيك",   // "how are you"
    "عندك",   // "you have" (Egyptian form)
];

// Gulf markers
const GULF_MARKERS: &[&str] = &[
    "وش",     // "what" (Gulf)
    "زين",    // "good/okay" (Gulf)
    "كيفك",   // "how are you" (Gulf)
    "ابغى",   // "I want" (Saudi)
    "وايد",   // "very/many" (Emirati)
    "شلونك",  // "how are you" (Iraqi/Gulf)
];

// Levantine markers
const LEVANTINE_MARKERS: &[&str] = &[
    "شو",     // "what" (Levantine)
    "كيفك",   // "how are you"
    "هيك",    // "like this"
    "مش",     // "not" (also Egyptian — shared)
    "يلا",    // "let's go/come on"
    "بدي",    // "I want" (Levantine)
];

// Moroccan Darija markers (heavy French loanwords)
const MOROCCAN_MARKERS: &[&str] = &[
    "واش",    // "is it/are you"
    "كيداير", // "how are you" (Darija)
    "بزاف",   // "a lot" (Darija)
    "باغي",   // "I want" (Darija)
];

// Iraqi markers
const IRAQI_MARKERS: &[&str] = &[
    "شلون",   // "how" (Iraqi)
    "اشكو",   // "why" (Iraqi)
    "هواية",  // "a lot" (Iraqi)
    "يبه",    // address term (Iraqi)
];
```

Score each dialect by marker count. Return highest scoring
with confidence proportional to marker count.

**Step 2 — Wire into classifier**

When `classify()` returns `ar` (Arabic):
1. Run `detect_arabic_dialect()` on the input
2. Add to `ClassificationResult`:
   ```rust
   pub dialect: Option<ArabicDialect>,
   pub dialect_confidence: f32,
   pub dialect_note: Option<String>,
   ```

**Step 3 — CLI output**

```
[AUGUR] Language: ar (Arabic) — confidence: HIGH
         Dialect: Egyptian (Masri) — confidence: 0.74
         Indicators: إيه, كده, مش, عايز
         ⚠ Dialect detection is approximate. Verify with a
           human linguist fluent in Arabic dialects if dialect
           origin is material to your case.
```

**Step 4 — Translation advisory update**

When dialect is detected, add to `advisory_notice`:
"Detected dialect: [Egyptian/Gulf/Levantine/etc] Arabic.
NLLB-200 translates Modern Standard Arabic most accurately.
Dialectal content may have reduced translation quality.
Verify critical passages with a certified Arabic translator."

**Step 5 — Tests**

```rust
#[test]
fn egyptian_markers_detected() {
    let text = "إيه ده؟ عايز كده";
    let result = detect_arabic_dialect(text);
    assert!(matches!(result.detected_dialect, ArabicDialect::Egyptian));
}

#[test]
fn gulf_markers_detected() {
    let text = "وش تبي؟ زين وايد";
    let result = detect_arabic_dialect(text);
    assert!(matches!(result.detected_dialect, ArabicDialect::Gulf));
}

#[test]
fn no_markers_returns_unknown() {
    let text = "مرحبا بالعالم"; // generic MSA
    let result = detect_arabic_dialect(text);
    // May return Unknown or low confidence
    assert!(result.confidence < 0.5 || 
            matches!(result.detected_dialect, ArabicDialect::Unknown |
                     ArabicDialect::ModernStandard));
}

#[test]
fn dialect_advisory_present_when_dialect_detected() {
    let text = "إيه ده؟ عايز كده بجد";
    let result = detect_arabic_dialect(text);
    if result.confidence > 0.5 {
        assert!(!result.advisory.is_empty());
    }
}
```

### Acceptance criteria — PRIORITY 1

- [ ] `ArabicDialect` enum with 8+ variants
- [ ] Lexical marker detection for Egyptian, Gulf, Levantine, Moroccan, Iraqi
- [ ] Dialect result in `ClassificationResult`
- [ ] CLI shows dialect when detected
- [ ] Translation advisory updated with dialect warning
- [ ] 4 new tests pass
- [ ] Clippy clean

---

# GROUP B — NEW CAPABILITIES

## PRIORITY 2 — SRT/VTT Subtitle Format Support

### Context

Subtitle files (.srt, .vtt) are common in video evidence —
screen recordings, downloaded content, court hearing transcripts,
surveillance footage. They contain timestamped text that AUGUR
should translate without needing to run Whisper STT.

### Implementation

**Step 1 — Input type extension**

```rust
pub enum PipelineInput {
    Text(String),
    Audio(PathBuf),
    Image(PathBuf),
    Video(PathBuf),
    Pdf(PathBuf),
    Subtitle(PathBuf),    // NEW — .srt or .vtt
}
```

Extend `detect_input_kind` to route `.srt` and `.vtt`.

**Step 2 — SRT parser**

SRT format:
```
1
00:00:01,000 --> 00:00:04,000
First subtitle line
Second line of first subtitle

2
00:00:05,500 --> 00:00:08,000
Second subtitle
```

```rust
pub struct SubtitleEntry {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn parse_srt(content: &str) -> Vec<SubtitleEntry>
pub fn parse_vtt(content: &str) -> Vec<SubtitleEntry>
```

**Step 3 — VTT parser**

WebVTT format:
```
WEBVTT

00:00:01.000 --> 00:00:04.000
First subtitle

00:00:05.500 --> 00:00:08.000
Second subtitle
```

Similar to SRT but with different timestamp format and optional
cue identifiers/settings.

**Step 4 — Pipeline integration**

For subtitle inputs:
1. Parse SRT/VTT into `Vec<SubtitleEntry>`
2. Classify language of concatenated text
3. If foreign: translate each entry individually
4. Return `PipelineResult` with translated entries preserving timestamps

**Step 5 — CLI output**

```bash
augur translate --input subtitles.srt --target en
```

Output:
```
[AUGUR] Input: subtitles.srt (SRT subtitle, 247 entries)
[AUGUR] Language: ar (Arabic) — dialect: Egyptian
[AUGUR] Translating 247 entries...

1 (00:00:01,000 --> 00:00:04,000)
  Arabic: مرحبا بالعالم
  English: Hello world

2 (00:00:05,500 --> 00:00:08,000)
  Arabic: كيف حالك
  English: How are you

⚠ MACHINE TRANSLATION NOTICE...
```

**Step 6 — Translated SRT output**

Add `--output-srt` flag that produces a translated SRT file:
```
1
00:00:01,000 --> 00:00:04,000
Hello world

2
00:00:05,500 --> 00:00:08,000
How are you
```

This allows examiners to view the translated subtitles alongside
the original video in any media player.

**Step 7 — Tests**

```rust
#[test]
fn srt_parser_extracts_entries_correctly() {
    let srt = "1\n00:00:01,000 --> 00:00:04,000\nHello\n\n";
    let entries = parse_srt(srt);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].start_ms, 1000);
    assert_eq!(entries[0].text, "Hello");
}

#[test]
fn vtt_parser_handles_webvtt_header() {
    let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nHello\n";
    let entries = parse_vtt(vtt);
    assert_eq!(entries.len(), 1);
}

#[test]
fn srt_timestamps_parse_to_milliseconds() {
    // "01:23:45,678" → 5025678ms
    assert_eq!(parse_srt_timestamp("01:23:45,678"), 5_025_678);
}

#[test]
fn subtitle_input_detected_by_extension() {
    assert!(matches!(
        detect_input_kind(Path::new("subtitles.srt")),
        PipelineInput::Subtitle(_)
    ));
}
```

### Acceptance criteria — PRIORITY 2

- [ ] SRT format parsed with timestamps
- [ ] VTT format parsed with timestamps
- [ ] Language classification runs on concatenated text
- [ ] Per-entry translation preserving timestamps
- [ ] `--output-srt` produces translated SRT file
- [ ] Batch processing includes .srt and .vtt types
- [ ] MT advisory present on all translated output
- [ ] 4 new tests pass
- [ ] Clippy clean

---

## PRIORITY 3 — YARA Pattern Integration

### Context

YARA is the industry standard for malware and content detection.
Forensic investigators use YARA rules to scan evidence for specific
patterns — extremist content signatures, malware indicators, PII
patterns, cryptocurrency wallet addresses, and more.

Integrating YARA with AUGUR creates a powerful combination:
classify foreign language content → translate → scan with YARA
for content of interest. An examiner can scan Arabic chat logs
for known threat actor signatures in the translated output.

### Implementation

**Step 1 — YARA dependency**

```toml
yara = "0.28"    # Rust bindings to libyara
```

Note: yara crate requires libyara system library:
```bash
brew install yara
```

This is an acceptable system dependency — yara is standard on
forensic workstations.

If yara crate has build issues, implement a subprocess approach:
```bash
yara <rules_file> <input_file>
```
Same pattern as ffmpeg/tesseract.

**Step 2 — YARA engine**

Create `crates/augur-core/src/yara_scan.rs`:

```rust
pub struct YaraEngine {
    rules_path: PathBuf,
    // compiled rules cached after first load
}

pub struct YaraMatch {
    pub rule_name: String,
    pub tags: Vec<String>,
    pub meta: HashMap<String, String>,
    pub matched_strings: Vec<YaraStringMatch>,
}

pub struct YaraStringMatch {
    pub identifier: String,   // the $ variable name in the rule
    pub offset: usize,        // byte offset in scanned content
    pub data: String,         // matched text
}

impl YaraEngine {
    pub fn load(rules_path: &Path) -> Result<Self, AugurError>;

    /// Scan text content (translated or original)
    pub fn scan_text(&self, text: &str) -> Result<Vec<YaraMatch>, AugurError>;

    /// Scan a file
    pub fn scan_file(&self, path: &Path) -> Result<Vec<YaraMatch>, AugurError>;
}
```

**Step 3 — Integration with translation pipeline**

When `--yara-rules <path>` is specified:
1. Translate the content as normal
2. Scan the TRANSLATED text with YARA rules
3. Also scan the ORIGINAL text
4. Include matches in the pipeline result

This enables: "scan Arabic chat logs for English-language threat
actor signatures in the translation."

**Step 4 — CLI integration**

```bash
# Scan a single file
augur translate --input recording.mp3 --target en \
    --yara-rules /path/to/rules.yar

# Batch scan with YARA
augur batch --input /evidence --target en \
    --yara-rules /path/to/rules/

# Output:
[AUGUR] YARA scan: 3 matches found
  Rule: suspicious_url (tags: network, c2)
    Match in translation at offset 142: "evil.com/payload"
  Rule: bitcoin_wallet (tags: financial)
    Match in original (Arabic) at offset 89: "1BvBMSEYstW..."
```

**Step 5 — Built-in starter rules**

Include a small set of built-in YARA rules in
`data/yara_rules/starter.yar`:

```yara
rule bitcoin_wallet_address {
    meta:
        description = "Bitcoin wallet address pattern"
        forensic_value = "High"
    strings:
        $btc = /\b[13][a-km-zA-HJ-NP-Z1-9]{25,34}\b/
    condition:
        $btc
}

rule url_pattern {
    meta:
        description = "URL detected in content"
    strings:
        $url = /https?:\/\/[^\s]{10,}/
    condition:
        $url
}

rule phone_number_intl {
    meta:
        description = "International phone number pattern"
    strings:
        $phone = /\+[1-9]\d{6,14}/
    condition:
        $phone
}
```

**Step 6 — Tests**

```rust
#[test]
fn yara_scan_finds_url_in_text() {
    // Text with https://evil.com → url_pattern rule matches
}

#[test]
fn yara_scan_finds_bitcoin_address() {
    // Text with valid BTC address → bitcoin_wallet_address matches
}

#[test]
fn yara_no_rules_path_returns_clear_error() {
    // Path to non-existent rules file → AugurError, not panic
}

#[test]
fn yara_match_includes_offset_and_data() {
    // Match result → offset > 0, data non-empty
}
```

### Acceptance criteria — PRIORITY 3

- [ ] YARA engine loads rules from file or directory
- [ ] Text scanning works (translated and original)
- [ ] File scanning works
- [ ] `--yara-rules` flag on translate and batch commands
- [ ] Built-in starter rules included
- [ ] YARA matches in batch JSON report
- [ ] 4 new tests pass
- [ ] Clippy clean
- [ ] Offline invariant maintained (YARA is fully local)

---

# GROUP C — PRODUCTION HARDENING

## PRIORITY 4 — Error Recovery and Resilience

### Context

In production forensic use, AUGUR must handle corrupt files,
unexpected formats, very large files, and edge cases without
crashing. This priority hardens every pipeline entry point.

### Implementation

**Step 1 — File size limits**

Add configurable limits to prevent OOM on large files:

```rust
pub struct PipelineOptions {
    pub max_file_size_bytes: u64,   // default: 500MB for audio/video
    pub max_text_bytes: usize,      // default: 10MB for text
    pub max_pdf_pages: u32,         // default: 500 pages
    pub max_batch_files: usize,     // default: 10,000
    pub timeout_seconds: u64,       // default: 300 per file
}
```

When a file exceeds limits:
- Return `AugurError::FileTooLarge { size, limit }`
- Batch processing: capture error, continue with next file
- CLI: display warning, skip file

**Step 2 — Corrupt file handling**

Every file type parser must handle corruption gracefully:

```rust
// Audit all parsers for unwrap() on file reads
// Replace with proper error propagation
// Add corruption-specific error variants

pub enum AugurError {
    // existing...
    CorruptFile { path: String, reason: String },
    UnsupportedEncoding { detected: String },
    FileTooLarge { size_bytes: u64, limit_bytes: u64 },
    ProcessTimeout { seconds: u64 },
}
```

**Step 3 — Timeout enforcement**

Wrap STT and translation calls with timeout:

```rust
use std::time::{Duration, Instant};

fn with_timeout<F, T>(
    timeout: Duration,
    f: F,
) -> Result<T, AugurError>
where F: FnOnce() -> Result<T, AugurError>
{
    // Use thread timeout or tokio timeout
    // Return AugurError::ProcessTimeout if exceeded
}
```

**Step 4 — Retry logic for subprocess calls**

NLLB-200 and Whisper subprocess calls can fail transiently.
Add retry with backoff:

```rust
pub fn with_retry<F, T>(
    max_attempts: u32,
    f: F,
) -> Result<T, AugurError>
where F: Fn() -> Result<T, AugurError>
{
    let mut last_err = None;
    for attempt in 0..max_attempts {
        match f() {
            Ok(result) => return Ok(result),
            Err(e) => {
                log::warn!("Attempt {} failed: {}", attempt + 1, e);
                last_err = Some(e);
                if attempt < max_attempts - 1 {
                    std::thread::sleep(
                        Duration::from_millis(500 * (attempt as u64 + 1))
                    );
                }
            }
        }
    }
    Err(last_err.unwrap()) // safe: max_attempts > 0
}
```

**Step 5 — Memory-safe large file handling**

For large audio files, don't load entire file into memory.
Stream through ffmpeg preprocessing in chunks if possible.
For large text files, process in 10MB windows with overlap.

**Step 6 — Tests**

```rust
#[test]
fn file_too_large_returns_error_not_panic() {
    // Create fake file size metadata exceeding limit
    // Verify FileTooLarge error returned
}

#[test]
fn corrupt_pdf_returns_error_not_panic() {
    // Write random bytes to a .pdf file
    // Verify CorruptFile error, not panic
}

#[test]
fn empty_file_handled_gracefully() {
    // Zero-byte audio file → clear error, not panic
}

#[test]
fn retry_succeeds_on_third_attempt() {
    // Mock function that fails twice then succeeds
    // Verify with_retry returns success
}
```

### Acceptance criteria — PRIORITY 4

- [ ] File size limits configurable with sensible defaults
- [ ] `FileTooLarge` error variant with size info
- [ ] `CorruptFile` error variant
- [ ] Retry with backoff on subprocess calls
- [ ] Timeout enforcement on STT/translation
- [ ] Empty file handled without panic
- [ ] 4 new tests pass
- [ ] Clippy clean

---

## PRIORITY 5 — Performance Benchmarking Suite

### Context

Sprint 9 added parallel batch processing with a manual benchmark.
This priority adds a formal benchmark suite that runs automatically
and tracks performance regressions.

### Implementation

**Step 1 — Benchmark fixtures**

Create `tests/benchmarks/`:
```
tests/benchmarks/
├── arabic_short.txt      ← 50 words Arabic
├── arabic_medium.txt     ← 500 words Arabic
├── arabic_long.txt       ← 2000 words Arabic
├── mixed_languages.txt   ← Arabic + English mixed
├── pashto_sample.txt     ← Pashto text with dialect markers
└── README.md             ← explains fixtures and expected results
```

**Step 2 — Benchmark binary**

Create `apps/augur-cli/src/benchmark.rs`:

```rust
pub struct BenchmarkResult {
    pub test_name: String,
    pub duration_ms: u64,
    pub chars_per_second: f64,
    pub words_per_second: f64,
    pub passed: bool,
    pub notes: String,
}

pub fn run_benchmarks(options: &BenchmarkOptions) -> Vec<BenchmarkResult>
```

**Step 3 — CLI command**

```bash
augur benchmark [--full] [--output results.json]
```

Output:
```
[AUGUR] Running benchmark suite...

Classification benchmarks:
  arabic_short.txt    (50 words):   2ms   ✓ (< 10ms threshold)
  arabic_medium.txt  (500 words):   8ms   ✓ (< 50ms threshold)
  arabic_long.txt   (2000 words):  31ms   ✓ (< 200ms threshold)

Translation benchmarks (requires models):
  arabic_short → en:   4.2s   ✓
  arabic_medium → en: 19.1s   ✓

Parallel batch benchmark:
  20 files sequential:   9.1s
  20 files parallel(4):  2.4s
  Speedup: 3.8x          ✓ (> 2x threshold)

All benchmarks passed.
Results saved: results.json
```

**Step 4 — Regression detection**

If `--compare previous_results.json` is provided:
```bash
augur benchmark --compare last_run.json
```

Report any benchmark that is >20% slower than previous run
as a regression warning.

**Step 5 — Tests**

```rust
#[test]
fn benchmark_classification_completes_under_threshold() {
    // 50-word Arabic text
    // Verify classification < 50ms
}

#[test]
fn benchmark_result_serializes_to_json() {
    // BenchmarkResult → valid JSON
}
```

### Acceptance criteria — PRIORITY 5

- [ ] Benchmark fixtures committed to tests/benchmarks/
- [ ] `augur benchmark` command runs classification benchmarks
- [ ] Translation benchmarks run with `--full` flag
- [ ] JSON output format
- [ ] Regression detection against previous run
- [ ] 2 new tests pass
- [ ] Clippy clean

---

# GROUP D — FULL INTEGRATION

## PRIORITY 6 — Strata Plugin Live Integration Test

### Context

Sprint 8 vendored the Strata plugin SDK and implemented the trait.
This priority verifies the integration actually works end-to-end
by running AUGUR as a Strata plugin against real evidence.

### Implementation

**Step 1 — Integration test with real evidence**

Add to `tests/` (gated on `AUGUR_RUN_INTEGRATION_TESTS=1`):

```rust
#[test]
#[ignore]
fn strata_plugin_processes_real_arabic_evidence() {
    // Create a temp dir with an Arabic text file
    // Build a mock PluginContext pointing at it
    // Run AugurStrataPlugin::execute()
    // Verify: at least one artifact returned
    // Verify: all artifacts have is_advisory = true
    // Verify: all advisory_notice non-empty
    // Verify: confidence == Medium
}
```

**Step 2 — Plugin metadata validation**

```rust
#[test]
#[cfg(feature = "strata")]
fn strata_plugin_metadata_complete() {
    let plugin = AugurStrataPlugin::new("en");
    assert!(!plugin.name().is_empty());
    assert!(!plugin.version().is_empty());
    assert!(!plugin.description().is_empty());
    assert_eq!(plugin.category(), "Analyzer");
}
```

**Step 3 — Document the wiring**

Update `docs/STRATA_INTEGRATION.md`:

```markdown
# AUGUR — Strata Plugin Integration

## Adding AUGUR to Strata

1. Add to Strata's Cargo.toml:
   ```toml
   augur-plugin-sdk = { path = "../verify/crates/augur-plugin-sdk",
                         features = ["strata"] }
   ```

2. Add to PLUGIN_NAMES in Strata's lib.rs:
   ```rust
   "AUGUR",
   ```

3. Add to PLUGIN_DATA in Strata's types/index.ts:
   ```typescript
   {
     name: "AUGUR",
     version: "1.0.0",
     category: "Analyzer",
     description: "Foreign language detection and translation"
   }
   ```

4. Wire in run_plugin:
   ```rust
   "AUGUR" => {
     let plugin = AugurStrataPlugin::new("en");
     plugin.execute(context)
   }
   ```

## What AUGUR adds to Strata

AUGUR surfaces foreign-language content as Communications
artifacts in Strata's UI. Every translation artifact:
- Category: Communications / Foreign Language
- Confidence: Medium (MT output)
- is_advisory: true
- advisory_notice: "[MT — review by certified translator] ..."
```

### Acceptance criteria — PRIORITY 6

- [ ] Integration test written (gated, runs with real evidence)
- [ ] Plugin metadata validation test passes
- [ ] `docs/STRATA_INTEGRATION.md` written and complete
- [ ] `cargo build --features augur-plugin-sdk/strata` still passes
- [ ] 2 new tests pass
- [ ] Clippy clean under both configs

---

## PRIORITY 7 — Multi-Format Input Detection Hardening

### Context

The current `detect_input_kind` function uses file extension only.
Real forensic evidence often has wrong or missing extensions.
Content-based detection should be attempted when extension is
unknown or ambiguous.

### Implementation

**Step 1 — Magic byte detection**

```rust
pub fn detect_input_kind_robust(path: &Path) -> PipelineInput {
    // First: try extension
    let by_extension = detect_input_kind(path);

    // If extension is unknown/ambiguous, probe magic bytes
    if matches!(by_extension, PipelineInput::Audio(_)) {
        // Verify it's actually audio by checking magic bytes
        if let Ok(magic) = read_magic_bytes(path, 12) {
            if is_pdf_magic(&magic) {
                return PipelineInput::Pdf(path.to_path_buf());
            }
            if is_mp4_magic(&magic) || is_mov_magic(&magic) {
                return PipelineInput::Video(path.to_path_buf());
            }
        }
    }
    by_extension
}

fn read_magic_bytes(path: &Path, n: usize) -> Result<Vec<u8>, AugurError>;

fn is_pdf_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

fn is_mp4_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[4..8] == b"ftyp"
}

fn is_wav_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WAVE"
}
```

**Common magic bytes to detect:**
- PDF: `%PDF`
- MP4/MOV: bytes 4-8 = `ftyp`
- WAV: `RIFF....WAVE`
- MP3: `ID3` or `0xFF 0xFB`
- JPEG: `0xFF 0xD8 0xFF`
- PNG: `0x89 PNG`
- ZIP: `PK\x03\x04` (also covers DOCX, XLSX)
- GZIP: `\x1F\x8B`

**Step 2 — No-extension file handling**

Files with no extension (common in Unix/Linux evidence) should
be classified by content:

```bash
augur translate --input suspicious_file --target en
```

Output:
```
[AUGUR] No extension detected — probing file content...
[AUGUR] Detected: PDF document (magic: %PDF)
[AUGUR] Processing as PDF...
```

**Step 3 — Tests**

```rust
#[test]
fn pdf_with_wrong_extension_detected_correctly() {
    // Write PDF content to a .mp3 file
    // Verify detect_input_kind_robust returns Pdf
}

#[test]
fn wav_magic_detected() {
    // WAV magic bytes → is_wav_magic returns true
}

#[test]
fn unknown_magic_falls_through_gracefully() {
    // Random bytes → no crash, returns fallback input type
}
```

### Acceptance criteria — PRIORITY 7

- [ ] Magic byte detection for PDF, MP4, WAV, MP3, JPEG, PNG, ZIP
- [ ] No-extension files handled by content probing
- [ ] Wrong-extension files corrected by magic bytes
- [ ] `detect_input_kind_robust` used as the primary detector
- [ ] 3 new tests pass
- [ ] Clippy clean

---

## PRIORITY 8 — Comprehensive Documentation and Help

### Context

AUGUR is production-ready but its documentation is scattered
across CLAUDE.md, sprint logs, and README stubs. This priority
creates examiner-facing documentation that enables deployment
without developer assistance.

### Implementation

**Step 1 — User manual**

Create `docs/USER_MANUAL.md`:

```markdown
# AUGUR User Manual — v1.0

## For Law Enforcement and Intelligence Analysts

AUGUR is an offline-first foreign language evidence processing tool.
No evidence ever leaves your machine.

## Quick Start

### First-time setup
```bash
augur self-test          # check what's installed
augur self-test --full   # test full pipeline (downloads models)
```

### Classify a file
```bash
augur classify --text "مرحبا بالعالم" --target en
augur classify --file evidence.txt --target en
```

### Translate a file
```bash
augur translate --input recording.mp3 --target en
augur translate --input document.pdf --target en
augur translate --input interview.mp4 --target en --diarize
```

### Process a directory
```bash
augur batch --input /evidence/phone_dump --target en --output report.json
augur batch --input /evidence --target en --format html --output report.html
```

### Package results for sharing
```bash
augur package --input /evidence --output case-001.zip
```

## Model Setup

AUGUR downloads models on first use...
[complete model setup instructions]

## Air-Gap Installation

For classified environments without internet access...
[complete air-gap instructions referencing AIRGAP_INSTALL.md]

## Language Support

[table of all 200 NLLB languages with ISO codes]

## Known Limitations

[reference to LANGUAGE_LIMITATIONS.md]

## Machine Translation Advisory

[full advisory text and legal implications]
```

**Step 2 — Quick reference card**

Create `docs/QUICK_REFERENCE.md`:
A one-page cheat sheet with the most common commands,
model download paths, and the MT advisory text.

**Step 3 — Deployment guide**

Create `docs/DEPLOYMENT.md`:
How to deploy AUGUR on a forensic workstation, including
model pre-download, air-gap setup, and integration with
case management systems.

**Step 4 — Update README**

Rewrite `README.md` to be examiner-facing, not developer-facing:
- What AUGUR does (in plain language)
- Quick install
- Three example commands
- Link to full docs

**Step 5 — `augur --docs` command**

```bash
augur --docs           # opens USER_MANUAL.md in $PAGER or prints it
augur --docs geoip     # shows geoip-specific help
augur --docs languages # shows language support table
```

### Acceptance criteria — PRIORITY 8

- [ ] `docs/USER_MANUAL.md` written (complete, examiner-focused)
- [ ] `docs/QUICK_REFERENCE.md` written (one page)
- [ ] `docs/DEPLOYMENT.md` written
- [ ] README.md rewritten for examiners
- [ ] `augur --docs` command works
- [ ] No new tests required (documentation sprint)
- [ ] All doc files committed

---

## After all priorities complete

```bash
cargo test --workspace 2>&1 | grep "test result" | tail -5
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo build --features augur-plugin-sdk/strata 2>&1 | tail -3
```

Commit:
```bash
git add -A
git commit -m "feat: augur-super-sprint arabic dialect + SRT/VTT + YARA + hardening + docs"
```

Report full results covering all 8 priorities:
- Which passed, which skipped
- Test count before (131) and after
- YARA build status (libyara available or subprocess fallback)
- `augur self-test` output after all changes
- Any deviations from spec

---

## What this super sprint does NOT touch

- Core STT/translation model weights (already optimized)
- Archon integration (separate concern)
- Strata source code (separate repo)
- Any Chinese-origin AI models (hard rule, always)

---

_AUGUR Super Sprint authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_8 priorities across 4 groups. Execute all._
_Report after each group completes._
_This is a comprehensive production-readiness sprint._
_After this sprint, AUGUR is ready for field deployment._
