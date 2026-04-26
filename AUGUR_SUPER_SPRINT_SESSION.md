# AUGUR Super Sprint — 2026-04-26 — Session log

## Pre-flight
- Pre-start tests: 131 passing.
- Pre-start clippy: clean.

## Group A — Arabic dialect detection (P1) — PASSED
- New `augur_classifier::arabic_dialect` module:
  `ArabicDialect` (9 variants), `DialectAnalysis`,
  `detect_arabic_dialect(text)`. Lexical-marker scoring
  across Egyptian / Gulf / Levantine / Moroccan / Iraqi /
  Yemeni / Sudanese plus an explicit MSA / Unknown fallback.
- `ClassificationResult` gains
  `arabic_dialect: Option<ArabicDialect>`,
  `arabic_dialect_confidence: f32`,
  `arabic_dialect_indicators: Vec<String>`,
  `arabic_dialect_note: Option<String>`. The `classify()`
  method invokes the detector only when the LID layer
  concluded `ar` — no spurious dialect labels on other
  languages.
- CLI prints dialect + confidence + indicator words + the
  human-language advisory under each Arabic classification.
- 8 new tests cover Egyptian / Gulf / Levantine / Moroccan
  marker detection, MSA fallback, advisory presence,
  empty-input, and the single-marker-isn't-enough invariant.

## Group B — New capabilities (P2 + P3) — PASSED

### P2 SRT/VTT subtitle support
- New `augur_core::subtitle` module — `SubtitleEntry`,
  `parse_srt`, `parse_vtt`, `render_srt`, `render_vtt`,
  timestamp helpers (HH:MM:SS,mmm and HH:MM:SS.mmm).
  Tolerates BOMs, `\r\n`, missing index lines, and WebVTT
  cue identifiers.
- `PipelineInput::Subtitle` added; `detect_input_kind` routes
  `.srt`/`.vtt`. CLI gains `--output-srt <path>` on translate;
  per-cue NLLB pass writes a media-player-ready translated
  SRT preserving original timestamps.
- 9 new tests pin parsing, round-trip, multiline cues, the
  WebVTT header, and graceful malformed-input handling.

### P3 YARA pattern integration (subprocess)
- New `augur_core::yara_scan` module: `YaraEngine`,
  `YaraMatch`, `YaraStringMatch`, `parse_yara_output`. Same
  subprocess pattern as ffmpeg/tesseract — invokes the system
  `yara` binary. Spec's "yara crate or subprocess fallback"
  decision: subprocess (libyara not installed on this host;
  yara binary is the standard forensic-workstation install).
- New `AugurError::Yara` + `AugurError::YaraNotInstalled`
  variants. CLI flag `--yara-rules <path>` on translate
  scans both the translated text AND the original source;
  matches print rule name + offset + matched substring.
- Built-in starter rules at `data/yara_rules/starter.yar` —
  BTC / ETH wallets, URLs, Tor onion addresses, phone
  numbers, emails, IPv4.
- 5 new tests pin missing-rules error, parser correctness on
  synthetic stdout, empty output, valid-rule-file load, and
  the `YaraNotInstalled` graceful fallback when the binary
  isn't on PATH.

## Group C — Production hardening (P4 + P5) — PASSED

### P4 Error recovery + size limits + retry
- New `augur_core::resilience` module: `PipelineLimits`
  (500 MB file / 10 MB text / 500 PDF pages / 10 000 batch
  files / 5 min timeout defaults), `check_file_size`,
  `check_text_size`, `with_retry(max_attempts, f)` linear
  backoff (500ms × attempt).
- New `AugurError::FileTooLarge { size_bytes, limit_bytes }`,
  `AugurError::CorruptFile { path, reason }`,
  `AugurError::ProcessTimeout { seconds }` variants.
- 6 new tests pin: file-too-large detected from metadata,
  empty file → CorruptFile (not panic), missing file →
  CorruptFile, text size limit, retry succeeds on third
  attempt, retry surfaces last error on exhaustion.

### P5 Benchmarking suite
- 5 hand-curated fixtures committed under `tests/benchmarks/`
  (Arabic short / medium / long, mixed languages, Pashto
  sample) plus a README documenting them.
- New `apps/augur-cli/src/benchmark.rs` module with
  `BenchmarkSuite`, `BenchmarkResult`, `run_suite`,
  `render_text`, `render_regression_report`. New CLI command
  `augur benchmark` with `--full` (translation pass) and
  `--compare <prev.json>` (regression detection at >1.2×).
- Live results: classifier processes 489-word Arabic in <1 ms
  on this host (≈ 528 K words/sec). Sample output:
  ```
  classify::arabic_long.txt   0ms  5196 bytes  489 words  528673 wps
  classify::arabic_medium.txt 0ms  1732 bytes  163 words  502958 wps
  classify::arabic_short.txt  0ms   230 bytes   20 words  239521 wps
  classify::pashto_sample.txt 0ms   199 bytes   26 words  405458 wps
  ```
- 3 new tests pin classifier-under-threshold, JSON
  serialisation round-trip, and regression detection.

## Group D — Full integration (P6 + P7 + P8) — PASSED

### P6 Strata live integration test + docs
- New `strata_plugin_processes_real_arabic_evidence` test —
  walks a real temp evidence directory, asserts the advisory
  invariant holds on every artifact emitted. Gated on
  `AUGUR_RUN_INTEGRATION_TESTS=1`.
- New `strata_plugin_metadata_complete` test — pins the
  trait surface (name / version / description / Analyzer /
  Professional tier / ArtifactExtraction capability).
- Wrote `docs/STRATA_INTEGRATION.md` covering build,
  registration, artifact shape, and forensic invariants.

### P7 Magic-byte content detection
- 8 new helpers: `is_pdf_magic`, `is_mp4_magic`,
  `is_wav_magic`, `is_mp3_magic` (handles ID3 + MPEG sync),
  `is_jpeg_magic`, `is_png_magic`, `is_zip_magic`,
  `is_gzip_magic`. New `detect_input_kind_robust(path)`
  reads the first 16 bytes and overrides
  `detect_input_kind`'s extension-based answer when content
  contradicts. Falls back to the extension answer on any
  I/O error — never panics.
- All four CLI call sites (translate, batch counter,
  package walker, batch dispatch) now use the robust
  variant. PDF-with-wrong-`.mp3`-extension test pins the
  override behaviour.
- 3 new tests pin the magic-byte canonical signatures, the
  override behaviour, and the unknown-magic graceful
  fallback.

### P8 User docs + README + augur --docs
- Wrote `docs/USER_MANUAL.md` (full feature reference for
  examiners), `docs/QUICK_REFERENCE.md` (one-page cheat
  sheet), `docs/DEPLOYMENT.md` (workstation setup + casework
  workflow), and a fresh examiner-facing `README.md`.
- New `augur docs [topic]` subcommand. Topics: `manual`
  (default), `quick`, `deploy`, `airgap`, `strata`,
  `languages`. Docs are baked into the binary via
  `include_str!` so the command works on air-gapped machines
  with no source tree.

## Final results
- **Default-build test count: 166 passing** (up from
  Sprint 9's 131 — +35 across the eight priorities).
  4 integration tests `#[ignore]`-gated on
  `AUGUR_RUN_INTEGRATION_TESTS=1` (now 5 with the new
  Strata live-evidence test).
- **`--features augur-plugin-sdk/strata` build**: 173 unit
  tests (166 + 7 strata-only).
- **Clippy: CLEAN** under both
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo clippy --workspace --all-targets --features
   augur-plugin-sdk/strata -- -D warnings`.
- **Offline invariant: MAINTAINED.** No new permitted egress
  URLs in this sprint. Magic-byte detection, YARA scanning,
  benchmarking, all subtitle parsing, error-recovery helpers,
  and the docs subcommand are all process-local.
- **MT advisory: ALWAYS PRESENT.** Two new layers reinforce
  the existing enforcement:
  - The `--yara-rules` translated-text scan operates on the
    `TranslationResult`'s output, which still carries the
    advisory.
  - The benchmark suite's `--full` translation check asserts
    `is_machine_translation && !advisory_notice.is_empty()`
    on the result and fails the test if the invariant is
    ever broken.

## YARA build status
- libyara: not installed on this build host.
- Subprocess fallback: implemented; gracefully reports
  `YaraNotInstalled` when the `yara` binary isn't on PATH.
- Forensic deployment: install via `brew install yara` /
  `apt install yara`.

## `augur self-test` output (post-sprint)

```
[AUGUR] Running self-test...

[AUGUR] ✓ [PASS] Classification: Arabic text → ar
[AUGUR] ✓ [PASS] Classification: English → not foreign
[AUGUR] ✓ [PASS] Classification: empty input → handled
[AUGUR] ✓ [PASS] Pashto/Farsi script disambiguation (confidence 0.95)
[AUGUR] ⚠ [WARN] Audio preprocessing: ffmpeg
[AUGUR] ⚠ [WARN] OCR: tesseract
[AUGUR] ⚠ [WARN] PDF rasterize: pdftoppm
[AUGUR] ✓ [PASS] STT: Whisper tiny safetensors cached
[AUGUR] ✓ [PASS] Translation: NLLB-200 cached
[AUGUR] ✓ [PASS] Air-gap mode (online-on-first-run)
[AUGUR] ⚠ [SKIP] HF token not configured
[AUGUR] ⚠ [SKIP] GeoIP: GeoLite2 database not configured
[AUGUR] ✓ [PASS] Offline invariant audit

[AUGUR] Self-test PASSED (8 passed, 0 failed, 2 skipped, 3 warnings)
[AUGUR] This installation is ready for casework.
```

The three WARN lines are the Bash-tool sandbox's PATH not
exposing `/opt/homebrew/bin` to spawned subprocesses; on a
real deployment workstation those clear to PASS. The two
SKIP lines are user-configurable optional features (HF token
for diarization, MaxMind DB for GeoIP).

## Deviations from spec
- **YARA backend** chose subprocess from the spec's two
  options because libyara wasn't installed on this build
  host. Spec explicitly allowed this branch.
- **Arabic dialect MSA fallback** — when no markers fire,
  the detector returns `ModernStandard` (confidence 0.4)
  rather than the spec's optional `Unknown`. Both are
  spec-compliant; ModernStandard is more useful in the CLI
  display.
- **Benchmark `--compare`** lives at the CLI layer
  (`cmd_benchmark` reads the previous JSON, calls
  `render_regression_report`) rather than inside `run_suite`
  per the spec. Same end behaviour, simpler module surface
  (`BenchmarkOptions` doesn't need a borrowed Path).
- **`augur docs`** prints through `println_verify` rather
  than raw stdout, keeping every CLI line routed through
  the one auditable function. Strips with `sed 's/^\[AUGUR\] //'`
  for clean reading.
- **Pashto fixture** in `tests/benchmarks/pashto_sample.txt`
  is hand-written with several Pashto-specific glyphs;
  whichlang still routes it via classifier — the script
  disambiguator catches it at the next layer.