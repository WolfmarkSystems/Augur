//! Sprint 12 P5 — model status reporting for the Models menu.
//!
//! Mirrors the catalog the installer ships and probes
//! `~/Library/Application Support/AUGUR/models/` for what's
//! actually present on disk. The desktop app does not download
//! models itself — that's the installer's job — so this file
//! only reads.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};

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
