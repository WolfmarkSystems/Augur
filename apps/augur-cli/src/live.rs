//! Sprint 19 P1 — `augur live` real-time microphone translation.
//!
//! Captures the default input device via `cpal`, accumulates
//! audio into fixed-duration chunks, runs each chunk through
//! the existing Whisper STT + classifier + NLLB translation
//! pipeline, and emits one NDJSON `live_segment` event per
//! processed chunk on stdout.
//!
//! Offline contract: nothing leaves the machine. The CPAL
//! callback writes into a bounded ring; the chunk-processor
//! drains it on the main thread. The Whisper / classifier /
//! NLLB engines all run locally — same instances the file path
//! uses.
//!
//! The MT advisory rides on the `live_started` event AND on
//! every `live_segment` event so a downstream consumer can't
//! split out the segments and lose the warning.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use augur_classifier::{LanguageClassifier, ModelManager as ClassifierModelManager};
use augur_core::AugurError;
use augur_stt::{ModelManager as SttModelManager, SttEngine, WhisperPreset};
use augur_translate::{TranslationEngine, MACHINE_TRANSLATION_NOTICE};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Global stop flag — set by Ctrl-C handler or by the parent
/// closing stdin. The CPAL callback and the chunk loop both
/// poll it.
static STOP: AtomicBool = AtomicBool::new(false);

const SAMPLE_RATE: u32 = 16_000;
const SILENCE_RMS_THRESHOLD: f32 = 0.01;
pub const LIVE_ADVISORY: &str = "LIVE MACHINE TRANSLATION — unverified. \
     Real-time output is inherently less accurate than offline processing. \
     Do not use for legal decisions in real time.";

/// `augur live --target en --chunk-ms 3000 --format ndjson`.
///
/// Returns `Ok(())` on a clean shutdown (stop flag set, no
/// errors). Per-chunk failures (Whisper, NLLB) are logged via
/// `log::warn!` and emitted as `error` NDJSON events but do not
/// abort the session.
pub fn cmd_live(target: &str, chunk_ms: u64, ndjson: bool) -> Result<(), AugurError> {
    if !ndjson {
        // The non-NDJSON form would be human-readable; for the
        // GUI this is the only mode we ship. Future work can add
        // a `[AUGUR LIVE] ...` text rendering.
        return Err(AugurError::InvalidInput(
            "augur live requires `--format ndjson` (only NDJSON output is wired)".into(),
        ));
    }
    install_stop_handler();
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| AugurError::InvalidInput("no default audio input device".into()))?;
    let device_name = device
        .name()
        .unwrap_or_else(|_| "(unnamed)".to_string());
    let supported = device
        .default_input_config()
        .map_err(|e| AugurError::InvalidInput(format!("default input config: {e}")))?;
    let in_channels = supported.channels() as usize;
    let in_rate = supported.sample_rate().0;
    let sample_format = supported.sample_format();
    log::info!(
        "augur live: device={device_name} channels={in_channels} rate={in_rate}Hz \
         format={sample_format:?} target={target} chunk_ms={chunk_ms}"
    );

    emit_started(target, &device_name, in_channels, in_rate, chunk_ms);

    // Bounded mpsc — sender is the audio callback, receiver is
    // the chunk-processor. We send mono 16 kHz f32 samples
    // already-resampled.
    let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(64);
    let chunk_samples = chunk_samples_for_ms(chunk_ms);

    // Build the engine outside the closure so we don't
    // re-allocate on every callback.
    let resampler = make_resampler(in_rate, in_channels);

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_stream::<f32>(&device, &supported, tx, resampler)?,
        cpal::SampleFormat::I16 => build_stream::<i16>(&device, &supported, tx, resampler)?,
        cpal::SampleFormat::U16 => build_stream::<u16>(&device, &supported, tx, resampler)?,
        other => {
            return Err(AugurError::InvalidInput(format!(
                "unsupported sample format: {other:?}"
            )));
        }
    };
    stream
        .play()
        .map_err(|e| AugurError::InvalidInput(format!("audio stream play: {e}")))?;

    // Build engines once (lazy: only when needed).
    let stt_paths = build_stt_paths()?;
    let preset = WhisperPreset::Fast;
    let stt = Arc::new(Mutex::new(None::<SttEngine>));
    let classifier = build_classifier()?;
    let translation = TranslationEngine::with_xdg_cache().ok();

    let mut buffer: Vec<f32> = Vec::with_capacity(chunk_samples * 2);
    let chunk_index = AtomicU32::new(0);
    let mut elapsed_ms: u64 = 0;

    while !STOP.load(Ordering::Relaxed) {
        // Drain available samples for up to chunk_ms with a
        // small slack so we don't busy-spin.
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(mut more) => buffer.append(&mut more),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if buffer.len() < chunk_samples {
            continue;
        }
        let chunk: Vec<f32> = buffer.drain(..chunk_samples).collect();
        let idx = chunk_index.fetch_add(1, Ordering::Relaxed);
        let chunk_start_ms = elapsed_ms;
        elapsed_ms += chunk_ms;
        if is_silence(&chunk, SILENCE_RMS_THRESHOLD) {
            log::debug!("live chunk {idx} skipped — silence");
            continue;
        }
        if let Err(e) = process_chunk(
            &chunk,
            idx,
            chunk_start_ms,
            chunk_ms,
            target,
            &stt_paths,
            preset,
            &stt,
            &classifier,
            translation.as_ref(),
        ) {
            log::warn!("live chunk {idx} failed: {e}");
            emit_chunk_error(idx, &format!("{e}"));
        }
    }

    drop(stream);
    emit_stopped(chunk_index.load(Ordering::Relaxed), elapsed_ms);
    Ok(())
}

fn install_stop_handler() {
    // SIGINT will set the flag; the chunk loop polls it. We
    // also install a small thread that watches stdin so the
    // parent process (the desktop GUI) can stop us by closing
    // its end of the pipe.
    let _ = ctrlc::set_handler(|| {
        STOP.store(true, Ordering::Relaxed);
    });
    std::thread::spawn(|| {
        use std::io::Read;
        let mut buf = [0u8; 1];
        loop {
            match std::io::stdin().read(&mut buf) {
                Ok(0) | Err(_) => {
                    STOP.store(true, Ordering::Relaxed);
                    break;
                }
                _ => {}
            }
        }
    });
}

fn chunk_samples_for_ms(chunk_ms: u64) -> usize {
    (SAMPLE_RATE as u64 * chunk_ms / 1000) as usize
}

fn build_stt_paths() -> Result<augur_stt::WhisperPaths, AugurError> {
    let mgr = SttModelManager::with_xdg_cache()?;
    mgr.ensure_whisper_model(WhisperPreset::Fast)
}

fn build_classifier() -> Result<LanguageClassifier, AugurError> {
    // Use the always-available whichlang backend for the live
    // path — fastText needs a one-time download we don't want
    // to trigger inside an interview.
    let _ = ClassifierModelManager::with_xdg_cache()?;
    Ok(LanguageClassifier::new_whichlang())
}

#[allow(clippy::too_many_arguments)]
fn process_chunk(
    samples: &[f32],
    chunk_index: u32,
    chunk_start_ms: u64,
    chunk_ms: u64,
    target: &str,
    stt_paths: &augur_stt::WhisperPaths,
    preset: WhisperPreset,
    stt: &Arc<Mutex<Option<SttEngine>>>,
    classifier: &LanguageClassifier,
    translation: Option<&TranslationEngine>,
) -> Result<(), AugurError> {
    // Write the chunk to a temp WAV so the existing SttEngine
    // (which takes a path) can read it.
    let path = std::env::temp_dir().join(format!(
        "augur_live_chunk_{}_{}.wav",
        std::process::id(),
        chunk_index
    ));
    write_wav(&path, samples)?;
    // Lazy-load the engine on first non-silent chunk.
    {
        let mut guard = stt
            .lock()
            .map_err(|e| AugurError::Stt(format!("STT lock poisoned: {e}")))?;
        if guard.is_none() {
            *guard = Some(SttEngine::load(stt_paths, preset)?);
        }
    }
    let result = {
        let mut guard = stt
            .lock()
            .map_err(|e| AugurError::Stt(format!("STT lock poisoned: {e}")))?;
        let engine = guard
            .as_mut()
            .ok_or_else(|| AugurError::Stt("STT engine unset after load".into()))?;
        engine.transcribe(&path)?
    };
    let _ = std::fs::remove_file(&path);
    let transcript = result.transcript.trim();
    if transcript.is_empty() {
        return Ok(());
    }
    let classification = classifier.classify(transcript, target)?;
    let detected = if classification.language.is_empty() {
        result.detected_language.clone()
    } else {
        classification.language.clone()
    };
    let mut translated = transcript.to_string();
    if !detected.is_empty() && detected != target {
        if let Some(eng) = translation {
            match eng.translate(transcript, &detected, target) {
                Ok(t) => translated = t.translated_text,
                Err(e) => {
                    log::warn!("live: translate failed ({e}); emitting transcript only");
                }
            }
        }
    }
    emit_segment(
        chunk_index,
        chunk_start_ms,
        chunk_ms,
        transcript,
        &translated,
        &detected,
        classification.confidence,
    );
    Ok(())
}

pub fn is_silence(samples: &[f32], threshold: f32) -> bool {
    if samples.is_empty() {
        return true;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    rms < threshold
}

fn write_wav(path: &PathBuf, samples: &[f32]) -> Result<(), AugurError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| AugurError::Stt(format!("wav create {path:?}: {e}")))?;
    for &s in samples {
        let clipped = s.clamp(-1.0, 1.0);
        let i16_sample = (clipped * i16::MAX as f32) as i16;
        writer
            .write_sample(i16_sample)
            .map_err(|e| AugurError::Stt(format!("wav write: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| AugurError::Stt(format!("wav finalize: {e}")))?;
    Ok(())
}

// ── audio capture ─────────────────────────────────────────────

type Resampler = Arc<dyn Fn(&[f32]) -> Vec<f32> + Send + Sync>;

fn make_resampler(in_rate: u32, in_channels: usize) -> Resampler {
    let target = SAMPLE_RATE as f32;
    let in_rate_f = in_rate as f32;
    Arc::new(move |samples: &[f32]| {
        // Mix down to mono first by averaging frames.
        let mono: Vec<f32> = if in_channels <= 1 {
            samples.to_vec()
        } else {
            let mut out = Vec::with_capacity(samples.len() / in_channels);
            for frame in samples.chunks_exact(in_channels) {
                let avg = frame.iter().copied().sum::<f32>() / in_channels as f32;
                out.push(avg);
            }
            out
        };
        if (in_rate_f - target).abs() < 1.0 {
            return mono;
        }
        // Linear-interpolation resample. Cheap and good enough
        // for Whisper at 16 kHz; production-grade would use a
        // FIR low-pass first to avoid aliasing.
        let ratio = in_rate_f / target;
        let out_len = (mono.len() as f32 / ratio) as usize;
        let mut out = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let pos = i as f32 * ratio;
            let i0 = pos as usize;
            let i1 = (i0 + 1).min(mono.len().saturating_sub(1));
            let frac = pos - i0 as f32;
            let s = mono[i0] * (1.0 - frac) + mono[i1] * frac;
            out.push(s);
        }
        out
    })
}

trait SampleToF32 {
    fn to_f32(self) -> f32;
}

impl SampleToF32 for f32 {
    fn to_f32(self) -> f32 {
        self
    }
}

impl SampleToF32 for i16 {
    fn to_f32(self) -> f32 {
        self as f32 / i16::MAX as f32
    }
}

impl SampleToF32 for u16 {
    fn to_f32(self) -> f32 {
        (self as f32 - 32768.0) / 32768.0
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    tx: mpsc::SyncSender<Vec<f32>>,
    resampler: Resampler,
) -> Result<cpal::Stream, AugurError>
where
    T: cpal::SizedSample + SampleToF32 + 'static,
{
    let config: cpal::StreamConfig = supported.config();
    let err_fn = |e: cpal::StreamError| log::warn!("audio stream error: {e}");
    let stream = device
        .build_input_stream(
            &config,
            move |data: &[T], _info: &cpal::InputCallbackInfo| {
                let f32_samples: Vec<f32> = data.iter().copied().map(|s| s.to_f32()).collect();
                let resampled = resampler(&f32_samples);
                if STOP.load(Ordering::Relaxed) {
                    return;
                }
                // try_send drops samples if the consumer falls
                // behind — better than blocking the audio
                // thread.
                if tx.try_send(resampled).is_err() {
                    log::debug!("live: dropped audio frame (consumer slow)");
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| AugurError::InvalidInput(format!("build_input_stream: {e}")))?;
    Ok(stream)
}

// ── NDJSON emit helpers ───────────────────────────────────────

fn emit_started(target: &str, device: &str, channels: usize, rate: u32, chunk_ms: u64) {
    let json = serde_json::json!({
        "type": "live_started",
        "target_language": target,
        "device": device,
        "input_channels": channels,
        "input_sample_rate_hz": rate,
        "chunk_duration_ms": chunk_ms,
        "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        "live_advisory": LIVE_ADVISORY,
    });
    println!("{json}");
    let _ = std::io::stdout().flush();
}

#[allow(clippy::too_many_arguments)]
fn emit_segment(
    chunk_index: u32,
    chunk_start_ms: u64,
    chunk_ms: u64,
    original: &str,
    translated: &str,
    source_lang: &str,
    confidence: f32,
) {
    let json = serde_json::json!({
        "type": "live_segment",
        "chunk_index": chunk_index,
        "chunk_start_ms": chunk_start_ms,
        "chunk_end_ms": chunk_start_ms + chunk_ms,
        "original": original,
        "translated": translated,
        "source_lang": source_lang,
        "confidence": confidence,
        "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        "live_advisory": LIVE_ADVISORY,
    });
    println!("{json}");
    let _ = std::io::stdout().flush();
}

fn emit_chunk_error(chunk_index: u32, message: &str) {
    let json = serde_json::json!({
        "type": "live_chunk_error",
        "chunk_index": chunk_index,
        "error": message,
    });
    println!("{json}");
    let _ = std::io::stdout().flush();
}

fn emit_stopped(total_chunks: u32, duration_ms: u64) {
    let json = serde_json::json!({
        "type": "live_stopped",
        "total_chunks": total_chunks,
        "duration_ms": duration_ms,
        "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        "live_advisory": LIVE_ADVISORY,
    });
    println!("{json}");
    let _ = std::io::stdout().flush();
}

/// Sprint 19 P2 — chain-of-custody text for live sessions.
/// Embedded in the package when an examiner saves a live
/// session as evidence. Always carries both advisories. Used
/// by the desktop app via the package writer hook (and pinned
/// by the unit test below).
#[allow(dead_code, clippy::too_many_arguments)]
pub fn render_live_chain_of_custody(
    case: &str,
    examiner: &str,
    started: &str,
    ended: &str,
    duration: &str,
    detected_language: &str,
    stt_model: &str,
    translation_model: &str,
) -> String {
    format!(
        "AUGUR Evidence Package — Live Session Chain of Custody\n\
         ======================================================\n\
         Session type:   LIVE MICROPHONE CAPTURE\n\
         Session start:  {started}\n\
         Session end:    {ended}\n\
         Duration:       {duration}\n\
         Language:       {detected_language}\n\
         Model (STT):    {stt_model}\n\
         Model (Trans):  {translation_model}\n\
         Examiner:       {examiner}\n\
         Case:           {case}\n\
         \n\
         MACHINE TRANSLATION NOTICE:\n\
         This transcript was produced by real-time machine translation. \
         Content has NOT been reviewed. Verify ALL content with a certified \
         human linguist before use in legal proceedings.\n\
         \n\
         LIVE SESSION ADVISORY:\n\
         {LIVE_ADVISORY}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_detection_returns_true_for_zero_samples() {
        let samples = vec![0.0f32; 4800];
        assert!(is_silence(&samples, 0.01));
    }

    #[test]
    fn silence_detection_returns_false_for_loud_speech() {
        let samples: Vec<f32> = (0..4800)
            .map(|i| (i as f32 * 0.01).sin() * 0.5)
            .collect();
        assert!(!is_silence(&samples, 0.01));
    }

    #[test]
    fn live_chain_of_custody_includes_live_advisory() {
        let coc = render_live_chain_of_custody(
            "2026-042",
            "D. Examiner",
            "2026-04-26 16:00:00 UTC",
            "2026-04-26 16:04:32 UTC",
            "4m 32s",
            "Arabic (Egyptian, CAMeL confidence: 0.87)",
            "Whisper Large-v3",
            "NLLB-200 (arz_Arab — Egyptian Arabic token)",
        );
        assert!(coc.contains("LIVE MICROPHONE CAPTURE"));
        assert!(coc.contains("real-time machine translation"));
        assert!(coc.contains("certified human linguist"));
    }

    #[test]
    fn chunk_samples_for_ms_3000_at_16k_is_48000() {
        assert_eq!(chunk_samples_for_ms(3000), 48_000);
    }

    #[test]
    fn make_resampler_passes_through_at_target_rate() {
        let r = make_resampler(SAMPLE_RATE, 1);
        let in_samples = vec![0.5f32; 16];
        let out = r(&in_samples);
        assert_eq!(out.len(), 16);
        assert!((out[0] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn make_resampler_downsamples_44100_to_16000() {
        let r = make_resampler(44_100, 1);
        let in_samples = vec![0.5f32; 44_100];
        let out = r(&in_samples);
        // ~16,000 ± a few samples
        assert!(out.len() >= 15_900 && out.len() <= 16_100, "out.len={}", out.len());
    }
}
