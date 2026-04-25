//! Whisper STT scaffold.
//!
//! The real engine wraps `whisper-rs` (OpenAI Whisper, fully offline).
//! Sprint 1 only defines the public shape — P3 wires the binding,
//! the model-cache download, and 16 kHz mono PCM preprocessing.

use std::path::{Path, PathBuf};
use verify_core::VerifyError;

/// Whisper model presets. Speed / accuracy tradeoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperPreset {
    /// `ggml-tiny.bin` — ~75 MB. Fast, lower accuracy.
    Fast,
    /// `ggml-base.bin` — ~142 MB. Balanced.
    Balanced,
    /// `ggml-large-v3.bin` — ~2.9 GB. Most accurate.
    Accurate,
}

impl WhisperPreset {
    pub fn model_filename(&self) -> &'static str {
        match self {
            Self::Fast => "ggml-tiny.bin",
            Self::Balanced => "ggml-base.bin",
            Self::Accurate => "ggml-large-v3.bin",
        }
    }

    pub fn download_url(&self) -> &'static str {
        // Upstream GGML Whisper mirrors (Hugging Face). The URLs are
        // stable; whichever download host VERIFY ships with,
        // `ModelManager` checks the expected size before accepting
        // the file.
        match self {
            Self::Fast => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
            }
            Self::Balanced => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin"
            }
            Self::Accurate => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin"
            }
        }
    }

    /// Expected on-disk size in bytes. `ModelManager` uses this as a
    /// crude integrity check before handing the path to whisper-rs.
    pub fn expected_size_bytes(&self) -> u64 {
        match self {
            Self::Fast => 77_691_713,         // ≈ 75 MB
            Self::Balanced => 147_951_465,    // ≈ 142 MB
            Self::Accurate => 3_094_623_691,  // ≈ 2.9 GB
        }
    }
}

/// One timestamped chunk of a transcript.
#[derive(Debug, Clone)]
pub struct SttSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Result of a single `transcribe` call.
#[derive(Debug, Clone)]
pub struct SttResult {
    /// Full concatenated transcript.
    pub transcript: String,
    /// Detected language — ISO 639-1 code.
    pub detected_language: String,
    /// Whisper's language-confidence score, 0.0–1.0.
    pub confidence: f32,
    /// Per-segment timestamps + text.
    pub segments: Vec<SttSegment>,
}

/// Sprint 1 scaffold of the Whisper engine.
#[derive(Debug)]
pub struct SttEngine {
    _model_path: PathBuf,
    _preset: WhisperPreset,
}

impl SttEngine {
    /// Sprint 1 stub — P3 wires `whisper-rs`.
    pub fn load(model_path: &Path, preset: WhisperPreset) -> Result<Self, VerifyError> {
        Ok(Self {
            _model_path: model_path.to_path_buf(),
            _preset: preset,
        })
    }

    /// Sprint 1 stub.
    pub fn transcribe(&self, _audio_path: &Path) -> Result<SttResult, VerifyError> {
        Err(VerifyError::Stt(
            "transcribe not yet implemented — see Sprint 1 P3".to_string(),
        ))
    }
}
