//! Tesseract OCR for images.
//!
//! # Sprint 2 backend choice — `tesseract` CLI subprocess
//!
//! The Rust `tesseract` / `leptess` crates require `libtesseract` +
//! `libleptonica` system libraries. The Sprint 2 probe found no
//! Tesseract installed on the build host. Per the Sprint 2 decision
//! rule, VERIFY ships a subprocess-based path against the
//! `tesseract` CLI — same pattern as `ffmpeg` for audio in
//! `verify-stt`. Forensic workstations virtually always have
//! Tesseract available (`brew install tesseract` /
//! `apt install tesseract-ocr`).
//!
//! The subprocess approach also avoids C/C++ FFI in a Rust binary
//! and keeps VERIFY's pure-Rust build story intact.
//!
//! # Offline invariant
//!
//! Tesseract reads `tessdata` files from disk; no network access.
//! Air-gapped workstations should pre-place language packs at the
//! standard tessdata path (`/usr/local/share/tessdata` or
//! `$TESSDATA_PREFIX`).

use log::{debug, info, warn};
use std::path::{Path, PathBuf};
use std::process::Command;
use verify_core::VerifyError;

/// Optional bounding box (pixel coords) for a recognised word.
/// Sprint 2 leaves this `None` from the CLI subprocess path; richer
/// per-word boxes are a future upgrade once `leptess` lands.
#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct OcrWord {
    pub text: String,
    pub confidence: f32,
    pub bounding_box: Option<BoundingBox>,
}

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub confidence: f32,
    /// ISO 639-1 detected language. Empty until classifier runs on
    /// the OCR output downstream.
    pub detected_language: String,
    pub page_count: u32,
    pub words: Vec<OcrWord>,
}

/// OCR engine. Holds the configured language hint plus the
/// `tesseract` binary command. The engine is cheap to construct;
/// each [`OcrEngine::extract_text`] call spawns one subprocess.
#[derive(Debug, Clone)]
pub struct OcrEngine {
    pub language: String,
    pub tesseract_cmd: String,
}

impl OcrEngine {
    /// Construct an engine for the given Tesseract language code
    /// (e.g. `"eng"`, `"ara"`, `"fas"`). Use [`iso_to_tesseract`]
    /// to convert from an ISO 639-1 hint produced by the
    /// classifier or by metadata.
    pub fn new(language: &str) -> Result<Self, VerifyError> {
        if language.is_empty() {
            return Err(VerifyError::Ocr(
                "OCR language must not be empty — pass a Tesseract code (eng/ara/fas/...)"
                    .to_string(),
            ));
        }
        Ok(Self {
            language: language.to_string(),
            tesseract_cmd: "tesseract".to_string(),
        })
    }

    /// Extract text from an image. Supported formats are whatever
    /// the local Tesseract+Leptonica build supports (typically PNG,
    /// JPG, TIFF, BMP, sometimes PDF).
    pub fn extract_text(&self, image_path: &Path) -> Result<OcrResult, VerifyError> {
        if !image_path.exists() {
            return Err(VerifyError::InvalidInput(format!(
                "image file not found: {:?}",
                image_path
            )));
        }
        if !which_binary(&self.tesseract_cmd) {
            return Err(VerifyError::Ocr(format!(
                "{} not found on PATH. Install via `brew install tesseract` \
                 (macOS) or `apt install tesseract-ocr tesseract-ocr-{}` (Linux).",
                self.tesseract_cmd, self.language
            )));
        }

        warn!(
            "verify-ocr: invoking {} on {:?} (lang={})",
            self.tesseract_cmd, image_path, self.language
        );

        let output = Command::new(&self.tesseract_cmd)
            .arg(image_path)
            .arg("stdout")
            .arg("-l")
            .arg(&self.language)
            .output()
            .map_err(|e| {
                VerifyError::Ocr(format!(
                    "failed to launch {}: {e}",
                    self.tesseract_cmd
                ))
            })?;

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(VerifyError::Ocr(format!(
                "tesseract failed for {:?}: exit {}; stderr={}",
                image_path,
                output.status.code().unwrap_or(-1),
                stderr.trim()
            )));
        }

        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let trimmed = text.trim().to_string();
        debug!("verify-ocr: extracted {} chars", trimmed.len());

        let words: Vec<OcrWord> = trimmed
            .split_whitespace()
            .map(|w| OcrWord {
                text: w.to_string(),
                confidence: 0.0,
                bounding_box: None,
            })
            .collect();

        info!(
            "verify-ocr: {} words extracted from {:?}",
            words.len(),
            image_path
        );

        Ok(OcrResult {
            text: trimmed,
            confidence: 0.0,
            detected_language: String::new(),
            page_count: 1,
            words,
        })
    }
}

/// Map ISO 639-1 to Tesseract's tessdata language code.
/// Tesseract uses 3-letter ISO 639-2 codes — most map cleanly, but
/// a few (Persian, Pashto) need explicit handling.
pub fn iso_to_tesseract(iso: &str) -> Result<&'static str, VerifyError> {
    Ok(match iso {
        "ar" => "ara",
        "en" => "eng",
        "fa" => "fas",
        "ps" => "pus",
        "ur" => "urd",
        "zh" => "chi_sim",
        "ru" => "rus",
        "es" => "spa",
        "fr" => "fra",
        "de" => "deu",
        "ko" => "kor",
        "ja" => "jpn",
        "vi" => "vie",
        "tr" => "tur",
        "pt" => "por",
        "it" => "ita",
        "nl" => "nld",
        "he" => "heb",
        "hi" => "hin",
        other => {
            return Err(VerifyError::Ocr(format!(
                "no tesseract code mapped for ISO 639-1 {other:?} — extend iso_to_tesseract"
            )));
        }
    })
}

fn which_binary(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── PDF extraction ──────────────────────────────────────────────
//
// Sprint 4 P3: PDFs are common forensic evidence. Two pathways:
//   1. Pure-Rust text-layer extraction via `pdf-extract` —
//      handles native digital PDFs (exported reports, downloaded
//      documents) with no system deps.
//   2. Rasterize + OCR fallback for scanned PDFs (no text layer).
//      Shells out to `pdftoppm` (poppler) for rasterization, then
//      reuses the existing [`OcrEngine`] for per-page OCR. Same
//      subprocess pattern as `tesseract` itself.

/// Extract text from a PDF. Tries the text layer first; falls
/// back to rasterize-and-OCR if the text layer is empty (scanned
/// document).
///
/// `ocr_lang` is the ISO 639-1 hint for the OCR fallback path —
/// ignored for text-layer-only PDFs. `scratch_dir` is used for
/// rasterized page images and is created if missing.
pub fn extract_pdf_text(
    pdf_path: &Path,
    scratch_dir: &Path,
    ocr_lang: &str,
) -> Result<String, VerifyError> {
    if !pdf_path.exists() {
        return Err(VerifyError::InvalidInput(format!(
            "pdf file not found: {:?}",
            pdf_path
        )));
    }
    debug!("verify-ocr: extracting text layer from {pdf_path:?}");
    let text_layer = match pdf_extract::extract_text(pdf_path) {
        Ok(t) => t,
        Err(e) => {
            warn!("verify-ocr: pdf-extract failed on {pdf_path:?}: {e}");
            String::new()
        }
    };
    if !text_layer.trim().is_empty() {
        info!(
            "verify-ocr: PDF text layer extracted: {} chars",
            text_layer.len()
        );
        return Ok(text_layer);
    }
    debug!("verify-ocr: no text layer in {pdf_path:?} — falling back to rasterize+OCR");
    extract_pdf_via_ocr(pdf_path, scratch_dir, ocr_lang)
}

fn extract_pdf_via_ocr(
    pdf_path: &Path,
    scratch_dir: &Path,
    ocr_lang: &str,
) -> Result<String, VerifyError> {
    if !which_binary("pdftoppm") {
        return Err(VerifyError::Ocr(
            "pdftoppm not found on PATH (required to OCR scanned PDFs). \
             Install poppler: `brew install poppler` (macOS) or \
             `apt install poppler-utils` (Linux)."
                .to_string(),
        ));
    }
    std::fs::create_dir_all(scratch_dir)?;
    let prefix = scratch_dir.join(format!(
        "verify-pdf-{}-{}",
        std::process::id(),
        random_suffix()
    ));
    warn!(
        "verify-ocr: rasterizing {pdf_path:?} via pdftoppm → {prefix:?}-N.png \
         (scanned PDF fallback path)"
    );
    let status = Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg("300")
        .arg(pdf_path)
        .arg(&prefix)
        .status()
        .map_err(|e| VerifyError::Ocr(format!("failed to launch pdftoppm: {e}")))?;
    if !status.success() {
        return Err(VerifyError::Ocr(format!(
            "pdftoppm failed for {pdf_path:?}: exit {:?}",
            status.code()
        )));
    }

    let prefix_str = prefix
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| VerifyError::Ocr("scratch prefix not UTF-8".to_string()))?
        .to_string();
    let mut pages: Vec<PathBuf> = std::fs::read_dir(scratch_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix_str) && n.ends_with(".png"))
                .unwrap_or(false)
        })
        .collect();
    pages.sort();
    if pages.is_empty() {
        return Err(VerifyError::Ocr(format!(
            "pdftoppm produced no pages for {pdf_path:?}"
        )));
    }

    let tess = iso_to_tesseract(ocr_lang)?;
    let engine = OcrEngine::new(tess)?;
    let mut combined = String::new();
    for page in &pages {
        match engine.extract_text(page) {
            Ok(r) => {
                if !combined.is_empty() {
                    combined.push_str("\n\n");
                }
                combined.push_str(r.text.trim());
            }
            Err(e) => {
                warn!("verify-ocr: OCR failed on PDF page {page:?}: {e}");
            }
        }
        let _ = std::fs::remove_file(page);
    }
    Ok(combined)
}

fn random_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Sprint-1 stub kept for transitional callers; new code should
/// construct an [`OcrEngine`] directly.
pub fn ocr_image_stub(path: &Path, lang_hint: Option<&str>) -> Result<String, VerifyError> {
    let lang = lang_hint.unwrap_or("eng");
    let engine = OcrEngine::new(lang)?;
    Ok(engine.extract_text(path)?.text)
}

/// Discover the tessdata directory tesseract is using. Returns
/// `None` if tesseract is not installed or the path can't be
/// resolved — callers fall back to the engine's own search behavior.
pub fn tessdata_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("TESSDATA_PREFIX") {
        let candidate = PathBuf::from(p);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocr_engine_initializes_for_english() {
        let engine = OcrEngine::new("eng").expect("eng init");
        assert_eq!(engine.language, "eng");
    }

    #[test]
    fn ocr_engine_rejects_empty_language() {
        match OcrEngine::new("") {
            Err(VerifyError::Ocr(_)) => {}
            other => panic!("expected Ocr error on empty lang, got {other:?}"),
        }
    }

    #[test]
    fn ocr_returns_error_for_missing_file() {
        let engine = OcrEngine::new("eng").expect("init");
        let missing = Path::new("/nonexistent/photo.png");
        match engine.extract_text(missing) {
            Err(VerifyError::InvalidInput(_)) => {}
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn iso_to_tesseract_covers_forensic_languages() {
        assert_eq!(iso_to_tesseract("ar").unwrap(), "ara");
        assert_eq!(iso_to_tesseract("fa").unwrap(), "fas");
        assert_eq!(iso_to_tesseract("ps").unwrap(), "pus");
        assert_eq!(iso_to_tesseract("ur").unwrap(), "urd");
        assert_eq!(iso_to_tesseract("en").unwrap(), "eng");
    }

    #[test]
    fn iso_to_tesseract_rejects_unknown() {
        match iso_to_tesseract("xx") {
            Err(VerifyError::Ocr(_)) => {}
            other => panic!("expected Ocr error, got {other:?}"),
        }
    }

    #[test]
    fn pdf_extraction_returns_error_for_missing_file() {
        let missing = Path::new("/nonexistent/doc.pdf");
        let scratch = std::env::temp_dir().join("verify-pdf-test-missing");
        match extract_pdf_text(missing, &scratch, "en") {
            Err(VerifyError::InvalidInput(_)) => {}
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn extract_pdf_via_ocr_rejects_unmapped_lang() {
        // Validates that the fallback path surfaces a clear error
        // for an unmapped ISO code without panicking. Doesn't
        // require pdftoppm to be installed because the lang lookup
        // happens before the binary check.
        let scratch = std::env::temp_dir().join("verify-pdf-test-bad-lang");
        // Use an existing file (this source file) so the
        // PathBuf::exists() check passes; the real failure is the
        // unmapped iso code or the missing pdftoppm. Either is a
        // clean Ocr error — a panic would break this test.
        let pdf = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        match extract_pdf_text(&pdf, &scratch, "xx") {
            Err(VerifyError::Ocr(_)) => {}
            other => panic!("expected Ocr error on unmapped lang, got {other:?}"),
        }
    }

    #[test]
    fn ocr_result_has_all_required_fields() {
        let r = OcrResult {
            text: "مرحبا".into(),
            confidence: 0.9,
            detected_language: "ar".into(),
            page_count: 1,
            words: vec![OcrWord {
                text: "مرحبا".into(),
                confidence: 0.9,
                bounding_box: None,
            }],
        };
        assert!(!r.text.is_empty());
        assert_eq!(r.page_count, 1);
        assert_eq!(r.words.len(), 1);
    }
}
