# AUGUR — Forensic Language Analysis
# Wolfmark Systems — CLAUDE.md
# Version: 1.0.0
# Last updated: 2026-04-26

## What AUGUR is

Offline forensic foreign language detection, transcription, and
translation. Built by operators for law enforcement and intelligence
professionals. No evidence leaves the machine. Ever.

Two shipping modes, one codebase:
- Standalone CLI — `augur translate / batch / package / install /
  live / self-test / benchmark / geoip / timestamp / config / setup
  / docs`
- Tauri desktop apps — `apps/augur-installer/` (one-click setup
  wizard) and `apps/augur-desktop/` (split-view translation
  workspace + live mic mode + evidence package wizard)

---

## Architecture

### Crates
- `augur-core`        — pipeline orchestrator, dialect routing,
                        report rendering, model registry, GeoIP,
                        YARA, timestamps, resilience limits
- `augur-classifier`  — language identification (whichlang +
                        fasttext-pure-rs + CAMeL Tools wrapper +
                        Pashto/Farsi script disambiguator +
                        Arabic dialect lexical fallback)
- `augur-stt`         — speech-to-text (candle-whisper, Metal GPU
                        on macOS), pyannote diarization, video
                        audio extraction, model selection cascade
- `augur-translate`   — NLLB-200 (Python subprocess: transformers
                        OR ctranslate2 backend), SeamlessM4T,
                        dialect-aware token routing
- `augur-ocr`         — Tesseract subprocess + pdf-extract +
                        pdftoppm fallback for scanned PDFs
- `augur-plugin-sdk`  — Strata plugin adapter (feature-gated;
                        vendored SDK lives under `vendor/`)

### Apps
- `apps/augur-cli/`        — CLI binary `augur`
- `apps/augur-installer/`  — Tauri 2 installer wizard (5-step
                             modal, NDJSON download progress)
- `apps/augur-desktop/`    — Tauri 2 main GUI: split-view doc /
                             transcript / batch / live workspaces;
                             47-language picker (3 quality tiers);
                             Package Wizard; Review Panel;
                             Model Manager; About dialog

### Bundled Python workers (subprocess pattern; offline contract)
- `crates/augur-translate/src/script.py`           — NLLB-200 transformers
- `crates/augur-translate/src/script_ct2.py`       — NLLB-200 ctranslate2
- `crates/augur-translate/src/seamless_worker.py`  — SeamlessM4T
- `crates/augur-classifier/src/camel_worker.py`    — CAMeL Tools dialect ID
- `crates/augur-stt/src/diarize.py`                — pyannote.audio

### Scripts
- `scripts/build_installer.sh`     — npm install + cargo tauri build
- `scripts/build_desktop.sh`       — same for the desktop app
- `scripts/generate_icons.sh`      — sips + iconutil from icon_1024.png
- `scripts/prepare_release.sh`     — both builds + SHA-256 sidecar
- `scripts/build_airgap_package.sh` — pre-staged model bundle for SCIF

---

## Hard rules (non-negotiable)

1. **Zero `.unwrap()` in production code paths** (test code may use
   `.unwrap()` / `.expect()` inside `#[cfg(test)]`)
2. **Zero `unsafe{}` without an inline justification comment**
3. **Zero `println!` in production** — only the audited
   `println_verify` (suppressed under `NDJSON_MODE`) and the
   spec-authorised NDJSON emit sites in `cmd_translate_ndjson` /
   `cmd_batch` / `cmd_package` / `cmd_install` / `cmd_live`
4. **MT advisory on EVERY output surface** — non-suppressible
5. **Offline invariant** — no content leaves the machine during
   inference. Default code paths never make network calls; the
   only egress is the documented one-time model downloads
6. **All download URLs are named constants** in
   `augur-core::models::urls` (mirrored in `augur-stt::whisper`
   for the candle paths and `augur-translate` for NLLB)
7. **No Chinese-origin AI models at any level** (Qwen, Baidu,
   ERNIE, GLM, MiniMax, Kimi, ByteDance, Wenxin, DeepSeek)
   — pinned by `tests/quality_gate.rs::no_chinese_origin_models_in_url_surface`
8. **Dialect advisory always accompanies (never replaces) the
   MT advisory** when an Arabic dialect is detected
9. **`cargo test --workspace`** must pass after every change
10. **`cargo clippy --workspace -- -D warnings`** must be clean

---

## MT Advisory (mandatory text)

```
Machine translation — verify with a certified human translator
for legal proceedings.
```

Source of truth: `augur_core::MT_ADVISORY` (re-exported as
`augur_translate::MACHINE_TRANSLATION_NOTICE` for backward
compatibility). This text — or the `LIVE_ADVISORY` companion in
the live-microphone path — must appear on every output surface.
Cannot be suppressed. Cannot be configured away.

### Live-mode advisory (additional, not a replacement)

```
LIVE MACHINE TRANSLATION — unverified. Real-time output is
inherently less accurate than offline processing. Do not use for
legal decisions in real time.
```

Source: `augur_cli::live::LIVE_ADVISORY`. Rides alongside the MT
advisory on every `live_started` / `live_segment` /
`live_chunk_error` / `live_stopped` NDJSON event AND in the
WorkspaceLive top banner / footer pill / live-session
chain-of-custody.

### Dialect advisory (additional, fires when Arabic dialect detected)

Source: `augur_core::dialect_routing::dialect_advisory_text`. Always
non-empty for every `DialectKind`; rides on every `dialect_routing`
NDJSON event and in the desktop's Dialect Card.

---

## Model stack

```
Language ID:    whichlang (embedded, instant, 16 langs)            ← production default
                fasttext-pure-rs + lid.176.ftz (176 langs)         ← optional via --classifier-backend fasttext
                CAMeL Tools (Arabic dialects, 7 families)          ← optional, MADAR-26 model

STT:            candle-whisper (Pure Rust, Metal GPU on macOS)
                whisper-tiny      (75 MB)
                whisper-base      (142 MB)
                whisper-large-v3  (2.9 GB)                         ← Sprint 10 standard tier
                whisper-pashto    (community fine-tune, 150 MB)    ← Sprint 10 full tier
                whisper-dari      (community fine-tune, 150 MB)    ← Sprint 10 full tier
                Auto-cascade:     LargeV3 → Base → Tiny

Translation:    NLLB-200-distilled-600M (2.4 GB, default)
                NLLB-200-1.3B           (5.2 GB, higher quality)
                SeamlessM4T-medium      (2.4 GB, code-switching)
                ctranslate2 backend     (2.85× warm speedup vs transformers, INT8)
                Dialect routing:        arz_Arab / apc_Arab / acm_Arab / ary_Arab / ara_Arab

Diarization:    pyannote.audio  (opt-in, HF token required)
GeoIP:          MaxMind GeoLite2 (AUGUR_GEOIP_PATH or ~/.cache/augur/GeoLite2-City.mmdb)
YARA:           libyara via the system `yara` binary (subprocess)
```

### URL surface

Every download URL is a named `pub const` in `augur-core::models::urls`
(or the legacy mirrors in `augur-stt::whisper` and `augur-translate`).
Total named URL constants: **15** in `crates/`. Pinned by
`tests/quality_gate.rs::all_download_urls_are_named_constants`.

---

## Test count

**208 workspace tests passing as of v1.0.0** (0 failed, 4 ignored —
integration tests gated on `VERIFY_RUN_INTEGRATION_TESTS=1`).

Per-app:
- Desktop crate (`apps/augur-desktop/src-tauri/`): 23 tests
- Installer crate (`apps/augur-installer/src-tauri/`): 9 tests

---

## MT advisory surface inventory (audited Sprint 20)

The advisory rides on **27 distinct surfaces** as of v1.0.0
(target was 18 — exceeded):

| # | Surface | Carrier |
| --- | --- | --- |
| 1  | CLI text-mode translate output      | `print_advisory()` → `println_verify` |
| 2  | CLI translate NDJSON `complete` event | `machine_translation_notice` field |
| 3  | CLI translate NDJSON `dialect_routing` event | `machine_translation_notice` field |
| 4  | CLI batch JSON report top-level     | `BatchResult.machine_translation_notice` |
| 5  | CLI batch JSON report summary       | `BatchSummary.machine_translation_notice` |
| 6  | CLI batch CSV header (`# …` line)   | Sprint 20 P1 — leading comment row |
| 7  | CLI batch HTML report top + bottom  | `render_batch_html` x2 sites |
| 8  | CLI batch NDJSON `batch_complete` event | `machine_translation_notice` field |
| 9  | CLI evidence package MANIFEST.json  | `Manifest.machine_translation_notice` |
| 10 | CLI evidence package CHAIN_OF_CUSTODY.txt | `render_chain_of_custody` |
| 11 | CLI evidence package per-segment .txt | `build_translation_text` appends `(MT_ADVISORY)` |
| 12 | CLI evidence package `review/REVIEW_REQUIRED.txt` | `render_review_required_txt` (Sprint 17 P2) |
| 13 | CLI live `live_started` event       | `live_advisory` + `machine_translation_notice` |
| 14 | CLI live `live_segment` event       | both fields per chunk |
| 15 | CLI live `live_stopped` event       | both fields on close |
| 16 | CLI live chain-of-custody helper    | `render_live_chain_of_custody` |
| 17 | Desktop status-bar pill             | `status-mt-advisory` (always visible) |
| 18 | Desktop Help → MT Advisory modal    | `mt_advisory_text` Tauri command |
| 19 | Desktop About dialog                | amber callout in `AboutDialog.tsx` |
| 20 | Desktop export HTML (top + bottom)  | `render_html` x2 sites |
| 21 | Desktop export JSON top-level + per-flag | `mt_advisory` + `flagged_segments[].machine_translation_notice` |
| 22 | Desktop export ZIP (MANIFEST + CoC + per-segment + review/) | 4 different writes |
| 23 | Desktop Package Wizard complete screen | `mt-advisory` div |
| 24 | Desktop Save Live Session dialog    | `mt-advisory` div |
| 25 | Desktop Live workspace banner       | `live-banner` (top, role="alert") |
| 26 | Desktop Live workspace footer pill  | `live-footer-advisory` |
| 27 | Installer wizard Complete screen    | `mt-advisory` div in `Complete.tsx` |

---

## Known limitations

- **Pashto (ps) / Farsi (fa) confusion** — script disambiguation
  (Sprint 9) helps but is not definitive. Human verification
  required. See `docs/LANGUAGE_LIMITATIONS.md`.
- **NLLB-200 trained primarily on Modern Standard Arabic** —
  dialectal content has reduced quality. Dialect routing
  (Sprint 15) mitigates by using `arz_Arab` / `apc_Arab` /
  `acm_Arab` / `ary_Arab` tokens or routing Moroccan Darija to
  SeamlessM4T when installed.
- **Batch translation is per-file rayon-parallel** but each file's
  internal pipeline is sequential. STT-engine pool reuse on
  parallel batch is a Sprint 10+ follow-up.
- **Live mode accuracy is lower than offline** due to 3-second
  chunks and lack of cross-chunk context (Whisper benefits from
  ~30 s context windows). Live advisory communicates this.
- **macOS bundles ship ad-hoc signed** (`signingIdentity: null`)
  pending an Apple Developer account. Gatekeeper warns on first
  launch; right-click → Open works.

---

## Examiner-facing docs

- `docs/USER_MANUAL.md`         — full user guide
- `docs/QUICK_REFERENCE.md`     — one-page cheat sheet
- `docs/DEPLOYMENT.md`          — agency deployment guide
- `docs/LANGUAGE_LIMITATIONS.md` — known limitations
- `docs/STRATA_INTEGRATION.md`  — how to add AUGUR to Strata
- `docs/AIRGAP_INSTALL.md`      — air-gapped install procedure
- `CHANGELOG.md`                — version history (v1.0.0 lands Sprint 18)
- `VERSION`                     — single-source-of-truth `1.0.0`
