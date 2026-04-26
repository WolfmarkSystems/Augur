# VERIFY — Claude Code Guidelines

VERIFY is a forensic translation and transcription tool. It surfaces
foreign-language content inside digital evidence — text, audio,
video, and images — translating it into the examiner's working
language without requiring an internet connection.

Two shipping modes, one codebase:
- **Standalone binary** — `verify translate --input evidence.mp4 --target en`
- **Strata plugin** — loaded via `strata-plugin-sdk`; VERIFY's
  artifacts surface in the Strata UI alongside the forensic plugins.

---

## OFFLINE INVARIANT — hard architectural requirement

VERIFY is offline-first by design. **No translation request, no
audio file, no image, and no classified content ever leaves the
examiner's machine.** This is non-negotiable.

Every feature that requires a network call must be:
1. Optional — not in the default code path
2. Clearly labeled in the API (`ensure_*_model`, `download_*`, …)
3. Gated behind an explicit `--online` flag in the CLI

The only permitted network egress in the default path is the
**first-run model download** in `verify-classifier::ModelManager`
(fastText `lid.176.ftz`, ~900 KB) and `verify-stt`'s Whisper
preset downloads — both cached under `~/.cache/verify/models/`.

Before shipping any code, confirm: does this function make a
network call? If yes, it is not in the default path, it is
documented inline, and it is reachable only via an explicit
opt-in API.

---

## Hard rules (absolute)

### Code safety

- **Zero `.unwrap()` in production code paths.** Use `?` operator
  or `match`. Test `.unwrap()` / `.expect()` inside `#[cfg(test)]`
  blocks is permitted.
- **Zero `unsafe{}` blocks** without an explicit justification
  comment. If a dependency requires unsafe, the dependency needs
  justification before being added.
- **Zero `println!`** in production code. Use `log::debug!`,
  `log::info!`, `log::warn!`, `log::error!`. The CLI's
  `env_logger` captures these; raw stdout breaks downstream
  consumers (Strata plugin IPC, scripting).
- **No unnecessary dependencies.** Every new `Cargo.toml` entry
  must justify itself. Prefer crates already in the workspace.

### Error handling

- All errors handled explicitly — no silent failures.
- Every error path either propagates with `?`, logs via
  `log::warn!` / `log::error!`, or surfaces to the caller as a
  typed `VerifyError` variant.
- Sub-crates map their internal errors into `VerifyError` at
  their public boundary. Callers never need to juggle a pile
  of unrelated error enums.

### Testing

- `cargo test --workspace` must pass after every change.
- `cargo clippy --workspace -- -D warnings` must be clean.
- Parser / classifier / STT engines must cover:
  1. A known-good fixture (real input, expected output)
  2. An empty / missing input (returns empty result or `Err`; no panic)
  3. A malformed / corrupt input (returns `Err`; no panic)

### Discipline

- **Plan before code.** Read and understand before touching
  anything. Surface assumptions; ask when uncertain.
- **Simplicity first.** Minimum code that solves the problem.
  No speculative configurability.
- **Surgical changes.** Touch only what the task requires. Match
  existing style even if you would write it differently.
- **No TODO / FIXME** in committed code. File an issue instead.

---

## Crate responsibilities

- **`verify-core`** — pipeline orchestrator. Owns the unified
  `VerifyError` type and the `Pipeline` entry point. Sub-crates
  map their errors into `VerifyError` at their public boundary.
  No ML, no audio, no OCR logic lives here — just the
  dispatch + glue.

- **`verify-classifier`** — language identification. The router
  that runs in front of the heavy pipeline. fastText LID
  (`lid.176.ftz`, 176 languages) or `whichlang` (pure-Rust
  fallback) — final choice documented inline when picked.
  Decides whether a given text is foreign vs the examiner's
  target language and routes the correct pipeline to it.

- **`verify-stt`** — Whisper speech-to-text. Three model
  presets (Fast / Balanced / Accurate) with size + URL
  constants in the `WhisperPreset` enum. Audio preprocessing
  to 16 kHz mono f32 PCM via `hound` (WAV) or `ffmpeg`
  subprocess (everything else). Emits `SttResult` with a full
  transcript, detected language, and timestamped segments.

- **`verify-translate`** — NLLB-200 translation. Sprint 1 is a
  stub (`translate_stub` returns `TRANSLATION_STUB`); Sprint 2
  wires Meta's NLLB-200 model for 200-language offline
  translation. Every call takes source language + target
  language explicitly so the classifier's output feeds the
  translator directly.

- **`verify-ocr`** — Tesseract image OCR. Sprint 1 stub;
  Sprint 2 wires `leptess` bindings + language packs so
  foreign-language text in screenshots / scans / photos can
  be lifted out and handed to the translator.

- **`verify-plugin-sdk`** — Strata plugin adapter. Sprint 1
  stub; Sprint 2 wires `strata-plugin-sdk::StrataPlugin` so
  VERIFY surfaces inside Strata as an artifact emitter
  (`mitre_technique = T1005`).

- **`verify-cli` (under `apps/`)** — the `verify` binary.
  Three subcommands (`classify` / `transcribe` / `translate`).
  Fully offline by default; `--online` opt-in for
  not-yet-scoped online features (none in Sprint 1).

---

## Key architectural decisions

- **Weights are downloaded on first run, cached under
  `~/.cache/verify/models/` (XDG-compliant).** `ModelManager`
  owns this logic. Every download verifies file size before
  accepting the artifact. Weights are NEVER committed to git
  (see `.gitignore`).

- **The fastText classifier is the router, not a nice-to-have.**
  An examiner with a 500 GB image should not wait for VERIFY
  to fully translate every text blob. The classifier runs on
  every input first; only the foreign subset is queued for
  STT + NLLB. Keeping the classifier lightweight is a design
  goal, not an optimisation.

- **Pipeline order (audio input):**
  `preprocess → STT → classifier(transcript) → translate → emit`
  The classifier runs on the STT output so we can handle
  language-mixed audio (e.g. a phone call that switches
  between English and Arabic).

- **Pipeline order (text input):**
  `classifier(raw text) → translate → emit`
  No STT, no preprocessing. Cheapest path.

- **Pipeline order (image input):**
  `OCR(lang_hint=auto) → classifier(ocr_output) → translate → emit`
  Tesseract can take a language hint; if the classifier has
  already run elsewhere on metadata (EXIF UserComment etc.),
  that hint is propagated.

- **Two shipping modes share a pipeline.** The standalone
  CLI and the Strata plugin both go through `verify-core`'s
  `Pipeline`. The plugin adapter translates pipeline results
  into `ArtifactRecord`s; the CLI formats them for stdout.
  No divergent code paths — same results in both modes.

---

## What is in scope for Sprint 1

- Workspace scaffold (P1) — 6 crates, 1 app, CLAUDE.md, `.gitignore`,
  first git commit.
- fastText language classifier (P2) — `ModelManager` with
  first-run download, `LanguageClassifier::classify`, 3 unit
  tests (Arabic, English-as-not-foreign, empty input).
- Whisper STT (P3) — `WhisperPreset` enum, `ModelManager`
  extension for Whisper models, `SttEngine::transcribe`,
  audio preprocessing, 3 unit tests.
- CLI wiring (P4) — `classify` / `transcribe` / `translate`
  subcommands via `clap`. `translate` prints the
  `TRANSLATION_STUB` sentinel from `verify-translate` in
  Sprint 1 — replaced by real NLLB in Sprint 2.

## Sprint 2 decisions (shipped 2026-04-25)

- **Whisper STT — `candle-whisper` (pure Rust, Metal).** The
  candle build probe completed in ~44 s on macOS ARM64 with the
  `metal` feature; no cmake / FFI. We fetch safetensors weights
  from `openai/whisper-{tiny,base,large-v3}` via `hf-hub`, bundle
  the 80-bin and 128-bin mel filter banks under
  `crates/verify-stt/assets/`, and run a greedy decoder with
  timestamp tokens to produce per-segment `[start_ms, end_ms,
  text]` tuples. The Sprint 1 GGML URL constants were retired —
  candle reads safetensors only.
- **NLLB-200 translation — Python + transformers subprocess.**
  candle does not ship NLLB's MBart-style architecture, so per
  the decision rule we ship Option B: a bundled
  `crates/verify-translate/src/script.py` driven by `python3 -c`
  per call. HF cache is forced under
  `~/.cache/verify/models/nllb/` via `VERIFY_HF_CACHE`. The model
  is `facebook/nllb-200-distilled-600M`. ctranslate2 (Option C)
  is a drop-in performance upgrade — same script shape.
- **Machine-translation advisory is load-bearing.** Every
  `TranslationResult` carries `is_machine_translation = true` and
  a non-empty `advisory_notice`. The CLI prints the notice on
  every translate run; there is no suppression flag. The
  `verify_translate::tests::machine_translation_advisory_always_present`
  test pins this invariant in the build.
- **Tesseract OCR — subprocess (no `tesseract` installed at build
  time).** Same pattern as `ffmpeg` for audio: spawn the
  `tesseract` CLI with `<input> stdout -l <lang>`. The `tesseract`
  / `leptess` Rust crates require `libtesseract`+`libleptonica`
  system libs; subprocessing keeps VERIFY's pure-Rust build
  story intact and avoids C/C++ FFI inside the binary.
- **Pipeline orchestration lives in the CLI.** `verify-core`
  exposes the data shapes (`PipelineInput`, `PipelineResult`,
  `TimedSegment`) but does not depend on the engines —
  introducing such a dep would cycle (each engine already
  depends on `verify-core` for `VerifyError`). The CLI wires
  classifier + STT + OCR + translation directly. A future
  `verify-pipeline` crate can house this glue if a second
  embedder (e.g. the real Strata plugin) needs it.
- **`verify-plugin-sdk` adapter shape only.** Upstream
  `strata-plugin-sdk` is not yet vendored into this workspace,
  so we ship the `ArtifactRecord` + `Confidence` + plugin
  metadata shapes plus the `artifact_from_translation` converter.
  The `StrataPlugin` trait `impl` is a thin shim landed when the
  SDK appears.

## Sprint 3 decisions (shipped 2026-04-25)

- **Video pipeline — ffmpeg `-vn` audio extraction.** New
  `verify_stt::extract_audio_from_video` writes a 16 kHz mono WAV
  to a scratch dir and hands off to the Sprint 2 STT path.
  `PipelineInput::Video` was added to verify-core; the CLI auto-
  detects video by extension via `detect_input_kind` (mp4/mov/
  avi/mkv/m4v/wmv/webm/3gp). Translated transcripts preserve
  per-segment timestamps via `TranslationEngine::translate_segments`,
  which translates each STT segment independently and pins
  `[start_ms, end_ms, source_text, translated_text]` tuples on
  the result.
- **ctranslate2 NLLB swap with graceful fallback.** A second
  bundled worker script (`crates/verify-translate/src/script_ct2.py`)
  runs the same `facebook/nllb-200-distilled-600M` via ctranslate2.
  `TranslationEngine::backend` is `Backend::Auto` by default,
  preferring ct2 when its converted model exists at
  `<hf_cache>/ct2/`; otherwise it falls back to the Sprint 2
  transformers worker. Explicit `Backend::Ctranslate2` triggers
  a one-time HF→CT2 conversion (int8 quantization) via the python
  `TransformersConverter`. The CLI exposes
  `--translation-backend auto|transformers|ct2`. Live benchmark
  was not run on this build host (sentencepiece + transformers
  were not installed); literature reports 3–5× CPU speedup, which
  the spec author cited as the motivating gain.
- **Batch processing.** New `verify batch` subcommand walks a
  directory recursively, classifies each file, translates the
  foreign-language ones, and writes a JSON report carrying the
  mandatory machine-translation notice at the top level. Per-file
  errors are captured into the report's `error` field so one bad
  file cannot abort a 1 000-file evidence run. Symlinks are not
  followed (forensic discipline). The walker uses
  `std::fs::read_dir` recursively rather than pulling in
  `walkdir` — fewer deps.
- **Real Strata plugin trait — feature-gated.** Vendoring the
  full Strata `strata-plugin-sdk` tree into VERIFY pulls
  `strata-fs`, which transitively requires NTFS/APFS/ext4/EWF
  filesystem parsers. That violates the "no unnecessary
  dependencies" hard rule for a translation tool. Resolution: the
  real `impl StrataPlugin for VerifyStrataPlugin` lives behind
  the `strata` feature in `verify-plugin-sdk` and is a path
  dependency to `~/Wolfmark/strata/crates/strata-plugin-sdk`
  (sibling workspace). Default build stays lean; `cargo build
  --features verify-plugin-sdk/strata` opts in. The advisory
  notice survives Strata's `ArtifactRecord` shape (which has no
  `is_advisory` field) by living in two places: a `[MT — review
  by a certified human translator]` prefix on the artifact
  `title` and the `is_machine_translation` + `advisory_notice`
  keys in `raw_data`. Both are pinned by
  `assert_advisory_invariant` and four feature-gated tests.

## What remains for Sprint 4+

- Speaker diarization (who said what).
- Real-time transcription (post-v1.0).

## Sprint 4 decisions (shipped 2026-04-25)

- **whichlang is now the production default classifier.** Sprint 1
  diagnostic (`crates/verify-classifier/examples/lid_label_probe.rs`,
  feature-gated as `fasttext-probe`) confirmed the
  `fasttext = "0.8.0"` crate is NOT binary-compatible with
  Meta's published `lid.176.ftz`: Arabic classifies as
  `__label__eo` (Esperanto), Russian / Persian / Chinese drift
  similarly. The wire format the crate parses does not match
  `.ftz`'s actual layout, so labels and weights deserialize out
  of alignment. The CLI's `--classifier-backend` defaults to
  `whichlang` (16 languages, embedded weights, no network);
  `fasttext` is now flagged EXPERIMENTAL on `--help`,
  `load_fasttext()` warns on every call, and the diagnostic
  example is committed as feature-gated. Sprint 5 evaluates
  `fasttext-pure-rs` as a 176-language replacement.
- **Whisper temperature fallback (per-segment).** New
  `verify_stt::TranscribeOptions` exposes the standard OpenAI
  parameters (`temperature`, `temperature_increment`,
  `max_temperature_retries`, `no_speech_threshold`,
  `compression_ratio_threshold`, `rng_seed`). Each 30-second mel
  chunk is decoded; if `no_speech_prob > no_speech_threshold`
  the chunk is accepted as silence, else if the unique-character
  ratio of the produced text falls below
  `compression_ratio_threshold` the chunk is re-decoded at the
  next temperature step (sampling from `softmax(logits/T)`
  instead of argmax). The `rng_seed` default is fixed for
  forensic reproducibility — same audio + same seed produces
  identical transcripts. CLI: `verify transcribe --temperature
  0.0 --max-retries 5`.
- **PDF input** auto-routed by extension. New
  `verify_ocr::extract_pdf_text` tries the pure-Rust
  `pdf-extract` text layer first (handles digitally-generated
  PDFs with no system deps); falls back to a `pdftoppm` (poppler)
  rasterize step + per-page Tesseract OCR for scanned PDFs.
  Missing `pdftoppm` returns a clear `VerifyError::Ocr` with the
  install hint. PDFs flow through the standard
  classifier → NLLB pipeline; `verify batch --types audio,video,image,pdf`
  honors them.
- **ctranslate2 benchmark (M1 Max, NLLB-200-distilled-600M, INT8).**
  Same 98-word forensic-style Arabic paragraph
  (`tests/fixtures/arabic_100_words.txt`) translated through both
  bundled worker scripts:

  | Backend       | Warm time | Cold time (incl. conversion) |
  | ------------- | --------- | ----------------------------- |
  | transformers  | 19.15 s   | 150.78 s                      |
  | ctranslate2   |  6.73 s   |  11.33 s (conversion ≈ 4 s)   |

  **Speedup: 2.85× warm.** Output quality is equivalent — both
  produce fluent English with consistent terminology
  ("investigation team", "scene of the accident", "northern
  suburbs", etc). `Backend::Auto` was kept as: prefer ct2 when
  `<hf_cache>/ct2/` exists, else transformers. Fresh installs
  pay the transformers cost on the first call; once an examiner
  runs `--translation-backend ct2` once, the converted model is
  cached and `Auto` picks it forever after. The reproducer
  script is checked in at `tests/run_benchmark.py`.

## Sprint 5 decisions (shipped 2026-04-26)

- **`fasttext-pure-rs` confirmed binary-compatible with
  `lid.176.ftz` — replaces the broken `fasttext = "0.8"` crate.**
  Sprint 5 P1 probe (`crates/verify-classifier/examples/lid_pure_probe.rs`,
  feature-gated as `fasttext-probe`): Arabic / Chinese / Russian /
  Spanish / Persian / Urdu all classify correctly with high
  confidence (0.96–0.99 on the major languages). Pashto confuses
  with Persian — known model-level limitation, not a parser bug.
  The 176-language fastText backend is now production-ready;
  whichlang remains the CLI default (no model download). The
  `lid_label_probe` example was deleted along with the broken
  `fasttext = "0.8"` dep; the live integration tests
  (`fasttext_pure_rs_classifies_arabic_correctly`,
  `fasttext_pure_rs_classifies_forensic_languages`) gate on
  `VERIFY_RUN_INTEGRATION_TESTS=1`.
- **Speaker diarization via pyannote.audio subprocess.** New
  `verify-stt::diarize` module: `DiarizationEngine`,
  `DiarizationSegment`, `EnrichedSegment`, `HfTokenManager`,
  bundled `diarize.py` worker. Same offline-first contract as
  the NLLB workers — `~/.cache/verify/models/pyannote/` for
  weights, JSON-over-stdio for IO. The HF token (required to
  download the gated `pyannote/speaker-diarization-3.1` model)
  lives at `~/.cache/verify/hf_token` (chmod 0600 on Unix);
  `verify setup --hf-token <T>` writes it. Diarization is opt-in
  via `verify translate --diarize`; default behavior is
  unchanged. STT segments are merged with diarization segments
  by maximum temporal overlap (`merge_stt_with_diarization`);
  the CLI prints the resulting `EnrichedSegment` stream as
  `[start - end] SPEAKER_NN: text` followed by
  `SPEAKER_NN: translated_text`. Audio/video only — text/image/PDF
  inputs ignore the flag with an explicit log line.
- **Air-gap package for offline-only deployments.** New
  `scripts/build_airgap_package.sh` produces
  `verify-airgap-<preset>-<date>.tar.gz` containing
  `lid.176.ftz`, the chosen Whisper preset (tiny/base/large-v3),
  the NLLB-200-distilled-600M snapshot, and an `install.sh` that
  copies them into `~/.cache/verify/models/` on the destination
  machine. The Rust-side
  `verify_classifier::ModelManager::ensure_lid_model()` now
  consults `VERIFY_AIRGAP_PATH` before any network egress;
  pre-staged weights short-circuit the curl path. Documented in
  `docs/AIRGAP_INSTALL.md`. Both Whisper and NLLB use Hugging
  Face's own cache layout, so the install script populates those
  directly rather than going through a separate Rust-side env
  override.

## Sprint 6 decisions (shipped 2026-04-26 — overnight run)

- **Batch report — CSV output + aggregate summary + progress
  file.** `verify batch --output report.csv` emits an
  RFC-4180-escaped CSV (`render_batch_csv` + `BATCH_CSV_HEADER`).
  Any other extension serializes JSON. The JSON form now carries
  a `summary` field (`BatchSummary`) with `total_files /
  processed / foreign_language_files / translated_files / errors
  / languages_detected: {iso → count} / processing_time_seconds`
  plus the mandatory `machine_translation_notice`. While a batch
  is running, `<output>.progress.json` is rewritten after each
  file (counts + last 3 file paths) so an examiner can `tail`
  it during multi-hour evidence runs without forcing a full
  results-vec clone per iteration.
- **Confidence tiers + short-input advisory.** New
  `ConfidenceTier::{High, Medium, Low}` on
  `ClassificationResult` plus
  `classify_confidence(score, word_count) -> ConfidenceTier` and
  `confidence_advisory(tier, word_count) -> Option<String>`.
  Inputs under `SHORT_INPUT_WORD_COUNT = 10` always demote to
  `Low` regardless of model score and surface a "Short input
  (N words) — verify with a human linguist if critical"
  advisory. The CLI prints the tier + word count + advisory on
  every classification; the batch JSON / CSV per-file rows
  carry `confidence_tier` and `confidence_advisory` fields.
- **`verify self-test [--full]`** — pre-deployment readiness
  check. Default form is fully offline: 11 checks covering
  classification (Arabic / English / empty), tooling
  availability (ffmpeg / tesseract / pdftoppm), model-cache
  filesystem state (Whisper, NLLB), `VERIFY_AIRGAP_PATH`, and
  HF-token presence. `--full` adds an end-to-end translation
  check that asserts the mandatory MT advisory survives the
  inference path; missing Python / transformers degrades it to
  `Skip`, never `Fail`. `ready_for_casework` is `true` only
  when zero checks failed; `Skip` and `Warning` are advisory.
- **Pashto / Persian disambiguation.** Both `whichlang` and
  `lid.176.ftz` confuse Pashto with Farsi at the model level
  (Sprint 5 P1 probe). Resolution: when
  `TranslationEngine::advisory()` builds a `TranslationResult`
  with `source_language == "fa"`, it appends
  `FARSI_PASHTO_ADVISORY` to the notice (in addition to the
  mandatory machine-translation advisory — never replacing it).
  Examiner-facing rationale, mitigation, and other commonly-
  confused language pairs documented in
  `docs/LANGUAGE_LIMITATIONS.md`.

## What remains for Sprint 7+

- Examiner-assigned speaker labels (overwrite `SPEAKER_00` →
  `Suspect A` and persist across runs).
- Real-time transcription (post-v1.0).
- Optional auto-conversion to ct2 on first install — pay the
  one-time cost up front for the 2.85× steady-state speedup.
- Script-aware Pashto/Persian tiebreaker (orthographic features
  rather than statistical n-grams) — the Sprint 6 advisory is
  the right examiner-facing answer; a script-level disambiguator
  would be a quality improvement on top.
- Sr/Hr/Bs, Ms/Id, Hi/Ur language-pair advisories along the
  same shape as Sprint 6's fa/ps disambiguation.
