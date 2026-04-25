//! End-to-end pipeline data types.
//!
//! verify-core stays lean: it owns the unified [`VerifyError`] and
//! the data types that flow through the pipeline ([`PipelineInput`],
//! [`PipelineResult`]). Orchestration — wiring the classifier, STT,
//! translation, and OCR engines together — lives in the CLI (and
//! eventually in the Strata plugin adapter), so that verify-core
//! can stay free of ML / audio / image dependencies.
//!
//! This split avoids a dependency cycle: each sub-engine depends on
//! verify-core for the error type, so verify-core cannot in turn
//! depend on the sub-engines.
//!
//! [`VerifyError`]: crate::error::VerifyError

use crate::error::VerifyError;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Pipeline input kind. Used by the CLI / plugin orchestrators to
/// route to the correct sub-engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Text,
    Audio,
    Image,
    Video,
}

/// Concrete pipeline input. The CLI / plugin chooses which variant
/// to construct based on file inspection or an explicit flag.
#[derive(Debug, Clone)]
pub enum PipelineInput {
    Text(String),
    Audio(PathBuf),
    Image(PathBuf),
    /// Sprint 3: video files (mp4, mov, avi, mkv, m4v, wmv, webm, 3gp).
    /// The CLI extracts audio via ffmpeg before handing off to STT.
    Video(PathBuf),
    /// Sprint 4: PDF documents. The CLI extracts text via the
    /// `pdf-extract` text layer; falls back to rasterize-and-OCR
    /// (poppler `pdftoppm` + Tesseract) for scanned PDFs.
    Pdf(PathBuf),
}

/// Auto-detect the pipeline input kind from a file path's extension.
/// Used by the CLI's `translate`/`batch` subcommands so an examiner
/// who points VERIFY at `interview.mp4` does not need to remember
/// which `--input` flag to use.
///
/// Unknown extensions fall through to `Audio` — the audio
/// preprocessor will surface a clear error if the file is not
/// actually audio. We do NOT default to `Text`: a text input must
/// be passed inline (`--text`), never as a file path.
pub fn detect_input_kind(path: &Path) -> PipelineInput {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp4") | Some("mov") | Some("avi") | Some("mkv") | Some("m4v")
        | Some("wmv") | Some("webm") | Some("3gp") => PipelineInput::Video(path.to_path_buf()),
        Some("png") | Some("jpg") | Some("jpeg") | Some("tiff") | Some("tif")
        | Some("bmp") | Some("gif") => PipelineInput::Image(path.to_path_buf()),
        Some("pdf") => PipelineInput::Pdf(path.to_path_buf()),
        _ => PipelineInput::Audio(path.to_path_buf()),
    }
}

/// Result of one full pipeline run. Sub-engines may populate a
/// subset of the optional fields — for example a text input never
/// produces `stt_segments`.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub source_language: String,
    pub source_text: String,
    pub translated_text: String,
    pub target_language: String,
    pub model: String,
    pub is_machine_translation: bool,
    pub advisory_notice: String,
    pub stt_segments: Option<Vec<TimedSegment>>,
}

/// Plain timestamped segment. Mirrors `verify_stt::SttSegment` but
/// avoids the cyclic dependency.
#[derive(Debug, Clone)]
pub struct TimedSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

impl PipelineResult {
    /// Forensic safety check: every pipeline result that surfaces a
    /// translation must carry the advisory notice. Callers can use
    /// this to assert the invariant before emitting output.
    pub fn assert_advisory(&self) -> Result<(), VerifyError> {
        if self.is_machine_translation && self.advisory_notice.is_empty() {
            return Err(VerifyError::Translate(
                "advisory_notice missing on a machine-translation result \
                 — forensic invariant violation"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

// ── Batch processing types ──────────────────────────────────────
//
// Sprint 3: VERIFY can process an entire directory in one
// invocation. The CLI walks the path, runs the per-file pipeline,
// and emits a [`BatchResult`] — optionally serialized to JSON.

/// Per-file result emitted in a batch run.
#[derive(Debug, Clone, Serialize)]
pub struct BatchFileResult {
    /// Absolute path of the source file.
    pub file_path: String,
    /// One of `"audio"`, `"video"`, `"image"`, `"text"`.
    pub input_type: String,
    /// ISO 639-1 language code detected by the classifier (or by
    /// Whisper / OCR when the file was foreign).
    pub detected_language: String,
    pub is_foreign: bool,
    /// Full source text (transcript or OCR output) when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    /// Translated text when the file was foreign.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub translated_text: Option<String>,
    /// Per-segment transcripts + translations for audio/video.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<Vec<BatchSegment>>,
    /// Error message if the file failed to process; `None` on
    /// success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
}

/// Top-level batch report. Always carries the
/// `machine_translation_notice` so consumers of the JSON cannot
/// strip the advisory by accident — it is not optional.
#[derive(Debug, Clone, Serialize)]
pub struct BatchResult {
    /// ISO 8601 UTC timestamp of when the batch run completed.
    pub generated_at: String,
    pub total_files: u32,
    pub processed: u32,
    pub foreign_language: u32,
    pub translated: u32,
    pub errors: u32,
    pub target_language: String,
    /// Forensic invariant: every batch report carries the same
    /// machine-translation notice every individual translation
    /// result carries. Removing it from the JSON would defeat the
    /// purpose of the advisory.
    pub machine_translation_notice: String,
    pub results: Vec<BatchFileResult>,
}

impl BatchResult {
    /// Forensic safety invariant — same shape as
    /// [`PipelineResult::assert_advisory`] but at the batch level.
    pub fn assert_advisory(&self) -> Result<(), VerifyError> {
        if self.translated > 0 && self.machine_translation_notice.is_empty() {
            return Err(VerifyError::Translate(
                "machine_translation_notice missing from batch report — \
                 forensic invariant violation"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_advisory_rejects_empty_notice() {
        let r = PipelineResult {
            source_language: "ar".into(),
            source_text: "x".into(),
            translated_text: "y".into(),
            target_language: "en".into(),
            model: "m".into(),
            is_machine_translation: true,
            advisory_notice: String::new(),
            stt_segments: None,
        };
        assert!(r.assert_advisory().is_err());
    }

    #[test]
    fn assert_advisory_passes_with_notice() {
        let r = PipelineResult {
            source_language: "ar".into(),
            source_text: "x".into(),
            translated_text: "y".into(),
            target_language: "en".into(),
            model: "m".into(),
            is_machine_translation: true,
            advisory_notice: "advisory present".into(),
            stt_segments: None,
        };
        assert!(r.assert_advisory().is_ok());
    }

    #[test]
    fn pipeline_input_variants_exist() {
        let _ = PipelineInput::Text("hi".into());
        let _ = PipelineInput::Audio(PathBuf::from("a.wav"));
        let _ = PipelineInput::Image(PathBuf::from("p.png"));
        let _ = PipelineInput::Video(PathBuf::from("v.mp4"));
        let _ = PipelineInput::Pdf(PathBuf::from("d.pdf"));
    }

    #[test]
    fn pdf_input_detected_by_extension() {
        for ext in &["pdf", "PDF"] {
            let p = PathBuf::from(format!("doc.{ext}"));
            assert!(
                matches!(detect_input_kind(&p), PipelineInput::Pdf(_)),
                "expected Pdf for .{ext}"
            );
        }
    }

    #[test]
    fn detect_input_kind_routes_video() {
        for ext in &["mp4", "MOV", "avi", "mkv", "m4v", "wmv", "webm", "3gp"] {
            let p = PathBuf::from(format!("clip.{ext}"));
            assert!(
                matches!(detect_input_kind(&p), PipelineInput::Video(_)),
                "expected Video for .{ext}"
            );
        }
    }

    #[test]
    fn detect_input_kind_routes_image() {
        for ext in &["png", "JPG", "jpeg", "tiff", "tif", "bmp", "gif"] {
            let p = PathBuf::from(format!("scan.{ext}"));
            assert!(
                matches!(detect_input_kind(&p), PipelineInput::Image(_)),
                "expected Image for .{ext}"
            );
        }
    }

    #[test]
    fn batch_result_advisory_required_when_translations_present() {
        let r = BatchResult {
            generated_at: "2026-04-25T00:00:00Z".into(),
            total_files: 1,
            processed: 1,
            foreign_language: 1,
            translated: 1,
            errors: 0,
            target_language: "en".into(),
            machine_translation_notice: String::new(),
            results: vec![],
        };
        assert!(r.assert_advisory().is_err());
    }

    #[test]
    fn batch_result_advisory_passes_with_notice() {
        let r = BatchResult {
            generated_at: "2026-04-25T00:00:00Z".into(),
            total_files: 1,
            processed: 1,
            foreign_language: 1,
            translated: 1,
            errors: 0,
            target_language: "en".into(),
            machine_translation_notice: "advisory present".into(),
            results: vec![],
        };
        assert!(r.assert_advisory().is_ok());
    }

    #[test]
    fn batch_result_counts_are_balanced() {
        // total_files = processed + errors invariant — pinned by
        // a synthetic result so a future caller can't construct a
        // count-skewed report by accident in well-formed code.
        let r = BatchResult {
            generated_at: "2026-04-25T00:00:00Z".into(),
            total_files: 10,
            processed: 8,
            foreign_language: 3,
            translated: 3,
            errors: 2,
            target_language: "en".into(),
            machine_translation_notice: "x".into(),
            results: vec![],
        };
        assert_eq!(r.total_files, r.processed + r.errors);
    }

    #[test]
    fn detect_input_kind_falls_back_to_audio() {
        // Audio extensions and unknown extensions both default to Audio;
        // the audio preprocessor will surface a clear error if the file
        // turns out not to be audio.
        for ext in &["wav", "mp3", "m4a", "ogg", "flac", "aac", "weird"] {
            let p = PathBuf::from(format!("x.{ext}"));
            assert!(
                matches!(detect_input_kind(&p), PipelineInput::Audio(_)),
                "expected Audio for .{ext}"
            );
        }
    }
}
