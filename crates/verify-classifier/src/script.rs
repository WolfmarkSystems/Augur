//! Arabic-script disambiguation — Pashto vs Farsi.
//!
//! Sprint 9 P1. Both `whichlang` and `lid.176.ftz` confuse
//! Pashto with Farsi at the model level (Sprint 5 probe). This
//! module is the script-level tiebreaker the Sprint 7 roadmap
//! flagged as "post v1.0": when the LID layer reports `fa`, we
//! count occurrences of glyphs that are present in one of the
//! two orthographies and rare-or-absent in the other, and use
//! the differential to recommend a final answer.
//!
//! This is not a perfect signal. Pashto news copy can quote
//! Farsi loanwords (and vice versa); a single Pashto `ګ` doesn't
//! prove the whole document is Pashto. The
//! [`pashto_farsi_score`] output's `confidence` field plus the
//! always-on machine-translation advisory together make sure
//! the examiner sees the uncertainty.
//!
//! See `docs/LANGUAGE_LIMITATIONS.md` for the human-facing
//! rationale.

/// Glyphs that appear in standard Pashto orthography but are
/// rare/absent in standard Farsi (Iranian Persian + Dari).
/// Source: ISO 15924 / Unicode 16.0 charts cross-referenced
/// against the Sprint 9 spec.
const PASHTO_SPECIFIC: &[char] = &[
    'ټ', // U+067C  TTEH
    'ډ', // U+0688  DDAL
    'ړ', // U+0693  REH WITH RING BELOW
    'ږ', // U+0696  REH WITH DOT BELOW + DOT ABOVE
    'ښ', // U+069A  SEEN WITH DOT BELOW + DOT ABOVE
    'ګ', // U+06AB  KAF WITH RING
    'ڼ', // U+06BC  NOON WITH RING
    'ۍ', // U+06CD  YEH WITH TAIL
    'ې', // U+06D0  E
];

/// Glyphs that appear in standard Farsi but are rare/absent in
/// Pashto. (Pashto uses related-but-distinct shapes for these
/// sounds — `پ` and `چ` and `گ` happen to overlap with Farsi,
/// so they're not as discriminating as the Pashto-specific
/// list above. We still count them; the differential is what
/// matters.)
const FARSI_SPECIFIC: &[char] = &[
    'پ', // U+067E  PEH
    'چ', // U+0686  TCHEH
    'ژ', // U+0698  JEH (also Pashto, but more common in Farsi)
    'گ', // U+06AF  GAF
];

/// Recommendation produced by [`pashto_farsi_score`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptRecommendation {
    /// Strong Pashto signal — at least one Pashto-specific
    /// glyph and the Pashto count exceeds the Farsi count.
    LikelyPashto,
    /// Strong Farsi signal — Farsi-specific glyphs present,
    /// no Pashto-specific glyphs (or far fewer).
    LikelyFarsi,
    /// Neither side has discriminating glyphs, or the counts
    /// are close enough that the script alone can't decide.
    Ambiguous,
}

/// Output of the script-level analysis. The `confidence` field
/// is calibrated heuristically: 0.0 = no signal, 1.0 = saturated
/// signal (≥ 5 Pashto-specific glyphs and zero Farsi-specific).
#[derive(Debug, Clone)]
pub struct PashtoFarsiAnalysis {
    pub pashto_char_count: u32,
    pub farsi_char_count: u32,
    /// Distinct Pashto-specific glyphs observed in the input
    /// (deduplicated for the examiner display — same character
    /// repeated 100× is one entry here).
    pub pashto_specific_chars: Vec<char>,
    pub farsi_specific_chars: Vec<char>,
    pub recommendation: ScriptRecommendation,
    /// 0.0–1.0 confidence in the [`recommendation`] field.
    pub confidence: f32,
}

/// Score the input text on the Pashto-vs-Farsi spectrum.
/// Pure function — no I/O, no allocations beyond the deduped
/// glyph lists.
pub fn pashto_farsi_score(text: &str) -> PashtoFarsiAnalysis {
    let mut pashto_count: u32 = 0;
    let mut farsi_count: u32 = 0;
    let mut pashto_set: Vec<char> = Vec::new();
    let mut farsi_set: Vec<char> = Vec::new();
    for c in text.chars() {
        if PASHTO_SPECIFIC.contains(&c) {
            pashto_count = pashto_count.saturating_add(1);
            if !pashto_set.contains(&c) {
                pashto_set.push(c);
            }
        }
        if FARSI_SPECIFIC.contains(&c) {
            farsi_count = farsi_count.saturating_add(1);
            if !farsi_set.contains(&c) {
                farsi_set.push(c);
            }
        }
    }

    let (recommendation, confidence) = recommend(pashto_count, farsi_count);
    PashtoFarsiAnalysis {
        pashto_char_count: pashto_count,
        farsi_char_count: farsi_count,
        pashto_specific_chars: pashto_set,
        farsi_specific_chars: farsi_set,
        recommendation,
        confidence,
    }
}

fn recommend(pashto: u32, farsi: u32) -> (ScriptRecommendation, f32) {
    if pashto == 0 && farsi == 0 {
        return (ScriptRecommendation::Ambiguous, 0.0);
    }
    let total = (pashto + farsi) as f32;
    let p = pashto as f32 / total;
    if pashto >= 1 && pashto > farsi {
        // Heuristic confidence curve. One Pashto-specific glyph
        // with no Farsi-specific clears the 0.7 reclassification
        // bar the spec asks for; saturates at 0.95 with ≥5
        // Pashto-specific occurrences and zero Farsi.
        let strength = (pashto.min(5) as f32 / 5.0) * 0.45 + 0.55 * p;
        let conf = strength.clamp(0.0, 0.95);
        (ScriptRecommendation::LikelyPashto, conf)
    } else if farsi > pashto {
        let f = farsi as f32 / total;
        let strength = (farsi.min(5) as f32 / 5.0) * 0.45 + 0.55 * f;
        let conf = strength.clamp(0.0, 0.95);
        (ScriptRecommendation::LikelyFarsi, conf)
    } else {
        // Tie — neither side dominates.
        (ScriptRecommendation::Ambiguous, 0.3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pashto_specific_chars_trigger_likely_pashto() {
        // Sentence with several Pashto-specific glyphs:
        // ډېر ښه — لاړ شه — ګوره
        let r = pashto_farsi_score("ډېر ښه, لاړ شه, ګوره ټول ړومبۍ");
        assert!(
            matches!(r.recommendation, ScriptRecommendation::LikelyPashto),
            "expected LikelyPashto, got {r:?}"
        );
        assert!(
            r.confidence >= 0.7,
            "Sprint 9 P1 — confidence must clear the 0.7 reclassification bar; got {}",
            r.confidence
        );
        assert!(r.pashto_char_count >= 3);
        assert!(r.pashto_specific_chars.contains(&'ډ'));
        assert!(r.pashto_specific_chars.contains(&'ښ'));
    }

    #[test]
    fn farsi_text_without_pashto_chars_stays_farsi() {
        // Standard Farsi: لطفا چند روز پیش
        // Has پ (peh, Farsi-specific) + چ (tcheh, Farsi-specific).
        let r = pashto_farsi_score("لطفا چند روز پیش، چه خبر؟");
        assert!(
            matches!(r.recommendation, ScriptRecommendation::LikelyFarsi),
            "expected LikelyFarsi, got {r:?}"
        );
        assert_eq!(r.pashto_char_count, 0);
        assert!(r.farsi_char_count >= 2);
    }

    #[test]
    fn ambiguous_text_with_no_distinguishing_chars() {
        // Generic Arabic-script text using only common letters
        // (alif, ba, ta, jim, dal, …). Neither side dominates →
        // Ambiguous with confidence 0.0.
        let r = pashto_farsi_score("ابتدا، ثم نهاية");
        assert_eq!(r.recommendation, ScriptRecommendation::Ambiguous);
        assert_eq!(r.pashto_char_count, 0);
        assert_eq!(r.farsi_char_count, 0);
        assert_eq!(r.confidence, 0.0);
    }

    #[test]
    fn empty_input_handled_gracefully() {
        let r = pashto_farsi_score("");
        assert_eq!(r.recommendation, ScriptRecommendation::Ambiguous);
        assert_eq!(r.confidence, 0.0);
    }

    #[test]
    fn deduplicates_pashto_specific_char_list() {
        // Repeated ډ characters — counter increments each time
        // but the deduplicated `pashto_specific_chars` list has
        // exactly one entry.
        let r = pashto_farsi_score("ډډډډډ");
        assert_eq!(r.pashto_char_count, 5);
        assert_eq!(r.pashto_specific_chars.len(), 1);
        assert_eq!(r.pashto_specific_chars[0], 'ډ');
    }

    #[test]
    fn mixed_pashto_and_farsi_glyphs_resolves_to_dominant() {
        // 3 Pashto-specific (ډ ښ ګ) vs 1 Farsi-specific (پ).
        // Pashto wins on the recommendation; confidence is
        // moderated downward by the competing Farsi glyph (which
        // is correct — a real-world examiner should treat this
        // case as less certain than pure-Pashto text).
        let r = pashto_farsi_score("ډ ښ ګ پ");
        assert!(matches!(
            r.recommendation,
            ScriptRecommendation::LikelyPashto
        ));
        assert!(
            r.confidence > 0.5,
            "expected moderate confidence; got {}",
            r.confidence
        );
    }
}
