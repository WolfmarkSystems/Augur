//! Sprint 15 P2 — Arabic dialect → translation route selector.
//!
//! NLLB-200 was trained primarily on Modern Standard Arabic.
//! Egyptian and Levantine dialects translate well via NLLB
//! dialect-specific tokens (arz_Arab, apc_Arab); Moroccan Darija
//! has heavy French influence and works much better through
//! SeamlessM4T when it's installed. This module owns that
//! decision and surfaces every routing call with a human-
//! readable reason and a non-suppressible dialect advisory.

use serde::Serialize;

/// Mirror of `augur_classifier::ArabicDialect`. Duplicated here
/// so `augur-core` does not gain a runtime dep on the classifier
/// crate (the classifier already depends on core; an
/// inverted dep would cycle). Conversions live at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DialectKind {
    ModernStandard,
    Egyptian,
    Levantine,
    Gulf,
    Iraqi,
    Moroccan,
    Yemeni,
    Sudanese,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TranslationRoute {
    /// NLLB-200 with the standard `ara_Arab` token.
    NllbDefault,
    /// NLLB-200 with the Egyptian `arz_Arab` token.
    NllbEgyptian,
    /// NLLB-200 with the North-Levantine `apc_Arab` token.
    NllbLevantine,
    /// NLLB-200 with the Mesopotamian `acm_Arab` token.
    NllbIraqi,
    /// NLLB-200 with the Moroccan `ary_Arab` token (limited).
    NllbMoroccan,
    /// SeamlessM4T — preferred for Moroccan Darija when
    /// installed.
    SeamlessM4T,
}

impl TranslationRoute {
    pub fn label(self) -> &'static str {
        match self {
            Self::NllbDefault => "nllb_default",
            Self::NllbEgyptian => "nllb_egyptian",
            Self::NllbLevantine => "nllb_levantine",
            Self::NllbIraqi => "nllb_iraqi",
            Self::NllbMoroccan => "nllb_moroccan",
            Self::SeamlessM4T => "seamless_m4t",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RoutingDecision {
    pub route: TranslationRoute,
    pub reason: String,
    pub model_used: String,
    /// Always non-empty. Augments — never replaces — the
    /// machine-translation advisory.
    pub dialect_advisory: String,
    pub confidence: f32,
}

impl RoutingDecision {
    pub fn route_label(&self) -> &'static str {
        self.route.label()
    }
}

/// NLLB-200 source token for a given dialect. Default is
/// `ara_Arab` (MSA). Gulf dialects collapse to MSA — Gulf is
/// close enough to standard Arabic that NLLB handles it without
/// a dialect-specific token.
pub fn arabic_nllb_token(dialect: DialectKind) -> &'static str {
    match dialect {
        DialectKind::Egyptian => "arz_Arab",
        DialectKind::Levantine => "apc_Arab",
        DialectKind::Iraqi => "acm_Arab",
        DialectKind::Moroccan => "ary_Arab",
        _ => "ara_Arab",
    }
}

/// Always non-empty. Dialect-specific advisory text that
/// accompanies — never replaces — the MT advisory.
pub fn dialect_advisory_text(dialect: DialectKind) -> String {
    let dialect_name = match dialect {
        DialectKind::Egyptian => "Egyptian (Masri) Arabic",
        DialectKind::Gulf => "Gulf Arabic",
        DialectKind::Levantine => "Levantine Arabic",
        DialectKind::Moroccan => "Moroccan Darija",
        DialectKind::Iraqi => "Iraqi Arabic",
        DialectKind::ModernStandard => "Modern Standard Arabic (MSA)",
        DialectKind::Yemeni => "Yemeni Arabic",
        DialectKind::Sudanese => "Sudanese Arabic",
        DialectKind::Unknown => "Arabic (dialect unresolved)",
    };
    format!(
        "Detected dialect: {dialect_name}. Machine translation quality varies by dialect. \
         NLLB-200 was trained primarily on Modern Standard Arabic — \
         dialectal content may have reduced translation accuracy. \
         Verify all translations with a certified Arabic linguist \
         before use in legal proceedings."
    )
}

/// Compact analysis input — owned strings so callers don't have
/// to wrestle with classifier types crossing the crate boundary.
#[derive(Debug, Clone)]
pub struct DialectAnalysisInput {
    pub detected_dialect: DialectKind,
    pub confidence: f32,
}

pub fn route_arabic_translation(
    dialect: &DialectAnalysisInput,
    seamless_installed: bool,
) -> RoutingDecision {
    let advisory = dialect_advisory_text(dialect.detected_dialect);
    match dialect.detected_dialect {
        DialectKind::Egyptian if dialect.confidence >= 0.70 => RoutingDecision {
            route: TranslationRoute::NllbEgyptian,
            reason: "Egyptian (Masri) detected with high confidence. Routing to \
                     Egyptian Arabic NLLB token (arz_Arab) for better accuracy on Masri content."
                .into(),
            model_used: "NLLB-200 (Egyptian Arabic token: arz_Arab)".into(),
            dialect_advisory: advisory,
            confidence: dialect.confidence,
        },
        DialectKind::Moroccan if dialect.confidence >= 0.65 => {
            if seamless_installed {
                RoutingDecision {
                    route: TranslationRoute::SeamlessM4T,
                    reason:
                        "Moroccan Darija detected. SeamlessM4T selected — significantly \
                         better than NLLB-200 on Darija due to French-Arabic code-switching \
                         in Maghrebi dialects."
                            .into(),
                    model_used: "SeamlessM4T Medium".into(),
                    dialect_advisory: advisory,
                    confidence: dialect.confidence,
                }
            } else {
                RoutingDecision {
                    route: TranslationRoute::NllbDefault,
                    reason:
                        "Moroccan Darija detected but SeamlessM4T not installed. Falling back \
                         to NLLB-200; quality may be reduced. Install SeamlessM4T for better \
                         Darija translation: `augur install --model seamless-m4t-medium`."
                            .into(),
                    model_used: "NLLB-200 (fallback — SeamlessM4T preferred)".into(),
                    dialect_advisory: advisory,
                    confidence: dialect.confidence,
                }
            }
        }
        DialectKind::Levantine if dialect.confidence >= 0.65 => RoutingDecision {
            route: TranslationRoute::NllbLevantine,
            reason: "Levantine Arabic detected (Syrian/Lebanese/Palestinian/Jordanian). \
                     Routing to Levantine NLLB token (apc_Arab)."
                .into(),
            model_used: "NLLB-200 (Levantine Arabic token: apc_Arab)".into(),
            dialect_advisory: advisory,
            confidence: dialect.confidence,
        },
        DialectKind::Iraqi if dialect.confidence >= 0.65 => RoutingDecision {
            route: TranslationRoute::NllbIraqi,
            reason: "Iraqi Arabic detected. Routing to Mesopotamian NLLB token (acm_Arab)."
                .into(),
            model_used: "NLLB-200 (Iraqi Arabic token: acm_Arab)".into(),
            dialect_advisory: advisory,
            confidence: dialect.confidence,
        },
        DialectKind::Gulf if dialect.confidence >= 0.65 => RoutingDecision {
            route: TranslationRoute::NllbDefault,
            reason: "Gulf Arabic detected (Saudi/Emirati/Kuwaiti/Qatari). Using standard \
                     Arabic NLLB token — Gulf Arabic is close to MSA for NLLB-200."
                .into(),
            model_used: "NLLB-200 (standard Arabic token: ara_Arab)".into(),
            dialect_advisory: advisory,
            confidence: dialect.confidence,
        },
        _ => RoutingDecision {
            route: TranslationRoute::NllbDefault,
            reason: format!(
                "Arabic dialect: {:?} (confidence: {:.2}). Using standard Modern Standard \
                 Arabic translation path.",
                dialect.detected_dialect, dialect.confidence
            ),
            model_used: "NLLB-200 (standard Arabic token: ara_Arab)".into(),
            dialect_advisory: advisory,
            confidence: dialect.confidence,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis(d: DialectKind, c: f32) -> DialectAnalysisInput {
        DialectAnalysisInput { detected_dialect: d, confidence: c }
    }

    #[test]
    fn egyptian_high_confidence_routes_to_nllb_egyptian() {
        let d = route_arabic_translation(&analysis(DialectKind::Egyptian, 0.89), false);
        assert_eq!(d.route, TranslationRoute::NllbEgyptian);
        assert!(d.model_used.contains("arz_Arab"));
    }

    #[test]
    fn moroccan_routes_to_seamless_when_installed() {
        let d = route_arabic_translation(&analysis(DialectKind::Moroccan, 0.75), true);
        assert_eq!(d.route, TranslationRoute::SeamlessM4T);
    }

    #[test]
    fn moroccan_falls_back_to_nllb_when_seamless_missing() {
        let d = route_arabic_translation(&analysis(DialectKind::Moroccan, 0.75), false);
        assert_eq!(d.route, TranslationRoute::NllbDefault);
        assert!(d.reason.contains("SeamlessM4T"));
        assert!(d.reason.contains("augur install"));
    }

    #[test]
    fn dialect_advisory_always_non_empty() {
        for d in [
            DialectKind::Egyptian,
            DialectKind::Gulf,
            DialectKind::Levantine,
            DialectKind::Moroccan,
            DialectKind::Iraqi,
            DialectKind::ModernStandard,
            DialectKind::Yemeni,
            DialectKind::Sudanese,
            DialectKind::Unknown,
        ] {
            assert!(!dialect_advisory_text(d).is_empty(), "advisory empty for {d:?}");
        }
    }

    #[test]
    fn arabic_nllb_token_correct_per_dialect() {
        assert_eq!(arabic_nllb_token(DialectKind::Egyptian), "arz_Arab");
        assert_eq!(arabic_nllb_token(DialectKind::Levantine), "apc_Arab");
        assert_eq!(arabic_nllb_token(DialectKind::Iraqi), "acm_Arab");
        assert_eq!(arabic_nllb_token(DialectKind::Gulf), "ara_Arab");
        assert_eq!(arabic_nllb_token(DialectKind::Moroccan), "ary_Arab");
        assert_eq!(arabic_nllb_token(DialectKind::ModernStandard), "ara_Arab");
        assert_eq!(arabic_nllb_token(DialectKind::Unknown), "ara_Arab");
    }

    #[test]
    fn low_confidence_dialect_uses_default_path() {
        let d = route_arabic_translation(&analysis(DialectKind::Egyptian, 0.45), false);
        assert_eq!(d.route, TranslationRoute::NllbDefault);
    }

    #[test]
    fn routing_decision_dialect_advisory_present() {
        let d = route_arabic_translation(&analysis(DialectKind::Levantine, 0.80), false);
        assert!(!d.dialect_advisory.is_empty());
        assert!(d.dialect_advisory.contains("dialect"));
    }
}
