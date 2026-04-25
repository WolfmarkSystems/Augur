//! Sprint 1 scaffold.
//!
//! The real `LanguageClassifier` wraps a fastText LID model (or
//! `whichlang` as the pure-Rust fallback — final choice documented in
//! CLAUDE.md when the binding is picked in P2). `ModelManager` owns
//! the ONLY network call VERIFY makes: the first-run download of the
//! ~900KB `lid.176.ftz` model, cached under `~/.cache/verify/models/`.
//!
//! Everything in this file is placeholders so the workspace compiles
//! cleanly and downstream crates can `use` the types immediately.

use std::path::{Path, PathBuf};
use verify_core::VerifyError;

/// Result of a single classification pass.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    /// Detected language — ISO 639-1 code (e.g. "ar", "zh", "ru").
    pub language: String,
    /// Model confidence, 0.0–1.0.
    pub confidence: f32,
    /// `true` when `language != target_language`.
    pub is_foreign: bool,
    /// Whichever target the examiner asked for.
    pub target_language: String,
}

/// Owns the on-disk model cache (`~/.cache/verify/models/`). The
/// first call to [`ModelManager::ensure_lid_model`] is the only
/// network egress VERIFY performs in its default code path — every
/// subsequent run returns the cached path.
#[derive(Debug, Clone)]
pub struct ModelManager {
    pub cache_dir: PathBuf,
}

impl ModelManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// Sprint 1 stub — real implementation lands in P2 of Sprint 1.
    pub fn ensure_lid_model(&self) -> Result<PathBuf, VerifyError> {
        Err(VerifyError::ModelManager(
            "ensure_lid_model not yet implemented — see Sprint 1 P2".to_string(),
        ))
    }
}

/// Sprint 1 scaffold of the classifier. P2 wires in fastText /
/// whichlang and a real `classify` path.
#[derive(Debug)]
pub struct LanguageClassifier {
    _model_path: PathBuf,
}

impl LanguageClassifier {
    /// Sprint 1 stub. Returns Ok with a placeholder handle so the
    /// CLI layer can compile against the real API shape today; in
    /// P2 this switches to actually loading the model.
    pub fn load(model_path: &Path) -> Result<Self, VerifyError> {
        Ok(Self {
            _model_path: model_path.to_path_buf(),
        })
    }

    /// Sprint 1 stub. Returns a clear "not implemented" error so
    /// accidental usage is loud, not silent.
    pub fn classify(
        &self,
        _text: &str,
        _target_language: &str,
    ) -> Result<ClassificationResult, VerifyError> {
        Err(VerifyError::Classifier(
            "classify not yet implemented — see Sprint 1 P2".to_string(),
        ))
    }
}
