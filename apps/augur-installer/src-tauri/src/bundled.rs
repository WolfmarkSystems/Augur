//! Sprint 11 P3 — bundled binaries (ffmpeg, tesseract).
//!
//! Resolved relative to the Tauri resource directory. The
//! installer .dmg ships these binaries; on first run we set
//! the executable bit and they live alongside the model cache.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

pub fn ffmpeg_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resource_dir()
        .ok()
        .map(|p| p.join("resources").join("ffmpeg"))
}

pub fn tesseract_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resource_dir()
        .ok()
        .map(|p| p.join("resources").join("tesseract").join("tesseract"))
}

/// Set the executable bit on bundled binaries. macOS resource
/// extraction loses the +x bit; this is the canonical fix.
/// Missing files are tolerated — the installer logs and
/// proceeds (some tier choices skip these components).
#[cfg(unix)]
pub fn setup_bundled_bins(app: &AppHandle) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let candidates: Vec<PathBuf> = [ffmpeg_path(app), tesseract_path(app)]
        .into_iter()
        .flatten()
        .collect();
    for path in candidates {
        if path.exists() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("chmod {path:?}: {e}"))?;
        } else {
            log::warn!("bundled binary not present: {path:?}");
        }
    }
    Ok(())
}

#[cfg(not(unix))]
pub fn setup_bundled_bins(_app: &AppHandle) -> Result<(), String> {
    // Windows uses the file extension, not a permission bit.
    Ok(())
}
