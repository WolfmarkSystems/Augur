//! Sprint 10 P1 — `augur install` subcommand.
//!
//! Drives the model registry: lists what's available, probes what's
//! installed, and downloads any models in a chosen tier. Direct-file
//! models (`fasttext-lid`, the standalone Whisper presets the
//! existing engines manage) delegate to the engine's own
//! `ensure_*_model` helper. HuggingFace snapshot models (NLLB, CAMeL,
//! SeamlessM4T, community Whisper fine-tunes) shell out to a small
//! Python `huggingface_hub.snapshot_download` one-liner — same
//! offline-first contract as the NLLB / pyannote workers.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command as PCommand;

use augur_classifier::ModelManager as ClassifierModelManager;
use augur_core::models::{
    cache_root, install_path, is_installed, models_for_tier, total_size_for_tier,
    ModelSpec, ModelTier, ModelType, ALL_MODELS,
};
use augur_core::AugurError;
use augur_stt::{ModelManager as SttModelManager, WhisperPreset};

use crate::println_augur;

pub fn cmd_install(
    profile: Option<&str>,
    list: bool,
    status: bool,
    airgap_output: Option<&Path>,
) -> Result<(), AugurError> {
    if list {
        print_catalog();
        return Ok(());
    }
    if status {
        print_status();
        return Ok(());
    }
    let profile = profile.ok_or_else(|| {
        AugurError::InvalidInput(
            "augur install: a profile (minimal/standard/full), --list, --status, or --airgap is required"
                .into(),
        )
    })?;
    let tier = parse_tier(profile)?;
    if let Some(out) = airgap_output {
        return build_airgap(tier, out);
    }
    install_tier(tier)
}

fn parse_tier(profile: &str) -> Result<ModelTier, AugurError> {
    match profile {
        "minimal" => Ok(ModelTier::Minimal),
        "standard" => Ok(ModelTier::Standard),
        "full" => Ok(ModelTier::Full),
        other => Err(AugurError::InvalidProfile(other.to_string())),
    }
}

fn print_catalog() {
    println_augur("Model catalog");
    println_augur("");
    for tier in [ModelTier::Minimal, ModelTier::Standard, ModelTier::Full] {
        let label = tier_label(tier);
        let total = total_size_for_tier(&tier);
        println_augur(format!(
            "─ {} tier ({:.1} GB cumulative, {} model(s))",
            label,
            total as f64 / 1e9,
            models_for_tier(&tier).len()
        ));
        for m in ALL_MODELS.iter().filter(|m| m.tier == tier) {
            println_augur(format!(
                "    [{}] {} — {} ({:.1} MB) — {}",
                m.id,
                m.name,
                type_label(m.model_type),
                m.size_bytes as f64 / 1e6,
                m.quality_note,
            ));
        }
        println_augur("");
    }
    println_augur(
        "Tiers nest: standard includes minimal; full includes everything.",
    );
    println_augur("Install:  augur install minimal | standard | full");
    println_augur("Status:   augur install --status");
    println_augur("Air-gap:  augur install --airgap <path.tar> --profile <tier>");
}

fn print_status() {
    println_augur("Installation status");
    println_augur("");
    let mut installed_bytes: u64 = 0;
    let mut total_bytes: u64 = 0;
    for m in ALL_MODELS {
        let inst = is_installed(m);
        total_bytes += m.size_bytes;
        if inst {
            installed_bytes += m.size_bytes;
        }
        let mark = if inst { "✓ installed" } else { "✗ missing  " };
        println_augur(format!(
            "  {} [{:<22}] {:<32} {:>8.1} MB  ({})",
            mark,
            m.id,
            m.name,
            m.size_bytes as f64 / 1e6,
            tier_label(m.tier),
        ));
    }
    println_augur("");
    println_augur(format!(
        "Installed: {:.2} GB / {:.2} GB ({} of {} models)",
        installed_bytes as f64 / 1e9,
        total_bytes as f64 / 1e9,
        ALL_MODELS.iter().filter(|m| is_installed(m)).count(),
        ALL_MODELS.len(),
    ));
    println_augur("Run `augur install <tier>` to fill gaps.");
}

fn tier_label(t: ModelTier) -> &'static str {
    match t {
        ModelTier::Minimal => "minimal",
        ModelTier::Standard => "standard",
        ModelTier::Full => "full",
    }
}

fn type_label(t: ModelType) -> &'static str {
    match t {
        ModelType::Stt => "stt",
        ModelType::Translation => "translation",
        ModelType::Classifier => "classifier",
        ModelType::Diarization => "diarization",
        ModelType::Unified => "unified",
    }
}

fn install_tier(tier: ModelTier) -> Result<(), AugurError> {
    let models = models_for_tier(&tier);
    let total = total_size_for_tier(&tier);
    println_augur(format!(
        "Installing {} profile ({} models, {:.1} GB)",
        tier_label(tier),
        models.len(),
        total as f64 / 1e9,
    ));
    let count = models.len();
    let mut failed: Vec<String> = Vec::new();
    for (idx, spec) in models.iter().enumerate() {
        let label = format!("[{}/{}] {}", idx + 1, count, spec.name);
        if is_installed(spec) {
            println_augur(format!("  {label} — already installed ✓"));
            continue;
        }
        println_augur(format!(
            "  {label} ({:.1} MB) — fetching…",
            spec.size_bytes as f64 / 1e6
        ));
        match install_model(spec) {
            Ok(()) => {
                if let Err(e) = verify_model_integrity(spec) {
                    failed.push(format!("{}: integrity check failed: {e}", spec.id));
                    println_augur(format!("    ✗ integrity FAIL — {e}"));
                } else {
                    println_augur("    ✓ done");
                }
            }
            Err(e) => {
                failed.push(format!("{}: {e}", spec.id));
                println_augur(format!("    ✗ FAIL — {e}"));
            }
        }
    }
    if failed.is_empty() {
        println_augur("Installation complete. Run `augur self-test` to verify.");
        Ok(())
    } else {
        println_augur(format!(
            "Installation finished with {} failure(s):",
            failed.len()
        ));
        for f in &failed {
            println_augur(format!("  - {f}"));
        }
        Err(AugurError::DownloadFailed {
            model: "tier".into(),
            reason: format!("{} model(s) failed", failed.len()),
        })
    }
}

fn install_model(spec: &ModelSpec) -> Result<(), AugurError> {
    match spec.id {
        "whisper-tiny" => {
            let mgr = SttModelManager::with_xdg_cache()?;
            mgr.ensure_whisper_model(WhisperPreset::Fast)?;
            Ok(())
        }
        "whisper-base" => {
            let mgr = SttModelManager::with_xdg_cache()?;
            mgr.ensure_whisper_model(WhisperPreset::Balanced)?;
            Ok(())
        }
        "whisper-large-v3" => {
            let mgr = SttModelManager::with_xdg_cache()?;
            mgr.ensure_whisper_model(WhisperPreset::Accurate)?;
            Ok(())
        }
        "fasttext-lid" => {
            let mgr = ClassifierModelManager::with_xdg_cache()?;
            mgr.ensure_lid_model()?;
            Ok(())
        }
        "nllb-600m" => hf_snapshot("facebook/nllb-200-distilled-600M", &install_path(spec)?),
        "nllb-1.3b" => hf_snapshot("facebook/nllb-200-1.3B", &install_path(spec)?),
        "camel-arabic" => hf_snapshot(
            "CAMeL-Lab/bert-base-arabic-camelbert-mix-did-madar-corpus26",
            &install_path(spec)?,
        ),
        "seamless-m4t-medium" => {
            hf_snapshot("facebook/seamless-m4t-medium", &install_path(spec)?)
        }
        "whisper-pashto" => download_safetensors(spec),
        "whisper-dari" => download_safetensors(spec),
        "pyannote-diarization" => Err(AugurError::DownloadFailed {
            model: spec.id.into(),
            reason:
                "pyannote requires `pip3 install --user pyannote.audio` and `augur setup --hf-token <T>`; \
                 model is fetched on first diarize call"
                    .into(),
        }),
        other => Err(AugurError::DownloadFailed {
            model: other.into(),
            reason: "unknown model id".into(),
        }),
    }
}

/// Shell out to `python3 -c "from huggingface_hub import snapshot_download; ..."`.
/// Reuses the user's existing huggingface_hub install — same dep
/// chain that NLLB and pyannote workers already require.
fn hf_snapshot(repo: &str, target_dir: &Path) -> Result<(), AugurError> {
    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent)?;
    }
    let cache_dir = target_dir
        .parent()
        .unwrap_or(Path::new("."))
        .to_string_lossy()
        .into_owned();
    let script = format!(
        "from huggingface_hub import snapshot_download; \
         snapshot_download(repo_id='{repo}', cache_dir='{cache_dir}')"
    );
    let output = PCommand::new("python3")
        .arg("-c")
        .arg(&script)
        .output()
        .map_err(|e| AugurError::DownloadFailed {
            model: repo.into(),
            reason: format!("could not spawn python3: {e}"),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AugurError::DownloadFailed {
            model: repo.into(),
            reason: format!("snapshot_download failed: {}", stderr.trim()),
        });
    }
    Ok(())
}

fn download_safetensors(spec: &ModelSpec) -> Result<(), AugurError> {
    let path = install_path(spec)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let status = PCommand::new("curl")
        .arg("-L")
        .arg("--fail")
        .arg("-o")
        .arg(&path)
        .arg(spec.download_url)
        .status()
        .map_err(|e| AugurError::DownloadFailed {
            model: spec.id.into(),
            reason: format!("could not spawn curl: {e}"),
        })?;
    if !status.success() {
        return Err(AugurError::DownloadFailed {
            model: spec.id.into(),
            reason: format!("curl exited with {status}"),
        });
    }
    Ok(())
}

/// Sprint 10 P1 spec — verify SHA-256 if the registry pins one.
/// Empty `sha256` is a documented "no checksum yet" state and the
/// installer logs and skips. Future deployments populate this
/// before shipping.
pub fn verify_model_integrity(spec: &ModelSpec) -> Result<(), AugurError> {
    if spec.sha256.is_empty() {
        log::warn!(
            "no SHA-256 pinned for {} — integrity check skipped",
            spec.id
        );
        return Ok(());
    }
    let path = install_path(spec)?;
    if !path.is_file() {
        // Snapshot-style models materialise as directories; SHA
        // verification is per-file and only meaningful for
        // single-file artifacts. We log and skip the directory case.
        log::warn!(
            "SHA-256 verification skipped for {} (not a single file)",
            spec.id
        );
        return Ok(());
    }
    let computed = sha256_of_path(&path)?;
    if computed != spec.sha256 {
        return Err(AugurError::IntegrityFailure {
            model: spec.id.into(),
            expected: spec.sha256.into(),
            computed,
        });
    }
    Ok(())
}

fn sha256_of_path(path: &Path) -> Result<String, AugurError> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        s.push_str(&format!("{byte:02x}"));
    }
    Ok(s)
}

fn build_airgap(tier: ModelTier, output: &Path) -> Result<(), AugurError> {
    let models = models_for_tier(&tier);
    let missing: Vec<&'static str> = models
        .iter()
        .filter(|m| !is_installed(m))
        .map(|m| m.id)
        .collect();
    if !missing.is_empty() {
        return Err(AugurError::InvalidInput(format!(
            "cannot build air-gap package: {} model(s) not installed locally: {}",
            missing.len(),
            missing.join(", ")
        )));
    }
    let cache = cache_root()?;
    let manifest = airgap_manifest_json(&tier);
    let manifest_path =
        std::env::temp_dir().join(format!("augur-airgap-manifest-{}.json", std::process::id()));
    fs::write(&manifest_path, manifest)?;
    let readme_path =
        std::env::temp_dir().join(format!("augur-airgap-readme-{}.txt", std::process::id()));
    fs::write(&readme_path, AIRGAP_README)?;
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    // Build tar by shelling out — `tar` is on every Unix machine
    // an examiner would deploy AUGUR to, and avoids pulling a
    // `tar` Rust crate dep just for one operation.
    let cache_parent = cache.parent().ok_or_else(|| {
        AugurError::ModelManager("model cache root has no parent".into())
    })?;
    let cache_name = cache
        .file_name()
        .ok_or_else(|| AugurError::ModelManager("model cache root has no name".into()))?;
    let status = PCommand::new("tar")
        .arg("-cf")
        .arg(output)
        .arg("-C")
        .arg(cache_parent)
        .arg(cache_name)
        .arg("-C")
        .arg(manifest_path.parent().unwrap_or(Path::new("/")))
        .arg(manifest_path.file_name().unwrap_or_default())
        .arg("-C")
        .arg(readme_path.parent().unwrap_or(Path::new("/")))
        .arg(readme_path.file_name().unwrap_or_default())
        .status()
        .map_err(|e| AugurError::DownloadFailed {
            model: "airgap".into(),
            reason: format!("could not spawn tar: {e}"),
        })?;
    let _ = fs::remove_file(&manifest_path);
    let _ = fs::remove_file(&readme_path);
    if !status.success() {
        return Err(AugurError::DownloadFailed {
            model: "airgap".into(),
            reason: format!("tar exited with {status}"),
        });
    }
    let archive_sha = sha256_of_path(output)?;
    let sha_path = {
        let mut p = output.to_path_buf();
        let new_name = format!(
            "{}.sha256",
            output.file_name().unwrap_or_default().to_string_lossy()
        );
        p.set_file_name(new_name);
        p
    };
    let mut sha_file = fs::File::create(&sha_path)?;
    writeln!(
        sha_file,
        "{}  {}",
        archive_sha,
        output.file_name().unwrap_or_default().to_string_lossy()
    )?;
    println_augur(format!("Air-gap package written: {}", output.display()));
    println_augur(format!("SHA-256 sidecar:        {}", sha_path.display()));
    Ok(())
}

fn airgap_manifest_json(tier: &ModelTier) -> String {
    let models: Vec<serde_json::Value> = models_for_tier(tier)
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "name": m.name,
                "size_bytes": m.size_bytes,
                "filename": m.filename,
                "sha256": m.sha256,
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "augur_version": env!("CARGO_PKG_VERSION"),
        "tier": tier_label(*tier),
        "models": models,
    }))
    .unwrap_or_else(|_| "{}".into())
}

const AIRGAP_README: &str = "AUGUR air-gap package
==========================================================

This archive contains AUGUR model weights for offline install
on a machine without internet access.

Install:
  1. Copy the .tar to the destination machine.
  2. Verify integrity:  sha256sum -c <archive>.sha256
  3. Extract into the user home:  tar -xf <archive> -C ~/
  4. Confirm install:  augur install --status
  5. Run self-test:    augur self-test

The augur binary itself is NOT included — install it separately
via your distribution channel before extracting these weights.
";

#[cfg(test)]
mod tests {
    use super::*;
    use augur_core::models::find_model;

    #[test]
    fn parse_tier_accepts_known() {
        assert_eq!(parse_tier("minimal").unwrap(), ModelTier::Minimal);
        assert_eq!(parse_tier("standard").unwrap(), ModelTier::Standard);
        assert_eq!(parse_tier("full").unwrap(), ModelTier::Full);
    }

    #[test]
    fn parse_tier_rejects_unknown() {
        assert!(matches!(
            parse_tier("xl"),
            Err(AugurError::InvalidProfile(_))
        ));
    }

    #[test]
    fn integrity_skips_when_no_sha_pinned() {
        let spec = find_model("whisper-tiny").expect("registry has whisper-tiny");
        // sha256 is empty in the shipped registry; verification
        // must succeed (warn-and-skip path).
        verify_model_integrity(spec).expect("empty sha256 → ok");
    }

    #[test]
    fn install_path_handles_all_models() {
        for spec in ALL_MODELS {
            let _ = install_path(spec).expect("path resolves");
        }
    }

    #[test]
    fn unknown_install_id_is_classifier_error() {
        let bogus = ModelSpec {
            id: "does-not-exist",
            name: "x",
            description: "",
            size_bytes: 0,
            tier: ModelTier::Minimal,
            model_type: ModelType::Classifier,
            download_url: "",
            filename: "x",
            sha256: "",
            languages: &[],
            quality_note: "",
        };
        assert!(matches!(
            install_model(&bogus),
            Err(AugurError::DownloadFailed { .. })
        ));
    }
}
