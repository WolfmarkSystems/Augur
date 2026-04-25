//! Whisper STT — audio to transcript.
//!
//! Sprint 2: real candle-whisper inference with timestamped segments
//! and Whisper-native language detection. See [`whisper`] for the
//! backend choice rationale (candle-whisper > whisper-rs because the
//! latter requires cmake + a C++ toolchain).

pub mod whisper;

pub use whisper::{
    extract_audio_from_video, ModelManager, SttEngine, SttResult, SttSegment, WhisperPaths,
    WhisperPreset, WHISPER_MODEL_URL_BASE, WHISPER_MODEL_URL_LARGE_V3, WHISPER_MODEL_URL_TINY,
};
