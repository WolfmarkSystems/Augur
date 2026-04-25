//! End-to-end Whisper STT integration tests.
//!
//! Marked `#[ignore]` by default — they require the real
//! `ggml-tiny.bin` model (~75 MB) to be downloaded via
//! `ModelManager::ensure_whisper_model` and a real audio fixture
//! under `tests/fixtures/`. Run explicitly with:
//!
//! ```sh
//! VERIFY_RUN_INTEGRATION_TESTS=1 cargo test -p verify-stt \
//!     --test whisper_integration -- --include-ignored
//! ```
//!
//! Sprint 1 ships these as stub-shape tests — they exercise
//! `ModelManager::ensure_whisper_model` and the preprocessing
//! pipeline, and assert that `SttEngine::transcribe` currently
//! returns the Sprint-1 stub error. Sprint 2 flips them to assert
//! real transcript content once whisper-rs (or its pure-Rust
//! replacement) lands.

use verify_stt::{ModelManager, WhisperPreset};

/// Gate: these tests only execute when explicitly opted in via
/// `VERIFY_RUN_INTEGRATION_TESTS=1`. Checked inside each test so
/// a developer running `cargo test -- --include-ignored` without
/// the env var gets a clear "skipping" message, not a false
/// network egress.
fn integration_gate_ok() -> bool {
    std::env::var("VERIFY_RUN_INTEGRATION_TESTS").ok().as_deref() == Some("1")
}

#[test]
#[ignore = "requires VERIFY_RUN_INTEGRATION_TESTS=1 and network access on first run"]
fn ensure_whisper_tiny_model_round_trip() {
    if !integration_gate_ok() {
        eprintln!("VERIFY_RUN_INTEGRATION_TESTS != 1 — skipping integration body");
    } else {
        let mgr = ModelManager::with_xdg_cache().expect("HOME");
        let path = mgr
            .ensure_whisper_model(WhisperPreset::Fast)
            .expect("ensure_whisper_model");
        assert!(path.exists(), "model path should exist after ensure");
        let size = std::fs::metadata(&path).expect("metadata").len();
        assert!(
            size >= 50_000_000,
            "tiny model should be ≥ 50 MB, got {size}"
        );
    }
}

#[test]
#[ignore = "Sprint 1: STT stub returns a structured VerifyError — Sprint 2 flips to a real transcript assertion"]
fn stub_transcribe_returns_structured_error_on_valid_wav() {
    if !integration_gate_ok() {
        eprintln!("VERIFY_RUN_INTEGRATION_TESTS != 1 — skipping integration body");
    }
    // Sprint 2 wires the real check. Placeholder today.
}
