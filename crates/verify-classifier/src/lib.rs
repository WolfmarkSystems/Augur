//! Language identification — VERIFY's router.
//!
//! Runs in front of the heavy pipeline. Decides whether content is
//! foreign relative to the examiner's target language; only the
//! foreign subset is queued for STT + NLLB.
//!
//! Sprint 1 scaffold: type definitions only. Real implementation
//! (fastText LID or `whichlang` fallback) lands in P2 of Sprint 1.

pub mod classifier;

pub use classifier::{ClassificationResult, LanguageClassifier, ModelManager};
