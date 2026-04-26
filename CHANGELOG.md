# AUGUR Changelog

## v1.0.0 — 2026-04-26

### First release

**Core capabilities:**
- Foreign language detection (176 languages via fastText, 200 via NLLB-200)
- Speech-to-text transcription (Whisper, 99 languages — Tiny / Base / Large-v3)
- Machine translation (NLLB-200-distilled-600M and 1.3B, 200 languages)
- SeamlessM4T integration for code-switched content
- Speaker diarization (pyannote.audio, optional)
- Arabic dialect detection (Egyptian, Gulf, Levantine, Moroccan, Iraqi, Yemeni,
  Sudanese) via CAMeL Tools or lexical-marker fallback
- Dialect-aware translation routing (NLLB dialect tokens arz/apc/acm/ary,
  SeamlessM4T preferred for Moroccan Darija)
- True per-segment streaming during inference

**Offline capabilities:**
- All processing performed locally — no evidence leaves the machine
- Air-gap deployment support (`AUGUR_AIRGAP_PATH`)
- Tiered model installation (minimal 2.5 GB / standard 11 GB / full 15 GB)
- SHA-256 integrity verification on downloaded models

**Forensic features:**
- Chain-of-custody text on every evidence package
- Evidence package export (ZIP with MANIFEST + chain of custody +
  per-segment translations + optional `review/` directory)
- Case management — persistent case number, examiner name, agency,
  recent files
- Human review workflow — segment flagging with examiner notes,
  Mark Reviewed / Mark Disputed / Remove Flag actions, persistent
  per-file flag state
- Flagged segments rendered explicitly in HTML / JSON / ZIP exports
  with `[PENDING HUMAN REVIEW]` markers and a dedicated
  "Segments Requiring Human Review" section
- GeoIP forensic helper (MaxMind GeoLite2-City)
- Forensic timestamp converter (Unix / Apple / Windows / WebKit / HFS+)
- YARA pattern integration for translated and original text

**Desktop applications:**
- AUGUR Installer — one-click 5-step setup wizard with live download
  progress, profile selection, and air-gap-friendly bundle support
- AUGUR Desktop — split-view document and transcript workspace,
  37-language picker (3 quality tiers), batch directory mode,
  dialect-routing live status, and the Review panel
- Title bar with persistent case number
- File menu with Recent Files (last 10, MRU)

**CLI:**
- `augur translate / batch / package / install / self-test / benchmark /
  geoip / timestamp / config / setup / docs`
- NDJSON streaming output mode (`--format ndjson`,
  `--format-progress ndjson`) for desktop GUI integration
- About dialog with the mandatory MT advisory in every desktop app

**Machine Translation Advisory:**
All translations produced by AUGUR are machine-generated and require
verification by a certified human translator before use in legal
proceedings. The advisory is non-suppressible — it appears in the
status bar, the About dialog, every HTML / JSON / ZIP export, every
NDJSON `complete` event, the package's MANIFEST.json and
CHAIN_OF_CUSTODY.txt, and per-flag in `review/flagged_segments.json`.
