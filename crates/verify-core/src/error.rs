//! Unified error type for the VERIFY pipeline.
//!
//! Sub-crates (`verify-classifier`, `verify-stt`, `verify-translate`,
//! `verify-ocr`, `verify-plugin-sdk`) map their internal errors into
//! `VerifyError` at their public boundary so callers never need to
//! juggle a pile of unrelated error enums.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerifyError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("classifier error: {0}")]
    Classifier(String),

    #[error("stt error: {0}")]
    Stt(String),

    #[error("translate error: {0}")]
    Translate(String),

    #[error("ocr error: {0}")]
    Ocr(String),

    #[error("model-manager error: {0}")]
    ModelManager(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("geoip error: {0}")]
    GeoIp(String),

    /// Sprint 7 P1 — distinct from `GeoIp(...)` so callers can
    /// surface MaxMind setup instructions to the examiner without
    /// string-matching error messages. The MaxMind license bars
    /// VERIFY from auto-downloading the GeoLite2 database, so
    /// "not configured" is a first-class state.
    #[error("geoip database not configured: {0}")]
    GeoIpNotConfigured(String),
}
