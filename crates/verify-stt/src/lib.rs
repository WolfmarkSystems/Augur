//! Whisper STT — audio to transcript.
//!
//! Sprint 1 scaffold. P3 wires `whisper-rs` and real audio
//! preprocessing (16 kHz mono f32 PCM, via `hound` for WAV and
//! `ffmpeg` subprocess for everything else).

pub mod whisper;

pub use whisper::{
    ModelManager, SttEngine, SttResult, SttSegment, WhisperPreset,
    WHISPER_MODEL_URL_BASE, WHISPER_MODEL_URL_LARGE_V3, WHISPER_MODEL_URL_TINY,
};
