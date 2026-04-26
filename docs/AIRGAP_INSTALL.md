# AUGUR Air-Gap Installation

For workstations that cannot reach the internet at all (classified
environments, evidence vaults). AUGUR's default code path expects
to download model weights once on first use; this guide replaces
that step with a USB-transferred bundle.

## What ships in the package

A single `augur-airgap-<preset>-<date>.tar.gz` containing:

- **`lid.176.ftz`** — Meta's 176-language fastText LID model
  (~900 KB), the input the optional `--classifier-backend fasttext`
  path needs.
- **`whisper/`** — Hugging Face safetensors weights + tokenizer +
  config for one Whisper preset (`tiny` ≈ 150 MB, `base` ≈ 290 MB,
  `large-v3` ≈ 3 GB).
- **`nllb/`** — `facebook/nllb-200-distilled-600M` snapshot
  (~2.4 GB) used by the translation pipeline.
- **`install.sh`** — copies the staged files into
  `~/.cache/augur/models/` on the destination machine.

The whichlang classifier ships embedded in the binary itself, so
even with no air-gap package AUGUR can still classify the 16
major languages without any network or filesystem access.

## On an internet-connected machine

```bash
# tiny preset (fast, ~150 MB Whisper)
bash scripts/build_airgap_package.sh tiny

# or balanced (~290 MB Whisper)
bash scripts/build_airgap_package.sh base

# or accurate (~3 GB Whisper)
bash scripts/build_airgap_package.sh large-v3
```

The script writes `augur-airgap-<preset>-YYYYMMDD.tar.gz` to the
current directory. It needs `curl` + `python3` with `huggingface-hub`
on the build host.

## Transfer

USB / external HDD / write-once optical media — whatever the
target environment's evidence-handling rules permit. Verify the
SHA-256 on both sides; the tarball is reproducible enough that
two builds of the same preset on the same day should match.

## On the air-gapped workstation

```bash
mkdir augur-airgap
tar -xzf augur-airgap-tiny-YYYYMMDD.tar.gz -C augur-airgap
bash augur-airgap/install.sh
```

`install.sh` copies the staged files into `~/.cache/augur/models/`.
After it runs, `verify` finds every model in the cache without any
network call.

### Alternative: run from the package directly

If you'd rather not stage a copy under `~/.cache/augur/`, point
AUGUR at the unpacked package via `AUGUR_AIRGAP_PATH`:

```bash
export AUGUR_AIRGAP_PATH=$HOME/augur-airgap
augur classify --classifier-backend fasttext --text "مرحبا" --target en
```

The classifier's `ModelManager` checks `AUGUR_AIRGAP_PATH` before
attempting any network egress; if `$AUGUR_AIRGAP_PATH/lid.176.ftz`
exists, it is copied into the cache and the download path is
skipped. Whisper and NLLB use Hugging Face's own cache layout — the
`install.sh` script handles them by populating the cache directly.

## Verify the install

```bash
# whichlang — works without any model files (sanity check)
augur classify --text "Hola mundo" --target en

# fastText (176 languages) — uses the air-gap LID model
augur classify --classifier-backend fasttext \
    --text "مرحبا بالعالم" --target en

# end-to-end — Whisper + NLLB + advisory notice
augur translate --input sample.wav --target en
```

If any of those fail with a network error, the corresponding
weight is missing from the cache; check `~/.cache/augur/models/`
or rebuild the package against the right preset.

## Threat model

- The package contains nothing examiner-specific — it's the same
  bytes for every customer with the same preset.
- All processing on the air-gapped machine is local. Audio,
  video, image, PDF, and translated content never leave it.
- The machine-translation advisory notice still fires on every
  translation surface. This is enforced by the same Rust-side
  invariants regardless of how models got onto disk.
