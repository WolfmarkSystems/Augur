//! End-to-end Whisper STT integration tests.
//!
//! Marked `#[ignore]` by default — they require:
//! - downloading the `openai/whisper-tiny` safetensors weights
//!   (~150 MB) via `ModelManager::ensure_whisper_model` on first run,
//! - and a real audio fixture under `tests/fixtures/sample.wav`
//!   (16 kHz mono WAV — see the README in this folder for the
//!   recommended jfk.wav from the candle demo dataset).
//!
//! Run with:
//!
//! ```sh
//! VERIFY_RUN_INTEGRATION_TESTS=1 cargo test -p verify-stt \
//!     --test whisper_integration -- --include-ignored
//! ```

use std::path::PathBuf;
use verify_stt::{ModelManager, SttEngine, WhisperPreset};

fn integration_gate_ok() -> bool {
    std::env::var("VERIFY_RUN_INTEGRATION_TESTS").ok().as_deref() == Some("1")
}

#[test]
#[ignore = "requires VERIFY_RUN_INTEGRATION_TESTS=1 and network access on first run"]
fn ensure_whisper_tiny_model_round_trip() {
    if !integration_gate_ok() {
        eprintln!("VERIFY_RUN_INTEGRATION_TESTS != 1 — skipping integration body");
        return;
    }
    let mgr = ModelManager::with_xdg_cache().expect("HOME");
    let paths = mgr
        .ensure_whisper_model(WhisperPreset::Fast)
        .expect("ensure_whisper_model");
    assert!(paths.config.exists(), "config.json should exist");
    assert!(paths.tokenizer.exists(), "tokenizer.json should exist");
    assert!(paths.weights.exists(), "model.safetensors should exist");
    let weights_size = std::fs::metadata(&paths.weights).expect("metadata").len();
    assert!(
        weights_size >= 50_000_000,
        "tiny safetensors should be ≥ 50 MB, got {weights_size}"
    );
}

#[test]
#[ignore = "Sprint 2: requires VERIFY_RUN_INTEGRATION_TESTS=1, model download, and a fixture WAV"]
fn transcribe_real_audio_yields_non_empty_transcript() {
    if !integration_gate_ok() {
        eprintln!("VERIFY_RUN_INTEGRATION_TESTS != 1 — skipping integration body");
        return;
    }

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample.wav");
    if !fixture.exists() {
        eprintln!(
            "fixture {:?} not present — skipping (drop a 16 kHz mono WAV at this path to enable)",
            fixture
        );
        return;
    }

    let mgr = ModelManager::with_xdg_cache().expect("HOME");
    let paths = mgr
        .ensure_whisper_model(WhisperPreset::Fast)
        .expect("ensure_whisper_model");
    let mut engine = SttEngine::load(&paths, WhisperPreset::Fast).expect("SttEngine::load");
    let result = engine.transcribe(&fixture).expect("transcribe");

    assert!(
        !result.transcript.trim().is_empty(),
        "real transcript should be non-empty"
    );
    assert!(
        !result.detected_language.is_empty(),
        "language detection should populate a code"
    );
    assert!(
        !result.segments.is_empty(),
        "real transcripts must produce ≥ 1 timestamped segment"
    );
    for w in result.segments.windows(2) {
        assert!(
            w[0].end_ms <= w[1].start_ms,
            "segment ordering invariant violated: {w:?}"
        );
    }
}
