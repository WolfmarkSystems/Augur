//! Error-recovery primitives — file size limits + retry-with-backoff.
//!
//! Super Sprint Group C P4. Forensic batch runs hit corrupt
//! files, oversized inputs, and transient subprocess failures.
//! These helpers turn "panic / silent failure" cases into
//! structured errors that the batch layer can capture in
//! `BatchFileResult.error` and continue.

use crate::error::VerifyError;
use std::path::Path;

/// Pipeline-wide size + count limits. Defaults match the
/// VERIFY Sprint super-sprint spec (500 MB audio/video, 10 MB
/// text, 500 PDF pages, 10 000 batch files, 5 minutes / file).
#[derive(Debug, Clone, Copy)]
pub struct PipelineLimits {
    pub max_file_size_bytes: u64,
    pub max_text_bytes: usize,
    pub max_pdf_pages: u32,
    pub max_batch_files: usize,
    pub timeout_seconds: u64,
}

impl Default for PipelineLimits {
    fn default() -> Self {
        Self {
            max_file_size_bytes: 500 * 1024 * 1024,
            max_text_bytes: 10 * 1024 * 1024,
            max_pdf_pages: 500,
            max_batch_files: 10_000,
            timeout_seconds: 300,
        }
    }
}

/// Reject files that exceed `max_file_size_bytes`. Reads the
/// metadata once; never opens the file body. Used at the top
/// of every per-file pipeline path.
pub fn check_file_size(path: &Path, limits: &PipelineLimits) -> Result<(), VerifyError> {
    let meta = std::fs::metadata(path).map_err(|e| {
        VerifyError::CorruptFile {
            path: path.to_string_lossy().into_owned(),
            reason: format!("metadata read: {e}"),
        }
    })?;
    let size = meta.len();
    if size == 0 {
        return Err(VerifyError::CorruptFile {
            path: path.to_string_lossy().into_owned(),
            reason: "empty file (0 bytes)".to_string(),
        });
    }
    if size > limits.max_file_size_bytes {
        return Err(VerifyError::FileTooLarge {
            size_bytes: size,
            limit_bytes: limits.max_file_size_bytes,
        });
    }
    Ok(())
}

/// Reject text content that exceeds `max_text_bytes`. Cheap
/// O(1) check on the byte length of an in-memory string.
pub fn check_text_size(text: &str, limits: &PipelineLimits) -> Result<(), VerifyError> {
    let len = text.len();
    if len > limits.max_text_bytes {
        return Err(VerifyError::FileTooLarge {
            size_bytes: len as u64,
            limit_bytes: limits.max_text_bytes as u64,
        });
    }
    Ok(())
}

/// Run a fallible operation with up to `max_attempts` tries and
/// linear backoff (`500ms * attempt`). Logs each retry at WARN
/// so an examiner reading the run output sees the recovery.
///
/// `max_attempts` must be ≥ 1; we panic on `0` because that's
/// a programming error not a runtime condition. The caller
/// can't legitimately ask for "zero attempts."
pub fn with_retry<F, T>(max_attempts: u32, mut f: F) -> Result<T, VerifyError>
where
    F: FnMut() -> Result<T, VerifyError>,
{
    assert!(max_attempts >= 1, "with_retry requires max_attempts >= 1");
    let mut last_err: Option<VerifyError> = None;
    for attempt in 0..max_attempts {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                log::warn!(
                    "with_retry: attempt {}/{} failed: {e}",
                    attempt + 1,
                    max_attempts
                );
                last_err = Some(e);
                if attempt + 1 < max_attempts {
                    std::thread::sleep(std::time::Duration::from_millis(
                        500 * (attempt as u64 + 1),
                    ));
                }
            }
        }
    }
    // SAFETY-of-logic: max_attempts >= 1 guaranteed by the
    // assertion above, so last_err is always Some by the time
    // we exit the loop without returning Ok.
    Err(last_err.expect("with_retry: assert max_attempts >= 1 already enforced"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tmp(name: &str, body: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "verify-resilience-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn file_too_large_returns_error_not_panic() {
        let path = write_tmp("oversized.bin", &vec![0u8; 1024]);
        let limits = PipelineLimits {
            max_file_size_bytes: 100,
            ..PipelineLimits::default()
        };
        match check_file_size(&path, &limits) {
            Err(VerifyError::FileTooLarge {
                size_bytes,
                limit_bytes,
            }) => {
                assert_eq!(size_bytes, 1024);
                assert_eq!(limit_bytes, 100);
            }
            other => panic!("expected FileTooLarge, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_file_returns_corrupt_not_panic() {
        let path = write_tmp("empty.bin", &[]);
        match check_file_size(&path, &PipelineLimits::default()) {
            Err(VerifyError::CorruptFile { reason, .. }) => {
                assert!(reason.contains("empty"));
            }
            other => panic!("expected CorruptFile on empty, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_returns_corrupt_not_panic() {
        let path = std::path::Path::new("/nonexistent/verify/resilience-test-xyz");
        match check_file_size(path, &PipelineLimits::default()) {
            Err(VerifyError::CorruptFile { .. }) => {}
            other => panic!("expected CorruptFile, got {other:?}"),
        }
    }

    #[test]
    fn text_size_limit_enforced() {
        let limits = PipelineLimits {
            max_text_bytes: 10,
            ..PipelineLimits::default()
        };
        assert!(check_text_size("under", &limits).is_ok());
        match check_text_size("0123456789012345", &limits) {
            Err(VerifyError::FileTooLarge { .. }) => {}
            other => panic!("expected FileTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn retry_succeeds_on_third_attempt() {
        // Rust closures need a Cell to mutate counters when
        // captured by FnMut without `move`.
        let attempts = std::cell::Cell::new(0u32);
        let result: Result<i32, VerifyError> = with_retry(5, || {
            attempts.set(attempts.get() + 1);
            if attempts.get() < 3 {
                Err(VerifyError::Stt("transient".to_string()))
            } else {
                Ok(42)
            }
        });
        assert_eq!(result.expect("succeeds"), 42);
        assert_eq!(attempts.get(), 3);
    }

    #[test]
    fn retry_returns_last_error_on_exhaustion() {
        let result: Result<i32, VerifyError> = with_retry(2, || {
            Err(VerifyError::Stt("perma-fail".to_string()))
        });
        match result {
            Err(VerifyError::Stt(msg)) => assert_eq!(msg, "perma-fail"),
            other => panic!("expected Err(Stt), got {other:?}"),
        }
    }
}
