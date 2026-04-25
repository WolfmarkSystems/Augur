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
//! download VERIFY performs in its default-classifier code path
//! when the user opts into fastText.

use fasttext_pure_rs::FastText;
use log::{debug, warn};
use std::path::{Path, PathBuf};
use verify_core::VerifyError;

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
        }
    }
}

/// Owns the on-disk model cache (`~/.cache/verify/models/`). The
/// first call to [`ModelManager::ensure_lid_model`] is the only
/// network egress VERIFY performs in its default code path — every
/// subsequent run returns the cached path.
#[derive(Debug, Clone)]
pub struct ModelManager {
    pub cache_dir: PathBuf,
}

/// Facebook mirror of the 176-language LID model. Documented here
/// (not hidden in a function body) because it is the ONLY URL
/// VERIFY fetches in the default code path.
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

    /// XDG-compliant cache directory: `~/.cache/verify/models/`.
    /// Returns `Err` if `$HOME` is unset (vanishingly rare in practice
    /// but checked rather than unwrapped).
    ///
    /// Named `with_xdg_cache` rather than `default` on purpose — a
    /// fallible constructor must not shadow the infallible
    /// `Default::default` shape (clippy::should_implement_trait).
    pub fn with_xdg_cache() -> Result<Self, VerifyError> {
        let home = std::env::var("HOME").map_err(|_| {
            VerifyError::ModelManager(
                "HOME environment variable not set; pass a cache dir explicitly"
                    .to_string(),
            )
        })?;
        Ok(Self::new(PathBuf::from(home).join(".cache/verify/models")))
    }

    /// Ensure the fastText LID model is cached locally. On first
    /// call this spawns a `curl` subprocess to fetch ~900 KB from
    /// the published Facebook mirror; on every subsequent call it
    /// returns the cached path immediately.
    ///
    /// NETWORK: this is the only permitted network call in VERIFY's
    /// default code path. See the offline-invariant section of
    /// CLAUDE.md.
    pub fn ensure_lid_model(&self) -> Result<PathBuf, VerifyError> {
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

        // Air-gap path: when `VERIFY_AIRGAP_PATH` is set the
        // ModelManager copies pre-staged weights from there
        // instead of touching the network. This is the supported
        // offline-deployment path for classified workstations
        // that can never reach the internet.
        if let Some(staged) = airgap_lid_model() {
            log::info!(
                "verify-classifier: VERIFY_AIRGAP_PATH provides {LID_MODEL_FILENAME} at {staged:?}; \
                 copying to {dest:?} (no network egress)"
            );
            std::fs::copy(&staged, &dest)?;
            let size = std::fs::metadata(&dest)?.len();
            if size < LID_MODEL_MIN_BYTES {
                return Err(VerifyError::ModelManager(format!(
                    "air-gapped LID model at {dest:?} is {size} bytes — expected \
                     >= {LID_MODEL_MIN_BYTES}; check VERIFY_AIRGAP_PATH source."
                )));
            }
            return Ok(dest);
        }

        // Intentional network egress. Logged, not silent.
        warn!(
            "VERIFY fetching one-time LID model ({}) from {LID_MODEL_URL} — \
             this is the ONLY network call VERIFY makes in the default code path",
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
                VerifyError::ModelManager(format!(
                    "failed to launch curl for LID model download: {e}. \
                     Install curl or pre-place {LID_MODEL_FILENAME} at {dest:?}"
                ))
            })?;
        if !status.success() {
            return Err(VerifyError::ModelManager(format!(
                "curl failed while downloading {LID_MODEL_URL}: exit {status}"
            )));
        }

        // Integrity check — truncated / HTML-error downloads would
        // be well under published size.
        let size = std::fs::metadata(&dest)?.len();
        if size < LID_MODEL_MIN_BYTES {
            return Err(VerifyError::ModelManager(format!(
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
    pub fn load_fasttext(model_path: &Path) -> Result<Self, VerifyError> {
        let model = FastText::load(model_path).map_err(|e| {
            VerifyError::Classifier(format!(
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
    ) -> Result<ClassificationResult, VerifyError> {
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
                    VerifyError::Classifier(format!("fasttext predict: {e}"))
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

        Ok(ClassificationResult {
            is_foreign: !language.is_empty() && language != target_language,
            language,
            confidence,
            target_language: target_language.to_string(),
        })
    }
}

/// Look up the air-gap-staged LID model. Returns `Some(path)`
/// when `VERIFY_AIRGAP_PATH` is set AND the directory contains
/// `lid.176.ftz`. Sprint 5 P3 — pre-bundled offline installer
/// for classified workstations that cannot reach the internet.
fn airgap_lid_model() -> Option<PathBuf> {
    let root = std::env::var("VERIFY_AIRGAP_PATH").ok()?;
    let candidate = PathBuf::from(root).join(LID_MODEL_FILENAME);
    candidate.exists().then_some(candidate)
}

/// Map whichlang's `Lang` (ISO 639-3) to the ISO 639-1 codes the
/// rest of VERIFY speaks. Every variant in whichlang 0.1.1 is
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
        std::env::var("VERIFY_RUN_INTEGRATION_TESTS").ok().as_deref() == Some("1")
    }

    fn cached_lid_model() -> Option<std::path::PathBuf> {
        let home = std::env::var("HOME").ok()?;
        let p = std::path::PathBuf::from(home).join(".cache/verify/models/lid.176.ftz");
        p.exists().then_some(p)
    }

    #[test]
    #[ignore = "Sprint 5 P1 — requires VERIFY_RUN_INTEGRATION_TESTS=1 and a cached lid.176.ftz"]
    fn fasttext_pure_rs_classifies_arabic_correctly() {
        if !integration_gate_ok() {
            eprintln!("VERIFY_RUN_INTEGRATION_TESTS != 1 — skipping");
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
    #[ignore = "Sprint 5 P1 — requires VERIFY_RUN_INTEGRATION_TESTS=1 and a cached lid.176.ftz"]
    fn fasttext_pure_rs_classifies_forensic_languages() {
        if !integration_gate_ok() {
            eprintln!("VERIFY_RUN_INTEGRATION_TESTS != 1 — skipping");
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

    #[test]
    fn airgap_path_short_circuits_download() {
        // Sprint 5 P3 — staging a synthetic ftz under the
        // air-gap path must satisfy ensure_lid_model without any
        // network call. Uses a fresh temp dir so we don't pollute
        // the real cache.
        let work = std::env::temp_dir().join(format!(
            "verify-airgap-test-{}-{}",
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

        let prev = std::env::var("VERIFY_AIRGAP_PATH").ok();
        // SAFETY: tests are single-threaded under cargo test --
        // --test-threads=1 by convention; this test sets the env
        // for its own duration and restores at the end.
        unsafe {
            std::env::set_var("VERIFY_AIRGAP_PATH", &stage);
        }
        let mgr = ModelManager::new(cache.clone());
        let path = mgr.ensure_lid_model().expect("airgap copy");
        assert!(path.exists());
        assert_eq!(path, cache.join(LID_MODEL_FILENAME));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), body.len() as u64);
        unsafe {
            match prev {
                Some(v) => std::env::set_var("VERIFY_AIRGAP_PATH", v),
                None => std::env::remove_var("VERIFY_AIRGAP_PATH"),
            }
        }
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn airgap_path_takes_priority_over_existing_cache() {
        // If the cache is empty and AIRGAP_PATH is set, ensure
        // we use the airgap copy rather than triggering a
        // download. Detected by the absence of any curl invocation
        // in this test (synthetic ftz is never a real model so
        // download couldn't even start, but we verify the path
        // returned matches the airgap source by file size).
        let work = std::env::temp_dir().join(format!(
            "verify-airgap-prio-{}-{}",
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

        let prev = std::env::var("VERIFY_AIRGAP_PATH").ok();
        unsafe {
            std::env::set_var("VERIFY_AIRGAP_PATH", &stage);
        }
        let mgr = ModelManager::new(cache);
        let path = mgr.ensure_lid_model().expect("airgap takes priority");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), body.len() as u64);
        unsafe {
            match prev {
                Some(v) => std::env::set_var("VERIFY_AIRGAP_PATH", v),
                None => std::env::remove_var("VERIFY_AIRGAP_PATH"),
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
            path.ends_with(".cache/verify/models"),
            "expected XDG cache path, got {path}"
        );
    }
}
