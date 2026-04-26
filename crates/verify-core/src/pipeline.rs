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
    /// Super Sprint Group B: timestamped subtitles (.srt / .vtt).
    /// Parser is in `verify_core::subtitle`; the CLI translates
    /// per cue, preserving timestamps.
    Subtitle(PathBuf),
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
        Some("srt") | Some("vtt") => PipelineInput::Subtitle(path.to_path_buf()),
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

/// Read up to `n` bytes from the start of `path`. Returns
/// `None` on any I/O error so the caller can fall back to
/// extension-based detection without surfacing the read error
/// (the extension answer is still useful even when the file is
/// unreadable for a moment).
fn read_magic_bytes(path: &Path, n: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; n];
    let read = f.read(&mut buf).ok()?;
    buf.truncate(read);
    Some(buf)
}

pub fn is_pdf_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}

/// MP4 / QuickTime: bytes 4..8 are `ftyp`. Used for both `.mp4`
/// and `.mov` (the variants differ by the brand bytes that
/// follow `ftyp`, which we don't inspect).
pub fn is_mp4_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[4..8] == b"ftyp"
}

pub fn is_wav_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE"
}

/// MP3: either `ID3` tag header or an MPEG audio sync word
/// (0xFF 0xFB / 0xFF 0xF3 / 0xFF 0xF2).
pub fn is_mp3_magic(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"ID3") {
        return true;
    }
    bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0
}

pub fn is_jpeg_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

pub fn is_png_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

pub fn is_zip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

pub fn is_gzip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x1F, 0x8B])
}

/// Super Sprint Group D P7 — content-aware input detection.
/// Calls [`detect_input_kind`] first (extension-based), then
/// probes the file's first 16 bytes to correct
/// wrong-or-missing-extension cases. Falls back to the
/// extension answer on any I/O error so this never panics.
pub fn detect_input_kind_robust(path: &Path) -> PipelineInput {
    let by_extension = detect_input_kind(path);
    let Some(magic) = read_magic_bytes(path, 16) else {
        return by_extension;
    };
    if is_pdf_magic(&magic) {
        return PipelineInput::Pdf(path.to_path_buf());
    }
    if is_mp4_magic(&magic) {
        return PipelineInput::Video(path.to_path_buf());
    }
    if is_jpeg_magic(&magic) || is_png_magic(&magic) {
        return PipelineInput::Image(path.to_path_buf());
    }
    if is_wav_magic(&magic) || is_mp3_magic(&magic) {
        return PipelineInput::Audio(path.to_path_buf());
    }
    by_extension
}

/// English-language display name for an ISO 639-1 code. Falls
/// back to the code itself when unmapped — the report should
/// degrade gracefully rather than panic on novel codes.
pub fn language_name_for(iso: &str) -> &'static str {
    match iso {
        "ar" => "Arabic",
        "zh" => "Chinese",
        "ru" => "Russian",
        "es" => "Spanish",
        "fr" => "French",
        "de" => "German",
        "ko" => "Korean",
        "ja" => "Japanese",
        "vi" => "Vietnamese",
        "tr" => "Turkish",
        "pt" => "Portuguese",
        "it" => "Italian",
        "nl" => "Dutch",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "id" => "Indonesian",
        "pl" => "Polish",
        "uk" => "Ukrainian",
        "fa" => "Persian (Farsi)",
        "ps" => "Pashto",
        "ur" => "Urdu",
        "en" => "English",
        "sv" => "Swedish",
        "fi" => "Finnish",
        "no" => "Norwegian",
        "da" => "Danish",
        "cs" => "Czech",
        "el" => "Greek",
        "ro" => "Romanian",
        "hu" => "Hungarian",
        "th" => "Thai",
        "bg" => "Bulgarian",
        _ => "(unknown)",
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

/// Sprint 8 P2 — per-language grouping for a multi-language
/// evidence directory. The CLI populates this on top of the
/// flat `results` list whenever `--all-foreign` (or any input
/// produced multiple detected languages); existing consumers
/// reading only `results` continue to work.
#[derive(Debug, Clone, Serialize)]
pub struct LanguageGroup {
    /// ISO 639-1 code (`"ar"`, `"zh"`, …).
    pub language_code: String,
    /// English-language display name (`"Arabic"`, `"Chinese"`, …).
    pub language_name: String,
    pub file_count: u32,
    /// Approximate total source-text word count across this
    /// group's files. Whitespace-tokenized; good enough for an
    /// "amount of evidence" gauge.
    pub total_words: u32,
    pub files: Vec<BatchFileResult>,
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
    /// Sprint 8 P2 — per-language file groupings. Empty when only
    /// one language is detected (or when no foreign files were
    /// processed).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub language_groups: Vec<LanguageGroup>,
    /// Sprint 8 P2 — most common detected foreign language across
    /// the batch. `None` when there are no foreign files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_language: Option<String>,
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

    /// Sprint 8 P2 — populate `language_groups` and
    /// `dominant_language` from the per-file `results`. Idempotent
    /// (overwrites whatever was there). Files with no detected
    /// language are skipped; English (`target_language` matches)
    /// is included so the report shows "ar: 8, zh: 3, en: 5".
    pub fn build_language_groups(&mut self) {
        use std::collections::BTreeMap;
        let mut buckets: BTreeMap<String, Vec<BatchFileResult>> = BTreeMap::new();
        for r in &self.results {
            if r.detected_language.is_empty() {
                continue;
            }
            buckets
                .entry(r.detected_language.clone())
                .or_default()
                .push(r.clone());
        }
        let mut groups: Vec<LanguageGroup> = buckets
            .into_iter()
            .map(|(code, files)| {
                let total_words: u32 = files
                    .iter()
                    .map(|f| {
                        f.source_text
                            .as_deref()
                            .map(|t| t.split_whitespace().count() as u32)
                            .unwrap_or(0)
                    })
                    .sum();
                let language_name = language_name_for(&code).to_string();
                LanguageGroup {
                    file_count: files.len() as u32,
                    total_words,
                    language_code: code,
                    language_name,
                    files,
                }
            })
            .collect();
        // Sort by file_count desc, then code asc for determinism.
        groups.sort_by(|a, b| {
            b.file_count
                .cmp(&a.file_count)
                .then_with(|| a.language_code.cmp(&b.language_code))
        });
        // Dominant *foreign* language — exclude target.
        let target = self.target_language.clone();
        self.dominant_language = groups
            .iter()
            .filter(|g| g.language_code != target)
            .max_by_key(|g| g.file_count)
            .map(|g| g.language_code.clone());
        self.language_groups = groups;
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
        let _ = PipelineInput::Subtitle(PathBuf::from("s.srt"));
    }

    #[test]
    fn subtitle_input_detected_by_extension() {
        for ext in &["srt", "SRT", "vtt", "VTT"] {
            let p = PathBuf::from(format!("subs.{ext}"));
            assert!(
                matches!(detect_input_kind(&p), PipelineInput::Subtitle(_)),
                "expected Subtitle for .{ext}"
            );
        }
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
            language_groups: Vec::new(),
            dominant_language: None,
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
            language_groups: Vec::new(),
            dominant_language: None,
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
            language_groups: Vec::new(),
            dominant_language: None,
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
            language_groups: Vec::new(),
            dominant_language: None,
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
    fn language_groups_correctly_populated() {
        // 3 Arabic, 2 Chinese, 1 errored. The errored row drops
        // out (no detected_language); the remaining 5 fan into
        // 2 groups, sorted by file count descending.
        let mut r = BatchResult {
            generated_at: "2026-04-26T00:00:00Z".into(),
            total_files: 6,
            processed: 5,
            foreign_language: 5,
            translated: 5,
            errors: 1,
            target_language: "en".into(),
            machine_translation_notice: "MT.".into(),
            results: vec![
                make_row("/ev/a.mp3", "ar", true, Some("alpha"), Some("alpha"), None),
                make_row("/ev/b.mp3", "ar", true, Some("beta"), Some("beta"), None),
                make_row("/ev/c.mp3", "ar", true, Some("gamma"), Some("gamma"), None),
                make_row("/ev/d.mp3", "zh", true, Some("delta"), Some("delta"), None),
                make_row("/ev/e.mp3", "zh", true, Some("epsilon"), Some("epsilon"), None),
                make_row("/ev/f.mp3", "", false, None, None, Some("error")),
            ],
            summary: None,
            language_groups: Vec::new(),
            dominant_language: None,
        };
        r.build_language_groups();
        assert_eq!(r.language_groups.len(), 2);
        assert_eq!(r.language_groups[0].language_code, "ar");
        assert_eq!(r.language_groups[0].language_name, "Arabic");
        assert_eq!(r.language_groups[0].file_count, 3);
        assert_eq!(r.language_groups[1].language_code, "zh");
        assert_eq!(r.language_groups[1].file_count, 2);
        // Total words across the Arabic group: 3 source words.
        assert_eq!(r.language_groups[0].total_words, 3);
    }

    #[test]
    fn dominant_language_is_most_frequent_foreign() {
        // 5 Arabic, 2 Chinese, 1 Russian, 4 English (target).
        // Dominant *foreign* must be Arabic — English excluded
        // because it's the target_language.
        let mut r = BatchResult {
            generated_at: "2026-04-26T00:00:00Z".into(),
            total_files: 12,
            processed: 12,
            foreign_language: 8,
            translated: 8,
            errors: 0,
            target_language: "en".into(),
            machine_translation_notice: "MT.".into(),
            results: (0..5)
                .map(|i| make_row(&format!("/a/{i}"), "ar", true, Some("x"), Some("X"), None))
                .chain((0..2).map(|i| {
                    make_row(&format!("/z/{i}"), "zh", true, Some("y"), Some("Y"), None)
                }))
                .chain(std::iter::once(make_row(
                    "/r/1", "ru", true, Some("r"), Some("R"), None,
                )))
                .chain((0..4).map(|i| {
                    make_row(&format!("/e/{i}"), "en", false, Some("hi"), None, None)
                }))
                .collect(),
            summary: None,
            language_groups: Vec::new(),
            dominant_language: None,
        };
        r.build_language_groups();
        assert_eq!(r.dominant_language.as_deref(), Some("ar"));
        // The English group is still listed; it just isn't the
        // dominant *foreign* language.
        assert!(r
            .language_groups
            .iter()
            .any(|g| g.language_code == "en"));
    }

    #[test]
    fn dominant_language_is_none_when_no_foreign() {
        let mut r = BatchResult {
            generated_at: "2026-04-26T00:00:00Z".into(),
            total_files: 1,
            processed: 1,
            foreign_language: 0,
            translated: 0,
            errors: 0,
            target_language: "en".into(),
            machine_translation_notice: "MT.".into(),
            results: vec![make_row(
                "/e/1", "en", false, Some("hello"), None, None,
            )],
            summary: None,
            language_groups: Vec::new(),
            dominant_language: None,
        };
        r.build_language_groups();
        assert!(r.dominant_language.is_none());
    }

    #[test]
    fn language_name_for_covers_forensic_languages() {
        for (iso, expected) in &[
            ("ar", "Arabic"),
            ("fa", "Persian (Farsi)"),
            ("ps", "Pashto"),
            ("ur", "Urdu"),
            ("zh", "Chinese"),
            ("en", "English"),
        ] {
            assert_eq!(language_name_for(iso), *expected);
        }
        // Unknown ISO falls through to the sentinel.
        assert_eq!(language_name_for("xx"), "(unknown)");
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
    fn magic_byte_helpers_recognize_canonical_signatures() {
        assert!(is_pdf_magic(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3"));
        assert!(is_mp4_magic(b"\x00\x00\x00\x18ftypmp42extra"));
        assert!(is_wav_magic(b"RIFF\x24\x00\x00\x00WAVEfmt "));
        assert!(is_mp3_magic(b"ID3\x04\x00")); // tagged
        assert!(is_mp3_magic(&[0xFF, 0xFB, 0x90, 0x00])); // MPEG sync
        assert!(is_jpeg_magic(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(is_png_magic(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]));
        assert!(is_zip_magic(b"PK\x03\x04"));
        assert!(is_gzip_magic(&[0x1F, 0x8B]));
        // Negative cases.
        assert!(!is_pdf_magic(b"NOPE"));
        assert!(!is_mp4_magic(b"\x00\x00\x00\x00wxyz"));
        assert!(!is_wav_magic(b"RIFF........"));
    }

    #[test]
    fn pdf_with_wrong_extension_is_corrected_by_magic_bytes() {
        let dir = std::env::temp_dir().join(format!(
            "verify-magic-pdf-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("looks_like_audio.mp3");
        // Write PDF magic bytes + a few padding bytes.
        std::fs::write(&path, b"%PDF-1.4\n%\xe2\xe3\xcf\xd3 garbage").unwrap();
        let kind = detect_input_kind_robust(&path);
        assert!(
            matches!(kind, PipelineInput::Pdf(_)),
            "magic-byte detection should override .mp3 extension; got {kind:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_magic_falls_through_to_extension() {
        let dir = std::env::temp_dir().join(format!(
            "verify-magic-fallback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("unknown.png");
        std::fs::write(&path, b"\x00\x01\x02\x03 random bytes").unwrap();
        // Magic bytes don't match anything → fall back to
        // extension-based answer (.png → Image).
        let kind = detect_input_kind_robust(&path);
        assert!(matches!(kind, PipelineInput::Image(_)));
        let _ = std::fs::remove_dir_all(&dir);
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
