# AUGUR Deployment Guide

How to deploy AUGUR on a forensic workstation. Two paths
depending on whether the workstation has internet access.

## Connected workstation

1. **Build / install the binary.** From the AUGUR workspace:
   ```bash
   cargo build --release
   sudo install target/release/verify /usr/local/bin/verify
   augur self-test       # offline sanity check
   augur self-test --full # downloads models on first run
   ```

2. **System tooling.** AUGUR shells out to several standard
   forensic tools. Install whatever you need:
   ```bash
   brew install ffmpeg tesseract poppler yara
   pip3 install --user transformers torch sentencepiece ctranslate2
   pip3 install --user pyannote.audio   # only for --diarize
   ```

3. **Optional Strata integration.** If the workstation also
   runs Strata, build with the strata feature so AUGUR
   surfaces in the plugin grid:
   ```bash
   cargo build --features augur-plugin-sdk/strata --release
   ```
   See `STRATA_INTEGRATION.md` for the wiring on the Strata
   side.

4. **Optional MaxMind GeoLite2.** Free MaxMind account
   required:
   ```bash
   augur geoip --setup    # prints download instructions
   ```
   Place `GeoLite2-City.mmdb` at
   `~/.cache/augur/GeoLite2-City.mmdb` or set
   `AUGUR_GEOIP_PATH`.

5. **Configure the report metadata.** Once per workstation:
   ```bash
   augur config init
   augur config set agency_name      "Your Agency"
   augur config set examiner_name    "D. Examiner"
   augur config set examiner_badge   "12345"
   augur config set classification   "UNCLASSIFIED // FOUO"
   ```
   The config lives at `~/.augur_report.toml` and is included
   in batch JSON / HTML / evidence-package output.

## Air-gapped workstation

Follow `docs/AIRGAP_INSTALL.md` end-to-end. Summary:

1. On a connected build host: `bash scripts/build_airgap_package.sh tiny`
   — produces `augur-airgap-tiny-YYYYMMDD.tar.gz`.
2. USB-transfer the tarball + the AUGUR binary to the
   air-gapped workstation.
3. On the destination:
   ```bash
   tar -xzf augur-airgap-tiny-YYYYMMDD.tar.gz
   bash install.sh
   ```
   The install script populates `~/.cache/augur/models/` and
   prints the path to set as `AUGUR_AIRGAP_PATH` if you'd
   rather run from the unpacked package directory.

## Casework workflow

A typical run on an evidence directory:

```bash
# 1. Sanity check
augur self-test

# 2. Process the directory
augur batch \
    --input  /evidence/case-2026-001 \
    --target en \
    --threads 4 \
    --config ~/.augur_report.toml \
    --output /case-archive/case-2026-001-report.json

# 3. Make a shareable evidence package
augur package \
    --input  /evidence/case-2026-001 \
    --output /case-archive/case-2026-001.zip \
    --config ~/.augur_report.toml
```

The HTML form for prosecutor / report-bundle delivery:

```bash
augur batch \
    --input  /evidence/case-2026-001 \
    --target en \
    --format html \
    --config ~/.augur_report.toml \
    --output /case-archive/case-2026-001-report.html
```

## Hardening recommendations

- **Lock down the HF cache** if the same machine processes
  multiple cases:
  ```bash
  chmod 700 ~/.cache/augur
  ```
- **Disable network** during processing of high-sensitivity
  cases — AUGUR does not need network access after the
  one-time model download. `AUGUR_AIRGAP_PATH` enforces this
  for the LID model; for Whisper / NLLB the HF cache being
  populated is enough.
- **Verify clippy invariants** on each release build:
  ```bash
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
  Both should be clean before deployment to casework.

## What AUGUR won't do for you

- **Read encrypted containers.** Decrypt evidence containers
  with the appropriate forensic tooling (FTK, X-Ways, etc)
  before pointing AUGUR at the unpacked directory.
- **Decide what's relevant.** AUGUR surfaces foreign-language
  content with translations + advisories — examiner judgement
  determines what matters.
- **Replace a human translator.** Every translation is labeled
  machine-generated. For court proceedings, route through a
  certified linguist.
- **Identify speakers.** Diarization assigns anonymous
  `SPEAKER_NN` labels. These are NOT biometric identification.
