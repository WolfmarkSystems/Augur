//! Sprint 10 P4 — CAMeL Tools dialect identification.
//!
//! Carnegie Mellon's CAMeL Tools (`camel-tools` Python package)
//! provides ML-based Arabic dialect identification — far more
//! accurate than the Sprint 9 lexical-marker approach, especially
//! on short text.
//!
//! Same offline-first contract as the NLLB / SeamlessM4T workers:
//! a bundled `camel_worker.py` is invoked via subprocess, talking
//! JSON over stdin/stdout. When CAMeL Tools is not installed, the
//! caller falls back to the lexical detector that ships in
//! [`crate::arabic_dialect`].

use std::io::Write;
use std::process::{Command, Stdio};

use augur_core::AugurError;
use serde::Deserialize;

use crate::arabic_dialect::{detect_arabic_dialect, ArabicDialect, DialectAnalysis};

const CAMEL_SCRIPT: &str = include_str!("camel_worker.py");

/// CAMeL DID label set → AUGUR's `ArabicDialect`. CAMeL emits
/// MADAR-26-style city/region codes; we collapse them to the
/// dialect families AUGUR already tracks.
fn map_camel_label(label: &str) -> ArabicDialect {
    let upper = label.to_uppercase();
    match upper.as_str() {
        "MSA" => ArabicDialect::ModernStandard,
        // Egyptian cluster
        "CAI" | "ALX" | "ASW" | "EGY" => ArabicDialect::Egyptian,
        // Levantine cluster (Damascus/Beirut/Jerusalem/Amman)
        "DAM" | "BEI" | "JER" | "AMM" | "ALE" | "SAL" | "LEV" => ArabicDialect::Levantine,
        // Gulf cluster
        "RIY" | "JED" | "DOH" | "MUS" | "SAN" | "BAS" | "GLF" | "GUL" => ArabicDialect::Gulf,
        // Iraqi
        "BAG" | "MOS" | "IRQ" | "IRA" => ArabicDialect::Iraqi,
        // Moroccan / Maghrebi
        "RAB" | "FES" | "ALG" | "ALJ" | "TUN" | "TRI" | "BEN" | "MGR" | "MOR" => {
            ArabicDialect::Moroccan
        }
        "SAA" | "YEM" => ArabicDialect::Yemeni,
        "KHA" | "SDN" => ArabicDialect::Sudanese,
        _ => ArabicDialect::Unknown,
    }
}

#[derive(Deserialize)]
struct CamelResponse {
    dialect: Option<String>,
    confidence: Option<f32>,
    error: Option<String>,
}

/// Invoke CAMeL Tools dialect identification on `text`. On success
/// returns a [`DialectAnalysis`]; on failure returns
/// [`AugurError::Classifier`]. Callers should treat any error as a
/// signal to fall back to the lexical detector.
pub fn run_camel(text: &str) -> Result<DialectAnalysis, AugurError> {
    if text.trim().is_empty() {
        return Ok(DialectAnalysis {
            detected_dialect: ArabicDialect::Unknown,
            confidence: 0.0,
            indicator_words: Vec::new(),
            advisory: CAMEL_EMPTY_INPUT_ADVISORY.into(),
        });
    }
    let request = serde_json::json!({"text": text});
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(CAMEL_SCRIPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AugurError::Classifier(format!("could not spawn python3: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(request.to_string().as_bytes())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|e| AugurError::Classifier(format!("camel worker wait failed: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(AugurError::Classifier(format!(
            "camel worker exited {:?}; stderr={stderr}",
            output.status.code()
        )));
    }
    let resp: CamelResponse = serde_json::from_str(stdout.trim())
        .map_err(|e| AugurError::Classifier(format!("could not parse camel output: {e}; stdout={stdout:?}")))?;
    if let Some(err) = resp.error {
        return Err(AugurError::Classifier(format!("camel worker error: {err}")));
    }
    let label = resp
        .dialect
        .ok_or_else(|| AugurError::Classifier("camel worker returned no dialect".into()))?;
    let confidence = resp.confidence.unwrap_or(0.0);
    let dialect = map_camel_label(&label);
    Ok(DialectAnalysis {
        detected_dialect: dialect,
        confidence,
        indicator_words: vec![format!("CAMeL:{label}")],
        advisory: format!("{CAMEL_DIALECT_ADVISORY} (raw label: {label})"),
    })
}

/// Sprint 10 P4 — preferred entry point. Tries CAMeL when
/// available, falls back to the Sprint 9 lexical detector. The
/// `camel_installed` flag is precomputed (typically via
/// [`augur_core::models::InstalledModels`]) so this function never
/// touches the filesystem itself.
pub fn classify_arabic_dialect(text: &str, camel_installed: bool) -> DialectAnalysis {
    if camel_installed {
        match run_camel(text) {
            Ok(analysis) => return analysis,
            Err(e) => {
                log::warn!(
                    "augur-classifier: CAMeL invocation failed ({e}); falling back to lexical detector"
                );
            }
        }
    }
    detect_arabic_dialect(text)
}

pub const CAMEL_DIALECT_ADVISORY: &str =
    "Dialect identified by CAMeL Tools (Carnegie Mellon Univ.). Verify with a human Arabic linguist before relying on this label in legal proceedings.";

pub const CAMEL_EMPTY_INPUT_ADVISORY: &str =
    "CAMeL Tools dialect ID skipped — input was empty.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_used_when_not_installed() {
        // Should run lexical detector and not panic.
        let r = classify_arabic_dialect("مرحبا", false);
        // Lexical detector emits a non-empty advisory always.
        assert!(!r.advisory.is_empty());
    }

    #[test]
    fn map_camel_labels_cover_all_dialects() {
        assert_eq!(map_camel_label("MSA"), ArabicDialect::ModernStandard);
        assert_eq!(map_camel_label("CAI"), ArabicDialect::Egyptian);
        assert_eq!(map_camel_label("DAM"), ArabicDialect::Levantine);
        assert_eq!(map_camel_label("RIY"), ArabicDialect::Gulf);
        assert_eq!(map_camel_label("BAG"), ArabicDialect::Iraqi);
        assert_eq!(map_camel_label("RAB"), ArabicDialect::Moroccan);
        assert_eq!(map_camel_label("SAA"), ArabicDialect::Yemeni);
        assert_eq!(map_camel_label("KHA"), ArabicDialect::Sudanese);
        assert_eq!(map_camel_label("XXX"), ArabicDialect::Unknown);
    }

    #[test]
    fn fallback_runs_when_camel_unavailable_even_if_flagged() {
        // camel_installed == true but the python invocation will
        // fail in the test environment — the fallback path must
        // still produce a usable analysis.
        let r = classify_arabic_dialect("مرحبا بكم", true);
        assert!(!r.advisory.is_empty());
    }
}
