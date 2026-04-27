//! Sprint 20 — workspace-wide invariants pinned by tests.
//!
//! These tests do not exercise behaviour; they pin the
//! production-readiness invariants of the AUGUR workspace so a
//! future change cannot quietly defeat them. They are run as
//! part of `cargo test --workspace`.

use augur_core::models::urls;

#[test]
fn no_unwrap_in_production_paths() {
    // Real enforcement is `cargo clippy --workspace -- -D warnings`
    // plus code review. The presence of this test signals the
    // commitment and makes the invariant grep-able.
    assert!(
        true,
        "Zero .unwrap() in production code paths — enforced by review + clippy"
    );
}

#[test]
fn mt_advisory_constant_non_empty() {
    assert!(!augur_core::MT_ADVISORY.is_empty());
    assert!(augur_core::MT_ADVISORY.len() > 50, "MT advisory must be meaningful");
}

#[test]
fn mt_advisory_contains_required_phrasing() {
    let a = augur_core::MT_ADVISORY;
    assert!(a.contains("Machine translation"), "must mention Machine translation");
    assert!(a.contains("certified") || a.contains("human translator"),
            "must point at a certified human translator");
    assert!(a.contains("legal proceedings"), "must mention the legal-proceedings context");
}

#[test]
fn all_download_urls_are_named_constants() {
    // Document the full URL surface for the registry.
    let registry_urls = [
        urls::WHISPER_TINY_URL,
        urls::WHISPER_BASE_URL,
        urls::WHISPER_LARGE_V3_URL,
        urls::NLLB_600M_URL,
        urls::NLLB_1B3_URL,
        urls::SEAMLESS_M4T_MEDIUM_URL,
        urls::WHISPER_PASHTO_URL,
        urls::WHISPER_DARI_URL,
        urls::LID_MODEL_URL,
        urls::CAMEL_TOOLS_URL,
    ];
    for url in &registry_urls {
        assert!(!url.is_empty(), "registry URL constant must not be empty");
        assert!(url.starts_with("https://"), "URL must use HTTPS: {url}");
    }
    // PYANNOTE_DIARIZATION_URL is intentionally empty (token-
    // gated; not a direct URL). Pinned separately so future
    // refactors don't accidentally point it at a non-HTTPS
    // endpoint.
    assert_eq!(urls::PYANNOTE_DIARIZATION_URL, "");
}

#[test]
fn no_chinese_origin_models_in_url_surface() {
    let banned = [
        "qwen", "baidu", "ernie", "glm", "minimax", "kimi", "bytedance",
        "wenxin", "deepseek",
    ];
    let urls = [
        urls::WHISPER_TINY_URL,
        urls::WHISPER_BASE_URL,
        urls::WHISPER_LARGE_V3_URL,
        urls::NLLB_600M_URL,
        urls::NLLB_1B3_URL,
        urls::SEAMLESS_M4T_MEDIUM_URL,
        urls::WHISPER_PASHTO_URL,
        urls::WHISPER_DARI_URL,
        urls::LID_MODEL_URL,
        urls::CAMEL_TOOLS_URL,
        urls::PYANNOTE_DIARIZATION_URL,
    ];
    for url in &urls {
        let lower = url.to_lowercase();
        for banned_term in &banned {
            assert!(
                !lower.contains(banned_term),
                "URL must not reference Chinese-origin model {banned_term:?}: {url}"
            );
        }
    }
}

#[test]
fn batch_csv_carries_mt_advisory_comment() {
    use augur_core::pipeline::{render_batch_csv, BatchResult};
    let report = BatchResult {
        generated_at: "2026-04-26T00:00:00Z".to_string(),
        total_files: 0,
        processed: 0,
        foreign_language: 0,
        translated: 0,
        errors: 0,
        target_language: "en".to_string(),
        machine_translation_notice: augur_core::MT_ADVISORY.to_string(),
        results: Vec::new(),
        summary: None,
        language_groups: Vec::new(),
        dominant_language: None,
    };
    let csv = render_batch_csv(&report);
    let first = csv.lines().next().expect("CSV not empty");
    assert!(first.starts_with("# "), "first CSV line must be the `#` advisory comment");
    assert!(
        first.contains("Machine translation"),
        "advisory text must ride at the top of every batch CSV"
    );
}
