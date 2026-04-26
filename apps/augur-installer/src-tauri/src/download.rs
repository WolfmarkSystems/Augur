//! Sprint 11 P2 — model download with live progress events.
//!
//! Streams the response body chunk-by-chunk, writes to the
//! destination path, and emits `download-progress` Tauri events
//! to the frontend per chunk. SHA-256 verification + resume
//! support included.

use std::path::Path;

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub component_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub percent: f32,
    pub speed_mbps: f32,
    pub eta_seconds: u64,
}

pub async fn download_component(
    app: &AppHandle,
    component_id: &str,
    url: &str,
    dest_path: &Path,
    expected_bytes: u64,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| format!("client build failed: {e}"))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    let total = response.content_length().unwrap_or(expected_bytes);

    if let Some(parent) = dest_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("mkdir {parent:?}: {e}"))?;
    }

    let mut file = tokio::fs::File::create(dest_path)
        .await
        .map_err(|e| format!("create {dest_path:?}: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    let start = std::time::Instant::now();

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.map_err(|e| format!("stream error: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("write error: {e}"))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);

        let elapsed = start.elapsed().as_secs_f32();
        let speed_mbps = if elapsed > 0.0 {
            (downloaded as f32 / elapsed) / 1_000_000.0
        } else {
            0.0
        };
        let eta = if speed_mbps > 0.0 && downloaded < total {
            ((total - downloaded) as f32 / (speed_mbps * 1_000_000.0)) as u64
        } else {
            0
        };
        let percent = if total > 0 {
            (downloaded as f32 / total as f32) * 100.0
        } else {
            0.0
        };
        let progress = DownloadProgress {
            component_id: component_id.to_string(),
            bytes_downloaded: downloaded,
            total_bytes: total,
            percent,
            speed_mbps,
            eta_seconds: eta,
        };
        // Best-effort emit; never fail the download because the
        // UI lost a frame.
        let _ = app.emit("download-progress", progress);
    }

    file.flush()
        .await
        .map_err(|e| format!("flush error: {e}"))?;
    Ok(())
}

/// SHA-256 verification. Empty `expected` is a documented
/// "no checksum yet" state — returns `Ok(true)` without
/// touching the file.
pub async fn verify_sha256(path: &Path, expected: &str) -> Result<bool, String> {
    if expected.is_empty() {
        return Ok(true);
    }
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| format!("open {path:?}: {e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file.read(&mut buf).await.map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok(hex == expected)
}

/// Returns `true` when the file is missing or has the wrong size,
/// meaning the installer should re-download. Already-complete
/// files are skipped automatically — supports interrupted-and-
/// restarted installs.
pub async fn should_download(dest_path: &Path, expected_bytes: u64) -> bool {
    match tokio::fs::metadata(dest_path).await {
        Ok(meta) => meta.len() != expected_bytes,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn should_download_returns_true_for_missing_file() {
        let p = std::env::temp_dir().join(format!(
            "augur-installer-missing-{}",
            std::process::id()
        ));
        let _ = tokio::fs::remove_file(&p).await;
        assert!(should_download(&p, 1_000).await);
    }

    #[tokio::test]
    async fn should_download_returns_false_when_size_matches() {
        let p = std::env::temp_dir().join(format!(
            "augur-installer-existing-{}",
            std::process::id()
        ));
        tokio::fs::write(&p, b"hello world").await.unwrap();
        assert!(!should_download(&p, 11).await);
        let _ = tokio::fs::remove_file(&p).await;
    }

    #[tokio::test]
    async fn verify_sha256_skips_when_expected_empty() {
        let p = std::env::temp_dir().join(format!(
            "augur-installer-sha-empty-{}",
            std::process::id()
        ));
        tokio::fs::write(&p, b"x").await.unwrap();
        assert!(verify_sha256(&p, "").await.unwrap());
        let _ = tokio::fs::remove_file(&p).await;
    }
}
