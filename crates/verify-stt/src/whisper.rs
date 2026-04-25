//! Whisper STT — audio to transcript via candle (pure Rust).
//!
//! # Sprint 2 backend choice — candle-whisper
//!
//! The Sprint 1 `whisper-rs` probe was rejected: it wraps
//! `whisper.cpp` via FFI and requires `cmake` + a C++ toolchain at
//! build time. The Sprint 2 probe of Hugging Face's `candle`
//! framework built cleanly on macOS ARM64 in ~44 s with the
//! `metal` feature (no cmake, no FFI, pure Rust). This file wires
//! `candle-transformers::models::whisper` for real inference.
//!
//! Weights are now safetensors fetched from Hugging Face
//! (`openai/whisper-*`) rather than the GGML format used by
//! whisper.cpp — the GGML constants from Sprint 1 have been
//! retired. Mel filter banks (80- and 128-bin) are bundled into
//! the binary via [`include_bytes!`] from `assets/`.
//!
//! # Offline invariant
//!
//! Whisper model fetches via [`ModelManager::ensure_whisper_model`]
//! are the **second permitted network egress** in VERIFY, after
//! the fastText LID download in `verify-classifier`. Every egress
//! URL is a named top-level `const` so `grep WHISPER_MODEL` and
//! `grep LID_MODEL` surface every network call site in the whole
//! workspace.

use byteorder::{ByteOrder, LittleEndian};
use candle_core::{Device, IndexOp, Tensor, D};
use candle_nn::{ops::softmax, VarBuilder};
use candle_transformers::models::whisper::{
    self as m, audio as whisper_audio, model::Whisper, Config,
};
use log::{debug, info, warn};
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;
use verify_core::VerifyError;

/// Whisper model presets. Speed / accuracy tradeoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperPreset {
    /// `openai/whisper-tiny` — ~150 MB safetensors. Fast, lower accuracy.
    Fast,
    /// `openai/whisper-base` — ~290 MB safetensors. Balanced.
    Balanced,
    /// `openai/whisper-large-v3` — ~3 GB safetensors. Most accurate.
    Accurate,
}

// ── Egress-point constants ──────────────────────────────────────
//
// Every Whisper safetensors URL is a named top-level `const` so
// `grep WHISPER_MODEL_` from the workspace root enumerates every
// egress site. Same pattern as `LID_MODEL_URL` in
// verify-classifier — the offline invariant's audit trail.
// hf-hub composes the actual download from `<repo>/resolve/<rev>/<file>`,
// but we restate the URLs here so a grep finds them.

pub const WHISPER_MODEL_URL_TINY: &str =
    "https://huggingface.co/openai/whisper-tiny/resolve/main/model.safetensors";
pub const WHISPER_MODEL_URL_BASE: &str =
    "https://huggingface.co/openai/whisper-base/resolve/main/model.safetensors";
pub const WHISPER_MODEL_URL_LARGE_V3: &str =
    "https://huggingface.co/openai/whisper-large-v3/resolve/main/model.safetensors";

impl WhisperPreset {
    /// Hugging Face repo id for this preset.
    pub fn hf_repo(&self) -> &'static str {
        match self {
            Self::Fast => "openai/whisper-tiny",
            Self::Balanced => "openai/whisper-base",
            Self::Accurate => "openai/whisper-large-v3",
        }
    }

    /// Revision pinned for this preset. The candle example pins
    /// `refs/pr/22` for whisper-base because main lacks the safetensors
    /// upload at the time of writing; we follow that pinning to keep
    /// the download deterministic.
    pub fn hf_revision(&self) -> &'static str {
        match self {
            Self::Fast => "main",
            Self::Balanced => "refs/pr/22",
            Self::Accurate => "main",
        }
    }

    /// Direct safetensors URL — the named egress constant for this preset.
    pub fn download_url(&self) -> &'static str {
        match self {
            Self::Fast => WHISPER_MODEL_URL_TINY,
            Self::Balanced => WHISPER_MODEL_URL_BASE,
            Self::Accurate => WHISPER_MODEL_URL_LARGE_V3,
        }
    }

    /// Whether the preset is a multilingual model (vs `.en`-only).
    /// All three VERIFY presets are multilingual — forensic work
    /// virtually always involves non-English content.
    pub fn is_multilingual(&self) -> bool {
        true
    }
}

/// On-disk paths for a fully-cached Whisper preset. Returned by
/// [`ModelManager::ensure_whisper_model`].
#[derive(Debug, Clone)]
pub struct WhisperPaths {
    pub config: PathBuf,
    pub tokenizer: PathBuf,
    pub weights: PathBuf,
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

    /// Ensure all three Whisper artifacts (config.json,
    /// tokenizer.json, model.safetensors) are cached locally.
    /// Fast path returns paths with no network access; slow path
    /// uses `hf-hub` to fetch and cache under `<cache_root>/hf/`.
    ///
    /// NETWORK: one of only two permitted network calls in VERIFY's
    /// default code path — see the CLAUDE.md offline-invariant
    /// section. Every fetched URL is enumerable via
    /// `grep WHISPER_MODEL_URL_` from the workspace root.
    pub fn ensure_whisper_model(
        &self,
        preset: WhisperPreset,
    ) -> Result<WhisperPaths, VerifyError> {
        std::fs::create_dir_all(&self.cache_root)?;
        let hf_cache = self.cache_root.join("hf");
        std::fs::create_dir_all(&hf_cache)?;

        warn!(
            "VERIFY ensuring whisper model {} (rev {}) — \
             one-time per-preset network egress per the offline invariant. \
             Download URL: {}",
            preset.hf_repo(),
            preset.hf_revision(),
            preset.download_url(),
        );

        let api = hf_hub::api::sync::ApiBuilder::new()
            .with_cache_dir(hf_cache.clone())
            .build()
            .map_err(|e| {
                VerifyError::ModelManager(format!(
                    "failed to build hf-hub api at {hf_cache:?}: {e}"
                ))
            })?;
        let repo = api.repo(hf_hub::Repo::with_revision(
            preset.hf_repo().to_string(),
            hf_hub::RepoType::Model,
            preset.hf_revision().to_string(),
        ));

        let config = repo
            .get("config.json")
            .map_err(|e| hf_err(preset, "config.json", e))?;
        let tokenizer = repo
            .get("tokenizer.json")
            .map_err(|e| hf_err(preset, "tokenizer.json", e))?;
        let weights = repo
            .get("model.safetensors")
            .map_err(|e| hf_err(preset, "model.safetensors", e))?;

        debug!(
            "whisper {:?} cached: config={:?} tokenizer={:?} weights={:?}",
            preset, config, tokenizer, weights
        );
        Ok(WhisperPaths {
            config,
            tokenizer,
            weights,
        })
    }
}

fn hf_err(preset: WhisperPreset, file: &str, e: hf_hub::api::sync::ApiError) -> VerifyError {
    VerifyError::ModelManager(format!(
        "hf-hub failed fetching {} for preset {:?} from {}: {e}",
        file,
        preset,
        preset.hf_repo(),
    ))
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

/// The whisper STT engine. Owns the loaded candle model + tokenizer
/// + mel filter bank for the lifetime of one transcribe call.
pub struct SttEngine {
    model: Whisper,
    tokenizer: Tokenizer,
    config: Config,
    mel_filters: Vec<f32>,
    device: Device,
    preset: WhisperPreset,
}

impl std::fmt::Debug for SttEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SttEngine")
            .field("preset", &self.preset)
            .field("device", &self.device.location())
            .finish_non_exhaustive()
    }
}

// Bundled mel filter banks — extracted from the candle whisper example.
// 80-bin for tiny/base/small/medium/large-v1/v2. 128-bin for large-v3.
// 64320 bytes = 80 mel bins * 201 fft bins * 4 bytes (f32 LE).
const MEL_FILTERS_80: &[u8] = include_bytes!("../assets/melfilters.bytes");
const MEL_FILTERS_128: &[u8] = include_bytes!("../assets/melfilters128.bytes");

fn pick_device() -> Device {
    match Device::new_metal(0) {
        Ok(d) => {
            info!("verify-stt: using Metal device 0");
            d
        }
        Err(e) => {
            warn!("verify-stt: Metal unavailable ({e}); falling back to CPU");
            Device::Cpu
        }
    }
}

fn load_mel_filters(num_mel_bins: usize) -> Result<Vec<f32>, VerifyError> {
    let bytes = match num_mel_bins {
        80 => MEL_FILTERS_80,
        128 => MEL_FILTERS_128,
        n => {
            return Err(VerifyError::Stt(format!(
                "unsupported num_mel_bins={n} — VERIFY ships only 80-bin and 128-bin filter banks"
            )));
        }
    };
    let mut filters = vec![0f32; bytes.len() / 4];
    LittleEndian::read_f32_into(bytes, &mut filters);
    Ok(filters)
}

impl SttEngine {
    /// Load an engine from the cached safetensors weights + tokenizer
    /// + config produced by [`ModelManager::ensure_whisper_model`].
    pub fn load(paths: &WhisperPaths, preset: WhisperPreset) -> Result<Self, VerifyError> {
        for (label, p) in [
            ("config", &paths.config),
            ("tokenizer", &paths.tokenizer),
            ("weights", &paths.weights),
        ] {
            if !p.exists() {
                return Err(VerifyError::Stt(format!(
                    "whisper {label} not found at {:?}. \
                     Call ModelManager::ensure_whisper_model first.",
                    p
                )));
            }
        }

        let config_str = std::fs::read_to_string(&paths.config)?;
        let config: Config = serde_json::from_str(&config_str)
            .map_err(|e| VerifyError::Stt(format!("whisper config.json parse: {e}")))?;
        let tokenizer = Tokenizer::from_file(&paths.tokenizer)
            .map_err(|e| VerifyError::Stt(format!("whisper tokenizer.json load: {e}")))?;

        let mel_filters = load_mel_filters(config.num_mel_bins)?;
        let device = pick_device();

        // SAFETY: `from_mmaped_safetensors` is the candle-recommended
        // path for loading weights; mmap of a file we just wrote
        // ourselves under `~/.cache/verify/models/`. The unsafe is
        // bounded to file I/O, not arbitrary pointer arithmetic.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&paths.weights], m::DTYPE, &device).map_err(
                |e| VerifyError::Stt(format!("whisper safetensors mmap from {:?}: {e}", paths.weights)),
            )?
        };
        let model = Whisper::load(&vb, config.clone())
            .map_err(|e| VerifyError::Stt(format!("whisper model load: {e}")))?;

        Ok(Self {
            model,
            tokenizer,
            config,
            mel_filters,
            device,
            preset,
        })
    }

    /// Transcribe an arbitrary audio file. Preprocesses to 16 kHz
    /// mono PCM, runs the full encoder + decoder pipeline, returns
    /// [`SttResult`] with timestamped segments and a Whisper-detected
    /// language code (ISO 639-1).
    pub fn transcribe(&mut self, audio_path: &Path) -> Result<SttResult, VerifyError> {
        if !audio_path.exists() {
            return Err(VerifyError::InvalidInput(format!(
                "audio file not found: {:?}",
                audio_path
            )));
        }

        let scratch = std::env::temp_dir()
            .join(format!("verify-stt-{}-{}.wav", std::process::id(), random_suffix()));
        preprocess_audio(audio_path, &scratch)?;
        let pcm = read_pcm_f32(&scratch)?;
        let _ = std::fs::remove_file(&scratch);

        let mel_vec = whisper_audio::pcm_to_mel(&self.config, &pcm, &self.mel_filters);
        let mel_len = mel_vec.len();
        let num_mel_bins = self.config.num_mel_bins;
        let mel = Tensor::from_vec(
            mel_vec,
            (1, num_mel_bins, mel_len / num_mel_bins),
            &self.device,
        )
        .map_err(|e| VerifyError::Stt(format!("whisper mel tensor: {e}")))?;

        let (lang_code, lang_token, lang_conf) = self.detect_language(&mel)?;
        info!(
            "verify-stt: detected language {} ({:.2}) on preset {:?}",
            lang_code, lang_conf, self.preset
        );

        let segments = self.run_decoder(&mel, lang_token)?;

        let transcript = segments
            .iter()
            .map(|s| s.text.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        Ok(SttResult {
            transcript,
            detected_language: lang_code,
            confidence: lang_conf,
            segments,
        })
    }

    /// Whisper's intrinsic language-detection pass. Runs the encoder
    /// once on (up to) the first 30 s of mel and selects the
    /// highest-probability language token.
    fn detect_language(&mut self, mel: &Tensor) -> Result<(String, u32, f32), VerifyError> {
        let dims = mel.dims3().map_err(stt_err)?;
        let seq_len = dims.2;
        let max_src = self.model.config.max_source_positions;
        let mel = mel.narrow(2, 0, seq_len.min(max_src)).map_err(stt_err)?;

        let language_token_ids: Vec<u32> = LANGUAGE_CODES
            .iter()
            .map(|code| token_id(&self.tokenizer, &format!("<|{code}|>")))
            .collect::<Result<Vec<_>, _>>()?;
        let sot_token = token_id(&self.tokenizer, m::SOT_TOKEN)?;

        let audio_features = self.model.encoder.forward(&mel, true).map_err(stt_err)?;
        let tokens = Tensor::new(&[[sot_token]], &self.device).map_err(stt_err)?;
        let lang_token_ids_t =
            Tensor::new(language_token_ids.as_slice(), &self.device).map_err(stt_err)?;
        let ys = self
            .model
            .decoder
            .forward(&tokens, &audio_features, true)
            .map_err(stt_err)?;
        let logits = self
            .model
            .decoder
            .final_linear(&ys.i(..1).map_err(stt_err)?)
            .map_err(stt_err)?
            .i(0)
            .map_err(stt_err)?
            .i(0)
            .map_err(stt_err)?;
        let logits = logits
            .index_select(&lang_token_ids_t, 0)
            .map_err(stt_err)?;
        let probs = softmax(&logits, D::Minus1).map_err(stt_err)?;
        let probs: Vec<f32> = probs.to_vec1().map_err(stt_err)?;

        let (best_idx, &best_prob) = probs
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
            .ok_or_else(|| VerifyError::Stt("language detection empty".to_string()))?;

        let code = LANGUAGE_CODES[best_idx].to_string();
        let lang_token = token_id(&self.tokenizer, &format!("<|{code}|>"))?;
        Ok((code, lang_token, best_prob))
    }

    /// Greedy decoding loop with timestamps mode enabled. Each
    /// 30-second mel segment yields a list of timestamp tokens and
    /// text tokens; we split the text by timestamp boundaries to
    /// produce [`SttSegment`]s.
    fn run_decoder(
        &mut self,
        mel: &Tensor,
        language_token: u32,
    ) -> Result<Vec<SttSegment>, VerifyError> {
        let no_timestamps_token = token_id(&self.tokenizer, m::NO_TIMESTAMPS_TOKEN)?;
        let sot_token = token_id(&self.tokenizer, m::SOT_TOKEN)?;
        let transcribe_token = token_id(&self.tokenizer, m::TRANSCRIBE_TOKEN)?;
        let eot_token = token_id(&self.tokenizer, m::EOT_TOKEN)?;

        let vocab_size = self.config.vocab_size as u32;
        let suppress_vec: Vec<f32> = (0..vocab_size)
            .map(|i| {
                if self.config.suppress_tokens.contains(&i) {
                    f32::NEG_INFINITY
                } else {
                    0f32
                }
            })
            .collect();
        let suppress = Tensor::new(suppress_vec.as_slice(), &self.device).map_err(stt_err)?;

        let (_, _, content_frames) = mel.dims3().map_err(stt_err)?;
        let mut seek = 0usize;
        let mut out: Vec<SttSegment> = Vec::new();

        while seek < content_frames {
            let segment_size = (content_frames - seek).min(m::N_FRAMES);
            let mel_segment = mel.narrow(2, seek, segment_size).map_err(stt_err)?;
            let time_offset_s = (seek * m::HOP_LENGTH) as f64 / m::SAMPLE_RATE as f64;
            let segment_duration_s =
                (segment_size * m::HOP_LENGTH) as f64 / m::SAMPLE_RATE as f64;

            let audio_features = self
                .model
                .encoder
                .forward(&mel_segment, true)
                .map_err(stt_err)?;
            let sample_len = self.config.max_target_positions / 2;
            let mut tokens: Vec<u32> = vec![sot_token, language_token, transcribe_token];

            for i in 0..sample_len {
                let tokens_t = Tensor::new(tokens.as_slice(), &self.device)
                    .map_err(stt_err)?
                    .unsqueeze(0)
                    .map_err(stt_err)?;
                let ys = self
                    .model
                    .decoder
                    .forward(&tokens_t, &audio_features, i == 0)
                    .map_err(stt_err)?;
                let (_, seq_len, _) = ys.dims3().map_err(stt_err)?;
                let logits = self
                    .model
                    .decoder
                    .final_linear(&ys.i((..1, seq_len - 1..)).map_err(stt_err)?)
                    .map_err(stt_err)?
                    .i(0)
                    .map_err(stt_err)?
                    .i(0)
                    .map_err(stt_err)?;
                let logits = logits.broadcast_add(&suppress).map_err(stt_err)?;
                let logits_v: Vec<f32> = logits.to_vec1().map_err(stt_err)?;
                let next_token = logits_v
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.total_cmp(b))
                    .map(|(idx, _)| idx as u32)
                    .ok_or_else(|| VerifyError::Stt("empty logits".into()))?;
                tokens.push(next_token);
                if next_token == eot_token || tokens.len() > self.config.max_target_positions {
                    break;
                }
            }

            // Parse timestamps + text out of the produced token stream.
            // Whisper emits `<|t.tt|>` timestamp tokens that frame
            // text spans; tokens above `no_timestamps_token` are
            // timestamps, with each step worth 0.02 s.
            let mut current_text: Vec<u32> = Vec::new();
            let mut last_ts_s: Option<f32> = None;
            for &tok in &tokens {
                if tok == sot_token
                    || tok == eot_token
                    || tok == language_token
                    || tok == transcribe_token
                {
                    continue;
                }
                if tok > no_timestamps_token {
                    let ts_s = (tok - no_timestamps_token - 1) as f32 * 0.02;
                    if let Some(prev) = last_ts_s {
                        if !current_text.is_empty() {
                            let text = self
                                .tokenizer
                                .decode(&current_text, true)
                                .map_err(|e| VerifyError::Stt(format!("tokenizer decode: {e}")))?;
                            let trimmed = text.trim().to_string();
                            if !trimmed.is_empty() {
                                let start_s = time_offset_s + prev as f64;
                                let end_s = time_offset_s + ts_s as f64;
                                out.push(SttSegment {
                                    start_ms: (start_s * 1000.0) as u64,
                                    end_ms: (end_s * 1000.0) as u64,
                                    text: trimmed,
                                });
                            }
                            current_text.clear();
                        }
                    }
                    last_ts_s = Some(ts_s);
                } else {
                    current_text.push(tok);
                }
            }
            // Tail text without a closing timestamp — attribute to the
            // remainder of the segment.
            if !current_text.is_empty() {
                let text = self
                    .tokenizer
                    .decode(&current_text, true)
                    .map_err(|e| VerifyError::Stt(format!("tokenizer decode: {e}")))?;
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    let start_s = time_offset_s + last_ts_s.unwrap_or(0.0) as f64;
                    let end_s = time_offset_s + segment_duration_s;
                    out.push(SttSegment {
                        start_ms: (start_s * 1000.0) as u64,
                        end_ms: (end_s * 1000.0) as u64,
                        text: trimmed,
                    });
                }
            }

            seek += segment_size;
        }

        Ok(out)
    }
}

fn stt_err(e: candle_core::Error) -> VerifyError {
    VerifyError::Stt(format!("candle: {e}"))
}

fn token_id(tokenizer: &Tokenizer, token: &str) -> Result<u32, VerifyError> {
    tokenizer
        .token_to_id(token)
        .ok_or_else(|| VerifyError::Stt(format!("missing token id for {token}")))
}

fn random_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Read a 16 kHz mono 16-bit-PCM WAV (the canonical output of
/// `preprocess_audio`) into f32 samples in the range [-1, 1].
fn read_pcm_f32(path: &Path) -> Result<Vec<f32>, VerifyError> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| VerifyError::Stt(format!("hound open({:?}): {e}", path)))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 {
        return Err(VerifyError::Stt(format!(
            "expected 16 kHz mono after preprocessing, got {} Hz / {} channels",
            spec.sample_rate, spec.channels
        )));
    }
    let scale = 1.0 / i16::MAX as f32;
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| {
                s.map(|v| v as f32 * scale)
                    .map_err(|e| VerifyError::Stt(format!("hound i16 read: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|e| VerifyError::Stt(format!("hound f32 read: {e}"))))
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(samples)
}

/// The 99 ISO 639-1 codes Whisper recognises, in tokenizer order.
/// Preserves the same order as the canonical candle multilingual
/// example so token-id selection is stable.
const LANGUAGE_CODES: [&str; 99] = [
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it",
    "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur",
    "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn",
    "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si",
    "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo",
    "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln",
    "ha", "ba", "jw", "su",
];

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
        "preprocess_audio via ffmpeg: {:?} → {:?} (16 kHz mono s16 WAV)",
        input, output
    );
    let status = std::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(input)
        .arg("-ar")
        .arg("16000")
        .arg("-ac")
        .arg("1")
        .arg("-f")
        .arg("wav")
        .arg("-sample_fmt")
        .arg("s16")
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
/// nearest-neighbour approach; users with non-WAV containers should
/// install ffmpeg.
fn preprocess_wav_via_hound(input: &Path, output: &Path) -> Result<(), VerifyError> {
    let mut reader = hound::WavReader::open(input)
        .map_err(|e| VerifyError::Stt(format!("hound open({:?}) failed: {e}", input)))?;
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
    let mut writer = hound::WavWriter::create(output, out_spec)
        .map_err(|e| VerifyError::Stt(format!("hound create({:?}) failed: {e}", output)))?;
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
    fn preset_repos_are_correct() {
        assert_eq!(WhisperPreset::Fast.hf_repo(), "openai/whisper-tiny");
        assert_eq!(WhisperPreset::Balanced.hf_repo(), "openai/whisper-base");
        assert_eq!(WhisperPreset::Accurate.hf_repo(), "openai/whisper-large-v3");
    }

    #[test]
    fn preset_download_urls_point_at_named_constants() {
        assert_eq!(WhisperPreset::Fast.download_url(), WHISPER_MODEL_URL_TINY);
        assert_eq!(WhisperPreset::Balanced.download_url(), WHISPER_MODEL_URL_BASE);
        assert_eq!(
            WhisperPreset::Accurate.download_url(),
            WHISPER_MODEL_URL_LARGE_V3
        );
    }

    #[test]
    fn all_presets_are_multilingual() {
        assert!(WhisperPreset::Fast.is_multilingual());
        assert!(WhisperPreset::Balanced.is_multilingual());
        assert!(WhisperPreset::Accurate.is_multilingual());
    }

    #[test]
    fn stt_result_segments_are_chronological() {
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
        ];
        let r = SttResult {
            transcript: "hello world".into(),
            detected_language: "en".into(),
            confidence: 0.95,
            segments,
        };
        for w in r.segments.windows(2) {
            assert!(w[0].end_ms <= w[1].start_ms, "segment ordering: {w:?}");
        }
    }

    #[test]
    fn stt_result_has_all_required_fields() {
        // Synthetic SttResult constructable from public fields —
        // verifies the public API surface is present without
        // requiring a model download.
        let r = SttResult {
            transcript: "x".into(),
            detected_language: "ar".into(),
            confidence: 0.5,
            segments: vec![SttSegment {
                start_ms: 0,
                end_ms: 100,
                text: "x".into(),
            }],
        };
        assert!(!r.transcript.is_empty());
        assert_eq!(r.detected_language, "ar");
        assert!(r.confidence > 0.0);
        assert_eq!(r.segments.len(), 1);
    }

    #[test]
    fn preprocess_audio_rejects_missing_input() {
        let missing = Path::new("/nonexistent/source.mp3");
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

    #[test]
    fn load_reports_missing_paths_without_panic() {
        let bogus = WhisperPaths {
            config: PathBuf::from("/nonexistent/config.json"),
            tokenizer: PathBuf::from("/nonexistent/tokenizer.json"),
            weights: PathBuf::from("/nonexistent/model.safetensors"),
        };
        match SttEngine::load(&bogus, WhisperPreset::Fast) {
            Err(VerifyError::Stt(msg)) => assert!(msg.contains("not found"), "msg: {msg}"),
            other => panic!("expected Err(Stt(...)) on missing paths, got {other:?}"),
        }
    }

    #[test]
    fn mel_filters_load_for_supported_bins() {
        assert_eq!(load_mel_filters(80).expect("80-bin").len(), 80 * 201);
        assert_eq!(load_mel_filters(128).expect("128-bin").len(), 128 * 201);
        assert!(load_mel_filters(64).is_err());
    }

    #[test]
    fn language_codes_table_size_matches_whisper() {
        // Whisper recognises 99 languages — pin the table size so
        // anyone editing this list cannot silently drop entries.
        assert_eq!(LANGUAGE_CODES.len(), 99);
        assert!(LANGUAGE_CODES.contains(&"ar"));
        assert!(LANGUAGE_CODES.contains(&"fa"));
        assert!(LANGUAGE_CODES.contains(&"ps"));
        assert!(LANGUAGE_CODES.contains(&"ur"));
    }
}
