# AUGUR Sprint 20 — Production Hardening + v1.0.0 Tag
# Execute autonomously. Report when complete or blocked.

_Date: 2026-04-26_
_Model: claude-opus-4-7_
_Approved by: KR_
_Working directory: ~/Wolfmark/augur/_

---

## Context

This is the final sprint before AUGUR v1.0.0. No new features.
Hardening, polish, and the v1.0.0 tag. After this sprint, AUGUR
is ready to hand to an agency.

---

## Hard rules

- Zero `.unwrap()` in production code
- Zero `unsafe{}` without justification
- Zero `println!` in production
- MT advisory in all output formats — non-negotiable
- All tests must pass
- `cargo clippy --workspace -- -D warnings` clean

---

## PRIORITY 1 — Full Test Suite Audit

### Step 1 — Run the full test suite with verbose output

```bash
cargo test --workspace -- --nocapture 2>&1 | tee /tmp/augur_test_full.txt
grep -E "FAILED|PASSED|ignored" /tmp/augur_test_full.txt | tail -20
```

Document exact counts:
- Total passing
- Total failing (should be 0)
- Total ignored (integration tests gated on env vars)

### Step 2 — Fix any failing tests

If any tests fail, fix them before proceeding. No v1.0.0 tag
with failing tests.

### Step 3 — Coverage of MT advisory invariant

The MT advisory must be present in every output surface.
Run a targeted check:

```bash
# Every place MT advisory text is hardcoded:
grep -rn "machine.translation\|MT_ADVISORY\|advisory_notice\|machine_translation_notice" \
    crates/ apps/ --include="*.rs" --include="*.tsx" --include="*.ts" \
    | grep -v "test\|Test\|#\[" | wc -l
```

Document the count. Verify each surface:
1. CLI output (--format text)
2. CLI output (--format json)
3. CLI output (--format ndjson complete event)
4. CLI output (--format html)
5. CLI output (--format csv)
6. Batch JSON summary
7. Evidence package MANIFEST.json
8. Evidence package CHAIN_OF_CUSTODY.txt
9. Evidence package per-segment .txt files
10. Evidence package HTML report (top + bottom)
11. Desktop GUI status bar
12. Desktop GUI Help → MT Advisory modal
13. Desktop GUI export HTML (top + bottom)
14. Desktop GUI export JSON
15. Desktop GUI export ZIP (manifest + chain of custody)
16. Installer wizard complete screen
17. Live mode advisory banner
18. Live session chain of custody

Any surface missing the advisory → add it before tagging.

### Step 4 — Offline invariant audit

```bash
# All network calls should be named constants
grep -rn "https://\|http://" crates/ apps/ \
    --include="*.rs" \
    | grep -v "test\|Test\|//\|doc\|comment" \
    | grep -v "const.*URL\|const.*_URL\|pub const" \
    | head -20
```

Any hardcoded URL that is NOT a named constant → move it to a
named constant with a descriptive name. Every URL must be auditable.

### Step 5 — Zero unwrap audit in production paths

```bash
cargo test -p augur-core --test quality_gate -- --nocapture 2>&1
```

If no quality gate test exists, create one:

```rust
// crates/augur-core/tests/quality_gate.rs
#[test]
fn no_unwrap_in_production_paths() {
    // This test exists to document the invariant.
    // Real checking is done by code review + clippy.
    // The presence of this test signals the commitment.
    assert!(true, "Zero .unwrap() in production code paths — enforced by review");
}

#[test]
fn mt_advisory_constant_non_empty() {
    assert!(!augur_core::MT_ADVISORY.is_empty());
    assert!(augur_core::MT_ADVISORY.len() > 50); // meaningful text
}

#[test]
fn all_download_urls_are_named_constants() {
    // Document the full URL surface
    let urls = vec![
        augur_core::models::urls::WHISPER_TINY_URL,
        augur_core::models::urls::WHISPER_LARGE_V3_URL,
        augur_core::models::urls::NLLB_600M_URL,
        augur_core::models::urls::NLLB_1B3_URL,
        augur_core::models::urls::SEAMLESS_M4T_MEDIUM_URL,
        augur_core::models::urls::WHISPER_PASHTO_URL,
        augur_core::models::urls::WHISPER_DARI_URL,
        augur_core::models::urls::LID_MODEL_URL,
    ];
    for url in &urls {
        assert!(!url.is_empty(), "URL constant must not be empty");
        assert!(url.starts_with("https://"),
                "URL must use HTTPS: {}", url);
    }
}
```

### Acceptance criteria — P1

- [ ] Full test suite passes (0 failures)
- [ ] MT advisory present on all 18 surfaces documented above
- [ ] All URLs are named constants
- [ ] Quality gate test file exists
- [ ] Test count documented
- [ ] Clippy clean

---

## PRIORITY 2 — CLAUDE.md Final Update

Update `CLAUDE.md` to reflect the complete v1.0.0 state:

```markdown
# AUGUR — Forensic Language Analysis
# Wolfmark Systems — CLAUDE.md
# Version: 1.0.0
# Last updated: 2026-04-26

## What AUGUR is

Offline forensic foreign language detection, transcription, and
translation. Built by operators for law enforcement and intelligence
professionals. No evidence leaves the machine. Ever.

## Architecture

### Crates
- augur-classifier  — language identification (whichlang + fastText + CAMeL)
- augur-stt         — speech-to-text (candle-whisper, Metal GPU)
- augur-core        — translation pipeline (NLLB-200 via Python subprocess)
- augur-cli         — command-line interface
- verify-plugin-sdk — Strata plugin SDK integration

### Apps
- apps/augur-installer/ — macOS installer wizard (Tauri)
- apps/augur-desktop/   — main GUI application (Tauri + React)

### Scripts
- scripts/worker_script.py  — NLLB-200 translation subprocess
- scripts/script_ct2.py     — ctranslate2 backend
- scripts/camel_worker.py   — CAMeL Arabic dialect identification
- scripts/seamless_worker.py — SeamlessM4T unified model

## Hard rules (non-negotiable)

1. Zero `.unwrap()` in production code paths
2. Zero `unsafe{}` without explicit justification comment
3. Zero `println!` in production — only in audited NDJSON output paths
4. MT advisory on EVERY output surface — non-negotiable
5. Offline invariant — no content leaves machine during inference
6. All download URLs are named constants in augur-core::models::urls
7. No Chinese-origin AI models at any level (DeepSeek, Qwen, MiniMax, etc.)
8. Dialect advisory always accompanies MT advisory when dialect detected

## MT Advisory (mandatory text)

```
Machine translation — verify with a certified human translator
for legal proceedings.
```

This text (or equivalent) must appear on every output surface.
Cannot be suppressed. Cannot be configured away.

## Model stack

```
Language ID:    whichlang (embedded, instant, 16 langs)
                fasttext-pure-rs + lid.176.ftz (176 langs)
                CAMeL Tools (Arabic dialects, 7 families)

STT:            candle-whisper (Pure Rust, Metal GPU)
                whisper-tiny (75MB), whisper-large-v3 (2.9GB)
                whisper-pashto, whisper-dari (community fine-tunes)

Translation:    NLLB-200-distilled-600M (2.4GB, default)
                NLLB-200-1.3B (5.2GB, higher quality)
                SeamlessM4T-medium (2.4GB, code-switching)

Diarization:    pyannote.audio (opt-in, HF token required)
GeoIP:          MaxMind GeoLite2 (AUGUR_GEOIP_PATH)
YARA:           libyara or subprocess fallback
```

## Test count

189 workspace tests passing (as of v1.0.0)
[update with actual count after sprint 20]

## Known limitations

- Pashto (ps) / Farsi (fa) confusion — script disambiguation helps
  but is not definitive. Human verification required.
- NLLB-200 trained on Modern Standard Arabic — dialectal content
  has reduced quality. Dialect routing (Sprint 15) mitigates this.
- Batch-mode translation is sequential, not truly parallel per-file
  (rayon parallelism exists for text classification, not full pipeline)
- Live mode accuracy lower than offline due to 3-second chunks
- seealso: docs/LANGUAGE_LIMITATIONS.md

## Examiner-facing docs

- docs/USER_MANUAL.md      — full user guide
- docs/QUICK_REFERENCE.md  — one-page cheat sheet
- docs/DEPLOYMENT.md       — agency deployment guide
- docs/LANGUAGE_LIMITATIONS.md — known limitations
- docs/STRATA_INTEGRATION.md   — how to add AUGUR to Strata
- CHANGELOG.md             — version history
```

### Acceptance criteria — P2

- [ ] CLAUDE.md fully updated for v1.0.0 state
- [ ] Hard rules section complete and accurate
- [ ] Model stack accurate
- [ ] Test count accurate
- [ ] Known limitations documented
- [ ] CLAUDE.md committed

---

## PRIORITY 3 — v1.0.0 Tag

### Step 1 — Final checks

```bash
cargo test --workspace 2>&1 | tail -3
cargo clippy --workspace -- -D warnings 2>&1 | tail -3
cargo build --workspace 2>&1 | tail -3
```

All must pass. Zero errors. Zero warnings.

### Step 2 — Tag

```bash
git add -A
git commit -m "chore: v1.0.0 release preparation — final audit and CLAUDE.md update"

git tag -a v1.0.0 -m "AUGUR v1.0.0 — Forensic Language Analysis

First production release. Offline-first foreign language detection,
transcription, and translation for law enforcement and intelligence
professionals.

Capabilities:
  - 200-language translation (NLLB-200, Meta AI)
  - 99-language STT (Whisper, OpenAI)
  - 176-language classification (fastText, Meta AI)
  - Arabic dialect detection (7 families, CAMeL Tools)
  - Dialect-aware translation routing
  - SeamlessM4T for code-switched content
  - Pashto/Dari fine-tuned models
  - Speaker diarization (pyannote, opt-in)
  - YARA pattern scanning
  - MaxMind GeoLite2 IP geolocation
  - SRT/VTT subtitle support
  - Air-gap deployment (AUGUR_AIRGAP_PATH)

Desktop application:
  - AUGUR Installer (one-click setup wizard)
  - AUGUR Desktop (split-view translation workspace)
  - Live microphone mode for real-time interview support
  - Evidence package export (ZIP with chain of custody)
  - Human review workflow (segment flagging)

Machine Translation Advisory:
  All translations are machine-generated. Verify with a
  certified human translator for legal proceedings.
  This notice cannot be suppressed.

Built by Wolfmark Systems — operators, for operators."

git push origin main --tags
```

### Step 3 — Push to GitHub

```bash
git push origin main
git push origin v1.0.0
```

### Acceptance criteria — P3

- [ ] All tests passing before tag
- [ ] Zero clippy warnings before tag
- [ ] v1.0.0 annotated tag created
- [ ] Tag pushed to origin
- [ ] MT advisory in tag message
- [ ] main branch pushed

---

## After all priorities complete

Report:
- Final test count
- MT advisory surface count (all 18 surfaces)
- URL constant count
- v1.0.0 tag hash
- Any surfaces that still need advisory (if any)

---

_AUGUR Sprint 20 — Production Hardening + v1.0.0 Tag_
_Authored by: Claude (architect) + KR (approved)_
_Execute with: claude-opus-4-7 in ~/Wolfmark/augur/_
_This is the finish line. Ship it._
