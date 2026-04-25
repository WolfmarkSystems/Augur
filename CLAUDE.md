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

- Whisper temperature-fallback decoding + full timestamp rules
  (Sprint 2 ships greedy + suppress_tokens).
- Speaker diarization (who said what).
- PDF text extraction.
- Real-time transcription (post-v1.0).
- Live ctranslate2 benchmark on a machine with both transformers
  and ctranslate2 fully installed.
