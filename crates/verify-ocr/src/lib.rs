//! Tesseract OCR for images — stub in Sprint 1.
//!
//! Sprint 2 will wire `leptess` (Rust bindings to Tesseract) and
//! language packs so VERIFY can lift foreign-language text out of
//! photos, screenshots, and scanned documents before handing it to
//! the translator.

use std::path::Path;
use verify_core::VerifyError;

/// Sprint 1 stub.
pub fn ocr_image_stub(_path: &Path, _lang_hint: Option<&str>) -> Result<String, VerifyError> {
    Err(VerifyError::Ocr(
        "ocr_image not yet implemented — see Sprint 2".to_string(),
    ))
}
