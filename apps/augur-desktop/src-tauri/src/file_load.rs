//! Sprint 12 P6 — file open + type detection.

use std::path::Path;

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[derive(Debug, Serialize)]
pub struct LoadedFile {
    pub path: String,
    pub name: String,
    pub kind: &'static str,
    pub size_bytes: u64,
}

#[tauri::command]
pub async fn open_directory_dialog(app: AppHandle) -> Result<Option<String>, String> {
    let path = app.dialog().file().blocking_pick_folder();
    Ok(path.map(|p| p.to_string()))
}

#[tauri::command]
pub async fn open_evidence_dialog(app: AppHandle) -> Result<Option<String>, String> {
    let path = app
        .dialog()
        .file()
        .add_filter(
            "Evidence files",
            &[
                "mp3", "mp4", "wav", "m4a", "aac", "ogg", "flac", "mov", "avi", "mkv", "webm",
                "pdf", "txt", "md", "doc", "docx", "png", "jpg", "jpeg", "tiff", "srt", "vtt",
            ],
        )
        .blocking_pick_file();
    Ok(path.map(|p| p.to_string()))
}

#[tauri::command]
pub async fn detect_file_type(path: String) -> String {
    detect_kind(Path::new(&path)).to_string()
}

#[tauri::command]
pub async fn load_file_metadata(path: String) -> Result<LoadedFile, String> {
    let p = Path::new(&path);
    let metadata = std::fs::metadata(p).map_err(|e| format!("could not stat {path}: {e}"))?;
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("(unnamed)")
        .to_string();
    Ok(LoadedFile {
        path: path.clone(),
        name,
        kind: detect_kind(p),
        size_bytes: metadata.len(),
    })
}

fn detect_kind(p: &Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("mp3") | Some("wav") | Some("m4a") | Some("aac") | Some("ogg") | Some("flac") => {
            "audio"
        }
        Some("mp4") | Some("mov") | Some("avi") | Some("mkv") | Some("webm") | Some("m4v")
        | Some("3gp") => "video",
        Some("srt") | Some("vtt") => "subtitle",
        Some("png") | Some("jpg") | Some("jpeg") | Some("tiff") | Some("bmp") | Some("gif") => {
            "image"
        }
        _ => "document",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_kind_audio() {
        assert_eq!(detect_kind(Path::new("foo.MP3")), "audio");
        assert_eq!(detect_kind(Path::new("foo.wav")), "audio");
    }

    #[test]
    fn detect_kind_video() {
        assert_eq!(detect_kind(Path::new("foo.mp4")), "video");
        assert_eq!(detect_kind(Path::new("foo.MOV")), "video");
    }

    #[test]
    fn detect_kind_subtitle() {
        assert_eq!(detect_kind(Path::new("foo.srt")), "subtitle");
        assert_eq!(detect_kind(Path::new("foo.vtt")), "subtitle");
    }

    #[test]
    fn detect_kind_document_default() {
        assert_eq!(detect_kind(Path::new("foo.pdf")), "document");
        assert_eq!(detect_kind(Path::new("foo.txt")), "document");
        assert_eq!(detect_kind(Path::new("foo.unknown")), "document");
    }
}
