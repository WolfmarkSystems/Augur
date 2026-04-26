//! Sprint 11 P3/P4 — installer orchestration glue.
//!
//! Owns the per-component install path (download / chmod / copy)
//! and the `INSTALL_MANIFEST.json` written when everything is
//! done. Splitting this out of `main.rs` keeps the Tauri command
//! handlers thin.

use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use crate::bundled;
use crate::download;
use crate::profiles::{self, ALL_COMPONENTS, InstallComponent, Profile};
use crate::python;

#[derive(Debug, Serialize, Clone)]
pub struct ExistingInstallation {
    pub augur_support_dir: Option<String>,
    pub python_ready: bool,
    pub installed_component_ids: Vec<String>,
}

pub async fn check_existing(app: &AppHandle) -> serde_json::Value {
    let dir = python::augur_support_dir(app).ok().map(|p| p.display().to_string());
    let python_ready = python::is_python_ready(app);
    let mut installed: Vec<String> = Vec::new();
    if let Ok(models_dir) = python::models_dir(app) {
        for c in ALL_COMPONENTS {
            let p = models_dir.join(c.id);
            if p.exists() {
                installed.push(c.id.to_string());
            }
        }
    }
    serde_json::to_value(ExistingInstallation {
        augur_support_dir: dir,
        python_ready,
        installed_component_ids: installed,
    })
    .unwrap_or(serde_json::Value::Null)
}

pub fn model_path_for(app: &AppHandle, component_id: &str) -> Result<PathBuf, String> {
    python::model_path_for(app, component_id)
}

/// Set permissions on the bundled binaries when they're the
/// component being installed. ffmpeg + tesseract share one call
/// — we run it once and treat all subsequent bundled-component
/// installs as no-ops (idempotent).
pub async fn setup_bundled_component(app: &AppHandle, component_id: &str) -> Result<(), String> {
    match component_id {
        "ffmpeg" | "tesseract" => bundled::setup_bundled_bins(app),
        "python-runtime" => {
            // Python runtime extraction is a no-op in this
            // build — the .dmg-shipped tarball is unpacked by
            // a future installer step. Returning Ok here keeps
            // the install pipeline moving so the UI shows the
            // step transition correctly.
            log::info!("python-runtime: extraction step not yet wired (Sprint 12 follow-up)");
            Ok(())
        }
        other => {
            log::warn!("setup_bundled_component called with non-bundled id: {other}");
            Ok(())
        }
    }
}

pub async fn install_one(
    app: &AppHandle,
    component: &InstallComponent,
) -> Result<(), String> {
    if component.is_bundled {
        return setup_bundled_component(app, component.id).await;
    }
    let Some(url) = component.download_url else {
        // Components like `pyannote` have no direct URL (HF
        // token gated). Skip cleanly with an informational log.
        log::info!(
            "skipping {} — no direct download URL (token-gated or external setup)",
            component.id
        );
        return Ok(());
    };
    let dest = model_path_for(app, component.id)?;
    if !download::should_download(&dest, component.size_bytes).await {
        log::info!("{} already present at {dest:?}", component.id);
        return Ok(());
    }
    download::download_component(app, component.id, url, &dest, component.size_bytes).await
}

#[derive(Debug, Serialize)]
struct InstallManifest {
    profile: String,
    augur_installer_version: &'static str,
    components: Vec<&'static str>,
}

pub async fn write_manifest(app: &AppHandle, profile: &Profile) -> Result<(), String> {
    let comps = profiles::components_for_profile(profile);
    let manifest = InstallManifest {
        profile: format!("{profile:?}").to_lowercase(),
        augur_installer_version: env!("CARGO_PKG_VERSION"),
        components: comps.iter().map(|c| c.id).collect(),
    };
    let support = python::augur_support_dir(app)?;
    tokio::fs::create_dir_all(&support)
        .await
        .map_err(|e| format!("mkdir support dir: {e}"))?;
    let path = support.join("INSTALL_MANIFEST.json");
    let body = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("manifest serialize: {e}"))?;
    tokio::fs::write(&path, body)
        .await
        .map_err(|e| format!("write manifest {path:?}: {e}"))?;
    Ok(())
}
