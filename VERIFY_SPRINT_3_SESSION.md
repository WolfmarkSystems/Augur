# VERIFY Sprint 3 — 2026-04-25 — Session log

## P1 Video pipeline: PASSED
- Video formats detected: yes (mp4/mov/avi/mkv/m4v/wmv/webm/3gp)
  via `verify_core::pipeline::detect_input_kind`.
- Audio extraction: yes — `verify_stt::extract_audio_from_video`
  shells out to `ffmpeg -vn -ar 16000 -ac 1 -f wav -sample_fmt
  s16` into a per-process scratch path under
  `<tmp>/verify/video-scratch/`. Missing video / missing ffmpeg
  surface as structured `VerifyError::InvalidInput` and
  `VerifyError::Stt` respectively (no panics).
- Timestamped translation segments: yes —
  `TranslationEngine::translate_segments` translates each STT
  segment independently and pins `[start_ms, end_ms,
  source_text, translated_text]` tuples in
  `TranslationResult::segments`. CLI `--input video.mp4` prints
  both source and translated transcripts side-by-side.

## P2 ctranslate2: PASSED (with fallback; live bench skipped)
- Available on build host: ctranslate2 4.7.1 importable;
  sentencepiece + transformers were not installed, so live
  inference + benchmark cannot run on this host.
- Bundled second worker script
  (`crates/verify-translate/src/script_ct2.py`) handles both the
  one-time HF→CTranslate2 conversion (int8 quantization) and
  inference. Conversion target: `<hf_cache>/ct2/`.
- Backend selection: `Backend::Auto` (default) prefers ct2 when
  the converted model directory exists; falls back to transformers
  otherwise. `Backend::Ctranslate2` forces ct2 (and triggers
  conversion on first use); `Backend::Transformers` forces the
  Sprint 2 path. Auto-mode failures during ct2 inference fall
  back to transformers with a warning.
- CLI flag: `--translation-backend auto|transformers|ct2`
  global on every subcommand.
- Speed improvement: not measured on this host. The spec cites
  3–5× CPU speedup; pinned to revisit on a properly-provisioned
  forensic workstation.
- Fallback test: `backend_auto_falls_back_to_transformers_when_ct2_dir_absent`
  pins the dispatch logic.

## P3 Batch processing: PASSED
- Directory walk: yes — recursive `std::fs::read_dir`; symlinks
  intentionally not followed (forensic discipline). Files are
  sorted before processing for deterministic output.
- JSON output: yes —
  `cargo run -- batch --input dir/ --target en --output rpt.json`
  emits a top-level `BatchResult` with `machine_translation_notice`,
  `generated_at` (ISO 8601 UTC), `total_files`, `processed`,
  `foreign_language`, `translated`, `errors`, and a per-file
  `results` array. The advisory at the top level is enforced by
  `BatchResult::assert_advisory` and pinned by
  `batch_result_advisory_required_when_translations_present`.
- Error handling per file: yes — `process_one_file` returns
  `Result<BatchFileResult, VerifyError>`; failures are captured
  into the report's per-file `error` field so a corrupted MP3 in
  a 1 000-file evidence drop does not abort the run.
- `--types audio,video,image` filter implemented.
- Progress messages printed per file: `[N/Total] kind: path`.

## P4 Strata plugin: PASSED (feature-gated)
- Vendoring approach: rejected. Vendoring
  `strata-plugin-sdk` would also drag in `strata-fs`
  (NTFS / APFS / ext4 / EWF parsers + memmap2 + sysinfo + …) —
  a hard-rule violation ("no unnecessary dependencies") for a
  translation tool. Resolution documented inline in CLAUDE.md
  under "Sprint 3 decisions".
- Real trait impl: yes — `crates/verify-plugin-sdk/src/strata_impl.rs`
  contains `impl StrataPlugin for VerifyStrataPlugin` with
  `name`, `version`, `supported_inputs`, `plugin_type` (Analyzer),
  `capabilities` (ArtifactExtraction), `description`,
  `required_tier` (Professional), `run`, and `execute`. The
  `execute` path returns rich `ArtifactRecord`s and asserts the
  advisory invariant before returning.
- Build: `cargo build -p verify-plugin-sdk --features strata` —
  PASSED on this host (Strata workspace at
  `~/Wolfmark/strata/crates/strata-plugin-sdk` resolved as a
  sibling-workspace path dep).
- Forensic invariant: every `ArtifactRecord` carries the advisory
  in two places — title prefix `[MT — review by a certified human
  translator]` and `raw_data.advisory_notice` +
  `raw_data.is_machine_translation`. Pinned by 4 feature-gated
  tests.

## Final results
- **Default-build test count: 45 unit tests passing**, 2 integration
  tests `#[ignore]`-gated on `VERIFY_RUN_INTEGRATION_TESTS=1`.
- **Strata-feature test count: 49 unit tests passing**, same 2
  integration tests gated.
- **Clippy: CLEAN** under both
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo clippy --workspace --all-targets --features
   verify-plugin-sdk/strata -- -D warnings`.
- **Offline invariant: MAINTAINED.** No new permitted egress URLs
  in this sprint. Both translation worker scripts use
  `VERIFY_HF_CACHE` to keep all downloads under
  `~/.cache/verify/models/`.
- **Machine translation advisory: ALWAYS PRESENT.** Three
  enforcement layers preserved from Sprint 2:
  1. `TranslationEngine::advisory()` (only constructor for
     `TranslationResult` outside tests).
  2. CLI `print_advisory` on every translate run.
  3. Plugin SDK `artifact_from_translation` (lean) + Strata
     adapter `record_from_translation` (feature-gated).
  Plus a fourth layer added this sprint:
  `BatchResult::assert_advisory` rejects a batch report missing
  the top-level notice when ≥ 1 translation occurred.

## Pipeline shapes

- **Audio →** preprocess → Whisper (candle, Metal) → classifier →
  NLLB (auto/transformers/ct2) → segment-level translation →
  advisory print.
- **Video →** ffmpeg `-vn` extract → Whisper → classifier → NLLB
  → segment-level translation → advisory print.
- **Image →** Tesseract OCR → classifier → NLLB → advisory print.
- **Text →** classifier → NLLB → advisory print.
- **Batch →** walk → per-file dispatch through the four shapes
  above → JSON report with top-level advisory.
- **Strata plugin (`--features strata`) →** walk → per-file
  dispatch → `Vec<ArtifactRecord>` with title-prefix +
  raw_data advisory.
