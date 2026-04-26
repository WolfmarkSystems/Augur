//! Sprint 11 P3 — embedded Python runtime + pip dependency
//! installation.
//!
//! AUGUR's translation/diarization workers run inside a bundled
//! Python interpreter so an examiner doesn't need to install
//! Homebrew Python or fight virtualenvs. The Python tarball is
//! shipped inside the .dmg under `resources/python/`; on first
//! run we extract it to `~/Library/Application Support/AUGUR/python/`
//! and `pip install` the four model dep packages.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Top-level AUGUR support directory under the user's app-data
/// root. Same location the main `augur` CLI looks at.
///
/// macOS:   `~/Library/Application Support/AUGUR/`
/// Linux:   `~/.local/share/AUGUR/`
/// Windows: `%APPDATA%\AUGUR\`
pub fn augur_support_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|p| p.join("AUGUR"))
        .map_err(|e| format!("app_data_dir lookup failed: {e}"))
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    augur_support_dir(app).map(|p| p.join("models"))
}

pub fn python_dir(app: &AppHandle) -> Result<PathBuf, String> {
    augur_support_dir(app).map(|p| p.join("python"))
}

pub fn python_executable(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = python_dir(app)?;
    Ok(dir.join("bin").join("python3"))
}

pub fn is_python_ready(app: &AppHandle) -> bool {
    matches!(python_executable(app), Ok(p) if p.exists())
}

/// Concrete location an installed component lands at. Bundled
/// binaries land alongside the model cache under `models/<id>`
/// even though they aren't models — keeps the manifest simple.
pub fn model_path_for(app: &AppHandle, component_id: &str) -> Result<PathBuf, String> {
    models_dir(app).map(|m| m.join(component_id))
}

/// `pip install` the AUGUR Python deps into the bundled runtime.
/// Each package install emits a progress callback so the UI can
/// show "Installing transformers…" etc.
pub async fn install_pip_packages<F>(app: &AppHandle, on_progress: F) -> Result<(), String>
where
    F: Fn(String),
{
    let python = python_executable(app)?;
    let packages = [
        "transformers>=4.35.0",
        "ctranslate2>=3.20.0",
        "torch>=2.1.0",
        "torchaudio>=2.1.0",
        "sentencepiece>=0.1.99",
    ];
    for package in packages {
        on_progress(format!("Installing {package}…"));
        let status = tokio::process::Command::new(&python)
            .args(["-m", "pip", "install", "--quiet", package])
            .status()
            .await
            .map_err(|e| format!("pip spawn failed: {e}"))?;
        if !status.success() {
            return Err(format!(
                "Failed to install {package} (pip exited with {status})"
            ));
        }
    }
    Ok(())
}
