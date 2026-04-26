//! Language identification — AUGUR's router.
//!
//! Runs in front of the heavy pipeline. Decides whether content is
//! foreign relative to the examiner's target language; only the
//! foreign subset is queued for STT + NLLB.

pub mod arabic_dialect;
pub mod classifier;
pub mod script;

pub use arabic_dialect::{detect_arabic_dialect, ArabicDialect, DialectAnalysis};
pub use classifier::{
    classify_confidence, confidence_advisory, ClassificationResult, ConfidenceTier,
    LanguageClassifier, ModelManager, SHORT_INPUT_WORD_COUNT,
};
pub use script::{
    pashto_farsi_score, PashtoFarsiAnalysis, ScriptRecommendation,
};
