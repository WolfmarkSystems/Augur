//! Unified error type for the AUGUR pipeline.
//!
//! Sub-crates (`augur-classifier`, `augur-stt`, `augur-translate`,
//! `augur-ocr`, `augur-plugin-sdk`) map their internal errors into
//! `AugurError` at their public boundary so callers never need to
//! juggle a pile of unrelated error enums.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AugurError {
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
    /// AUGUR from auto-downloading the GeoLite2 database, so
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

    /// Sprint 10 P1 — `augur install <profile>` was invoked with an
    /// unknown profile name. Carries the offending value so the
    /// CLI can echo it back unmodified.
    #[error("invalid install profile: {0} (expected: minimal|standard|full)")]
    InvalidProfile(String),

    /// Sprint 10 P1 — SHA-256 verification failed after a model
    /// download. The downloaded file is left on disk for manual
    /// inspection; the installer aborts the rest of the profile.
    #[error("integrity failure for model {model}: expected {expected}, got {computed}")]
    IntegrityFailure {
        model: String,
        expected: String,
        computed: String,
    },

    /// Sprint 10 P1 — `augur install <profile>` could not retrieve
    /// the model. Distinct from `Io(...)` so the CLI can surface a
    /// "check your network or use airgap install" hint without
    /// string-matching.
    #[error("download failed for {model}: {reason}")]
    DownloadFailed { model: String, reason: String },
}
