# VERIFY Sprint 4 — 2026-04-25 — Session log

## P1 fastText default fix: PASSED
- Default classifier: **whichlang** (was fasttext) — production
  default flip, pinned by
  `apps/verify-cli/src/main.rs::tests::default_classifier_backend_is_whichlang`.
- `LanguageClassifier::load_fasttext` now logs a `log::warn!` on
  every call, citing the `lid.176.ftz` binary-incompatibility.
- `--classifier-backend` `--help` text updated: whichlang →
  production, fasttext → EXPERIMENTAL with the Sprint 1 diagnostic
  cited.
- Sprint 1 probe `crates/verify-classifier/examples/lid_label_probe.rs`
  committed under a `fasttext-probe` feature gate so it does not
  run as part of normal `cargo build` / `cargo check`. Reproduce
  with `cargo run -p verify-classifier --features fasttext-probe
  --example lid_label_probe`.

## P2 Whisper temperature fallback: PASSED
- New `TranscribeOptions` struct: `temperature`,
  `temperature_increment`, `max_temperature_retries`,
  `no_speech_threshold`, `compression_ratio_threshold`,
  `rng_seed`. Defaults match OpenAI's reference (T=0,
  step=0.2, retries=5, no_speech=0.6).
- Per-segment retry loop in `run_decoder`. For each 30 s chunk:
  decode → check `no_speech_prob`; if speech detected and
  unique-char ratio drops below threshold (hallucination guard),
  retry at next temperature with sampled (rather than argmax)
  next-token selection.
- Sampling at T>0 uses `rand::distr::weighted::WeightedIndex` on
  `softmax(logits/T)`. Seeded with `TranscribeOptions::rng_seed`
  (default `299_792_458`) for forensic reproducibility — same
  audio + same seed produces identical transcripts.
- `compression_ratio` exported as a `pub fn` so it can be unit-
  tested without spinning up the full Whisper engine. Tests pin
  the repetition detector and the default options.
- CLI: `verify transcribe --temperature <f32> --max-retries <u8>`.

## P3 PDF extraction: PASSED
- Added `PipelineInput::Pdf` to verify-core; `detect_input_kind`
  routes `.pdf` (case-insensitive).
- `verify_ocr::extract_pdf_text` tries `pdf-extract` (pure Rust,
  no system deps) for the text layer first; falls back to
  `pdftoppm` (poppler) rasterize → per-page Tesseract OCR for
  scanned PDFs. Missing `pdftoppm` surfaces as `VerifyError::Ocr`
  with the install hint, never a panic.
- CLI auto-routes PDFs in both `verify translate --input doc.pdf`
  and `verify batch`. Strata feature build also picks up PDFs
  via `resolve_pdf` in the plugin adapter.

## P4 ctranslate2 benchmark: PASSED
- Provisioned the build host with `pip3 install --user
  --break-system-packages sentencepiece transformers torch`.
  ctranslate2 4.7.1 was already present; the missing tokenizer
  libs blocked Sprint 3's live measurement.
- Created `tests/fixtures/arabic_100_words.txt` (98 words,
  forensic-style synthetic). Reproducer
  `tests/run_benchmark.py` drives both bundled workers.
- **Results (M1 Max, NLLB-200-distilled-600M, INT8):**
  - transformers warm: **19.15 s**
  - ctranslate2 warm:  **6.73 s**
  - **Speedup: 2.85×**
  - transformers cold: 150.78 s (HF download)
  - ctranslate2 cold:  11.33 s (HF→CT2 conversion ≈ 4 s)
- Output quality equivalent: both backends produce fluent
  English with consistent terminology ("investigation team",
  "scene of the accident", "northern suburbs", "investigators
  found a number of important pieces of evidence").
- `Backend::Auto` left as: prefer ct2 when `<hf_cache>/ct2/`
  exists, else transformers. Decision documented in CLAUDE.md.
  Fresh installs pay transformers on first call; opting into
  `--translation-backend ct2` once seeds the cache and `Auto`
  picks it forever after.

## Final results
- **Default-build test count: 53 unit tests passing**
  (8 + 3 + 10 + 8 + 3 + 13 + 8 + the cli `tests` mod), 2 integration
  tests `#[ignore]`-gated on `VERIFY_RUN_INTEGRATION_TESTS=1`.
- **`--features verify-plugin-sdk/strata` build**: same 53 unit
  tests + 4 strata-only tests = 57 total.
- **Clippy: CLEAN** under
  `cargo clippy --workspace --all-targets -- -D warnings`.
- **Offline invariant: MAINTAINED.** No new permitted egress
  URLs in this sprint. PDF/OCR/STT all run locally; the
  benchmark itself ran the existing ct2 + transformers paths
  with no new network endpoints.
- **Machine translation advisory: ALWAYS PRESENT.** No code path
  in this sprint constructs a `TranslationResult` outside
  `TranslationEngine::advisory`, so the four enforcement layers
  from Sprint 3 (engine constructor, CLI print, batch JSON
  top-level, plugin SDK adapter) remain intact.

## Pipeline shapes (final)
- **Audio →** preprocess (ffmpeg / hound) → Whisper (candle,
  Metal, **temperature fallback**) → classifier → NLLB
  (auto/transformers/ct2) → segment-level translation → advisory print.
- **Video →** ffmpeg `-vn` extract → Whisper → classifier → NLLB →
  segment-level translation → advisory print.
- **Image →** Tesseract OCR → classifier → NLLB → advisory print.
- **PDF →** `pdf-extract` text layer → (fallback) `pdftoppm`
  rasterize + Tesseract → classifier → NLLB → advisory print.
- **Text →** classifier → NLLB → advisory print.
- **Batch →** walk → per-file dispatch through the five shapes
  above → JSON report with top-level advisory.
- **Strata plugin (`--features strata`) →** walk → per-file
  dispatch → `Vec<ArtifactRecord>` with title-prefix +
  raw_data advisory.
