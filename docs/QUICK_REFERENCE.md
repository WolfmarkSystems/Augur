# VERIFY Quick Reference

## Most common commands

```bash
# Verify the install
verify self-test

# Single-file translation
verify translate --input <file>      --target en
verify translate --image <pic>       --target en --ocr-lang ar
verify translate --input <subs.srt>  --target en --output-srt out.srt

# Directory of evidence
verify batch --input <dir> --target en --output report.json
verify batch --input <dir> --target en --format html --output report.html
verify batch --input <dir> --target en --threads 4

# Evidence package for sharing
verify package --input <dir> --output case-001.zip

# Forensic utilities
verify timestamp 1762276748
verify geoip 8.8.8.8
```

## Cache layout

| Path                                         | Contents                       |
| -------------------------------------------- | ------------------------------ |
| `~/.cache/verify/models/lid.176.ftz`         | fastText LID model (optional)  |
| `~/.cache/verify/models/whisper/hf/`         | Whisper safetensors            |
| `~/.cache/verify/models/nllb/`               | NLLB-200 weights (HF cache)    |
| `~/.cache/verify/models/nllb/ct2/`           | ctranslate2 converted model    |
| `~/.cache/verify/models/pyannote/`           | pyannote diarization weights   |
| `~/.cache/verify/hf_token`                   | Hugging Face token (chmod 600) |
| `~/.cache/verify/GeoLite2-City.mmdb`         | MaxMind GeoIP DB (manual)      |

## Air-gap override

```bash
export VERIFY_AIRGAP_PATH=/path/to/staged/models
```

Bypasses the LID download. Whisper / NLLB / pyannote use the
HF cache layout — see `AIRGAP_INSTALL.md`.

## Forensic invariants — non-suppressible

1. Every translation output carries the **machine-translation
   advisory**: "Machine translation — verify with a certified
   human translator for legal proceedings." Console, batch
   JSON/CSV, HTML, evidence manifest, plugin artifact — all of
   them, every time, no flag turns it off.
2. Diarized transcripts carry a second advisory: speaker
   labels are NOT biometric identification.
3. Farsi (`fa`) detections include the Pashto/Persian
   disambiguation reminder.
4. Short input (<10 words) is always flagged `LOW` confidence
   regardless of model score.

## Where to look

| Question                              | File                                |
| ------------------------------------- | ----------------------------------- |
| What does VERIFY do, full picture?    | `docs/USER_MANUAL.md`               |
| Air-gap install on offline workstation | `docs/AIRGAP_INSTALL.md`            |
| Strata plugin integration             | `docs/STRATA_INTEGRATION.md`        |
| Language pair confusion / limits      | `docs/LANGUAGE_LIMITATIONS.md`      |
| Build / develop                       | `CLAUDE.md`                         |
