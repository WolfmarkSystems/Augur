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

## What is deferred to Sprint 2+

- NLLB-200 translation integration (replaces `TRANSLATION_STUB`).
- Tesseract OCR.
- Video pipeline (ffmpeg audio extract → STT → NLLB).
- `verify-plugin-sdk` wired to the real Strata plugin API.
- Integration tests against real audio / image fixtures.
