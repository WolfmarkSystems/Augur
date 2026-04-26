# AUGUR — Session State

_Last updated: 2026-04-24_
_Executor: claude-opus-4-7_
_Approved by: KR_

---

## AUGUR Sprint 1 — 2026-04-24

**P1 Scaffold: PASSED**
  - Workspace created: yes — 6 crates + 1 app + `tests/fixtures/`, matches the tree in AUGUR_SPRINT_1.md exactly
  - `cargo build --workspace` clean: yes (3.37s fresh; ~0.2s incremental)
  - `cargo clippy --workspace -- -D warnings` clean: yes
  - `cargo test --workspace`: 0 passed / 0 failed (no tests yet — expected)
  - `CLAUDE.md` written: covers hard rules, offline invariant, per-crate responsibilities, 4 pipeline-order architectural decisions
  - `.gitignore` covers `/target/`, `Cargo.lock`, `*.weights`, `*.bin`, `*.gguf`, `*.ggml`, `*.ftz`, `*.fasttext` (added mid-sprint), `models/`, `weights/`, `.cache/`, editor/OS noise
  - Git initialised: yes — first commit `945268e`

**P2 Classifier: PASSED**
  - fastText binding chosen: `fasttext = "0.8.0"` — advertises itself as *"fastText pure Rust implementation"*, no FFI, no system libs, clean `cargo build` on macOS ARM64 in ~4 seconds (well under the 10-min budget). API gives us `FastText::load_model(path)` + `.predict(text, k, threshold) -> Vec<Prediction>`.
  - Secondary dep: `whichlang = "0.1.1"` (pure-Rust, 16 languages, no I/O, no network) — picked as a dual primary rather than a dev-only dep so the Sprint 1 tests run fully offline AND so `--classifier-backend whichlang` is a working production fallback for air-gapped workstations.
  - Classification tests: **8/8 pass** (`cargo test -p augur-classifier`)
    - Arabic: `مرحبا بالعالم، كيف حالك اليوم؟` → `ar`, confidence 1.00, `is_foreign=true` ✅
    - Chinese: `你好,世界,你今天怎么样?` → `zh` ✅
    - Russian: `Привет мир, как у тебя сегодня дела?` → `ru` ✅
    - Spanish: `Hola mundo, ¿cómo estás hoy?...` → `es` ✅
    - English-as-not-foreign when `target=en` ✅
    - Empty input → `Ok` + confidence 0.0, no panic ✅
    - Whitespace-only input → same ✅
    - `ModelManager::with_xdg_cache()` paths end with `.cache/augur/models` ✅
  - `ModelManager::ensure_lid_model()` fetches `lid.176.ftz` via `curl -fL --silent --show-error` from the Facebook mirror (named `LID_MODEL_URL` const) on first run; cached-path fast-path with size check ≥ 500 KB on subsequent runs.

**P3 Whisper STT: PASSED (stub backend, real preprocessing)**
  - whisper-rs builds on ARM64: **NO** — build probe failed in 11 seconds with `is cmake not installed?` Installing `cmake` + Xcode C++ toolchain is a ~15 MB system dep that every examiner workstation would need at build time. Per the P3 15-min budget rule, I **removed `whisper-rs` from the default deps** and shipped a stub `SttEngine` behind the full public API. Decision recorded inline in `crates/augur-stt/Cargo.toml` and `src/whisper.rs`. Sprint 2 decision: either gate `whisper-rs` behind an opt-in cargo feature, or switch to a pure-Rust Whisper port (e.g. `candle-whisper`).
  - All three presets defined: yes — `Fast` / `Balanced` / `Accurate`, each exposing `model_filename()`, `download_url()`, `expected_size_bytes()`.
  - Egress-point constants — all URLs are top-level `pub const`s (`WHISPER_MODEL_URL_TINY` / `_BASE` / `_LARGE_V3`) alongside the classifier's `LID_MODEL_URL`. `grep MODEL_URL crates/` enumerates the complete network-egress audit surface from one grep.
  - `ModelManager::ensure_whisper_model(preset)` — per-preset cache subdir (`tiny/` / `base/` / `large-v3/`), fast-path + integrity check (≥ 50 MB lower bound).
  - Audio preprocessing: **real, not stubbed** — ffmpeg subprocess preferred (MP3 / M4A / MP4 audio / OGG / FLAC / WAV → 16 kHz mono s16 PCM); hound fallback for WAV-only when ffmpeg is absent (reads i16 or f32 samples, simple per-frame mono downmix, naïve nearest-neighbour resample to 16 kHz).
  - Unit tests: **8/8 pass** covering preset filenames, URL-const round-trip, preset size ordering, segment chronology, missing-model error path, missing-audio error path, missing-input preprocessing error path, XDG cache path.
  - Integration tests: 2 present under `tests/whisper_integration.rs`, marked `#[ignore]`, gated on `AUGUR_RUN_INTEGRATION_TESTS=1`. `cargo test -- --include-ignored` without the env var prints a skipping message and does nothing network — no false egress from a naïve override.

**P4 CLI: PASSED**
  - Binary: `apps/augur-cli/src/main.rs`, bin name `augur`, `cargo build --release` produces a 2.6 MB binary in 14 s.
  - Subcommands (all working):
    - `augur classify --text <s> --target <iso>` — real classification via fastText or whichlang
    - `augur transcribe --input <path> --preset [fast|balanced|accurate]` — surfaces STT stub cleanly with `[STT stub — Sprint 2]`
    - `augur translate --input <path> --target <iso> --preset [...]` — full pipeline: runs STT (stub) → classifies transcript → `translate_stub` returns the sentinel. Output format:
      ```
      [AUGUR] Language detected: ar (confidence: 0.97)
      [AUGUR] Transcript: [STT stub — Sprint 2]
      [AUGUR] Translation: [STUB — NLLB-200 integration coming in Sprint 2]
      ```
  - `--classifier-backend [fasttext|whichlang]` global flag. Default `fasttext` with graceful fallback to whichlang when the model isn't cached and the download fails (logs `log::warn!` so the fallback isn't silent).
  - `augur --version` / `-V` → **exact** `AUGUR 0.1.0 — Wolfmark Systems` (disabled clap's default `{bin_name} {version}` shape and intercepted the flag ourselves; the string lives in a `const VERSION_STRING` so it's greppable and tracks `CARGO_PKG_VERSION`).
  - `augur --help` shows *"All processing is local. No evidence leaves your machine."* as the offline-invariant banner at the top of `long_about`.
  - `verify` with no subcommand prints `[AUGUR] no subcommand given. Run \`augur --help\` for usage.` and exits 2 (matches clap's usage-error convention).
  - CLI defense: in `try_run_stt`, I added an audio-file-exists check BEFORE calling `ensure_whisper_model`. Without it a mistyped `--input` would trigger a 75 MB / 142 MB / 2.9 GB download for nothing — spotted during smoke testing (225 MB of accidental cache hits), fixed, re-verified zero unwanted egress.
  - `env_logger` wired with default level `warn` so the Whisper / fastText download-egress warnings always surface; `RUST_LOG=debug` adds pipeline traces.

---

## Verification snapshot (sprint close)

- `cargo build --workspace`: clean, ~3 s incremental
- `cargo build --release -p augur-cli`: clean, 2.6 MB binary produced
- `cargo test --workspace`: **16 passed, 0 failed, 2 ignored** (the two gated integration tests)
- `cargo clippy --workspace --all-targets -- -D warnings`: **clean** (0 warnings, 0 errors)
- Manual CLI smoke: Arabic / Chinese / Russian / Spanish / English / empty / no-subcommand all behave correctly; `--version` exact match; `--help` contains offline banner.

---

## Hard-rules check

- `.unwrap()` / `.expect()` in production: **0** (test `.expect()` only inside `#[cfg(test)]` blocks)
- `unsafe{}`: **0**
- `println!`: **0** in library code. The CLI binary funnels every user-visible line through a single `println_verify()` helper with an inline comment explaining the one permitted CLI-output path; one additional `println!` for `--version` output.
- TODO / FIXME: **0**

---

## Offline invariant: **MAINTAINED**

Four URL constants, all `pub const` or `const`, grep-able from one line:

```
$ grep -r "MODEL_URL" crates/
crates/augur-classifier/src/classifier.rs:const LID_MODEL_URL: &str = "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz";
crates/augur-stt/src/whisper.rs:pub const WHISPER_MODEL_URL_TINY:     &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
crates/augur-stt/src/whisper.rs:pub const WHISPER_MODEL_URL_BASE:     &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";
crates/augur-stt/src/whisper.rs:pub const WHISPER_MODEL_URL_LARGE_V3: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin";
```

Every egress is:
1. Reached only through a named `ensure_*_model()` method
2. Logged at `log::warn!` before the curl runs
3. Preceded by a cache-hit fast path
4. Size-checked on completion
5. Optional at the CLI level — `--classifier-backend whichlang` is a fully-offline production path that never egresses

Whichlang path has zero network access. ffmpeg/hound audio preprocessing is fully local. Plugin SDK / OCR / translate stubs have no network touch.

---

## Commits this sprint

- `945268e` — `chore: scaffold AUGUR workspace — 6 crates, CLI app, CLAUDE.md`
- `82e5c5a` — `feat: augur-classifier — fastText LID + whichlang fallback, ModelManager with one-time download, 8 unit tests`
- `a415b33` — `chore: add *.ftz to .gitignore — fastText model weights`
- `5a32b73` — `feat: augur-stt — Whisper STT engine, 3 presets, audio preprocessing (ffmpeg+hound), 8 unit + 2 ignored integration tests — whisper-rs deferred to Sprint 2 (cmake FFI gap)`
- `79de0b6` — `feat: augur-cli — classify + transcribe + translate stub subcommands, --classifier-backend flag, exact AUGUR 0.1.0 version string`

---

## Deferred to Sprint 2

- **Real Whisper inference** — either gate `whisper-rs` behind `cargo feature` or switch to a pure-Rust port. Drop-in replacement for the stub `SttEngine::transcribe` surface; no public-API churn required.
- **NLLB-200 translation** — replaces `augur-translate::TRANSLATION_STUB`. Add `NLLB_MODEL_URL_*` constants alongside the existing egress constants so the four-URL audit grep extends cleanly.
- **Tesseract OCR** — wire `leptess` + language packs; replaces `augur-ocr::ocr_image_stub`.
- **Video pipeline** — ffmpeg audio extract → STT → classifier → translate.
- **`augur-plugin-sdk`** — wire to the real `strata-plugin-sdk::StrataPlugin` trait so AUGUR surfaces inside Strata as an artifact emitter.
- **Integration tests vs real fixtures** — flip the two `#[ignore]` tests in `augur-stt/tests/whisper_integration.rs` to actual assertions once a real STT backend is wired.
- **Sinc-interpolation resampler** for the hound WAV fallback — current nearest-neighbour implementation is adequate for Sprint 1 but lossy for speech at non-16-kHz source rates.
