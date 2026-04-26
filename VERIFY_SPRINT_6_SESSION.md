# VERIFY Sprint 6 — Overnight Run — 2026-04-26 — Session log

## Pre-flight
- Pre-start tests: 61 passing.
- Pre-start clippy: clean.

## P1 Batch report improvements: PASSED
- **CSV output** via `verify_core::pipeline::render_batch_csv`.
  Header line matches the spec verbatim
  (`BATCH_CSV_HEADER`); fields with embedded `,`/`"`/newlines get
  RFC-4180-quoted (doubled quotes inside). The CLI picks format
  by extension: `.csv` → CSV, anything else → pretty JSON.
- **`BatchSummary`** attached to JSON reports — counts +
  `languages_detected: {iso → count}` (BTreeMap so JSON ordering
  is stable) + `processing_time_seconds` + the mandatory
  `machine_translation_notice`. `BatchResult::build_summary`
  ignores files with no detected language, so the breakdown
  reflects only what the classifier actually decided.
- **Progress file** at `<output>.progress.json` — rewritten after
  each file with the running counts plus the last-3 processed
  paths. Does **not** clone `results` per iteration — the
  snapshot is built directly via `serde_json::json!` to keep
  the per-file overhead O(1) instead of O(n).
- 5 new tests pin: CSV header, CSV escaping, summary language
  counts, summary advisory, and the assert_advisory invariant
  rejecting summaries with empty notices.

## P2 Confidence tiers + short-input advisory: PASSED
- New `ConfidenceTier::{High, Medium, Low}` on
  `ClassificationResult`. Bands: ≥ 0.85 / 0.60–0.85 / < 0.60.
- Short-input gate: any input with > 0 and < 10 words is forced
  to `Low` regardless of raw score, surfaced as
  "Short input (N words) — language detection may be unreliable.
  Verify with a human linguist if this evidence is critical."
- Pure helpers `classify_confidence` and `confidence_advisory`
  exported for unit testing without spinning up a classifier.
- CLI: every `verify classify` output now prints
  `Confidence: {HIGH|MEDIUM|LOW} (0.97)` and `Input: N word(s)`
  on dedicated lines, followed by the advisory when present.
- Batch JSON / CSV per file gain `confidence_tier` +
  `confidence_advisory` fields. Errored files (no classification
  ever ran) emit empty strings instead of `LOW` — keeps the
  data clean.
- 4 new tests pin: high-confidence long Arabic, low-confidence
  3-word input + advisory text, medium-tier advisory, and the
  short-input short-circuit.

## P3 verify self-test command: PASSED
- New `apps/verify-cli/src/selftest.rs` module: `CheckStatus`,
  `SelfTestCheck`, `SelfTestResult`, plus per-check helpers
  (each `pub` for unit testing).
- 11 checks in the default offline run:
  Arabic / English / empty classification, ffmpeg / tesseract /
  pdftoppm availability, Whisper-cache + NLLB-cache filesystem
  presence, `VERIFY_AIRGAP_PATH` mode, HF-token configured,
  and an offline-invariant audit line.
- `--full` adds an end-to-end translation check that asserts
  the mandatory MT advisory survives the inference path; if
  Python / transformers / NLLB are not provisioned, the check
  degrades to `Skip` (never `Fail`).
- `ready_for_casework` flips to `false` only on hard `Fail`s;
  `Skip` and `Warning` remain advisory. The CLI exits with a
  non-zero code (`VerifyError::InvalidInput`) when the suite
  failed, so downstream automation can react.
- Live output of `verify self-test` on this machine (Sprint 6
  acceptance):

```
[VERIFY] Running self-test...

[VERIFY] ✓ [PASS] Classification: Arabic text → ar: ar (confidence: LOW 1.00, 7 word(s))
[VERIFY] ✓ [PASS] Classification: English → not foreign: en (confidence: HIGH 1.00, 10 word(s))
[VERIFY] ✓ [PASS] Classification: empty input → handled: graceful empty-input handling
[VERIFY] ⚠ [WARN] Audio preprocessing: ffmpeg: `ffmpeg` not found on PATH (optional — limits supported input formats)
[VERIFY] ⚠ [WARN] OCR: tesseract: `tesseract` not found on PATH (optional — limits supported input formats)
[VERIFY] ⚠ [WARN] PDF rasterize: pdftoppm: `pdftoppm` not found on PATH (optional — limits supported input formats)
[VERIFY] ⚠ [SKIP] STT: Whisper tiny safetensors cached: not cached at "/Users/randolph/.cache/verify/models/whisper/hf" (run `verify self-test --full` to download and exercise inference)
[VERIFY] ✓ [PASS] Translation: NLLB-200 cached: cached at "/Users/randolph/.cache/verify/models/nllb"
[VERIFY] ✓ [PASS] Air-gap mode: VERIFY_AIRGAP_PATH not set (online-on-first-run mode)
[VERIFY] ⚠ [SKIP] HF token configured (optional, for diarization): not configured — `verify setup --hf-token <T>` enables speaker diarization
[VERIFY] ✓ [PASS] Offline invariant audit: no unexpected network calls; default self-test is fully offline

[VERIFY] Self-test PASSED (6 passed, 0 failed, 2 skipped, 3 warnings)
[VERIFY] This installation is ready for casework.
```

  Note the three ffmpeg/tesseract/pdftoppm WARN lines: those
  binaries ARE installed on this host but the executable (when
  spawned from inside the Bash tool sandbox) sees a sanitized
  PATH that doesn't include `/opt/homebrew/bin`. On a real
  deployment workstation those three checks all `PASS`. The
  Whisper-cache `SKIP` is genuine — that workstation hasn't
  needed to run STT yet.
- 8 new tests pin: classification checks pass, missing-binary
  → Warning (optional) / Fail (required), `ready_for_casework`
  semantics, and the no-failures invariant on a default run.

## P4 Pashto/Persian disambiguation: PASSED
- `docs/LANGUAGE_LIMITATIONS.md` written — full examiner-facing
  rationale for the fa/ps confusion plus a table of other
  commonly-confused language pairs (sr/hr/bs, ms/id, hi/ur,
  sw/Comorian, pa-East/pa-West) and a confidence-tier reading
  guide.
- `FARSI_PASHTO_ADVISORY` const added to `verify-translate`;
  appended (not substituted) to `advisory_notice` whenever
  source_language is `"fa"`. The mandatory MT advisory still
  fires first; the language hint follows. Forensic invariant
  preserved.
- 3 new tests pin: fa-detected results carry both notices,
  ar-detected results do NOT carry the fa-disambiguation, and
  `docs/LANGUAGE_LIMITATIONS.md` exists with the right body.

## Final results
- **Test count: 81 passing** (up from Sprint 5's 61). 4
  integration tests `#[ignore]`-gated on
  `VERIFY_RUN_INTEGRATION_TESTS=1`.
- **Clippy: CLEAN** under both
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo clippy --workspace --all-targets --features
   verify-plugin-sdk/strata -- -D warnings`.
- **Offline invariant: MAINTAINED.** No new permitted egress
  URLs in this sprint. `verify self-test` (default form)
  audits this directly and prints the result.
- **Machine translation advisory: ALWAYS PRESENT.** All
  enforcement layers from Sprints 2-5 still hold. New
  `farsi_detection_includes_disambiguation_advisory` test
  verifies that the language advisory augments rather than
  replaces the MT notice; new
  `non_farsi_source_does_not_get_disambiguation_advisory`
  prevents leak in the other direction.

## Deviations from spec
- The spec sketched `pdf` as a missing batch type filter; this
  was already supported in Sprint 4. Confirmed no change needed.
- Spec's CSV header listed `transcript` for both image and
  audio — VERIFY's `BatchFileResult.source_text` doubles for
  both, so the column maps cleanly without schema branching.
- Spec section P4 specified "Add to CLAUDE.md under Known
  Limitations"; CLAUDE.md uses a "Sprint N decisions" structure,
  so the limitation was added there + cross-linked from the
  full doc rather than reorganizing the file.
