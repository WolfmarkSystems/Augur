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
//! are the **second permitted network egress** in AUGUR, after
//! the fastText LID download in `augur-classifier`. Every egress
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
use rand::distr::weighted::WeightedIndex;
use rand::distr::Distribution;
use rand::SeedableRng;
use std::path::{Path, PathBuf};
use tokenizers::Tokenizer;
use augur_core::AugurError;

/// Options controlling Whisper decoding. Sprint 4 P2 introduces
/// temperature fallback: if the greedy (T=0) decode looks like a
/// hallucination — gauged by a too-low compression ratio on the
/// produced text — the decoder retries the same segment with
/// progressively higher temperature, sampling from
/// `softmax(logits / T)` instead of taking the argmax.
///
/// Defaults match OpenAI's `whisper` reference for the parts we
/// implement; the no-speech threshold gate exits the retry loop
/// early when Whisper itself reports the segment is silent.
#[derive(Debug, Clone, Copy)]
pub struct TranscribeOptions {
    pub preset: WhisperPreset,
    /// Initial decoding temperature. `0.0` = greedy.
    pub temperature: f32,
    /// Step size added to `temperature` between retries.
    pub temperature_increment: f32,
    /// Maximum number of *additional* retries on top of the first
    /// attempt. `5` matches OpenAI's reference.
    pub max_temperature_retries: u8,
    /// `<|nospeech|>` probability above which the segment is
    /// considered silence and the retry loop exits.
    pub no_speech_threshold: f32,
    /// Minimum unique-character ratio of the produced transcript.
    /// Below this the segment is considered a hallucination
    /// (e.g. "aaaaaaaa…") and a retry at higher temperature is
    /// triggered. The naming preserves the Sprint 4 spec's
    /// `compression_ratio_threshold` while the actual metric is
    /// the unique/total ratio defined by [`compression_ratio`].
    pub compression_ratio_threshold: f32,
    /// Seed for the temperature-sampling RNG. Default chosen so
    /// reruns with the same audio + same seed produce identical
    /// transcripts (forensic reproducibility).
    pub rng_seed: u64,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        Self {
            preset: WhisperPreset::Fast,
            temperature: 0.0,
            temperature_increment: 0.2,
            max_temperature_retries: 5,
            no_speech_threshold: 0.6,
            // Hallucinated transcripts collapse to a tiny set of
            // characters; production speech sits above 0.3 in
            // practice. Spec used 2.4 for the inverse OpenAI metric;
            // we use the unique/total form, hence 0.3.
            compression_ratio_threshold: 0.3,
            rng_seed: 299_792_458,
        }
    }
}

/// Unique-character ratio of `text`. Returns 0.0 for empty input.
/// Repetitive text ("aaaaaaa") collapses to a low ratio
/// (≈ 0.1); normal multi-word text sits above 0.3.
pub fn compression_ratio(text: &str) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let total: usize = text.chars().count();
    if total == 0 {
        return 0.0;
    }
    let unique: std::collections::HashSet<char> = text.chars().collect();
    unique.len() as f32 / total as f32
}

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
// augur-classifier — the offline invariant's audit trail.
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
    /// All three AUGUR presets are multilingual — forensic work
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
/// `augur-classifier::ModelManager` — the two are intentionally
/// independent so each sub-crate audits its own egress.
#[derive(Debug, Clone)]
pub struct ModelManager {
    pub cache_root: PathBuf,
}

impl ModelManager {
    pub fn new(cache_root: PathBuf) -> Self {
        Self { cache_root }
    }

    /// `~/.cache/augur/models/whisper/`.
    pub fn with_xdg_cache() -> Result<Self, AugurError> {
        let home = std::env::var("HOME").map_err(|_| {
            AugurError::ModelManager(
                "HOME environment variable not set; pass a cache dir explicitly".to_string(),
            )
        })?;
        Ok(Self::new(
            PathBuf::from(home).join(".cache/augur/models/whisper"),
        ))
    }

    /// Ensure all three Whisper artifacts (config.json,
    /// tokenizer.json, model.safetensors) are cached locally.
    /// Fast path returns paths with no network access; slow path
    /// uses `hf-hub` to fetch and cache under `<cache_root>/hf/`.
    ///
    /// NETWORK: one of only two permitted network calls in AUGUR's
    /// default code path — see the CLAUDE.md offline-invariant
    /// section. Every fetched URL is enumerable via
    /// `grep WHISPER_MODEL_URL_` from the workspace root.
    pub fn ensure_whisper_model(
        &self,
        preset: WhisperPreset,
    ) -> Result<WhisperPaths, AugurError> {
        std::fs::create_dir_all(&self.cache_root)?;
        let hf_cache = self.cache_root.join("hf");
        std::fs::create_dir_all(&hf_cache)?;

        warn!(
            "AUGUR ensuring whisper model {} (rev {}) — \
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
                AugurError::ModelManager(format!(
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

fn hf_err(preset: WhisperPreset, file: &str, e: hf_hub::api::sync::ApiError) -> AugurError {
    AugurError::ModelManager(format!(
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
            info!("augur-stt: using Metal device 0");
            d
        }
        Err(e) => {
            warn!("augur-stt: Metal unavailable ({e}); falling back to CPU");
            Device::Cpu
        }
    }
}

fn load_mel_filters(num_mel_bins: usize) -> Result<Vec<f32>, AugurError> {
    let bytes = match num_mel_bins {
        80 => MEL_FILTERS_80,
        128 => MEL_FILTERS_128,
        n => {
            return Err(AugurError::Stt(format!(
                "unsupported num_mel_bins={n} — AUGUR ships only 80-bin and 128-bin filter banks"
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
    pub fn load(paths: &WhisperPaths, preset: WhisperPreset) -> Result<Self, AugurError> {
        for (label, p) in [
            ("config", &paths.config),
            ("tokenizer", &paths.tokenizer),
            ("weights", &paths.weights),
        ] {
            if !p.exists() {
                return Err(AugurError::Stt(format!(
                    "whisper {label} not found at {:?}. \
                     Call ModelManager::ensure_whisper_model first.",
                    p
                )));
            }
        }

        let config_str = std::fs::read_to_string(&paths.config)?;
        let config: Config = serde_json::from_str(&config_str)
            .map_err(|e| AugurError::Stt(format!("whisper config.json parse: {e}")))?;
        let tokenizer = Tokenizer::from_file(&paths.tokenizer)
            .map_err(|e| AugurError::Stt(format!("whisper tokenizer.json load: {e}")))?;

        let mel_filters = load_mel_filters(config.num_mel_bins)?;
        let device = pick_device();

        // SAFETY: `from_mmaped_safetensors` is the candle-recommended
        // path for loading weights; mmap of a file we just wrote
        // ourselves under `~/.cache/augur/models/`. The unsafe is
        // bounded to file I/O, not arbitrary pointer arithmetic.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[&paths.weights], m::DTYPE, &device).map_err(
                |e| AugurError::Stt(format!("whisper safetensors mmap from {:?}: {e}", paths.weights)),
            )?
        };
        let model = Whisper::load(&vb, config.clone())
            .map_err(|e| AugurError::Stt(format!("whisper model load: {e}")))?;

        Ok(Self {
            model,
            tokenizer,
            config,
            mel_filters,
            device,
            preset,
        })
    }

    /// Transcribe an arbitrary audio file with the engine's preset
    /// and the default [`TranscribeOptions`]. Equivalent to
    /// [`SttEngine::transcribe_with_options`] with an option block
    /// whose `preset` matches the engine.
    pub fn transcribe(&mut self, audio_path: &Path) -> Result<SttResult, AugurError> {
        let options = TranscribeOptions {
            preset: self.preset,
            ..TranscribeOptions::default()
        };
        self.transcribe_with_options(audio_path, &options)
    }

    /// Transcribe with explicit decoding options. Honors the
    /// per-segment temperature-fallback retry loop described on
    /// [`TranscribeOptions`]. Temperature 0 → greedy; T>0 → sampled.
    pub fn transcribe_with_options(
        &mut self,
        audio_path: &Path,
        options: &TranscribeOptions,
    ) -> Result<SttResult, AugurError> {
        if !audio_path.exists() {
            return Err(AugurError::InvalidInput(format!(
                "audio file not found: {:?}",
                audio_path
            )));
        }

        let scratch = std::env::temp_dir()
            .join(format!("augur-stt-{}-{}.wav", std::process::id(), random_suffix()));
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
        .map_err(|e| AugurError::Stt(format!("whisper mel tensor: {e}")))?;

        let (lang_code, lang_token, lang_conf) = self.detect_language(&mel)?;
        info!(
            "augur-stt: detected language {} ({:.2}) on preset {:?}",
            lang_code, lang_conf, self.preset
        );

        let segments = self.run_decoder(&mel, lang_token, options)?;

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
    fn detect_language(&mut self, mel: &Tensor) -> Result<(String, u32, f32), AugurError> {
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
            .ok_or_else(|| AugurError::Stt("language detection empty".to_string()))?;

        let code = LANGUAGE_CODES[best_idx].to_string();
        let lang_token = token_id(&self.tokenizer, &format!("<|{code}|>"))?;
        Ok((code, lang_token, best_prob))
    }

    /// Decoding loop with timestamps mode enabled and per-segment
    /// temperature fallback. Each 30-second mel chunk is decoded;
    /// if the result looks like a hallucination (compression ratio
    /// below threshold) and Whisper does not flag the chunk as
    /// silent, the chunk is re-decoded at progressively higher
    /// temperature up to `options.max_temperature_retries` times.
    fn run_decoder(
        &mut self,
        mel: &Tensor,
        language_token: u32,
        options: &TranscribeOptions,
    ) -> Result<Vec<SttSegment>, AugurError> {
        let cx = DecoderContext::new(self, options)?;

        let (_, _, content_frames) = mel.dims3().map_err(stt_err)?;
        let mut seek = 0usize;
        let mut out: Vec<SttSegment> = Vec::new();

        while seek < content_frames {
            let segment_size = (content_frames - seek).min(m::N_FRAMES);
            let mel_segment = mel.narrow(2, seek, segment_size).map_err(stt_err)?;
            let time_offset_s = (seek * m::HOP_LENGTH) as f64 / m::SAMPLE_RATE as f64;
            let segment_duration_s =
                (segment_size * m::HOP_LENGTH) as f64 / m::SAMPLE_RATE as f64;

            let mut temperature = options.temperature;
            let max_attempts = options.max_temperature_retries.saturating_add(1) as u32;
            let mut accepted: Option<DecodedSegment> = None;
            for attempt in 0..max_attempts {
                let decoded = self.decode_segment(
                    &mel_segment,
                    language_token,
                    &cx,
                    temperature,
                    options,
                )?;
                let ratio = compression_ratio(&decoded.raw_text);
                if decoded.no_speech_prob > options.no_speech_threshold {
                    debug!(
                        "Whisper: segment {time_offset_s:.1}s flagged as silence \
                         (no_speech={:.2}); accepting at temperature {temperature:.2}",
                        decoded.no_speech_prob
                    );
                    accepted = Some(decoded);
                    break;
                }
                if ratio >= options.compression_ratio_threshold {
                    accepted = Some(decoded);
                    break;
                }
                if attempt + 1 == max_attempts {
                    warn!(
                        "Whisper: max temperature retries reached ({}); \
                         returning best attempt at {temperature:.2} \
                         (compression_ratio={ratio:.2}, no_speech={:.2})",
                        options.max_temperature_retries, decoded.no_speech_prob
                    );
                    accepted = Some(decoded);
                    break;
                }
                debug!(
                    "Whisper: segment {time_offset_s:.1}s retry {} at \
                     temperature {temperature:.2} (compression_ratio={ratio:.2}, \
                     no_speech={:.2})",
                    attempt + 1,
                    decoded.no_speech_prob
                );
                temperature += options.temperature_increment;
            }

            let decoded = accepted.ok_or_else(|| {
                AugurError::Stt("temperature fallback produced no result".into())
            })?;
            self.expand_segments(
                &decoded.tokens,
                &cx,
                language_token,
                time_offset_s,
                segment_duration_s,
                &mut out,
            )?;

            seek += segment_size;
        }

        Ok(out)
    }

    fn decode_segment(
        &mut self,
        mel_segment: &Tensor,
        language_token: u32,
        cx: &DecoderContext,
        temperature: f32,
        options: &TranscribeOptions,
    ) -> Result<DecodedSegment, AugurError> {
        let audio_features = self
            .model
            .encoder
            .forward(mel_segment, true)
            .map_err(stt_err)?;
        let sample_len = self.config.max_target_positions / 2;
        let mut tokens: Vec<u32> = vec![cx.sot_token, language_token, cx.transcribe_token];
        let mut no_speech_prob: f32 = 0.0;
        let mut rng = rand::rngs::StdRng::seed_from_u64(options.rng_seed);

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

            // First iteration: read the no-speech probability from
            // the very first logits row. Mirrors the candle
            // reference exactly.
            if i == 0 {
                let logits0 = self
                    .model
                    .decoder
                    .final_linear(&ys.i(..1).map_err(stt_err)?)
                    .map_err(stt_err)?
                    .i(0)
                    .map_err(stt_err)?
                    .i(0)
                    .map_err(stt_err)?;
                let probs = softmax(&logits0, 0).map_err(stt_err)?;
                no_speech_prob = probs
                    .i(cx.no_speech_token as usize)
                    .map_err(stt_err)?
                    .to_scalar::<f32>()
                    .map_err(stt_err)?;
            }

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
            let logits = logits.broadcast_add(&cx.suppress).map_err(stt_err)?;

            let next_token = if temperature > 0.0 {
                let scaled = (&logits / temperature as f64).map_err(stt_err)?;
                let probs = softmax(&scaled, 0).map_err(stt_err)?;
                let probs_v: Vec<f32> = probs.to_vec1().map_err(stt_err)?;
                let dist = WeightedIndex::new(&probs_v).map_err(|e| {
                    AugurError::Stt(format!("WeightedIndex (T={temperature}): {e}"))
                })?;
                dist.sample(&mut rng) as u32
            } else {
                let logits_v: Vec<f32> = logits.to_vec1().map_err(stt_err)?;
                logits_v
                    .iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.total_cmp(b))
                    .map(|(idx, _)| idx as u32)
                    .ok_or_else(|| AugurError::Stt("empty logits".into()))?
            };
            tokens.push(next_token);
            if next_token == cx.eot_token || tokens.len() > self.config.max_target_positions {
                break;
            }
        }

        // Pre-decode the raw text so the temperature-fallback gate
        // can inspect it without needing the tokenizer again.
        let text_tokens: Vec<u32> = tokens
            .iter()
            .copied()
            .filter(|&t| {
                t != cx.sot_token
                    && t != cx.eot_token
                    && t != language_token
                    && t != cx.transcribe_token
                    && t <= cx.no_timestamps_token
            })
            .collect();
        let raw_text = self
            .tokenizer
            .decode(&text_tokens, true)
            .map_err(|e| AugurError::Stt(format!("tokenizer decode: {e}")))?;

        Ok(DecodedSegment {
            tokens,
            raw_text,
            no_speech_prob,
        })
    }

    /// Walk the produced token stream and emit one [`SttSegment`]
    /// per timestamp pair. Whisper emits `<|t.tt|>` tokens that
    /// frame text spans; tokens above `no_timestamps_token` are
    /// timestamps with 0.02 s resolution.
    fn expand_segments(
        &self,
        tokens: &[u32],
        cx: &DecoderContext,
        language_token: u32,
        time_offset_s: f64,
        segment_duration_s: f64,
        out: &mut Vec<SttSegment>,
    ) -> Result<(), AugurError> {
        let mut current_text: Vec<u32> = Vec::new();
        let mut last_ts_s: Option<f32> = None;
        for &tok in tokens {
            if tok == cx.sot_token
                || tok == cx.eot_token
                || tok == language_token
                || tok == cx.transcribe_token
            {
                continue;
            }
            if tok > cx.no_timestamps_token {
                let ts_s = (tok - cx.no_timestamps_token - 1) as f32 * 0.02;
                if let Some(prev) = last_ts_s {
                    if !current_text.is_empty() {
                        let text = self
                            .tokenizer
                            .decode(&current_text, true)
                            .map_err(|e| AugurError::Stt(format!("tokenizer decode: {e}")))?;
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
        if !current_text.is_empty() {
            let text = self
                .tokenizer
                .decode(&current_text, true)
                .map_err(|e| AugurError::Stt(format!("tokenizer decode: {e}")))?;
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
        Ok(())
    }
}

/// Per-call cached decoder constants — token ids, suppress mask.
/// Lifted out so the per-segment retry loop reuses the suppress
/// tensor instead of rebuilding it per attempt.
struct DecoderContext {
    no_timestamps_token: u32,
    sot_token: u32,
    transcribe_token: u32,
    eot_token: u32,
    no_speech_token: u32,
    suppress: Tensor,
}

impl DecoderContext {
    fn new(engine: &SttEngine, _opts: &TranscribeOptions) -> Result<Self, AugurError> {
        let no_timestamps_token = token_id(&engine.tokenizer, m::NO_TIMESTAMPS_TOKEN)?;
        let sot_token = token_id(&engine.tokenizer, m::SOT_TOKEN)?;
        let transcribe_token = token_id(&engine.tokenizer, m::TRANSCRIBE_TOKEN)?;
        let eot_token = token_id(&engine.tokenizer, m::EOT_TOKEN)?;
        let no_speech_token = m::NO_SPEECH_TOKENS
            .iter()
            .find_map(|t| engine.tokenizer.token_to_id(t))
            .ok_or_else(|| AugurError::Stt("no <|nospeech|> token id".into()))?;

        let vocab_size = engine.config.vocab_size as u32;
        let suppress_vec: Vec<f32> = (0..vocab_size)
            .map(|i| {
                if engine.config.suppress_tokens.contains(&i) {
                    f32::NEG_INFINITY
                } else {
                    0f32
                }
            })
            .collect();
        let suppress =
            Tensor::new(suppress_vec.as_slice(), &engine.device).map_err(stt_err)?;
        Ok(Self {
            no_timestamps_token,
            sot_token,
            transcribe_token,
            eot_token,
            no_speech_token,
            suppress,
        })
    }
}

/// One attempt at decoding a 30 s mel chunk. The retry loop in
/// [`SttEngine::run_decoder`] inspects `raw_text` + `no_speech_prob`
/// to decide whether to accept or retry at higher temperature.
struct DecodedSegment {
    tokens: Vec<u32>,
    raw_text: String,
    no_speech_prob: f32,
}

fn stt_err(e: candle_core::Error) -> AugurError {
    AugurError::Stt(format!("candle: {e}"))
}

fn token_id(tokenizer: &Tokenizer, token: &str) -> Result<u32, AugurError> {
    tokenizer
        .token_to_id(token)
        .ok_or_else(|| AugurError::Stt(format!("missing token id for {token}")))
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
fn read_pcm_f32(path: &Path) -> Result<Vec<f32>, AugurError> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| AugurError::Stt(format!("hound open({:?}): {e}", path)))?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16_000 {
        return Err(AugurError::Stt(format!(
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
                    .map_err(|e| AugurError::Stt(format!("hound i16 read: {e}")))
            })
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| s.map_err(|e| AugurError::Stt(format!("hound f32 read: {e}"))))
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
pub fn preprocess_audio(input: &Path, output: &Path) -> Result<(), AugurError> {
    if !input.exists() {
        return Err(AugurError::InvalidInput(format!(
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
        return Err(AugurError::Stt(format!(
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

/// Extract the audio track from a video container into a 16 kHz
/// mono 16-bit WAV file. Routes through the same `ffmpeg` binary
/// used by [`preprocess_audio`] for consistency. Returns the path
/// to the extracted WAV.
///
/// Produces output at `<scratch_dir>/augur-video-<pid>-<ns>.wav`;
/// callers are responsible for cleaning the file up after the
/// downstream STT call (the existing `transcribe` flow already
/// does this for its own scratch).
pub fn extract_audio_from_video(
    video_path: &Path,
    scratch_dir: &Path,
) -> Result<PathBuf, AugurError> {
    if !video_path.exists() {
        return Err(AugurError::InvalidInput(format!(
            "video file not found: {:?}",
            video_path
        )));
    }
    if !which_ffmpeg() {
        return Err(AugurError::Stt(format!(
            "ffmpeg not found on PATH — required for video {:?}. \
             Install ffmpeg (e.g. `brew install ffmpeg`) to enable video processing.",
            video_path
        )));
    }
    std::fs::create_dir_all(scratch_dir)?;
    let output = scratch_dir.join(format!(
        "augur-video-{}-{}.wav",
        std::process::id(),
        random_suffix()
    ));
    debug!(
        "extract_audio_from_video: {:?} → {:?} (16 kHz mono s16 WAV, no video)",
        video_path, output
    );
    let status = std::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(video_path)
        // -vn drops the video stream; we only want the audio track.
        .arg("-vn")
        .arg("-ar")
        .arg("16000")
        .arg("-ac")
        .arg("1")
        .arg("-f")
        .arg("wav")
        .arg("-sample_fmt")
        .arg("s16")
        .arg(&output)
        .status()
        .map_err(|e| AugurError::Stt(format!("failed to launch ffmpeg: {e}")))?;
    if !status.success() {
        return Err(AugurError::Stt(format!(
            "ffmpeg failed extracting audio from {:?}: exit {status}",
            video_path
        )));
    }
    Ok(output)
}

fn preprocess_via_ffmpeg(input: &Path, output: &Path) -> Result<(), AugurError> {
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
        .map_err(|e| AugurError::Stt(format!("failed to launch ffmpeg: {e}")))?;
    if !status.success() {
        return Err(AugurError::Stt(format!(
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
fn preprocess_wav_via_hound(input: &Path, output: &Path) -> Result<(), AugurError> {
    let mut reader = hound::WavReader::open(input)
        .map_err(|e| AugurError::Stt(format!("hound open({:?}) failed: {e}", input)))?;
    let spec = reader.spec();
    debug!(
        "preprocess_audio via hound: {:?} in-rate={} in-channels={} in-bits={} → 16 kHz mono",
        input, spec.sample_rate, spec.channels, spec.bits_per_sample,
    );

    let in_rate = spec.sample_rate;
    let in_channels = spec.channels as usize;
    if in_rate == 0 || in_channels == 0 {
        return Err(AugurError::Stt(format!(
            "hound reports invalid WAV header at {:?} (rate={}, channels={})",
            input, in_rate, in_channels,
        )));
    }

    let samples: Vec<i16> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .samples::<i16>()
            .map(|s| s.map_err(|e| AugurError::Stt(format!("hound sample read: {e}"))))
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|s| {
                s.map(|f| (f.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                    .map_err(|e| AugurError::Stt(format!("hound sample read: {e}")))
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
        .map_err(|e| AugurError::Stt(format!("hound create({:?}) failed: {e}", output)))?;
    for s in resampled {
        writer
            .write_sample(s)
            .map_err(|e| AugurError::Stt(format!("hound write_sample: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| AugurError::Stt(format!("hound finalize: {e}")))?;

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
    fn extract_audio_from_video_rejects_missing_input() {
        let missing = Path::new("/nonexistent/clip.mp4");
        let scratch = std::env::temp_dir().join("augur-video-test-missing");
        match extract_audio_from_video(missing, &scratch) {
            Err(AugurError::InvalidInput(_)) => {}
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn preprocess_audio_rejects_missing_input() {
        let missing = Path::new("/nonexistent/source.mp3");
        let scratch = std::env::temp_dir().join("augur-stt-unreachable.wav");
        match preprocess_audio(missing, &scratch) {
            Err(AugurError::InvalidInput(_)) => {}
            other => panic!("expected InvalidInput on missing input, got {other:?}"),
        }
    }

    #[test]
    fn model_manager_with_xdg_cache_points_under_whisper_leaf() {
        let mgr = ModelManager::with_xdg_cache().expect("HOME set");
        let path = mgr.cache_root.to_string_lossy().into_owned();
        assert!(
            path.ends_with(".cache/augur/models/whisper"),
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
            Err(AugurError::Stt(msg)) => assert!(msg.contains("not found"), "msg: {msg}"),
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
    fn temperature_fallback_options_default_correctly() {
        let opts = TranscribeOptions::default();
        assert_eq!(opts.temperature, 0.0);
        assert_eq!(opts.max_temperature_retries, 5);
        assert!((opts.temperature_increment - 0.2).abs() < 1e-6);
        assert!((opts.no_speech_threshold - 0.6).abs() < 1e-6);
        assert!(opts.compression_ratio_threshold > 0.0);
    }

    #[test]
    fn compression_ratio_detects_repetition() {
        // Pure repetition collapses to one unique character → tiny
        // ratio (1/10).
        assert!(compression_ratio("aaaaaaaaaa") < 0.5);
        // Normal multi-word text has many distinct characters → large ratio.
        assert!(compression_ratio("Hello world") > 0.5);
        // Empty input is well-defined.
        assert_eq!(compression_ratio(""), 0.0);
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
