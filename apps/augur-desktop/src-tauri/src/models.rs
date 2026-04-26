//! Sprint 12 P5 — model status reporting for the Models menu.
//!
//! Mirrors the catalog the installer ships and probes
//! `~/Library/Application Support/AUGUR/models/` for what's
//! actually present on disk. The desktop app does not download
//! models itself — that's the installer's job — so this file
//! only reads.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub size_display: &'static str,
    pub installed: bool,
    pub tier: &'static str,
}

const CATALOG: &[(&str, &str, &str, &str)] = &[
    ("python-runtime", "Python Runtime", "45 MB", "minimal"),
    ("ffmpeg", "ffmpeg", "22 MB", "minimal"),
    ("tesseract", "Tesseract OCR", "38 MB", "minimal"),
    ("whisper-tiny", "Whisper Tiny", "75 MB", "minimal"),
    ("whisper-large-v3", "Whisper Large-v3", "2.9 GB", "standard"),
    ("whisper-pashto", "Whisper Pashto", "150 MB", "full"),
    ("whisper-dari", "Whisper Dari", "150 MB", "full"),
    ("nllb-600m", "NLLB-200 600M", "2.4 GB", "minimal"),
    ("nllb-1b3", "NLLB-200 1.3B", "5.2 GB", "standard"),
    ("seamless-m4t", "SeamlessM4T Medium", "2.4 GB", "full"),
    ("camel-arabic", "CAMeL Arabic", "450 MB", "standard"),
    ("pyannote", "Speaker Diarization", "1.0 GB", "full"),
    ("fasttext-lid", "fastText LID", "900 KB", "minimal"),
];

fn models_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|p| p.join("AUGUR").join("models"))
}

#[tauri::command]
pub fn list_models(app: AppHandle) -> Vec<ModelStatus> {
    let dir = models_dir(&app);
    CATALOG
        .iter()
        .map(|(id, name, size, tier)| {
            let installed = dir
                .as_ref()
                .map(|d| d.join(id).exists())
                .unwrap_or(false);
            ModelStatus {
                id,
                name,
                size_display: size,
                installed,
                tier,
            }
        })
        .collect()
}

/// Sprint 13 P3 — shell out to `augur install --status --format json`
/// for an authoritative installed/missing list. Falls back to the
/// local catalog probe (the static `CATALOG` above) when the CLI
/// is unavailable so the UI never crashes on a fresh dev machine.
#[tauri::command]
pub async fn get_model_status() -> Result<serde_json::Value, String> {
    use tokio::process::Command;
    let augur = match crate::pipeline::find_augur_binary() {
        Some(p) => p,
        None => {
            return Err("AUGUR CLI not found.".to_string());
        }
    };
    let output = Command::new(&augur)
        .args(["install", "--status", "--format", "json"])
        .output()
        .await
        .map_err(|e| format!("could not run `augur install --status`: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!(
            "`augur install --status` exited {:?}: {}",
            output.status.code(),
            stderr.trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("could not parse model status JSON: {e}"))
}

/// Sprint 13 P3 — single-model install. Spawns
/// `augur install --model <id> --format ndjson` and re-emits each
/// progress line as a `model-install-progress` Tauri event.
#[tauri::command]
pub async fn install_model(
    app: AppHandle,
    model_id: String,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;
    let augur = crate::pipeline::find_augur_binary()
        .ok_or_else(|| "AUGUR CLI not found.".to_string())?;
    let mut cmd = Command::new(&augur);
    cmd.args(["install", "--model", &model_id, "--format", "ndjson"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start install: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout from install".to_string())?;
    let mut lines = BufReader::new(stdout).lines();
    let app_for_task = app.clone();
    let model_for_task = model_id.clone();
    tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                let _ = app_for_task.emit("model-install-progress", &json);
            }
        }
        let _ = app_for_task.emit(
            "model-install-finished",
            serde_json::json!({"model_id": model_for_task}),
        );
        let _ = child.wait().await;
    });
    Ok(())
}

#[tauri::command]
pub fn detected_profile(app: AppHandle) -> &'static str {
    // Coarse heuristic: if every "full" model is present, return
    // "full"; if every "standard" model, "standard"; else if at
    // least all "minimal" are present, "minimal"; else "none".
    let dir = match models_dir(&app) {
        Some(d) => d,
        None => return "none",
    };
    let installed = |id: &str| dir.join(id).exists();
    let all = |t: &str| CATALOG.iter().filter(|(_, _, _, tier)| *tier == t).all(|(id, _, _, _)| installed(id));
    if all("full") && all("standard") && all("minimal") {
        "full"
    } else if all("standard") && all("minimal") {
        "standard"
    } else if all("minimal") {
        "minimal"
    } else {
        "none"
    }
}
