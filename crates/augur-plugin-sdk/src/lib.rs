//! Strata plugin adapter.
//!
//! Default build (no features): exposes the adapter *shape* only —
//! [`Confidence`], [`ArtifactRecord`], [`AugurStrataPlugin`], and
//! [`artifact_from_translation`]. This keeps AUGUR's standalone
//! build lean (no FFI, no platform-specific filesystem parsers).
//!
//! `--features strata` build: pulls in the upstream
//! `strata-plugin-sdk` (sibling Strata workspace at
//! `~/Wolfmark/strata/crates/strata-plugin-sdk`) and provides a
//! real `impl strata_plugin_sdk::StrataPlugin for AugurStrataPlugin`.
//! This is the production path when AUGUR is shipped as a Strata
//! plugin alongside the rest of the Strata plugin grid.
//!
//! # Forensic safety invariant
//!
//! Every artifact produced from a translation result carries
//! `is_advisory = true` and a non-empty `advisory_notice`. Same
//! invariant Strata uses for its own MT/heuristic artifacts: if
//! the analyst exports the result, the export must label it as
//! machine-generated. Both the lean adapter shape *and* the real
//! Strata trait impl share `artifact_from_translation` to enforce
//! this in one place.

#[cfg(feature = "strata")]
mod strata_impl;

#[cfg(feature = "strata")]
pub use strata_impl::run_on_directory;

use augur_translate::{TranslationResult, MACHINE_TRANSLATION_NOTICE};

/// Forensic confidence levels mirroring Strata's enum. Machine
/// translation always lands at `Medium` — high enough to surface,
/// low enough to remind the analyst it needs human review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// Artifact record emitted by the AUGUR plugin. Field shapes
/// mirror Strata's `ArtifactRecord` so the upstream trait `impl`
/// becomes a one-line adapter.
#[derive(Debug, Clone)]
pub struct ArtifactRecord {
    pub artifact_type: String,
    pub value: String,
    pub source_plugin: String,
    pub confidence: Confidence,
    pub is_advisory: bool,
    pub advisory_notice: String,
    pub mitre_technique: String,
}

/// AUGUR's plugin metadata. The trait `impl` against
/// `strata_plugin_sdk::StrataPlugin` lives behind a future feature
/// flag; for now this struct is the source of truth for what the
/// plugin reports to the host.
#[derive(Debug, Clone)]
pub struct AugurStrataPlugin;

impl Default for AugurStrataPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl AugurStrataPlugin {
    pub fn new() -> Self {
        Self
    }

    pub fn name(&self) -> &'static str {
        "AUGUR"
    }

    pub fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    pub fn description(&self) -> &'static str {
        "Foreign language detection and translation — surfaces translated content \
         as Strata artifacts (machine-translation; review by a certified human \
         translator before legal use)."
    }
}

/// Convert a translation result into a Strata artifact. The
/// advisory flag and notice are mandatory and copied verbatim from
/// the translation result; the artifact value is the translated
/// text, since that is what an analyst sees in the Strata UI.
pub fn artifact_from_translation(t: &TranslationResult) -> ArtifactRecord {
    let advisory_notice = if t.advisory_notice.is_empty() {
        MACHINE_TRANSLATION_NOTICE.to_string()
    } else {
        t.advisory_notice.clone()
    };
    ArtifactRecord {
        artifact_type: "augur_translation".to_string(),
        value: t.translated_text.clone(),
        source_plugin: "AUGUR".to_string(),
        confidence: Confidence::Medium,
        is_advisory: true,
        advisory_notice,
        mitre_technique: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use augur_translate::DEFAULT_NLLB_MODEL;

    fn fixture() -> TranslationResult {
        TranslationResult {
            source_text: "مرحبا".into(),
            translated_text: "Hello".into(),
            source_language: "ar".into(),
            target_language: "en".into(),
            confidence: 0.85,
            model: DEFAULT_NLLB_MODEL.into(),
            is_machine_translation: true,
            advisory_notice: MACHINE_TRANSLATION_NOTICE.into(),
            segments: None,
        }
    }

    #[test]
    fn translation_artifact_carries_advisory() {
        let a = artifact_from_translation(&fixture());
        assert!(a.is_advisory, "machine translation must be advisory");
        assert!(
            !a.advisory_notice.is_empty(),
            "advisory_notice must be non-empty"
        );
        assert_eq!(a.confidence, Confidence::Medium);
        assert_eq!(a.artifact_type, "augur_translation");
        assert_eq!(a.source_plugin, "AUGUR");
        assert_eq!(a.value, "Hello");
    }

    #[test]
    fn plugin_metadata_present() {
        let p = AugurStrataPlugin::new();
        assert_eq!(p.name(), "AUGUR");
        assert!(!p.version().is_empty());
        assert!(p.description().contains("Machine") || p.description().contains("machine"));
    }

    #[test]
    fn artifact_advisory_filled_even_if_source_blank() {
        let mut t = fixture();
        t.advisory_notice = String::new();
        let a = artifact_from_translation(&t);
        assert!(
            !a.advisory_notice.is_empty(),
            "adapter must back-fill the advisory notice"
        );
    }
}
