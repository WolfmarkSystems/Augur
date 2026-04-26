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

    #[error("yara error: {0}")]
    Yara(String),

    /// Super Sprint Group B P3 — distinct variant so the CLI can
    /// surface a specific install hint when the `yara` binary
    /// is missing without string-matching the error message.
    #[error("yara binary not installed: {0}")]
    YaraNotInstalled(String),

    /// Super Sprint Group C P4 — file size exceeded the
    /// configured pipeline limit. Returned with both the actual
    /// size and the limit so the examiner can decide whether to
    /// raise the limit or split the file.
    #[error("file too large: {size_bytes} bytes (limit: {limit_bytes} bytes)")]
    FileTooLarge { size_bytes: u64, limit_bytes: u64 },

    /// Super Sprint Group C P4 — file exists but a parser
    /// rejected its contents. Carries the path + a reason
    /// string so logs are useful.
    #[error("corrupt file at {path}: {reason}")]
    CorruptFile { path: String, reason: String },

    /// Super Sprint Group C P4 — a per-file / per-call timeout
    /// fired before the operation completed.
    #[error("operation timed out after {seconds}s")]
    ProcessTimeout { seconds: u64 },
}
