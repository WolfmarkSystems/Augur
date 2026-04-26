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
    /// One of `"audio"`, `"video"`, `"image"`, `"pdf"`, `"text"`.
    pub input_type: String,
    /// ISO 639-1 language code detected by the classifier (or by
    /// Whisper / OCR when the file was foreign).
    pub detected_language: String,
    pub is_foreign: bool,
    /// Sprint 6 P2 — categorical confidence band ("HIGH" /
    /// "MEDIUM" / "LOW"). Empty string when the file errored
    /// before classification could run.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub confidence_tier: String,
    /// Sprint 6 P2 — human-readable confidence advisory.
    /// Populated when `confidence_tier` is "MEDIUM" or "LOW".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence_advisory: Option<String>,
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

/// Aggregate statistics for a batch run. Sprint 6 P1 — gives
/// examiners a quick, at-a-glance summary of what came out of the
/// run plus the breakdown of which languages were detected. Always
/// carries `machine_translation_notice` so the advisory survives
/// even when consumers downsample the report to just the summary.
#[derive(Debug, Clone, Serialize)]
pub struct BatchSummary {
    pub total_files: u32,
    pub processed: u32,
    pub foreign_language_files: u32,
    pub translated_files: u32,
    pub errors: u32,
    /// Map of detected ISO 639-1 language code → file count.
    /// Files with no detected language (empty input) are excluded.
    pub languages_detected: std::collections::BTreeMap<String, u32>,
    /// Wall-clock duration of the batch run, seconds.
    pub processing_time_seconds: f64,
    /// Forensic invariant — same advisory the per-file
    /// `TranslationResult`s carry, restated at the summary level
    /// so a consumer reading only the summary still sees it.
    pub machine_translation_notice: String,
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
    /// Sprint 6 P1 — aggregate stats. Optional in the JSON so
    /// older consumers parsing pre-Sprint-6 reports still work.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<BatchSummary>,
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
        if let Some(s) = &self.summary {
            if s.translated_files > 0 && s.machine_translation_notice.is_empty() {
                return Err(VerifyError::Translate(
                    "BatchSummary.machine_translation_notice missing — \
                     forensic invariant violation"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Build a [`BatchSummary`] from `self` plus the wall-clock
    /// duration. Counts each detected language once per file —
    /// zero-length transcripts (empty audio, blank PDFs) are not
    /// counted.
    pub fn build_summary(&self, processing_time_seconds: f64, mt_notice: &str) -> BatchSummary {
        let mut languages_detected: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for r in &self.results {
            if r.detected_language.is_empty() {
                continue;
            }
            *languages_detected
                .entry(r.detected_language.clone())
                .or_insert(0) += 1;
        }
        BatchSummary {
            total_files: self.total_files,
            processed: self.processed,
            foreign_language_files: self.foreign_language,
            translated_files: self.translated,
            errors: self.errors,
            languages_detected,
            processing_time_seconds,
            machine_translation_notice: mt_notice.to_string(),
        }
    }
}

/// CSV-row form of [`BatchFileResult`]. Used by the CLI's
/// `--output report.csv` path. Fields match the spec column
/// order; embedded quotes / newlines / commas are escaped per
/// RFC 4180 by [`render_csv_row`].
#[derive(Debug, Clone)]
pub struct BatchCsvRow<'a> {
    pub file_path: &'a str,
    pub input_type: &'a str,
    pub detected_language: &'a str,
    pub is_foreign: bool,
    pub transcript: &'a str,
    pub translation: &'a str,
    pub error: &'a str,
}

/// CSV header row in the order spec'd by VERIFY_SPRINT_6.md P1a.
pub const BATCH_CSV_HEADER: &str =
    "file_path,input_type,detected_language,is_foreign,transcript,translation,error";

/// Render one CSV row with RFC-4180-compatible escaping. Fields
/// containing `,`, `"`, `\r`, or `\n` get wrapped in quotes; any
/// embedded `"` is doubled.
pub fn render_csv_row(row: &BatchCsvRow<'_>) -> String {
    fn esc(s: &str) -> String {
        if s.is_empty() {
            return String::new();
        }
        if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
            let escaped = s.replace('"', "\"\"");
            format!("\"{escaped}\"")
        } else {
            s.to_string()
        }
    }
    format!(
        "{},{},{},{},{},{},{}",
        esc(row.file_path),
        esc(row.input_type),
        esc(row.detected_language),
        if row.is_foreign { "true" } else { "false" },
        esc(row.transcript),
        esc(row.translation),
        esc(row.error),
    )
}

/// Serialize the whole [`BatchResult`] to CSV text — header line
/// plus one row per file. Errors propagated from individual rows
/// land in the `error` column.
pub fn render_batch_csv(report: &BatchResult) -> String {
    let mut out = String::with_capacity(BATCH_CSV_HEADER.len() + report.results.len() * 80);
    out.push_str(BATCH_CSV_HEADER);
    out.push('\n');
    for r in &report.results {
        let row = BatchCsvRow {
            file_path: &r.file_path,
            input_type: &r.input_type,
            detected_language: &r.detected_language,
            is_foreign: r.is_foreign,
            transcript: r.source_text.as_deref().unwrap_or(""),
            translation: r.translated_text.as_deref().unwrap_or(""),
            error: r.error.as_deref().unwrap_or(""),
        };
        out.push_str(&render_csv_row(&row));
        out.push('\n');
    }
    out
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

    fn make_row(
        path: &str,
        lang: &str,
        is_foreign: bool,
        src: Option<&str>,
        tx: Option<&str>,
        err: Option<&str>,
    ) -> BatchFileResult {
        BatchFileResult {
            file_path: path.into(),
            input_type: "audio".into(),
            detected_language: lang.into(),
            is_foreign,
            confidence_tier: if lang.is_empty() {
                String::new()
            } else {
                "HIGH".into()
            },
            confidence_advisory: None,
            source_text: src.map(str::to_string),
            translated_text: tx.map(str::to_string),
            segments: None,
            error: err.map(str::to_string),
        }
    }

    fn fixture_with_results(translated: u32) -> BatchResult {
        let r1 = make_row(
            "/ev/a.mp3",
            "ar",
            true,
            Some("مرحبا بالعالم"),
            Some("Hello world"),
            None,
        );
        let r2 = make_row(
            "/ev/b.mp3",
            "ar",
            true,
            Some("نص آخر"),
            Some("More text"),
            None,
        );
        let r3 = make_row(
            "/ev/c.mp3",
            "zh",
            true,
            Some("你好世界"),
            Some("Hello world"),
            None,
        );
        let r4 = make_row(
            "/ev/d.mp3",
            "en",
            false,
            Some("English text here"),
            None,
            None,
        );
        let r5 = make_row("/ev/e.mp3", "", false, None, None, Some("decoding failed"));
        BatchResult {
            generated_at: "2026-04-26T00:00:00Z".into(),
            total_files: 5,
            processed: 4,
            foreign_language: 3,
            translated,
            errors: 1,
            target_language: "en".into(),
            machine_translation_notice: "Machine translation — verify.".into(),
            results: vec![r1, r2, r3, r4, r5],
            summary: None,
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
            summary: None,
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
            summary: None,
        };
        assert!(r.assert_advisory().is_ok());
    }

    #[test]
    fn batch_result_counts_are_balanced() {
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
            summary: None,
        };
        assert_eq!(r.total_files, r.processed + r.errors);
    }

    #[test]
    fn batch_csv_output_has_correct_headers() {
        let r = fixture_with_results(3);
        let csv = render_batch_csv(&r);
        let first_line = csv.lines().next().expect("at least one line");
        assert_eq!(first_line, BATCH_CSV_HEADER);
        // Spot-check a translated row makes it through.
        assert!(csv.contains("Hello world"));
        // Boolean is_foreign rendered as `true`/`false`, not `True`/`1`.
        assert!(csv.contains(",true,"));
        assert!(csv.contains(",false,"));
    }

    #[test]
    fn batch_csv_escapes_commas_and_quotes() {
        let row = BatchCsvRow {
            file_path: "/ev/a.mp3",
            input_type: "audio",
            detected_language: "en",
            is_foreign: false,
            transcript: "He said \"hi, friend\" today",
            translation: "",
            error: "",
        };
        let line = render_csv_row(&row);
        // The transcript field contains both a quote and a comma →
        // gets RFC-4180-quoted with embedded `"` doubled.
        assert!(line.contains("\"He said \"\"hi, friend\"\" today\""));
    }

    #[test]
    fn batch_summary_languages_counts_correctly() {
        let r = fixture_with_results(3);
        let s = r.build_summary(1.5, &r.machine_translation_notice);
        assert_eq!(s.languages_detected.get("ar"), Some(&2));
        assert_eq!(s.languages_detected.get("zh"), Some(&1));
        assert_eq!(s.languages_detected.get("en"), Some(&1));
        // The errored file had no detected language → not counted.
        assert!(!s.languages_detected.contains_key(""));
        assert_eq!(s.translated_files, 3);
        assert_eq!(s.foreign_language_files, 3);
    }

    #[test]
    fn batch_summary_machine_translation_notice_present() {
        let r = fixture_with_results(3);
        let s = r.build_summary(0.0, &r.machine_translation_notice);
        assert!(!s.machine_translation_notice.is_empty());
    }

    #[test]
    fn batch_advisory_rejects_summary_with_empty_notice() {
        // Forensic invariant — even when the top-level field is
        // populated, an embedded summary missing the notice must
        // fail assert_advisory.
        let mut r = fixture_with_results(3);
        let mut s = r.build_summary(0.0, &r.machine_translation_notice);
        s.machine_translation_notice = String::new();
        r.summary = Some(s);
        assert!(r.assert_advisory().is_err());
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
