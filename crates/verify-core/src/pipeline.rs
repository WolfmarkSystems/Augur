//! End-to-end pipeline entry point.
//!
//! Sprint 1 scaffold — real implementation lands in Sprint 2 once the
//! classifier, STT, and NLLB crates expose their public APIs.

use crate::error::VerifyError;
use std::path::Path;

/// Pipeline input kind. Used by [`Pipeline::classify_and_dispatch`] to
/// route to the correct sub-engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Text,
    Audio,
    Image,
    Video,
}

/// Top-level orchestrator. Sprint 1 exposes only a placeholder
/// `new` constructor so downstream crates can refer to the type.
#[derive(Debug, Default)]
pub struct Pipeline;

impl Pipeline {
    pub fn new() -> Self {
        Self
    }

    /// Sprint 2+: classify → dispatch → collect. Sprint 1 returns
    /// `InvalidInput` so callers that accidentally wire this up
    /// early get a loud, structured error rather than a panic.
    pub fn classify_and_dispatch(
        &self,
        _input: &Path,
        _kind: InputKind,
        _target_language: &str,
    ) -> Result<(), VerifyError> {
        Err(VerifyError::InvalidInput(
            "pipeline not yet implemented — see VERIFY_SPRINT_1.md".to_string(),
        ))
    }
}
