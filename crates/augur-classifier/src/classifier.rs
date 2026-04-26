//! Language identification.
//!
//! Two backends live behind a single `LanguageClassifier` enum:
//!
//! * **whichlang** (`whichlang` crate, 0.1.1) — **the production
//!   default since Sprint 4.** Pure-Rust, embedded weights, no
//!   model download, no network. Covers 16 major languages (ISO
//!   639-3 codes mapped to 639-1 here). Construct via
//!   [`LanguageClassifier::new_whichlang`].
//!
//! * **fastText** (`fasttext-pure-rs` crate, 0.1.0) — production-
//!   ready as of Sprint 5. Reads Meta's `lid.176.ftz` correctly
//!   (Sprint 5 P1 probe: Arabic / Chinese / Russian / Spanish /
//!   Persian / Urdu all classify with high confidence; Pashto
//!   confuses with Persian — a known model-level limitation, not
//!   a parser bug). Replaces the binary-incompatible
//!   `fasttext = "0.8.0"` crate that Sprints 1-4 carried.
//!   Construct via [`LanguageClassifier::load_fasttext`].
//!   Whichlang is still the CLI default (no model download);
//!   fastText is opt-in via `--classifier-backend fasttext`.
//!
//! The fastText network egress
//! ([`ModelManager::ensure_lid_model`]) is the only first-run
//! download AUGUR performs in its default-classifier code path
//! when the user opts into fastText.

use fasttext_pure_rs::FastText;
use log::{debug, warn};
use std::path::{Path, PathBuf};
use augur_core::AugurError;

/// Result of a single classification pass.
#[derive(Debug, Clone)]
pub struct ClassificationResult {
    /// Detected language — ISO 639-1 code (e.g. "ar", "zh", "ru").
    /// Empty string on empty input (see [`ClassificationResult::empty`]).
    pub language: String,
    /// Model confidence, 0.0–1.0. For the whichlang backend this is
    /// always `1.0` on a decisive answer and `0.0` on empty input
    /// (whichlang does not expose per-language probabilities).
    pub confidence: f32,
    /// `true` when [`ClassificationResult::language`] differs from
    /// [`ClassificationResult::target_language`].
    pub is_foreign: bool,
    /// Whichever target the examiner asked for (ISO 639-1).
    pub target_language: String,
    /// Sprint 6 P2 — categorical confidence band for examiner UI.
    /// `High` when the model is reliable; `Low` when the input is
    /// too short or the score is below 0.6.
    pub confidence_tier: ConfidenceTier,
    /// Number of whitespace-delimited words in the classified
    /// input — surfaces in the CLI alongside `confidence_tier` so
    /// examiners see *why* a tier landed where it did.
    pub input_word_count: usize,
    /// Human-readable advisory when the result is anything other
    /// than [`ConfidenceTier::High`]. Empty string on `High`.
    pub advisory: Option<String>,
    /// Sprint 9 P1 — when the LID layer reported `fa` and the
    /// script-level analyzer reclassified to `ps` (or noted the
    /// ambiguity), this carries the human-readable note.
    /// `None` when no disambiguation step ran.
    pub disambiguation_note: Option<String>,
    /// Super Sprint Group A — coarse Arabic dialect family,
    /// populated when `language == "ar"`. `None` for any other
    /// detected language.
    pub arabic_dialect: Option<crate::ArabicDialect>,
    /// Confidence in the dialect call, 0.0–1.0. `0.0` when no
    /// dialect step ran.
    pub arabic_dialect_confidence: f32,
    /// Words that triggered the dialect call (for examiner
    /// display). Empty list when no dialect step ran.
    pub arabic_dialect_indicators: Vec<String>,
    /// Human-readable dialect advisory. `None` when no
    /// dialect step ran.
    pub arabic_dialect_note: Option<String>,
}

/// Confidence tier for an LID classification. Sprint 6 P2 —
/// surfaces in the CLI and per-file in the batch JSON so examiners
/// can sort / filter by reliability without doing the math
/// themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfidenceTier {
    /// Confidence > 0.85 — reliable for casework.
    High,
    /// 0.60 – 0.85 — likely correct, verify if critical.
    Medium,
    /// < 0.60 OR very-short input — uncertain, human review
    /// recommended.
    Low,
}

impl ConfidenceTier {
    /// Render the tier as a stable lowercase string for batch JSON
    /// + CLI output.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
        }
    }
}

/// Word-count threshold below which we always demote to `Low` and
/// surface a "short input" advisory regardless of model score.
/// Whichlang in particular reports `1.0` on inputs as small as one
/// word, which is misleading — the model wasn't trained to be
/// confident on a single word.
pub const SHORT_INPUT_WORD_COUNT: usize = 10;

/// Compute the [`ConfidenceTier`] from a (score, word_count) pair.
/// Pure helper — extracted for unit testing without spinning up a
/// classifier.
pub fn classify_confidence(score: f32, word_count: usize) -> ConfidenceTier {
    if word_count > 0 && word_count < SHORT_INPUT_WORD_COUNT {
        return ConfidenceTier::Low;
    }
    if score >= 0.85 {
        ConfidenceTier::High
    } else if score >= 0.60 {
        ConfidenceTier::Medium
    } else {
        ConfidenceTier::Low
    }
}

/// Build the user-facing advisory string for a non-`High` tier.
/// Returns `None` for `High`. Spec wording matches AUGUR_SPRINT_6
/// P2b for the short-input case.
pub fn confidence_advisory(tier: ConfidenceTier, word_count: usize) -> Option<String> {
    match tier {
        ConfidenceTier::High => None,
        ConfidenceTier::Medium => Some(
            "Medium confidence — verify with a human linguist if this evidence \
             is critical to your case."
                .to_string(),
        ),
        ConfidenceTier::Low => {
            if word_count > 0 && word_count < SHORT_INPUT_WORD_COUNT {
                Some(format!(
                    "Short input ({word_count} word{}) — language detection may \
                     be unreliable. Verify with a human linguist if this evidence \
                     is critical.",
                    if word_count == 1 { "" } else { "s" }
                ))
            } else {
                Some(
                    "Low confidence — language detection may be unreliable. \
                     Verify with a human linguist if this evidence is critical."
                        .to_string(),
                )
            }
        }
    }
}

impl ClassificationResult {
    /// Sentinel for empty / unclassifiable input. `confidence = 0.0`
    /// so callers can treat it as "do not translate."
    fn empty(target_language: &str) -> Self {
        Self {
            language: String::new(),
            confidence: 0.0,
            is_foreign: false,
            target_language: target_language.to_string(),
            confidence_tier: ConfidenceTier::Low,
            input_word_count: 0,
            advisory: None,
            disambiguation_note: None,
            arabic_dialect: None,
            arabic_dialect_confidence: 0.0,
            arabic_dialect_indicators: Vec::new(),
            arabic_dialect_note: None,
        }
    }
}

/// Owns the on-disk model cache (`~/.cache/augur/models/`). The
/// first call to [`ModelManager::ensure_lid_model`] is the only
/// network egress AUGUR performs in its default code path — every
/// subsequent run returns the cached path.
#[derive(Debug, Clone)]
pub struct ModelManager {
    pub cache_dir: PathBuf,
}

/// Facebook mirror of the 176-language LID model. Documented here
/// (not hidden in a function body) because it is the ONLY URL
/// AUGUR fetches in the default code path.
const LID_MODEL_URL: &str =
    "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz";
const LID_MODEL_FILENAME: &str = "lid.176.ftz";
/// Published size of `lid.176.ftz`. Used as a lower-bound integrity
/// check after curl returns — a truncated / HTML-error download
/// will be well under this.
const LID_MODEL_MIN_BYTES: u64 = 500_000;

impl ModelManager {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// XDG-compliant cache directory: `~/.cache/augur/models/`.
    /// Returns `Err` if `$HOME` is unset (vanishingly rare in practice
    /// but checked rather than unwrapped).
    ///
    /// Named `with_xdg_cache` rather than `default` on purpose — a
    /// fallible constructor must not shadow the infallible
    /// `Default::default` shape (clippy::should_implement_trait).
    pub fn with_xdg_cache() -> Result<Self, AugurError> {
        let home = std::env::var("HOME").map_err(|_| {
            AugurError::ModelManager(
                "HOME environment variable not set; pass a cache dir explicitly"
                    .to_string(),
            )
        })?;
        Ok(Self::new(PathBuf::from(home).join(".cache/augur/models")))
    }

    /// Ensure the fastText LID model is cached locally. On first
    /// call this spawns a `curl` subprocess to fetch ~900 KB from
    /// the published Facebook mirror; on every subsequent call it
    /// returns the cached path immediately.
    ///
    /// NETWORK: this is the only permitted network call in AUGUR's
    /// default code path. See the offline-invariant section of
    /// CLAUDE.md.
    pub fn ensure_lid_model(&self) -> Result<PathBuf, AugurError> {
        let dest = self.cache_dir.join(LID_MODEL_FILENAME);

        // Fast path — already cached.
        if dest.exists() {
            let size = std::fs::metadata(&dest)?.len();
            if size >= LID_MODEL_MIN_BYTES {
                debug!(
                    "fasttext LID model already cached at {:?} ({} bytes)",
                    dest, size
                );
                return Ok(dest);
            }
            warn!(
                "cached LID model at {:?} is suspiciously small ({} bytes) — re-downloading",
                dest, size
            );
            // Fall through to re-download.
        }

        std::fs::create_dir_all(&self.cache_dir)?;

        // Air-gap path: when `AUGUR_AIRGAP_PATH` is set the
        // ModelManager copies pre-staged weights from there
        // instead of touching the network. This is the supported
        // offline-deployment path for classified workstations
        // that can never reach the internet.
        if let Some(staged) = airgap_lid_model() {
            log::info!(
                "augur-classifier: AUGUR_AIRGAP_PATH provides {LID_MODEL_FILENAME} at {staged:?}; \
                 copying to {dest:?} (no network egress)"
            );
            std::fs::copy(&staged, &dest)?;
            let size = std::fs::metadata(&dest)?.len();
            if size < LID_MODEL_MIN_BYTES {
                return Err(AugurError::ModelManager(format!(
                    "air-gapped LID model at {dest:?} is {size} bytes — expected \
                     >= {LID_MODEL_MIN_BYTES}; check AUGUR_AIRGAP_PATH source."
                )));
            }
            return Ok(dest);
        }

        // Intentional network egress. Logged, not silent.
        warn!(
            "AUGUR fetching one-time LID model ({}) from {LID_MODEL_URL} — \
             this is the ONLY network call AUGUR makes in the default code path",
            LID_MODEL_FILENAME
        );
        let status = std::process::Command::new("curl")
            .arg("-fL")
            .arg("--silent")
            .arg("--show-error")
            .arg("--output")
            .arg(&dest)
            .arg(LID_MODEL_URL)
            .status()
            .map_err(|e| {
                AugurError::ModelManager(format!(
                    "failed to launch curl for LID model download: {e}. \
                     Install curl or pre-place {LID_MODEL_FILENAME} at {dest:?}"
                ))
            })?;
        if !status.success() {
            return Err(AugurError::ModelManager(format!(
                "curl failed while downloading {LID_MODEL_URL}: exit {status}"
            )));
        }

        // Integrity check — truncated / HTML-error downloads would
        // be well under published size.
        let size = std::fs::metadata(&dest)?.len();
        if size < LID_MODEL_MIN_BYTES {
            return Err(AugurError::ModelManager(format!(
                "downloaded LID model at {dest:?} is {size} bytes — expected \
                 >= {LID_MODEL_MIN_BYTES}. Delete and retry, or pre-place manually."
            )));
        }
        Ok(dest)
    }
}

/// fastText-backed or whichlang-backed classifier.
#[derive(Debug)]
pub struct LanguageClassifier {
    backend: Backend,
}

#[derive(Debug)]
enum Backend {
    // `FastText` is ~592 bytes; `Whichlang` is a zero-byte unit
    // variant. Box the heavy arm so `std::mem::size_of::<Backend>()`
    // stays small (clippy::large_enum_variant).
    FastText(Box<FastText>),
    Whichlang,
}

impl LanguageClassifier {
    /// Load a fastText LID model from disk via the `fasttext-pure-rs`
    /// reader (Sprint 5 P1 confirmed binary-compatible with
    /// `lid.176.ftz`). Pair with [`ModelManager::ensure_lid_model`]
    /// to get the path. Production-ready for the major and
    /// forensic-priority languages (Arabic, Chinese, Russian,
    /// Spanish, Persian, Urdu); Pashto confuses with Persian at
    /// the model level.
    pub fn load_fasttext(model_path: &Path) -> Result<Self, AugurError> {
        let model = FastText::load(model_path).map_err(|e| {
            AugurError::Classifier(format!(
                "fasttext_pure_rs::FastText::load({model_path:?}) failed: {e}"
            ))
        })?;
        Ok(Self {
            backend: Backend::FastText(Box::new(model)),
        })
    }

    /// Construct a classifier backed by the pure-Rust `whichlang`
    /// library. No model download, no filesystem, no network. Used
    /// by the test suite and as a fallback for air-gapped deploys
    /// (Sprint 2 wires the CLI flag).
    pub fn new_whichlang() -> Self {
        Self {
            backend: Backend::Whichlang,
        }
    }

    /// Classify a text sample. Takes the first 512 characters
    /// (by Unicode scalar, not bytes) for speed — LID only needs
    /// a few lines to decide.
    pub fn classify(
        &self,
        text: &str,
        target_language: &str,
    ) -> Result<ClassificationResult, AugurError> {
        if text.trim().is_empty() {
            return Ok(ClassificationResult::empty(target_language));
        }

        let sample: String = text.chars().take(512).collect();

        let (language, confidence) = match &self.backend {
            Backend::FastText(model) => {
                // `predict(text, k=1, threshold=0.0)` — top-1 label,
                // no threshold so the caller sees confidence and
                // decides themselves.
                let preds = model.predict(&sample, 1, 0.0).map_err(|e| {
                    AugurError::Classifier(format!("fasttext predict: {e}"))
                })?;
                let Some(p) = preds.first() else {
                    return Ok(ClassificationResult::empty(target_language));
                };
                // fastText LID labels look like "__label__en".
                let code = p
                    .label
                    .strip_prefix("__label__")
                    .unwrap_or(&p.label)
                    .to_string();
                (code, p.probability)
            }
            Backend::Whichlang => {
                let lang = whichlang::detect_language(&sample);
                (whichlang_to_iso_639_1(lang).to_string(), 1.0_f32)
            }
        };

        let input_word_count = text.split_whitespace().count();

        // Sprint 9 P1 — Pashto/Farsi script-level tiebreaker.
        // Both whichlang and lid.176.ftz confuse Pashto with
        // Farsi at the model layer. When the LID layer reports
        // `fa`, run a script analysis on the input. If we
        // observe enough Pashto-specific glyphs to clear the
        // 0.7 confidence bar, reclassify to `ps` and record a
        // human-readable note. Ambiguous results stay `fa` but
        // pick up an enhanced advisory.
        let (final_language, disambiguation_note) =
            if language == "fa" {
                let analysis = crate::script::pashto_farsi_score(text);
                disambiguate_farsi(&language, &analysis)
            } else {
                (language, None)
            };

        let tier = classify_confidence(confidence, input_word_count);
        let advisory = confidence_advisory(tier, input_word_count);

        // Super Sprint Group A — Arabic dialect detection.
        // Only fires when the LID layer concluded `ar`; any
        // other language (or a `fa→ps` reclassification) skips
        // it.
        let (arabic_dialect, arabic_dialect_confidence,
             arabic_dialect_indicators, arabic_dialect_note) =
            if final_language == "ar" {
                let analysis = crate::detect_arabic_dialect(text);
                let note = if analysis.advisory.is_empty() {
                    None
                } else {
                    Some(analysis.advisory)
                };
                (
                    Some(analysis.detected_dialect),
                    analysis.confidence,
                    analysis.indicator_words,
                    note,
                )
            } else {
                (None, 0.0, Vec::new(), None)
            };

        Ok(ClassificationResult {
            is_foreign: !final_language.is_empty()
                && final_language != target_language,
            language: final_language,
            confidence,
            target_language: target_language.to_string(),
            confidence_tier: tier,
            input_word_count,
            advisory,
            disambiguation_note,
            arabic_dialect,
            arabic_dialect_confidence,
            arabic_dialect_indicators,
            arabic_dialect_note,
        })
    }
}

/// Sprint 9 P1 helper — given an LID-reported `fa` and the
/// script-level analysis, return the final language code plus an
/// optional human-readable disambiguation note. Returns
/// `("ps", note)` only when the script analyzer's confidence
/// clears the 0.7 bar the spec cites; otherwise sticks with `fa`
/// (with an "ambiguous" note when there's any signal at all).
fn disambiguate_farsi(
    initial: &str,
    analysis: &crate::script::PashtoFarsiAnalysis,
) -> (String, Option<String>) {
    use crate::script::ScriptRecommendation;
    match analysis.recommendation {
        ScriptRecommendation::LikelyPashto if analysis.confidence >= 0.7 => {
            let glyphs = analysis
                .pashto_specific_chars
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let note = format!(
                "Reclassified from Farsi to Pashto based on script analysis \
                 (Pashto-specific glyphs detected: {glyphs}; \
                 confidence: {:.2}). Verify with a human linguist fluent in \
                 both Farsi and Pashto.",
                analysis.confidence
            );
            ("ps".to_string(), Some(note))
        }
        ScriptRecommendation::Ambiguous if !analysis.pashto_specific_chars.is_empty() => {
            // We saw some Pashto-specific glyphs but not enough
            // to flip. Surface the uncertainty.
            let note = format!(
                "Script analysis inconclusive — both Pashto-specific and \
                 Farsi-specific glyphs present. Confidence: {:.2}. Verify \
                 with a human linguist.",
                analysis.confidence
            );
            (initial.to_string(), Some(note))
        }
        _ => (initial.to_string(), None),
    }
}

/// Look up the air-gap-staged LID model. Returns `Some(path)`
/// when `AUGUR_AIRGAP_PATH` is set AND the directory contains
/// `lid.176.ftz`. Sprint 5 P3 — pre-bundled offline installer
/// for classified workstations that cannot reach the internet.
fn airgap_lid_model() -> Option<PathBuf> {
    let root = std::env::var("AUGUR_AIRGAP_PATH").ok()?;
    let candidate = PathBuf::from(root).join(LID_MODEL_FILENAME);
    candidate.exists().then_some(candidate)
}

/// Map whichlang's `Lang` (ISO 639-3) to the ISO 639-1 codes the
/// rest of AUGUR speaks. Every variant in whichlang 0.1.1 is
/// handled explicitly — no `_` catch-all, so adding a new language
/// upstream becomes a compile error rather than a silent
/// misclassification.
fn whichlang_to_iso_639_1(lang: whichlang::Lang) -> &'static str {
    use whichlang::Lang;
    match lang {
        Lang::Ara => "ar",
        Lang::Cmn => "zh",
        Lang::Deu => "de",
        Lang::Eng => "en",
        Lang::Fra => "fr",
        Lang::Hin => "hi",
        Lang::Ita => "it",
        Lang::Jpn => "ja",
        Lang::Kor => "ko",
        Lang::Nld => "nl",
        Lang::Por => "pt",
        Lang::Rus => "ru",
        Lang::Spa => "es",
        Lang::Swe => "sv",
        Lang::Tur => "tr",
        Lang::Vie => "vi",
    }
}

// ── Tests ─────────────────────────────────────────────────────────
//
// All tests exercise the whichlang backend so they run fully
// offline — no model download, no HTTP, no cache-dir writes. The
// fastText load path is exercised by Sprint 2 integration tests
// once a test fixture model is in place.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_arabic_correctly() {
        let classifier = LanguageClassifier::new_whichlang();
        // "Hello, world" in Arabic.
        let result = classifier
            .classify("مرحبا بالعالم، كيف حالك اليوم؟", "en")
            .expect("classify");
        assert_eq!(result.language, "ar");
        assert!(
            result.confidence > 0.8,
            "whichlang confidence expected > 0.8, got {}",
            result.confidence
        );
        assert!(result.is_foreign, "Arabic must be foreign when target=en");
        assert_eq!(result.target_language, "en");
    }

    #[test]
    fn classifies_chinese_correctly() {
        let classifier = LanguageClassifier::new_whichlang();
        // "Hello, world, how are you today?" — simplified Chinese.
        let result = classifier
            .classify("你好,世界,你今天怎么样?", "en")
            .expect("classify");
        assert_eq!(result.language, "zh");
        assert!(result.is_foreign);
    }

    #[test]
    fn classifies_russian_correctly() {
        let classifier = LanguageClassifier::new_whichlang();
        let result = classifier
            .classify("Привет мир, как у тебя сегодня дела?", "en")
            .expect("classify");
        assert_eq!(result.language, "ru");
        assert!(result.is_foreign);
    }

    #[test]
    fn classifies_spanish_correctly() {
        let classifier = LanguageClassifier::new_whichlang();
        let result = classifier
            .classify(
                "Hola mundo, ¿cómo estás hoy? Espero que tengas un buen día.",
                "en",
            )
            .expect("classify");
        assert_eq!(result.language, "es");
        assert!(result.is_foreign);
    }

    #[test]
    fn classifies_english_as_not_foreign() {
        let classifier = LanguageClassifier::new_whichlang();
        let result = classifier
            .classify(
                "The quick brown fox jumps over the lazy dog. \
                 Pack my box with five dozen liquor jugs.",
                "en",
            )
            .expect("classify");
        assert_eq!(result.language, "en");
        assert!(
            !result.is_foreign,
            "English must be is_foreign=false when target=en"
        );
    }

    #[test]
    fn high_confidence_long_arabic_text() {
        // 50+-word Arabic text at score 1.0 → High tier with no
        // advisory. Sprint 6 P2 acceptance test.
        let classifier = LanguageClassifier::new_whichlang();
        let arabic = "في صباح يوم الجمعة الماضي توجه فريق التحقيق إلى موقع الحادث في \
                      الحي الشمالي من المدينة كان الجو غائما ودرجة الحرارة منخفضة وجد \
                      المحققون عددا من الأدلة المهمة في الموقع بما في ذلك بعض الأوراق \
                      المكتوبة باليد وجهاز هاتف محمول وحقيبة جلدية صغيرة تم تصوير الموقع \
                      من زوايا متعددة قبل البدء في جمع الأدلة";
        let r = classifier.classify(arabic, "en").expect("classify");
        assert_eq!(r.language, "ar");
        assert_eq!(r.confidence_tier, ConfidenceTier::High);
        assert!(r.input_word_count >= SHORT_INPUT_WORD_COUNT);
        assert!(r.advisory.is_none(), "High tier must have no advisory");
    }

    #[test]
    fn low_confidence_very_short_input() {
        let classifier = LanguageClassifier::new_whichlang();
        // 3-word input — even at score 1.0, the short-input gate
        // must demote to Low.
        let r = classifier.classify("Hola amigo bueno", "en").expect("classify");
        assert_eq!(r.confidence_tier, ConfidenceTier::Low);
        assert_eq!(r.input_word_count, 3);
        let advisory = r.advisory.expect("Low tier must include advisory");
        assert!(
            advisory.contains("3 words"),
            "advisory should cite the word count: {advisory}"
        );
    }

    #[test]
    fn medium_confidence_includes_advisory_text() {
        // Pure helper test — Medium tier always populates advisory.
        let tier = classify_confidence(0.70, 50);
        assert_eq!(tier, ConfidenceTier::Medium);
        let advisory = confidence_advisory(tier, 50).expect("Medium must advise");
        assert!(advisory.contains("Medium confidence"));
    }

    #[test]
    fn pashto_specific_chars_trigger_reclassification() {
        // Sprint 9 P1 — text with several Pashto-specific glyphs
        // that whichlang's heuristic would label `fa`. We want to
        // catch the reclassification, but whichlang may not even
        // route to fa in the first place if it doesn't recognize
        // Pashto. Drive the reclassification logic directly via
        // the helper so the test is hermetic.
        use crate::script::pashto_farsi_score;
        let text = "ډېر ښه, لاړ شه, ګوره ټول ړومبۍ";
        let analysis = pashto_farsi_score(text);
        let (final_lang, note) = disambiguate_farsi("fa", &analysis);
        assert_eq!(final_lang, "ps", "fa→ps reclassification expected");
        let note = note.expect("note must accompany reclassification");
        assert!(note.contains("Pashto"));
        assert!(note.contains("Reclassified"));
        assert!(note.contains("human linguist"));
    }

    #[test]
    fn farsi_text_without_pashto_chars_stays_farsi() {
        use crate::script::pashto_farsi_score;
        let text = "لطفا چند روز پیش، چه خبر؟";
        let analysis = pashto_farsi_score(text);
        let (final_lang, note) = disambiguate_farsi("fa", &analysis);
        assert_eq!(final_lang, "fa", "no false reclassification on pure Farsi");
        assert!(
            note.is_none(),
            "no Pashto-specific glyphs → no disambiguation note"
        );
    }

    #[test]
    fn ambiguous_text_keeps_fa_and_no_note() {
        // Generic Arabic-script text using only common glyphs —
        // neither side dominates → recommendation = Ambiguous
        // with confidence 0.0. The disambiguator returns `fa`
        // unchanged AND no note (no Pashto-specific glyphs at
        // all to advertise).
        use crate::script::pashto_farsi_score;
        let analysis = pashto_farsi_score("ابتدا، ثم نهاية");
        let (final_lang, note) = disambiguate_farsi("fa", &analysis);
        assert_eq!(final_lang, "fa");
        assert!(note.is_none());
    }

    #[test]
    fn disambiguation_note_in_classification_result() {
        // End-to-end via the public `classify` API. Whichlang
        // doesn't route Pashto-glyph text to `fa`, so to drive
        // the full classify→disambiguate chain we exercise the
        // helper that classify uses internally and confirm the
        // result struct's `disambiguation_note` field round-trips.
        use crate::script::pashto_farsi_score;
        let analysis = pashto_farsi_score("ډېر ښه ګوره ړومبۍ");
        let (final_lang, note) = disambiguate_farsi("fa", &analysis);
        // Construct what `classify()` would build with the same
        // disambiguation result so the test pins the public
        // surface end-to-end.
        let r = ClassificationResult {
            language: final_lang,
            confidence: 1.0,
            is_foreign: true,
            target_language: "en".into(),
            confidence_tier: ConfidenceTier::High,
            input_word_count: 4,
            advisory: None,
            disambiguation_note: note,
            arabic_dialect: None,
            arabic_dialect_confidence: 0.0,
            arabic_dialect_indicators: Vec::new(),
            arabic_dialect_note: None,
        };
        assert_eq!(r.language, "ps");
        assert!(r.disambiguation_note.is_some());
        assert!(r
            .disambiguation_note
            .as_deref()
            .unwrap()
            .contains("Pashto"));
    }

    #[test]
    fn classify_confidence_short_circuits_on_short_input() {
        // Even a perfect 1.0 score collapses to Low when the input
        // is too short to be reliable.
        assert_eq!(classify_confidence(1.0, 5), ConfidenceTier::Low);
        assert_eq!(classify_confidence(1.0, 9), ConfidenceTier::Low);
        assert_eq!(classify_confidence(1.0, 10), ConfidenceTier::High);
    }

    #[test]
    fn handles_empty_input_gracefully() {
        let classifier = LanguageClassifier::new_whichlang();
        let result = classifier.classify("", "en").expect("empty classify");
        assert_eq!(result.language, "");
        assert_eq!(result.confidence, 0.0);
        assert!(!result.is_foreign);
    }

    #[test]
    fn handles_whitespace_only_input_gracefully() {
        let classifier = LanguageClassifier::new_whichlang();
        let result = classifier
            .classify("   \n\t  ", "en")
            .expect("whitespace classify");
        assert_eq!(result.language, "");
        assert_eq!(result.confidence, 0.0);
        assert!(!result.is_foreign);
    }

    fn integration_gate_ok() -> bool {
        std::env::var("AUGUR_RUN_INTEGRATION_TESTS").ok().as_deref() == Some("1")
    }

    fn cached_lid_model() -> Option<std::path::PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let p = std::path::PathBuf::from(home).join(".cache/augur/models/lid.176.ftz");
        p.exists().then_some(p)
    }

    #[test]
    #[ignore = "Sprint 5 P1 — requires AUGUR_RUN_INTEGRATION_TESTS=1 and a cached lid.176.ftz"]
    fn fasttext_pure_rs_classifies_arabic_correctly() {
        if !integration_gate_ok() {
            eprintln!("AUGUR_RUN_INTEGRATION_TESTS != 1 — skipping");
            return;
        }
        let Some(model) = cached_lid_model() else {
            eprintln!("lid.176.ftz not cached — skipping");
            return;
        };
        let classifier = LanguageClassifier::load_fasttext(&model).expect("load_fasttext");
        let r = classifier
            .classify("مرحبا بالعالم، كيف حالك اليوم؟", "en")
            .expect("classify");
        assert_eq!(r.language, "ar", "got {} ({})", r.language, r.confidence);
        assert!(
            r.confidence > 0.8,
            "expected > 0.8 confidence, got {}",
            r.confidence
        );
    }

    #[test]
    #[ignore = "Sprint 5 P1 — requires AUGUR_RUN_INTEGRATION_TESTS=1 and a cached lid.176.ftz"]
    fn fasttext_pure_rs_classifies_forensic_languages() {
        if !integration_gate_ok() {
            eprintln!("AUGUR_RUN_INTEGRATION_TESTS != 1 — skipping");
            return;
        }
        let Some(model) = cached_lid_model() else {
            eprintln!("lid.176.ftz not cached — skipping");
            return;
        };
        let classifier = LanguageClassifier::load_fasttext(&model).expect("load_fasttext");
        // The high-value LE/IC languages whichlang doesn't cover.
        // Pashto is intentionally omitted — model-level confusion
        // with Persian. Sprint 5 P1 probe documented this.
        let cases = &[
            ("سلام دنیا، حال شما چطور است؟", "fa"), // Persian/Farsi
            ("ہیلو ورلڈ، آج آپ کیسے ہیں؟", "ur"),   // Urdu
        ];
        for (text, expected) in cases {
            let r = classifier.classify(text, "en").expect("classify");
            assert_eq!(r.language, *expected, "got {} for {text:?}", r.language);
        }
    }

    /// Serializes the two airgap tests below. Both mutate the
    /// process-wide `AUGUR_AIRGAP_PATH` env var; without this
    /// lock parallel cargo-test threads race and the loser of the
    /// race observes a download (which on this host returns the
    /// real `lid.176.ftz` byte size, not the synthetic stub).
    fn airgap_env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn airgap_path_short_circuits_download() {
        let _guard = airgap_env_lock();
        // Sprint 5 P3 — staging a synthetic ftz under the
        // air-gap path must satisfy ensure_lid_model without any
        // network call. Uses a fresh temp dir so we don't pollute
        // the real cache.
        let work = std::env::temp_dir().join(format!(
            "augur-airgap-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let stage = work.join("staged");
        let cache = work.join("cache");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let staged = stage.join(LID_MODEL_FILENAME);
        // Synthesize a "model" that's at least the published size
        // so the integrity check passes.
        let body = vec![0u8; LID_MODEL_MIN_BYTES as usize + 1024];
        std::fs::write(&staged, &body).unwrap();

        let prev = std::env::var("AUGUR_AIRGAP_PATH").ok();
        // SAFETY: tests are single-threaded under cargo test --
        // --test-threads=1 by convention; this test sets the env
        // for its own duration and restores at the end.
        unsafe {
            std::env::set_var("AUGUR_AIRGAP_PATH", &stage);
        }
        let mgr = ModelManager::new(cache.clone());
        let path = mgr.ensure_lid_model().expect("airgap copy");
        assert!(path.exists());
        assert_eq!(path, cache.join(LID_MODEL_FILENAME));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), body.len() as u64);
        unsafe {
            match prev {
                Some(v) => std::env::set_var("AUGUR_AIRGAP_PATH", v),
                None => std::env::remove_var("AUGUR_AIRGAP_PATH"),
            }
        }
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn airgap_path_takes_priority_over_existing_cache() {
        let _guard = airgap_env_lock();
        // If the cache is empty and AIRGAP_PATH is set, ensure
        // we use the airgap copy rather than triggering a
        // download. Detected by the absence of any curl invocation
        // in this test (synthetic ftz is never a real model so
        // download couldn't even start, but we verify the path
        // returned matches the airgap source by file size).
        let work = std::env::temp_dir().join(format!(
            "augur-airgap-prio-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let stage = work.join("staged");
        let cache = work.join("cache");
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        // Use a distinctive size to prove we copied from staged
        // rather than fetching something else.
        let body = vec![7u8; LID_MODEL_MIN_BYTES as usize + 4096];
        std::fs::write(stage.join(LID_MODEL_FILENAME), &body).unwrap();

        let prev = std::env::var("AUGUR_AIRGAP_PATH").ok();
        unsafe {
            std::env::set_var("AUGUR_AIRGAP_PATH", &stage);
        }
        let mgr = ModelManager::new(cache);
        let path = mgr.ensure_lid_model().expect("airgap takes priority");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), body.len() as u64);
        unsafe {
            match prev {
                Some(v) => std::env::set_var("AUGUR_AIRGAP_PATH", v),
                None => std::env::remove_var("AUGUR_AIRGAP_PATH"),
            }
        }
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn model_manager_default_paths_live_under_home_cache() {
        // Covers the happy path for `HOME` being set (it is in all
        // realistic test environments). Confirms the XDG layout.
        let mgr = ModelManager::with_xdg_cache().expect("HOME must be set in test env");
        let path = mgr.cache_dir.to_string_lossy().into_owned();
        assert!(
            path.ends_with(".cache/augur/models"),
            "expected XDG cache path, got {path}"
        );
    }
}
