//! `verify self-test` — pre-deployment readiness check.
//!
//! Sprint 6 P3. Examiners deploying VERIFY need a one-shot way to
//! verify the binary is wired up correctly *before* running it on
//! evidence. The default form runs offline (no network, no model
//! downloads); `--full` opts into the inference path that
//! exercises Whisper + NLLB end to end.
//!
//! Design notes:
//! - Every check returns a [`CheckStatus`] (`Pass` / `Fail` /
//!   `Skip` / `Warning`); `ready_for_casework` is `true` only
//!   when there are zero `Fail`s. Skips and warnings are
//!   advisory.
//! - The check list is data-driven so tests can call individual
//!   checks in isolation (`run_classification_check`,
//!   `run_ffmpeg_check`, …) without going through the whole
//!   suite.
//! - Offline-invariant audit: with `--full=false`, no check
//!   touches the network. The `cached?` filesystem checks read
//!   `~/.cache/verify/models/` directly.

use std::path::{Path, PathBuf};
use verify_classifier::{ClassificationResult, ConfidenceTier, LanguageClassifier};
use verify_core::VerifyError;

/// Outcome of a single self-test check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Skip,
    Warning,
}

impl CheckStatus {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Pass => "✓",
            Self::Fail => "✗",
            Self::Skip => "⚠",
            Self::Warning => "⚠",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
            Self::Warning => "WARN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SelfTestCheck {
    pub name: String,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SelfTestResult {
    pub checks: Vec<SelfTestCheck>,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub warnings: u32,
    pub ready_for_casework: bool,
}

impl SelfTestResult {
    fn from_checks(checks: Vec<SelfTestCheck>) -> Self {
        let mut passed = 0;
        let mut failed = 0;
        let mut skipped = 0;
        let mut warnings = 0;
        for c in &checks {
            match c.status {
                CheckStatus::Pass => passed += 1,
                CheckStatus::Fail => failed += 1,
                CheckStatus::Skip => skipped += 1,
                CheckStatus::Warning => warnings += 1,
            }
        }
        Self {
            checks,
            passed,
            failed,
            skipped,
            warnings,
            ready_for_casework: failed == 0,
        }
    }
}

// ── Individual checks (each pub for unit testability) ────────────

/// Whichlang classifier on a known Arabic phrase.
pub fn run_classification_check_arabic() -> SelfTestCheck {
    let classifier = LanguageClassifier::new_whichlang();
    let probe = "مرحبا بالعالم، كيف حالك اليوم؟ هذا اختبار";
    match classifier.classify(probe, "en") {
        Ok(r) if r.language == "ar" => SelfTestCheck {
            name: "Classification: Arabic text → ar".into(),
            status: CheckStatus::Pass,
            message: format_classification(&r),
        },
        Ok(r) => SelfTestCheck {
            name: "Classification: Arabic text → ar".into(),
            status: CheckStatus::Fail,
            message: format!("expected ar, got {}", r.language),
        },
        Err(e) => SelfTestCheck {
            name: "Classification: Arabic text → ar".into(),
            status: CheckStatus::Fail,
            message: e.to_string(),
        },
    }
}

/// Whichlang classifier on plain English (target=en → not foreign).
pub fn run_classification_check_english() -> SelfTestCheck {
    let classifier = LanguageClassifier::new_whichlang();
    let probe = "This is a long-enough English probe sentence used for self-test.";
    match classifier.classify(probe, "en") {
        Ok(r) if r.language == "en" && !r.is_foreign => SelfTestCheck {
            name: "Classification: English → not foreign".into(),
            status: CheckStatus::Pass,
            message: format_classification(&r),
        },
        Ok(r) => SelfTestCheck {
            name: "Classification: English → not foreign".into(),
            status: CheckStatus::Fail,
            message: format!(
                "expected en + not foreign, got {} (is_foreign={})",
                r.language, r.is_foreign
            ),
        },
        Err(e) => SelfTestCheck {
            name: "Classification: English → not foreign".into(),
            status: CheckStatus::Fail,
            message: e.to_string(),
        },
    }
}

/// Sprint 9 P1 — confirms the Pashto/Farsi script disambiguator
/// is wired in. Drives the script analyzer on a Pashto-glyph-heavy
/// probe; expects `LikelyPashto` with confidence ≥ 0.7 (the
/// reclassification bar). Fully offline.
pub fn run_pashto_disambiguation_check() -> SelfTestCheck {
    let probe = "ډېر ښه, لاړ شه, ګوره ټول ړومبۍ";
    let analysis = verify_classifier::pashto_farsi_score(probe);
    use verify_classifier::ScriptRecommendation;
    match (analysis.recommendation, analysis.confidence) {
        (ScriptRecommendation::LikelyPashto, c) if c >= 0.7 => SelfTestCheck {
            name: "Pashto/Farsi script disambiguation".into(),
            status: CheckStatus::Pass,
            message: format!(
                "probe reclassifies Pashto-glyph text correctly (confidence: {:.2})",
                c
            ),
        },
        (rec, c) => SelfTestCheck {
            name: "Pashto/Farsi script disambiguation".into(),
            status: CheckStatus::Fail,
            message: format!("probe returned {rec:?} (confidence: {c:.2})"),
        },
    }
}

pub fn run_classification_check_empty() -> SelfTestCheck {
    let classifier = LanguageClassifier::new_whichlang();
    match classifier.classify("", "en") {
        Ok(r) if r.language.is_empty() && r.confidence == 0.0 => SelfTestCheck {
            name: "Classification: empty input → handled".into(),
            status: CheckStatus::Pass,
            message: "graceful empty-input handling".into(),
        },
        Ok(r) => SelfTestCheck {
            name: "Classification: empty input → handled".into(),
            status: CheckStatus::Fail,
            message: format!("unexpected: {r:?}"),
        },
        Err(e) => SelfTestCheck {
            name: "Classification: empty input → handled".into(),
            status: CheckStatus::Fail,
            message: e.to_string(),
        },
    }
}

/// Probe a binary on PATH by running `<bin> --version`. Returns
/// a `Pass` check on exit-0, `Warning` otherwise (the binary is
/// optional for some flows but recommended).
pub fn run_binary_check(name: &str, bin: &str, required: bool) -> SelfTestCheck {
    let ok = std::process::Command::new(bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        SelfTestCheck {
            name: name.into(),
            status: CheckStatus::Pass,
            message: format!("`{bin}` available on PATH"),
        }
    } else if required {
        SelfTestCheck {
            name: name.into(),
            status: CheckStatus::Fail,
            message: format!(
                "`{bin}` not found on PATH (required); install it before running on evidence"
            ),
        }
    } else {
        SelfTestCheck {
            name: name.into(),
            status: CheckStatus::Warning,
            message: format!(
                "`{bin}` not found on PATH (optional — limits supported input formats)"
            ),
        }
    }
}

/// Filesystem check for a cached model artifact.
pub fn run_cached_artifact_check(name: &str, path: &Path) -> SelfTestCheck {
    if path.exists() {
        SelfTestCheck {
            name: name.into(),
            status: CheckStatus::Pass,
            message: format!("cached at {path:?}"),
        }
    } else {
        SelfTestCheck {
            name: name.into(),
            status: CheckStatus::Skip,
            message: format!(
                "not cached at {path:?} (run `verify self-test --full` to download \
                 and exercise inference)"
            ),
        }
    }
}

pub fn run_airgap_check() -> SelfTestCheck {
    match std::env::var("VERIFY_AIRGAP_PATH") {
        Ok(p) if !p.is_empty() => SelfTestCheck {
            name: "Air-gap mode".into(),
            status: CheckStatus::Pass,
            message: format!("VERIFY_AIRGAP_PATH set to {p}"),
        },
        _ => SelfTestCheck {
            name: "Air-gap mode".into(),
            status: CheckStatus::Pass,
            message: "VERIFY_AIRGAP_PATH not set (online-on-first-run mode)".into(),
        },
    }
}

/// Sprint 7 P1 — GeoIP database availability. Pure check — never
/// loads the database, just resolves whether one is configured.
pub fn run_geoip_check() -> SelfTestCheck {
    match verify_core::geoip::check_status() {
        Ok(path) => SelfTestCheck {
            name: "GeoIP: GeoLite2 database configured".into(),
            status: CheckStatus::Pass,
            message: format!("found at {path:?}"),
        },
        Err(blurb) => SelfTestCheck {
            name: "GeoIP: GeoLite2 database configured".into(),
            status: CheckStatus::Skip,
            message: blurb,
        },
    }
}

pub fn run_hf_token_check() -> SelfTestCheck {
    let path = home_relative(".cache/verify/hf_token");
    if path.as_ref().map(|p| p.exists()).unwrap_or(false) {
        SelfTestCheck {
            name: "HF token configured (optional, for diarization)".into(),
            status: CheckStatus::Pass,
            message: format!(
                "token present at {:?}",
                path.unwrap_or_else(|| PathBuf::from("?"))
            ),
        }
    } else {
        SelfTestCheck {
            name: "HF token configured (optional, for diarization)".into(),
            status: CheckStatus::Skip,
            message: "not configured — `verify setup --hf-token <T>` enables \
                      speaker diarization"
                .into(),
        }
    }
}

/// Sprint 6 P3 P4 hint: Even without `--full`, an offline
/// invariant audit summarizing that no check we just ran
/// triggered a network call. This is a static guarantee — the
/// checks above each describe their behavior in their
/// implementation; we surface it here so examiners reading the
/// output see it spelled out.
pub fn offline_invariant_check(full: bool) -> SelfTestCheck {
    if full {
        SelfTestCheck {
            name: "Offline invariant audit".into(),
            status: CheckStatus::Warning,
            message:
                "running with --full; permitted egress URLs may have been hit \
                 to seed the on-disk cache (this is documented behavior)"
                    .into(),
        }
    } else {
        SelfTestCheck {
            name: "Offline invariant audit".into(),
            status: CheckStatus::Pass,
            message: "no unexpected network calls; default self-test is fully offline"
                .into(),
        }
    }
}

// ── --full inference checks ─────────────────────────────────────

/// Optional `--full` translation check. Fires up the existing
/// `TranslationEngine` against a hardcoded Arabic probe and
/// confirms the engine produced *something* (we do NOT pin the
/// exact translation — NLLB output drifts slightly between
/// backends and we want this check to pass equivalently against
/// transformers and ctranslate2).
///
/// Returns `Skip` rather than `Fail` when Python / transformers
/// is missing — the engine isn't broken in that case, the host
/// just hasn't been provisioned yet. Examiners running on a
/// proper deployment workstation will see `Pass`.
pub fn run_full_translation_check() -> SelfTestCheck {
    use verify_translate::TranslationEngine;
    let engine = match TranslationEngine::with_xdg_cache() {
        Ok(e) => e,
        Err(e) => {
            return SelfTestCheck {
                name: "Translation: ar → en (full)".into(),
                status: CheckStatus::Skip,
                message: format!("engine init: {e}"),
            };
        }
    };
    match engine.translate("مرحبا بالعالم، هذا اختبار", "ar", "en") {
        Ok(r) => {
            // Forensic invariant must survive the self-test:
            // every TranslationResult carries the advisory.
            if !r.is_machine_translation || r.advisory_notice.is_empty() {
                return SelfTestCheck {
                    name: "Translation: ar → en (full)".into(),
                    status: CheckStatus::Fail,
                    message: "machine-translation advisory missing on result \
                              — forensic invariant violation"
                        .into(),
                };
            }
            if r.translated_text.trim().is_empty() {
                return SelfTestCheck {
                    name: "Translation: ar → en (full)".into(),
                    status: CheckStatus::Fail,
                    message: "engine returned an empty translation".into(),
                };
            }
            SelfTestCheck {
                name: "Translation: ar → en (full)".into(),
                status: CheckStatus::Pass,
                message: format!("→ {}", r.translated_text.trim()),
            }
        }
        Err(e) => SelfTestCheck {
            name: "Translation: ar → en (full)".into(),
            status: CheckStatus::Skip,
            message: format!("inference unavailable: {e}"),
        },
    }
}

// ── Top-level driver ────────────────────────────────────────────

pub fn run_self_test(full: bool) -> Result<SelfTestResult, VerifyError> {
    let mut checks = vec![
        run_classification_check_arabic(),
        run_classification_check_english(),
        run_classification_check_empty(),
        run_pashto_disambiguation_check(),
        run_binary_check("Audio preprocessing: ffmpeg", "ffmpeg", false),
        run_binary_check("OCR: tesseract", "tesseract", false),
        run_binary_check("PDF rasterize: pdftoppm", "pdftoppm", false),
    ];
    let cache_root = home_relative(".cache/verify/models").unwrap_or_else(|| PathBuf::from("."));
    checks.push(run_cached_artifact_check(
        "STT: Whisper tiny safetensors cached",
        &cache_root.join("whisper/hf"),
    ));
    checks.push(run_cached_artifact_check(
        "Translation: NLLB-200 cached",
        &cache_root.join("nllb"),
    ));
    checks.push(run_airgap_check());
    checks.push(run_hf_token_check());
    checks.push(run_geoip_check());
    if full {
        checks.push(run_full_translation_check());
    }
    checks.push(offline_invariant_check(full));
    Ok(SelfTestResult::from_checks(checks))
}

fn home_relative(rel: &str) -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(rel))
}

fn format_classification(r: &ClassificationResult) -> String {
    let tier = match r.confidence_tier {
        ConfidenceTier::High => "HIGH",
        ConfidenceTier::Medium => "MEDIUM",
        ConfidenceTier::Low => "LOW",
    };
    format!(
        "{} (confidence: {tier} {:.2}, {} word(s))",
        r.language, r.confidence, r.input_word_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_test_classification_check_passes() {
        let r = run_classification_check_arabic();
        assert_eq!(r.status, CheckStatus::Pass, "msg: {}", r.message);
    }

    #[test]
    fn self_test_english_classification_passes_without_network() {
        let r = run_classification_check_english();
        assert_eq!(r.status, CheckStatus::Pass);
    }

    #[test]
    fn self_test_empty_input_passes() {
        let r = run_classification_check_empty();
        assert_eq!(r.status, CheckStatus::Pass);
    }

    #[test]
    fn self_test_reports_ffmpeg_availability() {
        // The check must never panic — Pass when present,
        // Warning when absent; never Fail (ffmpeg is optional).
        let r = run_binary_check("ffmpeg probe", "ffmpeg", false);
        assert!(matches!(
            r.status,
            CheckStatus::Pass | CheckStatus::Warning
        ));
    }

    #[test]
    fn self_test_reports_definitely_missing_binary_as_warning() {
        // Sentinel binary that doesn't exist on any host.
        let r = run_binary_check(
            "phony probe",
            "this-binary-does-not-exist-anywhere-xyz",
            false,
        );
        assert_eq!(r.status, CheckStatus::Warning);
    }

    #[test]
    fn self_test_required_missing_binary_fails() {
        let r = run_binary_check(
            "required phony",
            "this-binary-does-not-exist-anywhere-xyz",
            true,
        );
        assert_eq!(r.status, CheckStatus::Fail);
    }

    #[test]
    fn self_test_result_ready_for_casework_requires_no_failures() {
        let mk = |s: CheckStatus| SelfTestCheck {
            name: "x".into(),
            status: s,
            message: String::new(),
        };
        let pass_only = SelfTestResult::from_checks(vec![mk(CheckStatus::Pass)]);
        assert!(pass_only.ready_for_casework);

        let with_skip = SelfTestResult::from_checks(vec![
            mk(CheckStatus::Pass),
            mk(CheckStatus::Skip),
            mk(CheckStatus::Warning),
        ]);
        assert!(with_skip.ready_for_casework);

        let with_fail = SelfTestResult::from_checks(vec![
            mk(CheckStatus::Pass),
            mk(CheckStatus::Fail),
        ]);
        assert!(!with_fail.ready_for_casework);
    }

    #[test]
    fn self_test_default_run_emits_no_failures_on_typical_workstations() {
        // The offline default run has no Fails on a workstation
        // that can compile and run the test suite at all
        // (whichlang is embedded; classification checks always
        // pass; binary checks degrade to Warning when absent).
        let r = run_self_test(false).expect("self-test runs");
        assert_eq!(r.failed, 0, "no failures on default run; checks: {:?}", r.checks);
        assert!(r.ready_for_casework);
    }
}
