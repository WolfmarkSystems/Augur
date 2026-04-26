# VERIFY

**Offline-first forensic translation and transcription tool.**
For law enforcement and intelligence analysts.

VERIFY surfaces foreign-language content inside digital
evidence — text, audio, video, image, PDF, subtitle — and
translates it into the examiner's working language. **No
evidence ever leaves your machine.**

---

## What it does

- Detects the language of evidence in 16+ (whichlang) or
  176 (fastText) languages.
- Transcribes audio / video with Whisper (Metal-accelerated on
  Apple Silicon), with per-segment timestamps + optional
  speaker diarization.
- Translates with NLLB-200-distilled-600M (200 languages),
  with a faster ctranslate2 backend when available.
- OCRs images with Tesseract; extracts PDF text layers; parses
  SRT/VTT subtitles; geolocates IPs against MaxMind GeoLite2;
  converts forensic timestamps (Unix / Apple / Windows /
  WebKit / HFS+).
- Scans translated + original content with YARA rules.
- Bundles results into evidence packages with SHA-256 manifests,
  agency-branded HTML reports, and a chain-of-custody header.

## Three example commands

```bash
# Translate a single audio file
verify translate --input recording.mp3 --target en

# Walk an evidence directory and emit an HTML report
verify batch --input /evidence --target en \
    --format html --output report.html --threads 4

# Bundle the run into a shareable evidence package
verify package --input /evidence --output case-001.zip
```

## Install

```bash
git clone <verify repo> verify && cd verify
cargo build --release
sudo install target/release/verify /usr/local/bin/verify
verify self-test --full   # downloads models on first use
```

For air-gapped workstations: see
[`docs/AIRGAP_INSTALL.md`](docs/AIRGAP_INSTALL.md).

## Forensic discipline

Every translation VERIFY produces is labeled machine-generated.
The advisory fires on every output surface — console, batch
JSON / CSV / HTML, evidence manifest, Strata plugin artifact —
and is not suppressible by any flag.

For legal proceedings, **verify all translations with a
certified human translator.**

## Documentation

- [`docs/USER_MANUAL.md`](docs/USER_MANUAL.md) — full feature
  reference for examiners.
- [`docs/QUICK_REFERENCE.md`](docs/QUICK_REFERENCE.md) —
  one-page command cheat sheet.
- [`docs/DEPLOYMENT.md`](docs/DEPLOYMENT.md) — workstation
  setup + casework workflow.
- [`docs/AIRGAP_INSTALL.md`](docs/AIRGAP_INSTALL.md) — offline
  install for classified environments.
- [`docs/STRATA_INTEGRATION.md`](docs/STRATA_INTEGRATION.md) —
  building VERIFY as a Strata plugin.
- [`docs/LANGUAGE_LIMITATIONS.md`](docs/LANGUAGE_LIMITATIONS.md) —
  known classifier ambiguities (Pashto/Persian, Hindi/Urdu,
  short-text reliability) and how VERIFY surfaces them.
- [`CLAUDE.md`](CLAUDE.md) — engineering invariants + sprint
  decisions for developers.

## License

Proprietary — Wolfmark Systems.
