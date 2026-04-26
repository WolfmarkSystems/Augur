# VERIFY User Manual — v1.0

**For law enforcement and intelligence analysts.**

VERIFY is an offline-first foreign-language evidence
processing tool. It classifies the language of evidence
(audio, video, image, PDF, subtitle, plain text), runs Whisper
speech-to-text where applicable, translates the foreign-language
content to your working language with NLLB-200, and produces
forensic-grade reports + evidence packages.

**No evidence ever leaves your machine.** All inference runs
locally. The only network calls VERIFY's default code path
makes are first-run model weight downloads, all from named
constants the source code makes greppable. For air-gapped
deployments, see [`AIRGAP_INSTALL.md`](AIRGAP_INSTALL.md) — a
pre-bundled package transfers via USB.

**All translations are machine-generated.** They have NOT been
reviewed by a certified human translator. For legal
proceedings, verify all translations with a qualified human
linguist. This advisory fires on every translation surface —
console output, batch JSON/CSV, HTML report, evidence
manifest, Strata plugin artifact. It is not suppressible.

---

## Quick start

### First-time setup
```bash
verify self-test          # check what's installed (offline)
verify self-test --full   # exercise full pipeline (downloads models)
```

### Classify a string
```bash
verify classify --text "مرحبا بالعالم" --target en
```

### Translate a single file
```bash
verify translate --input recording.mp3   --target en
verify translate --input document.pdf    --target en
verify translate --input interview.mp4   --target en --diarize
verify translate --input subs.srt        --target en --output-srt subs.en.srt
verify translate --image photo.png       --target en --ocr-lang ar
```

### Process an evidence directory
```bash
verify batch --input /evidence --target en --output report.json
verify batch --input /evidence --target en --format html --output report.html
verify batch --input /evidence --target en --threads 4
verify batch --input /evidence --target en --types audio,video,pdf
```

### Package results for sharing
```bash
verify package --input /evidence --output case-001.zip
verify package --input /evidence --output case-001.zip --include-originals
```

### Forensic utilities
```bash
verify timestamp 1762276748              # auto-list interpretations
verify timestamp 1762276748 --format unix-seconds
verify geoip 8.8.8.8                     # MaxMind GeoLite2 lookup
verify geoip --setup                     # MaxMind setup instructions
```

---

## Subcommands at a glance

| Command            | Purpose                                                   |
| ------------------ | --------------------------------------------------------- |
| `classify`         | Detect the language of a text string                      |
| `transcribe`       | Whisper STT on an audio file (no translation)             |
| `translate`        | Full pipeline: classify → STT/OCR → NLLB                  |
| `batch`            | Walk a directory; classify + translate every file         |
| `package`          | Walk + translate + bundle into a ZIP with manifest        |
| `setup`            | Save Hugging Face token (for diarization)                 |
| `self-test`        | Pre-deployment readiness check                            |
| `benchmark`        | Time the pipeline against bundled fixtures                |
| `geoip`            | IP geolocation against a MaxMind GeoLite2 DB              |
| `timestamp`        | Convert forensic timestamps (Unix/Apple/Windows/etc)      |
| `config`           | Manage the TOML report config (agency / case / examiner)  |
| `docs`             | Show this manual or a focused reference                   |

Run `verify <command> --help` for full flag listings on any
subcommand.

---

## Model setup

VERIFY downloads model weights on first use into
`~/.cache/verify/models/`:

| Model                    | Size    | Purpose                                       |
| ------------------------ | ------- | --------------------------------------------- |
| `lid.176.ftz` (fastText) | ~900 KB | 176-language LID via `--classifier-backend fasttext` |
| `whisper-{tiny,base,large-v3}` | 150 MB – 3 GB | Whisper STT (preset chosen at runtime) |
| `nllb-200-distilled-600M` | ~2.4 GB | Translation                                   |

Whichlang (the default classifier) ships embedded — no download.

Tooling that VERIFY shells out to:

| Tool        | Required for                              | Install                                       |
| ----------- | ----------------------------------------- | --------------------------------------------- |
| `ffmpeg`    | Audio/video preprocessing                 | `brew install ffmpeg` / `apt install ffmpeg`  |
| `tesseract` | Image OCR                                 | `brew install tesseract`                      |
| `pdftoppm`  | Scanned-PDF rasterize fallback (poppler)  | `brew install poppler`                        |
| `python3` + transformers + sentencepiece + ctranslate2 | NLLB translation | `pip3 install --user transformers torch sentencepiece ctranslate2` |
| `pyannote.audio` | Speaker diarization (`--diarize`)    | `pip3 install --user pyannote.audio`          |
| `yara`      | YARA pattern scanning (`--yara-rules`)    | `brew install yara` / `apt install yara`      |
| MaxMind `GeoLite2-City.mmdb` | IP geolocation         | manual download — see `verify geoip --setup`  |

`verify self-test` reports the status of each.

---

## Air-gap installation

Classified workstations that cannot reach the internet should
follow [`AIRGAP_INSTALL.md`](AIRGAP_INSTALL.md). The short
version: one machine builds a tarball with all model weights,
USB-transfers it, and the destination machine runs
`install.sh`. `VERIFY_AIRGAP_PATH` shortcuts the LID model
fetch when set.

---

## Language support

VERIFY's translation layer (NLLB-200-distilled-600M) supports
all 200 languages NLLB ships. The classifier coverage depends
on backend:

- `--classifier-backend whichlang` (default): 16 major languages,
  no model download.
- `--classifier-backend fasttext`: 176 languages via
  `lid.176.ftz`. Production-ready as of Sprint 5; Pashto
  confuses with Persian at the model layer (the script-level
  disambiguator catches the most obvious cases).

Forensic-priority codes mapped end-to-end (classifier → NLLB):
Arabic (ar), Persian/Farsi (fa), Pashto (ps), Urdu (ur),
Chinese (zh), Russian (ru), Korean (ko), Japanese (ja),
Vietnamese (vi), Turkish (tr), Hebrew (he), Hindi (hi),
Indonesian (id), Polish (pl), Ukrainian (uk).

Arabic dialect families are detected as a coarse signal:
Modern Standard, Egyptian, Levantine, Gulf, Iraqi, Moroccan,
Yemeni, Sudanese. The translation engine still goes through
NLLB-MSA — dialect labels are advisory.

---

## Known limitations

See [`LANGUAGE_LIMITATIONS.md`](LANGUAGE_LIMITATIONS.md) for
the full catalog: Pashto/Persian confusion, short-text
unreliability, sr/hr/bs and ms/id and hi/ur ambiguity, and
guidance on confidence tiers.

---

## Machine translation advisory

> **All translations produced by VERIFY are machine-generated.**
> They have NOT been reviewed by a certified human translator.
> For legal proceedings, verify all translations with a
> qualified human linguist.

This appears on every output surface VERIFY produces. It is
not suppressible by any flag. Speaker labels (when `--diarize`
is used) carry an additional advisory: automated voice
segmentation is NOT biometric identification.
