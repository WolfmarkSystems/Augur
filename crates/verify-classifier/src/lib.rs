//! Language identification — VERIFY's router.
//!
//! Runs in front of the heavy pipeline. Decides whether content is
//! foreign relative to the examiner's target language; only the
//! foreign subset is queued for STT + NLLB.

pub mod classifier;

pub use classifier::{
    classify_confidence, confidence_advisory, ClassificationResult, ConfidenceTier,
    LanguageClassifier, ModelManager, SHORT_INPUT_WORD_COUNT,
};
