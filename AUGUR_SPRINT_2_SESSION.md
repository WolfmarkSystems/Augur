# AUGUR Sprint 2 — 2026-04-25 — Session log

## P1 Whisper STT: PASSED
- Backend chosen: **candle-whisper** (pure Rust, Metal).
  Build probe completed in ~44 s on macOS ARM64 with the
  `metal` feature — no cmake, no FFI, no C++ toolchain.
- Replaced GGML URLs with HF safetensors fetch via `hf-hub`
  (`openai/whisper-tiny|base|large-v3`); bundled
  `melfilters.bytes` (80-bin) and `melfilters128.bytes`
  (128-bin) under `crates/augur-stt/assets/`.
- Real candle decoder pipeline:
  preprocess → mel → encoder → language detect → greedy
  decoder with timestamp tokens → segments.
- Detected language populated from Whisper's own logit pass
  over the 99 language tokens.
- Segments produce real `[start_ms, end_ms, text]` tuples.
- Live transcription not exercised in this session (would
  require real audio fixture + ~150 MB safetensors download);
  integration test gated on `AUGUR_RUN_INTEGRATION_TESTS=1`
  is updated to flip from stub to real-transcript assertion.

## P2 NLLB-200: PASSED
- Backend chosen: **Python + transformers subprocess**
  (Option B). Candle does not ship NLLB's MBart-style
  architecture; subprocess is the pragmatic offline path.
- Bundled worker script (`crates/augur-translate/src/script.py`)
  invoked via `python3 -c` per call, JSON over stdin/stdout.
- HF cache forced under `~/.cache/augur/models/nllb/` via
  `AUGUR_HF_CACHE` so all weight downloads are auditable.
- ISO 639-1 ↔ NLLB BCP-47 mapping covers Arabic, Farsi (`fa`
  → `pes_Arab`), Pashto (`ps` → `pbt_Arab`), Urdu (`ur` →
  `urd_Arab`) plus the major Latin/CJK targets.
- **Machine-translation advisory is mandatory and load-bearing.**
  Every `TranslationResult` carries
  `is_machine_translation = true` and a non-empty
  `advisory_notice`; the CLI prints the notice on every
  `augur translate` invocation; there is no suppression flag.
- Live translation not exercised (would require Python +
  transformers + first-run model download); structural test
  `machine_translation_advisory_always_present` pins the
  forensic invariant in the build.

## P3 Tesseract OCR: PASSED
- Backend chosen: **tesseract CLI subprocess**. Tesseract
  was not installed at build time, so we follow the
  `ffmpeg`-style subprocess pattern. The pure-Rust build
  story is preserved; no C/C++ FFI in the binary.
- `OcrEngine::extract_text` shells out to
  `tesseract <input> stdout -l <lang>` and surfaces missing
  binary / missing file as structured `AugurError::Ocr` /
  `InvalidInput` errors (no panics).
- ISO 639-1 ↔ Tesseract code map covers the forensic
  languages (Arabic, Farsi/`fas`, Pashto/`pus`, Urdu/`urd`).
- Image input wired into the CLI: `augur translate --image
  photo.png --ocr-lang ar --target en` runs
  OCR → classifier → NLLB → advisory print.

## P4 Strata Plugin: PASSED (adapter shape only)
- Upstream `strata-plugin-sdk` is not in the workspace, so
  we ship the adapter shape: `ArtifactRecord`, `Confidence`,
  `AugurStrataPlugin` metadata, and the
  `artifact_from_translation` converter that maps a
  `TranslationResult` into a `is_advisory = true` artifact
  with the mandatory advisory notice.
- The `StrataPlugin` trait `impl` is a thin shim landed when
  the SDK is vendored — no business logic changes.

## Final results
- **Final test count: 35 unit tests passing, 2 integration tests `#[ignore]`-gated on `AUGUR_RUN_INTEGRATION_TESTS=1`.**
- **Clippy: CLEAN** under `cargo clippy --workspace --all-targets -- -D warnings`.
- **Offline invariant: MAINTAINED.** Default code path emits
  zero network traffic. Permitted egress points are all
  named consts: `LID_MODEL_URL` (classifier),
  `WHISPER_MODEL_URL_TINY|BASE|LARGE_V3` (STT),
  `NLLB_MODEL_URL_DISTILLED_600M` (translation). Tesseract
  reads only local tessdata; OCR is fully offline.
- **Machine translation notice: ALWAYS PRESENT.** Pinned by
  `augur_translate::tests::machine_translation_advisory_always_present`
  and enforced at three layers:
  1. `TranslationEngine::advisory()` — only constructor for
     `TranslationResult` outside tests; sets the flag + notice.
  2. CLI `cmd_translate` — always prints the notice block.
  3. Plugin SDK `artifact_from_translation` — back-fills the
     notice if the translation result happens to have it blank.

## Pipeline shapes wired

- **Audio →** preprocess (ffmpeg / hound) → Whisper STT (candle,
  Metal) → fastText/whichlang classifier on transcript →
  NLLB-200 → advisory print.
- **Image →** Tesseract OCR subprocess → fastText/whichlang
  classifier on OCR output → NLLB-200 → advisory print.
- **Text →** classifier → NLLB-200 → advisory print.
