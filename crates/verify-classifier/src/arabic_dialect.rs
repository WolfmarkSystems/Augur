//! Arabic dialect detection — lexical-marker approach.
//!
//! Super Sprint Group A. Standard NLLB-200 translates Modern
//! Standard Arabic (MSA) cleanly but degrades on heavy dialect
//! content. For LE/IC casework, dialect ALSO carries
//! geographic intent (Egyptian → Egypt, Gulf → Gulf states,
//! Darija → Morocco). This module surfaces a coarse dialect
//! signal so examiners see both.
//!
//! The detector is intentionally simple — a hardcoded lexicon
//! of high-confidence dialect markers per region. It is NOT a
//! statistical classifier. False negatives are cheap (the
//! dialect just shows up as `Unknown` and the rest of VERIFY
//! still works); false positives are also cheap because the
//! emitted advisory tells the examiner to verify with a human
//! linguist.
//!
//! See `docs/LANGUAGE_LIMITATIONS.md` for the broader
//! rationale on why automated dialect labels are advisory.

use serde::Serialize;

/// One of the major Arabic dialect families. The list is
/// non-exhaustive (real Arabic varies continuously across the
/// Arabic-speaking world); these are the dialect families with
/// the most distinctive lexical fingerprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArabicDialect {
    /// Modern Standard Arabic — the formal register used in
    /// media, education, and government documents.
    ModernStandard,
    /// Egyptian Arabic (Masri) — most widely understood
    /// vernacular thanks to Egyptian cinema.
    Egyptian,
    /// Levantine — Syrian, Lebanese, Palestinian, Jordanian.
    Levantine,
    /// Gulf — Saudi, Emirati, Kuwaiti, Qatari, Bahraini.
    Gulf,
    Iraqi,
    /// Moroccan Darija — heavy French / Berber influence.
    Moroccan,
    Yemeni,
    Sudanese,
    /// Detector saw no dialect markers at all (or fewer than
    /// the threshold for a confident call).
    Unknown,
}

impl ArabicDialect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModernStandard => "Modern Standard Arabic (MSA)",
            Self::Egyptian => "Egyptian (Masri)",
            Self::Levantine => "Levantine",
            Self::Gulf => "Gulf",
            Self::Iraqi => "Iraqi",
            Self::Moroccan => "Moroccan (Darija)",
            Self::Yemeni => "Yemeni",
            Self::Sudanese => "Sudanese",
            Self::Unknown => "Unknown",
        }
    }
}

/// Output of a single [`detect_arabic_dialect`] call. The
/// `indicator_words` list lets the CLI show the examiner *which*
/// words drove the call.
#[derive(Debug, Clone)]
pub struct DialectAnalysis {
    pub detected_dialect: ArabicDialect,
    pub confidence: f32,
    pub indicator_words: Vec<String>,
    pub advisory: String,
}

// ── Lexical markers ─────────────────────────────────────────────

const EGYPTIAN_MARKERS: &[&str] = &[
    "إيه",   // "what" (Masri)
    "كده",   // "like this"
    "عايز",  // "want" (m)
    "عايزة", // "want" (f)
    "ازيك",  // "how are you"
    "بقا",   // discourse particle
    "خلاص",  // "done/enough" (heavy in Masri)
];

const GULF_MARKERS: &[&str] = &[
    "وش",   // "what" (Gulf)
    "زين",  // "good/okay"
    "ابغى", // "I want" (Saudi)
    "وايد", // "very/many" (Emirati)
    "كذا",  // "like this"
];

const LEVANTINE_MARKERS: &[&str] = &[
    "شو",   // "what"
    "كيفك", // "how are you"
    "هيك",  // "like this"
    "يلا",  // "let's go"
    "بدي",  // "I want"
    "هلق",  // "now"
];

const MOROCCAN_MARKERS: &[&str] = &[
    "واش",     // "is it/are you"
    "كيداير",  // "how are you"
    "بزاف",    // "a lot"
    "باغي",    // "I want"
    "ديالي",   // "mine"
    "دابا",    // "now"
];

const IRAQI_MARKERS: &[&str] = &[
    "شلون",  // "how" (Iraqi)
    "اشكو",  // "why"
    "هواية", // "a lot"
    "يبه",   // address term
];

const YEMENI_MARKERS: &[&str] = &[
    "كيف",   // "how" (Yemeni form)
    "ذا",    // demonstrative
    "بيش",   // "with what"
];

const SUDANESE_MARKERS: &[&str] = &[
    "شنو",  // "what" (Sudanese)
    "ياخي", // "my brother"
    "زول",  // "person"
];

/// Marker shared by Egyptian + Levantine — the negation `مش`.
/// Counted toward the Levantine bucket only when no clearly-
/// Egyptian markers are also present (otherwise it's noise).
const SHARED_NEGATION: &str = "مش";

/// Threshold below which we don't make a confident dialect
/// call. Two clear markers → confidence floor; one marker →
/// `Unknown` to avoid false-positive labelling on a single
/// loanword that happens to overlap with a dialect.
const MIN_MARKERS_FOR_CONFIDENT_CALL: u32 = 2;

const ADVISORY_TEXT: &str =
    "Dialect detection is approximate — based on a hardcoded \
     lexicon of regional markers. Verify with a human linguist \
     fluent in Arabic dialects if dialect origin is material to \
     your case.";

/// Public detector. Counts marker occurrences per dialect,
/// picks the highest-scoring family above the confidence floor.
/// Pure function — no I/O, no allocations beyond the
/// indicator-word list.
pub fn detect_arabic_dialect(text: &str) -> DialectAnalysis {
    if text.trim().is_empty() {
        return DialectAnalysis {
            detected_dialect: ArabicDialect::Unknown,
            confidence: 0.0,
            indicator_words: Vec::new(),
            advisory: String::new(),
        };
    }

    let mut buckets: Vec<(ArabicDialect, &[&str])> = vec![
        (ArabicDialect::Egyptian, EGYPTIAN_MARKERS),
        (ArabicDialect::Gulf, GULF_MARKERS),
        (ArabicDialect::Levantine, LEVANTINE_MARKERS),
        (ArabicDialect::Moroccan, MOROCCAN_MARKERS),
        (ArabicDialect::Iraqi, IRAQI_MARKERS),
        (ArabicDialect::Yemeni, YEMENI_MARKERS),
        (ArabicDialect::Sudanese, SUDANESE_MARKERS),
    ];

    // Tokenize on whitespace + common Arabic punctuation. We
    // count tokens that appear verbatim in a marker list (no
    // morphological analysis — markers are chosen to be
    // self-contained words).
    let tokens: Vec<&str> = text
        .split(|c: char| {
            c.is_whitespace()
                || c == '.'
                || c == ','
                || c == '!'
                || c == '?'
                || c == '،'
                || c == '؟'
                || c == '؛'
        })
        .filter(|t| !t.is_empty())
        .collect();

    let mut indicator_words: Vec<String> = Vec::new();
    let mut scores: Vec<(ArabicDialect, u32)> = Vec::with_capacity(buckets.len());

    for (dialect, markers) in buckets.drain(..) {
        let mut count = 0u32;
        for tok in &tokens {
            if markers.contains(tok) {
                count = count.saturating_add(1);
                let owned = tok.to_string();
                if !indicator_words.contains(&owned) {
                    indicator_words.push(owned);
                }
            }
        }
        scores.push((dialect, count));
    }

    // Shared `مش` — credit Levantine ONLY if no Egyptian
    // marker dominates (otherwise it stays as Egyptian-leaning
    // noise that the Egyptian bucket already captured).
    let egyptian_score = scores
        .iter()
        .find(|(d, _)| matches!(d, ArabicDialect::Egyptian))
        .map(|(_, n)| *n)
        .unwrap_or(0);
    if egyptian_score == 0 {
        let neg_count = tokens.iter().filter(|t| **t == SHARED_NEGATION).count() as u32;
        if neg_count > 0 {
            for entry in scores.iter_mut() {
                if matches!(entry.0, ArabicDialect::Levantine) {
                    entry.1 = entry.1.saturating_add(neg_count);
                    let owned = SHARED_NEGATION.to_string();
                    if !indicator_words.contains(&owned) {
                        indicator_words.push(owned);
                    }
                }
            }
        }
    }

    // Pick the dialect with the highest count; ties broken by
    // declaration order (Egyptian first as the "broadest"
    // vernacular).
    scores.sort_by(|a, b| b.1.cmp(&a.1));
    let (top_dialect, top_score) = scores[0];

    if top_score < MIN_MARKERS_FOR_CONFIDENT_CALL {
        // Not enough signal. If we saw zero markers, label
        // ModernStandard (no vernacular fingerprint); if we
        // saw exactly one marker, fall back to Unknown to
        // avoid false-positive reclassification.
        let dialect = if top_score == 0 {
            ArabicDialect::ModernStandard
        } else {
            ArabicDialect::Unknown
        };
        let confidence = if top_score == 0 { 0.4 } else { 0.2 };
        return DialectAnalysis {
            detected_dialect: dialect,
            confidence,
            indicator_words,
            advisory: ADVISORY_TEXT.to_string(),
        };
    }

    // Confidence: 2 markers → 0.55; 5+ → 0.95. The curve is
    // tight on purpose — examiners should not over-trust the
    // dialect label.
    let confidence = (top_score.min(5) as f32 / 5.0) * 0.55 + 0.4;
    let confidence = confidence.clamp(0.4, 0.95);

    DialectAnalysis {
        detected_dialect: top_dialect,
        confidence,
        indicator_words,
        advisory: ADVISORY_TEXT.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egyptian_markers_detected() {
        let text = "إيه ده؟ عايز كده بجد، ازيك؟";
        let r = detect_arabic_dialect(text);
        assert_eq!(
            r.detected_dialect,
            ArabicDialect::Egyptian,
            "got {:?} (indicators: {:?})",
            r.detected_dialect,
            r.indicator_words
        );
        assert!(
            r.confidence >= 0.55,
            "confidence too low: {}",
            r.confidence
        );
        assert!(r.indicator_words.contains(&"إيه".to_string()));
    }

    #[test]
    fn gulf_markers_detected() {
        let text = "وش تبي؟ زين وايد كذا";
        let r = detect_arabic_dialect(text);
        assert_eq!(r.detected_dialect, ArabicDialect::Gulf);
        assert!(r.indicator_words.contains(&"وش".to_string()));
    }

    #[test]
    fn levantine_markers_detected() {
        let text = "شو هيك؟ بدي اروح هلق يلا";
        let r = detect_arabic_dialect(text);
        assert_eq!(r.detected_dialect, ArabicDialect::Levantine);
    }

    #[test]
    fn moroccan_markers_detected() {
        let text = "واش بزاف؟ كيداير، باغي ندير دابا";
        let r = detect_arabic_dialect(text);
        assert_eq!(r.detected_dialect, ArabicDialect::Moroccan);
    }

    #[test]
    fn no_markers_returns_modern_standard_or_unknown() {
        // Generic MSA — no vernacular fingerprint at all.
        let text = "مرحبا بالعالم. اليوم الجو جميل جدا";
        let r = detect_arabic_dialect(text);
        assert!(
            matches!(
                r.detected_dialect,
                ArabicDialect::ModernStandard | ArabicDialect::Unknown
            ),
            "got {:?}",
            r.detected_dialect
        );
    }

    #[test]
    fn dialect_advisory_present_when_dialect_detected() {
        let text = "إيه ده؟ عايز كده بجد";
        let r = detect_arabic_dialect(text);
        if r.confidence >= 0.55 {
            assert!(!r.advisory.is_empty());
            assert!(r.advisory.contains("Dialect"));
            assert!(r.advisory.contains("human linguist"));
        }
    }

    #[test]
    fn empty_input_returns_unknown_with_no_advisory() {
        let r = detect_arabic_dialect("");
        assert_eq!(r.detected_dialect, ArabicDialect::Unknown);
        assert_eq!(r.confidence, 0.0);
        assert!(r.advisory.is_empty());
    }

    #[test]
    fn single_marker_is_not_enough_to_reclassify() {
        // Only one marker → confidence floor of 0.2 + Unknown.
        let r = detect_arabic_dialect("شو حالك");
        assert!(matches!(
            r.detected_dialect,
            ArabicDialect::Unknown
        ));
    }
}
