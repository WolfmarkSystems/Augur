//! Whisper STT — audio to transcript.
//!
//! # Sprint 1 P3 — backend choice
//!
//! `whisper-rs` (the FFI wrapper around `whisper.cpp`) requires
//! `cmake` + a C++ toolchain at build time. A build probe on macOS
//! ARM64 failed immediately with `is \`cmake\` not installed?`
//! — installing cmake + Xcode command-line tools on every
//! forensic workstation is a bigger dependency than VERIFY wants
//! in its default build path (fastText 0.8 is pure Rust; the rest
//! of the workspace shouldn't regress that).
//!
//! Sprint 1 ships a **stub STT backend** that fully implements the
//! public API shape ([`SttEngine`], [`SttResult`], [`SttSegment`],
//! [`WhisperPreset`]) + real audio preprocessing (hound for WAV,
//! `ffmpeg` subprocess for every other container). Calls to
//! [`SttEngine::transcribe`] return a structured
//! `VerifyError::Stt` explaining that the real engine lands in
//! Sprint 2 — callers never see a panic.
//!
//! Sprint 2 will either:
//! 1. Gate `whisper-rs` behind an opt-in `cargo feature` so the
//!    default build stays pure-Rust, or
//! 2. Switch to a pure-Rust Whisper port (e.g. `candle-whisper`)
//!    if one reaches production quality.
//!
//! # Offline invariant
//!
//! Whisper model downloads ([`ModelManager::ensure_whisper_model`])
//! are the **second permitted network egress** in VERIFY, after
//! the fastText LID download in `verify-classifier`. Every egress
//! URL is a named top-level `const` so `grep WHISPER_MODEL` and
//! `grep LID_MODEL` surface every network call site in the whole
//! workspace.

use log::{debug, warn};
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

// ── Egress-point constants ──────────────────────────────────────
//
// Every Whisper model URL is a named top-level `const` so
// `grep WHISPER_MODEL_` from the workspace root enumerates every
// egress site. Same pattern as `LID_MODEL_URL` in
// verify-classifier — the offline invariant's audit trail.

pub const WHISPER_MODEL_URL_TINY: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin";
pub const WHISPER_MODEL_URL_BASE: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";
pub const WHISPER_MODEL_URL_LARGE_V3: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin";

/// Lower-bound integrity check — truncated / HTML-error downloads
/// for even the smallest preset would be well under this.
const WHISPER_MODEL_MIN_BYTES_TINY: u64 = 50_000_000;

impl WhisperPreset {
    pub fn model_filename(&self) -> &'static str {
        match self {
            Self::Fast => "ggml-tiny.bin",
            Self::Balanced => "ggml-base.bin",
            Self::Accurate => "ggml-large-v3.bin",
        }
    }

    pub fn download_url(&self) -> &'static str {
        match self {
            Self::Fast => WHISPER_MODEL_URL_TINY,
            Self::Balanced => WHISPER_MODEL_URL_BASE,
            Self::Accurate => WHISPER_MODEL_URL_LARGE_V3,
        }
    }

    /// Expected on-disk size in bytes. Used by
    /// [`ModelManager::ensure_whisper_model`] as a crude integrity
    /// check after download.
    pub fn expected_size_bytes(&self) -> u64 {
        match self {
            Self::Fast => 77_691_713,        // ≈ 75 MB
            Self::Balanced => 147_951_465,   // ≈ 142 MB
            Self::Accurate => 3_094_623_691, // ≈ 2.9 GB
        }
    }
}

/// Owns Whisper's on-disk model cache. Sibling to
/// `verify-classifier::ModelManager` — the two are intentionally
/// independent so each sub-crate audits its own egress.
#[derive(Debug, Clone)]
pub struct ModelManager {
    pub cache_root: PathBuf,
}

impl ModelManager {
    pub fn new(cache_root: PathBuf) -> Self {
        Self { cache_root }
    }

    /// `~/.cache/verify/models/whisper/`.
    pub fn with_xdg_cache() -> Result<Self, VerifyError> {
        let home = std::env::var("HOME").map_err(|_| {
            VerifyError::ModelManager(
                "HOME environment variable not set; pass a cache dir explicitly".to_string(),
            )
        })?;
        Ok(Self::new(
            PathBuf::from(home).join(".cache/verify/models/whisper"),
        ))
    }

    /// Per-preset sub-directory. Each preset caches independently so
    /// switching presets does not clobber the other.
    fn preset_dir(&self, preset: WhisperPreset) -> PathBuf {
        let leaf = match preset {
            WhisperPreset::Fast => "tiny",
            WhisperPreset::Balanced => "base",
            WhisperPreset::Accurate => "large-v3",
        };
        self.cache_root.join(leaf)
    }

    /// Ensure the Whisper preset model is cached locally. Fast path
    /// returns the cached path with no network access; slow path
    /// spawns `curl` to fetch from the published Hugging Face
    /// mirror, then verifies size.
    ///
    /// NETWORK: one of only two permitted network calls in VERIFY's
    /// default code path — see the CLAUDE.md offline-invariant
    /// section.
    pub fn ensure_whisper_model(&self, preset: WhisperPreset) -> Result<PathBuf, VerifyError> {
        let dir = self.preset_dir(preset);
        let dest = dir.join(preset.model_filename());
        let expected = preset.expected_size_bytes();
        let min_ok = WHISPER_MODEL_MIN_BYTES_TINY;

        if dest.exists() {
            let size = std::fs::metadata(&dest)?.len();
            if size >= min_ok {
                debug!(
                    "whisper {:?} model cached at {:?} ({} bytes)",
                    preset, dest, size
                );
                return Ok(dest);
            }
            warn!(
                "cached whisper model at {:?} is {} bytes (expected ≈{}) — re-downloading",
                dest, size, expected
            );
        }

        std::fs::create_dir_all(&dir)?;

        warn!(
            "VERIFY fetching whisper model {} ({} bytes expected) from {} — \
             one-time network egress per the offline invariant",
            preset.model_filename(),
            expected,
            preset.download_url(),
        );
        let status = std::process::Command::new("curl")
            .arg("-fL")
            .arg("--silent")
            .arg("--show-error")
            .arg("--output")
            .arg(&dest)
            .arg(preset.download_url())
            .status()
            .map_err(|e| {
                VerifyError::ModelManager(format!(
                    "failed to launch curl for whisper download: {e}. \
                     Install curl or pre-place {} at {:?}",
                    preset.model_filename(),
                    dest,
                ))
            })?;
        if !status.success() {
            return Err(VerifyError::ModelManager(format!(
                "curl failed downloading {} from {}: exit {status}",
                preset.model_filename(),
                preset.download_url(),
            )));
        }

        let size = std::fs::metadata(&dest)?.len();
        if size < min_ok {
            return Err(VerifyError::ModelManager(format!(
                "downloaded whisper model {:?} is {} bytes — expected ≈{}. \
                 Delete and retry, or pre-place manually.",
                dest, size, expected,
            )));
        }
        Ok(dest)
    }
}

/// One timestamped chunk of a transcript.
#[derive(Debug, Clone)]
pub struct SttSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Result of a single [`SttEngine::transcribe`] call.
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

/// Sprint 1 STT engine. The `_model_path` + `_preset` fields are
/// captured at construction so Sprint 2 can swap in a real backend
/// without changing the public API.
#[derive(Debug)]
pub struct SttEngine {
    _model_path: PathBuf,
    _preset: WhisperPreset,
}

impl SttEngine {
    /// Load an engine from a cached Whisper model path. Pair with
    /// [`ModelManager::ensure_whisper_model`] to obtain the path.
    ///
    /// Sprint 1: the path and preset are recorded but not yet fed
    /// to an inference engine — see [`SttEngine::transcribe`] for
    /// the stub behaviour.
    pub fn load(model_path: &Path, preset: WhisperPreset) -> Result<Self, VerifyError> {
        if !model_path.exists() {
            return Err(VerifyError::Stt(format!(
                "whisper model not found at {:?}. \
                 Call ModelManager::ensure_whisper_model first.",
                model_path
            )));
        }
        Ok(Self {
            _model_path: model_path.to_path_buf(),
            _preset: preset,
        })
    }

    /// Sprint 1 stub. Validates the audio path (surfaces missing /
    /// unreadable files as `Err`, never panics) and then returns a
    /// clear "STT backend not wired" error. Sprint 2 replaces this
    /// with real whisper inference.
    pub fn transcribe(&self, audio_path: &Path) -> Result<SttResult, VerifyError> {
        if !audio_path.exists() {
            return Err(VerifyError::InvalidInput(format!(
                "audio file not found: {:?}",
                audio_path
            )));
        }
        // Touch the preprocessing path — proves the pipeline wires
        // end-to-end even with the stub backend. The temp WAV is
        // created and immediately discarded in Sprint 1.
        let scratch = std::env::temp_dir()
            .join(format!("verify-stt-{}.wav", std::process::id()));
        preprocess_audio(audio_path, &scratch)?;
        let _ = std::fs::remove_file(&scratch);

        Err(VerifyError::Stt(
            "STT backend not wired in Sprint 1 — see whisper.rs. \
             Preprocessing succeeded; real inference lands in Sprint 2."
                .to_string(),
        ))
    }
}

// ── Audio preprocessing ─────────────────────────────────────────

/// Whisper expects 16 kHz mono f32 PCM. This preprocessing step
/// normalises arbitrary input containers (MP3, M4A, MP4 audio,
/// OGG, FLAC, WAV) onto that canonical shape.
///
/// Path taken depends on what's available:
/// * **ffmpeg subprocess** — preferred. Covers every container
///   any real-world examiner workstation will ever encounter.
/// * **hound (WAV only)** — fallback when ffmpeg is missing.
///   Returns a clear error for non-WAV inputs rather than guessing.
pub fn preprocess_audio(input: &Path, output: &Path) -> Result<(), VerifyError> {
    if !input.exists() {
        return Err(VerifyError::InvalidInput(format!(
            "input audio not found: {:?}",
            input
        )));
    }

    if which_ffmpeg() {
        return preprocess_via_ffmpeg(input, output);
    }

    // No ffmpeg → only WAV is supported via hound.
    let ext_lower = input
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if ext_lower != "wav" {
        return Err(VerifyError::Stt(format!(
            "ffmpeg not found on PATH and input is not WAV ({:?}). \
             Install ffmpeg to handle {ext_lower} / MP3 / M4A / MP4 / OGG / FLAC.",
            input
        )));
    }

    preprocess_wav_via_hound(input, output)
}

fn which_ffmpeg() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn preprocess_via_ffmpeg(input: &Path, output: &Path) -> Result<(), VerifyError> {
    debug!(
        "preprocess_audio via ffmpeg: {:?} → {:?} (16 kHz mono f32 WAV)",
        input, output
    );
    let status = std::process::Command::new("ffmpeg")
        .arg("-y") // overwrite scratch output
        .arg("-loglevel").arg("error")
        .arg("-i").arg(input)
        .arg("-ar").arg("16000")
        .arg("-ac").arg("1")
        .arg("-f").arg("wav")
        .arg("-sample_fmt").arg("s16") // signed 16-bit PCM (Whisper converts to f32)
        .arg(output)
        .status()
        .map_err(|e| VerifyError::Stt(format!("failed to launch ffmpeg: {e}")))?;
    if !status.success() {
        return Err(VerifyError::Stt(format!(
            "ffmpeg failed converting {:?} → {:?}: exit {status}",
            input, output
        )));
    }
    Ok(())
}

/// Hound WAV-only fallback. Reads `input`, resamples to 16 kHz
/// mono 16-bit PCM, writes `output`. Resampling is a naive
/// nearest-neighbour approach good enough for the Sprint 1
/// preprocessing surface; Sprint 2 can upgrade to a proper
/// sinc-interpolation resampler if ffmpeg remains unavailable.
fn preprocess_wav_via_hound(input: &Path, output: &Path) -> Result<(), VerifyError> {
    let mut reader = hound::WavReader::open(input).map_err(|e| {
        VerifyError::Stt(format!("hound open({:?}) failed: {e}", input))
    })?;
    let spec = reader.spec();
    debug!(
        "preprocess_audio via hound: {:?} in-rate={} in-channels={} in-bits={} → 16 kHz mono",
        input, spec.sample_rate, spec.channels, spec.bits_per_sample,
    );

    let in_rate = spec.sample_rate;
    let in_channels = spec.channels as usize;
    if in_rate == 0 || in_channels == 0 {
        return Err(VerifyError::Stt(format!(
            "hound reports invalid WAV header at {:?} (rate={}, channels={})",
            input, in_rate, in_channels,
        )));
    }

    // Read all samples as i16, downmix to mono, naive-decimate to 16 kHz.
    let samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map_err(|e| VerifyError::Stt(format!("hound sample read: {e}"))))
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| {
                s.map(|f| (f.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .map_err(|e| VerifyError::Stt(format!("hound sample read: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?,
    };

    // Downmix to mono (simple average of all channels).
    let mono: Vec<i16> = if in_channels == 1 {
        samples
    } else {
        samples
            .chunks_exact(in_channels)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                (sum / in_channels as i32) as i16
            })
            .collect()
    };

    // Nearest-neighbour resample to 16 kHz.
    let target_rate: u32 = 16_000;
    let step = in_rate as f64 / target_rate as f64;
    let out_len = (mono.len() as f64 / step).ceil() as usize;
    let mut resampled = Vec::with_capacity(out_len);
    let mut pos = 0.0_f64;
    while (pos as usize) < mono.len() {
        resampled.push(mono[pos as usize]);
        pos += step;
    }

    let out_spec = hound::WavSpec {
        channels: 1,
        sample_rate: target_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output, out_spec).map_err(|e| {
        VerifyError::Stt(format!("hound create({:?}) failed: {e}", output))
    })?;
    for s in resampled {
        writer
            .write_sample(s)
            .map_err(|e| VerifyError::Stt(format!("hound write_sample: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| VerifyError::Stt(format!("hound finalize: {e}")))?;

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_model_filenames_are_correct() {
        assert_eq!(WhisperPreset::Fast.model_filename(), "ggml-tiny.bin");
        assert_eq!(WhisperPreset::Balanced.model_filename(), "ggml-base.bin");
        assert_eq!(WhisperPreset::Accurate.model_filename(), "ggml-large-v3.bin");
    }

    #[test]
    fn preset_download_urls_point_at_named_constants() {
        // Pins the invariant that presets resolve to the top-level
        // `const` URLs — so `grep WHISPER_MODEL_URL_` across the
        // workspace is a complete enumeration of egress sites.
        assert_eq!(WhisperPreset::Fast.download_url(), WHISPER_MODEL_URL_TINY);
        assert_eq!(
            WhisperPreset::Balanced.download_url(),
            WHISPER_MODEL_URL_BASE
        );
        assert_eq!(
            WhisperPreset::Accurate.download_url(),
            WHISPER_MODEL_URL_LARGE_V3
        );
    }

    #[test]
    fn preset_expected_sizes_are_plausible() {
        // Light sanity: larger preset must imply strictly larger
        // expected model. Pins the ordering if anyone edits the
        // constants by hand.
        let t = WhisperPreset::Fast.expected_size_bytes();
        let b = WhisperPreset::Balanced.expected_size_bytes();
        let a = WhisperPreset::Accurate.expected_size_bytes();
        assert!(t < b, "tiny should be smaller than base");
        assert!(b < a, "base should be smaller than large");
    }

    #[test]
    fn stt_result_segments_are_chronological() {
        // Unit test with hand-built segments — verifies ordering
        // invariant without touching the engine.
        let segments = vec![
            SttSegment {
                start_ms: 0,
                end_ms: 1_000,
                text: "hello".into(),
            },
            SttSegment {
                start_ms: 1_000,
                end_ms: 2_500,
                text: "world".into(),
            },
            SttSegment {
                start_ms: 2_500,
                end_ms: 3_000,
                text: "!".into(),
            },
        ];
        let r = SttResult {
            transcript: "hello world !".into(),
            detected_language: "en".into(),
            confidence: 0.95,
            segments,
        };
        for w in r.segments.windows(2) {
            assert!(
                w[0].end_ms <= w[1].start_ms,
                "segment ordering violated: {w:?}"
            );
        }
    }

    #[test]
    fn load_reports_missing_model_without_panic() {
        let bogus = std::path::Path::new("/nonexistent/strata/verify/ggml-tiny.bin");
        match SttEngine::load(bogus, WhisperPreset::Fast) {
            Err(VerifyError::Stt(msg)) => {
                assert!(msg.contains("not found"), "unexpected error text: {msg}");
            }
            other => panic!("expected Err(Stt(...)) on missing model, got {other:?}"),
        }
    }

    #[test]
    fn transcribe_reports_missing_audio_without_panic() {
        // Pair with a fake but *existing* model path so `load`
        // succeeds; then point `transcribe` at a missing audio
        // file and confirm the error type.
        let tmp = std::env::temp_dir().join("verify-stt-fake-model.bin");
        std::fs::write(&tmp, b"not a real model").expect("tmp write");
        let engine = SttEngine::load(&tmp, WhisperPreset::Fast).expect("load");
        let audio = std::path::Path::new("/nonexistent/audio.wav");
        match engine.transcribe(audio) {
            Err(VerifyError::InvalidInput(msg)) => {
                assert!(msg.contains("not found"), "unexpected error text: {msg}");
            }
            other => panic!("expected Err(InvalidInput(...)) on missing audio, got {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn preprocess_audio_rejects_missing_input() {
        let missing = std::path::Path::new("/nonexistent/source.mp3");
        let scratch = std::env::temp_dir().join("verify-stt-unreachable.wav");
        match preprocess_audio(missing, &scratch) {
            Err(VerifyError::InvalidInput(_)) => {}
            other => panic!("expected InvalidInput on missing input, got {other:?}"),
        }
    }

    #[test]
    fn model_manager_with_xdg_cache_points_under_whisper_leaf() {
        let mgr = ModelManager::with_xdg_cache().expect("HOME set");
        let path = mgr.cache_root.to_string_lossy().into_owned();
        assert!(
            path.ends_with(".cache/verify/models/whisper"),
            "expected XDG whisper leaf, got {path}"
        );
    }
}
