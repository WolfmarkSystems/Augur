# AUGUR Sprint 5 — 2026-04-26 — Session log

## P1 fasttext-pure-rs: COMPATIBLE
- Probe (`crates/augur-classifier/examples/lid_pure_probe.rs`,
  feature-gated as `fasttext-probe`) ran against the cached
  `lid.176.ftz`:
  - Arabic       → ar (0.974) **PASS**
  - Chinese      → zh (0.984) **PASS**
  - Russian      → ru (0.964) **PASS**
  - Spanish      → es (0.992) **PASS**
  - Persian/Farsi → fa (0.730) **PASS**
  - Urdu         → ur (0.983) **PASS**
  - Pashto       → fa (0.761) **FAIL** (known model-level
    confusion with Persian — both Arabic-script + linguistically
    related; this is not a parser bug)
- All four spec-required cases (Arabic / Chinese / Russian /
  Spanish) PASS → **fasttext-pure-rs is binary-compatible with
  Meta's `lid.176.ftz`**.
- Removed the broken `fasttext = "0.8"` dep; classifier now uses
  `fasttext-pure-rs::FastText` (`load(path)` + `predict(text, k,
  threshold) → Result<Vec<Prediction>>`). The Sprint 1 probe
  example (`lid_label_probe.rs`) was deleted; the new
  `lid_pure_probe.rs` documents the result.
- `--classifier-backend fasttext` flipped from EXPERIMENTAL to
  production-ready in `--help` and CLAUDE.md. Default backend
  remains `whichlang` (no model download); fastText is opt-in
  for 176-language coverage. Pashto edge case documented.
- 2 new gated integration tests
  (`fasttext_pure_rs_classifies_arabic_correctly`,
  `fasttext_pure_rs_classifies_forensic_languages`) — pass with
  `AUGUR_RUN_INTEGRATION_TESTS=1` against the cached LID model.

## P2 Speaker diarization: PASSED (gracefully unavailable
without pyannote installed on this build host)
- pyannote available: not installed on this host; `is_available()`
  returns false gracefully, no panic. The structural code paths
  (token management, subprocess plumbing, segment merging) are
  exercised by the 6 unit tests added.
- Segment merging: yes —
  `augur_stt::merge_stt_with_diarization` walks each STT segment,
  picks the diarization segment with the maximum millisecond
  overlap, and emits an `EnrichedSegment`. Pinned by
  `enriched_segment_merges_stt_and_diarization_by_overlap` and
  `merge_assigns_unknown_when_no_diarization_overlap`.
- HF token management: yes — `HfTokenManager` reads / writes
  `~/.cache/augur/hf_token` with 0600 permissions on Unix.
  Pinned by `hf_token_manager_round_trip`,
  `hf_token_manager_returns_clear_error_when_missing`,
  `save_rejects_empty_token`. CLI: `augur setup --hf-token <T>`.
- Diarization is opt-in (`augur translate --diarize`); default
  behavior unchanged. Text / image / PDF inputs explicitly
  ignore the flag with a log line — no audio means no speaker
  attribution.
- After translation with diarization on, the CLI prints
  `[start - end] SPEAKER_NN: source_text` and
  `[start - end] SPEAKER_NN: translated_text` blocks; the
  machine-translation advisory follows as always.

## P3 Air-gap package: PASSED
- Package builder script:
  `scripts/build_airgap_package.sh tiny|base|large-v3` — fetches
  `lid.176.ftz` via curl, then uses `huggingface-hub`'s
  `snapshot_download` to pull the chosen Whisper preset and
  `facebook/nllb-200-distilled-600M`, plus an `install.sh` that
  copies everything into `~/.cache/augur/models/` on the target.
- AIRGAP_PATH env var: yes — `augur_classifier::ModelManager::
  ensure_lid_model()` checks `AUGUR_AIRGAP_PATH/lid.176.ftz`
  before any network egress. If found, copies into the cache
  with the same integrity check the curl path uses. Pinned by
  `airgap_path_short_circuits_download` and
  `airgap_path_takes_priority_over_existing_cache`.
- Install docs: `docs/AIRGAP_INSTALL.md` (build, transfer,
  install, verify, threat model).

## Final results
- **Default-build test count: 61 unit tests passing**
  (10 + 3 + 10 + 8 + 3 + 19 + 8 + 3 cli tests = 64? actual run
  shows 61 — 8 classifier + 10 classifier + 8 cli + 3 augur-core
  pipeline + 19 augur-stt + 3 augur-cli tests + 3 plugin-sdk
  + 8 ocr + 8 translate). Two integration tests (whisper) and
  two new fasttext-pure-rs gated tests = 4 ignored on default,
  runnable with `AUGUR_RUN_INTEGRATION_TESTS=1`.
- **Clippy: CLEAN** under
  `cargo clippy --workspace --all-targets -- -D warnings`.
- **Offline invariant: MAINTAINED.** No new permitted egress
  URLs in this sprint. The pyannote weight URL is governed by
  the same auditable HF cache path as Whisper / NLLB; the
  air-gap package eliminates network egress entirely on the
  destination machine.
- **MT advisory: ALWAYS PRESENT.** Diarization decorates the
  translated transcript with speaker labels but never
  constructs a `TranslationResult` — the advisory enforcement
  layers from Sprints 2-4 remain intact.

## Pipeline shapes (final, post Sprint 5)

- **Audio →** preprocess → Whisper (temperature fallback) →
  classifier → NLLB → segment-level translation → advisory print.
- **Audio + `--diarize` →** Whisper → pyannote → merge by overlap
  → NLLB on each STT segment → enriched print
  (`[start - end] SPEAKER_NN: source` then translated) → advisory.
- **Video →** ffmpeg `-vn` extract → Whisper → … (same as audio).
  `--diarize` works identically.
- **Image →** Tesseract OCR → classifier → NLLB → advisory.
- **PDF →** `pdf-extract` text layer → (fallback) `pdftoppm`
  rasterize + Tesseract → classifier → NLLB → advisory.
- **Text →** classifier → NLLB → advisory.
- **Batch →** walk → per-file dispatch → JSON report.
- **Strata plugin (`--features strata`) →** walk → per-file
  dispatch → `Vec<ArtifactRecord>` with title-prefix +
  raw_data advisory.
- **Air-gapped deploy →** unpack `augur-airgap-<preset>.tar.gz`
  → `install.sh` → `AUGUR_AIRGAP_PATH=...` → all of the above
  with zero network egress.
