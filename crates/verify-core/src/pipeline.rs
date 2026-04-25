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
use std::path::PathBuf;

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
    }
}
