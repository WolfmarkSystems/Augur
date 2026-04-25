//! `verify` — VERIFY's standalone CLI.
//!
//! Three subcommands (`classify` / `transcribe` / `translate`),
//! all offline by default. Sprint 1 ships real classification (via
//! fastText or whichlang) plus Sprint-1 stubs for STT and
//! translation; Sprint 2 replaces the stubs.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;
use verify_classifier::{LanguageClassifier, ModelManager as ClassifierModelManager};
use verify_core::pipeline::{
    detect_input_kind, BatchFileResult, BatchResult, BatchSegment, PipelineInput,
};
use verify_core::VerifyError;
use verify_ocr::{iso_to_tesseract, OcrEngine};
use verify_stt::{
    extract_audio_from_video, ModelManager as WhisperModelManager, SttEngine, SttResult,
    SttSegment, WhisperPreset,
};
use verify_translate::{
    Backend as TranslationBackend, TranslationEngine, TranslationResult,
    MACHINE_TRANSLATION_NOTICE,
};

/// Exact `--version` / `-V` output. Kept as a `const` so it's
/// greppable and so it doesn't drift from the `Cargo.toml`
/// version.
const VERSION_STRING: &str = concat!("VERIFY ", env!("CARGO_PKG_VERSION"), " — Wolfmark Systems");

#[derive(Debug, Parser)]
#[command(
    name = "verify",
    // `disable_version_flag = true` plus our own `--version` /
    // `-V` bool below — clap's default version output is
    // `{bin_name} {version}` which would produce
    // `verify VERIFY 0.1.0 — …`. We want the exact sentinel
    // string, so we intercept the flag ourselves.
    disable_version_flag = true,
    about = "VERIFY — forensic translation + transcription.\n\
             All processing is local. No evidence leaves your machine.",
    long_about = "VERIFY surfaces foreign-language content inside digital \
                  evidence — text, audio, video, and images — translating it \
                  into the examiner's working language.\n\
                  \n\
                  All processing is local. No evidence leaves your machine. \
                  The only network access VERIFY performs is a one-time \
                  download of model weights on first run, which can be \
                  pre-placed offline for air-gapped workstations."
)]
struct Cli {
    /// Print version (`VERIFY 0.1.0 — Wolfmark Systems`) and exit.
    #[arg(short = 'V', long = "version", global = false)]
    version: bool,

    #[command(subcommand)]
    command: Option<Command>,

    /// Language-identification backend. fastText = 176 languages
    /// (requires one-time `lid.176.ftz` model download on first
    /// run); whichlang = 16 major languages, pure-Rust, no model
    /// download, no network at all.
    ///
    /// Defaults to fastText with automatic fallback to whichlang
    /// when the fastText model has not been cached and the
    /// current run cannot reach the network.
    #[arg(long, value_enum, default_value_t = ClassifierBackend::Fasttext, global = true)]
    classifier_backend: ClassifierBackend,

    /// Translation backend.
    /// `auto` prefers ctranslate2 (3-5× faster on CPU than the
    /// transformers fallback) when its converted model exists in
    /// the cache; `transformers` and `ct2` force the respective
    /// backend (ct2 triggers a one-time HF→CTranslate2 model
    /// conversion on first use).
    #[arg(long, value_enum, default_value_t = CliTranslationBackend::Auto, global = true)]
    translation_backend: CliTranslationBackend,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliTranslationBackend {
    Auto,
    Transformers,
    Ct2,
}

impl From<CliTranslationBackend> for TranslationBackend {
    fn from(b: CliTranslationBackend) -> Self {
        match b {
            CliTranslationBackend::Auto => TranslationBackend::Auto,
            CliTranslationBackend::Transformers => TranslationBackend::Transformers,
            CliTranslationBackend::Ct2 => TranslationBackend::Ctranslate2,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ClassifierBackend {
    Fasttext,
    Whichlang,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Classify a text string and report its language.
    Classify {
        /// Text to classify (use quotes for multi-word input).
        #[arg(long)]
        text: String,

        /// Target language — ISO 639-1 code (e.g. "en", "ar",
        /// "zh"). Output uses this to decide whether the
        /// classified language is foreign.
        #[arg(long, default_value = "en")]
        target: String,
    },

    /// Transcribe an audio file to text.
    Transcribe {
        /// Path to the audio file (MP3 / M4A / MP4 audio / OGG /
        /// FLAC / WAV). Non-WAV formats require `ffmpeg` on PATH.
        #[arg(long)]
        input: PathBuf,

        /// Whisper model preset: fast / balanced / accurate.
        #[arg(long, value_enum, default_value_t = CliPreset::Balanced)]
        preset: CliPreset,
    },

    /// Full pipeline: classify → (STT/OCR if needed) → translate.
    /// Audio, video, and image inputs are auto-detected from
    /// `--input`'s extension. Every translation output carries a
    /// mandatory machine-translation advisory notice.
    Translate {
        /// Path to an audio, video, or image file. Auto-detected by
        /// extension: `.mp4/.mov/.avi/.mkv/.m4v/.wmv/.webm/.3gp` →
        /// video; `.png/.jpg/.tiff/.bmp/.gif` → image; everything
        /// else → audio.
        #[arg(long, conflicts_with_all = ["text", "image"])]
        input: Option<PathBuf>,

        /// Inline text input (skip STT/OCR entirely).
        #[arg(long, conflicts_with_all = ["input", "image"])]
        text: Option<String>,

        /// Path to an image file (forces image OCR even if the
        /// extension would auto-detect to something else).
        #[arg(long, conflicts_with_all = ["input", "text"])]
        image: Option<PathBuf>,

        /// OCR language hint (ISO 639-1). Defaults to English; for
        /// non-Latin scripts pass `--ocr-lang ar` etc. so Tesseract
        /// loads the right tessdata file.
        #[arg(long, default_value = "en")]
        ocr_lang: String,

        /// Target language — ISO 639-1 code.
        #[arg(long, default_value = "en")]
        target: String,

        /// Whisper model preset.
        #[arg(long, value_enum, default_value_t = CliPreset::Balanced)]
        preset: CliPreset,
    },

    /// Process an entire directory of evidence files. Walks the
    /// folder, classifies each file, and translates the foreign-
    /// language ones. Optionally writes a consolidated JSON report.
    Batch {
        /// Path to the evidence directory.
        #[arg(long)]
        input: PathBuf,

        /// Target language — ISO 639-1 code.
        #[arg(long, default_value = "en")]
        target: String,

        /// Comma-separated list of input kinds to include
        /// (`audio,video,image`). Default: all three.
        #[arg(long, value_delimiter = ',')]
        types: Option<Vec<String>>,

        /// Optional output path for the JSON report.
        #[arg(long)]
        output: Option<PathBuf>,

        /// OCR language hint for image files (ISO 639-1).
        #[arg(long, default_value = "en")]
        ocr_lang: String,

        /// Whisper model preset for audio/video files.
        #[arg(long, value_enum, default_value_t = CliPreset::Balanced)]
        preset: CliPreset,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliPreset {
    Fast,
    Balanced,
    Accurate,
}

impl From<CliPreset> for WhisperPreset {
    fn from(p: CliPreset) -> Self {
        match p {
            CliPreset::Fast => WhisperPreset::Fast,
            CliPreset::Balanced => WhisperPreset::Balanced,
            CliPreset::Accurate => WhisperPreset::Accurate,
        }
    }
}

fn main() -> ExitCode {
    // `env_logger` honors `RUST_LOG`. Default level is `warn` so
    // the model-download egress warnings surface; `RUST_LOG=debug`
    // adds pipeline tracing.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn"),
    )
    .init();

    let cli = Cli::parse();

    // Intercept `--version` / `-V` before dispatching to a
    // subcommand so the examiner sees the exact VERSION_STRING.
    if cli.version {
        println!("{VERSION_STRING}");
        return ExitCode::SUCCESS;
    }

    let Some(command) = cli.command else {
        // No subcommand and no --version — mirror clap's default
        // behaviour of pointing the user at --help. ExitCode 2
        // matches clap's own usage-error convention.
        eprintln!("[VERIFY] no subcommand given. Run `verify --help` for usage.");
        return ExitCode::from(2);
    };

    match run(command, cli.classifier_backend, cli.translation_backend.into()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Always surface errors on stderr — never panic.
            eprintln!("[VERIFY] error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    command: Command,
    backend: ClassifierBackend,
    translation_backend: TranslationBackend,
) -> Result<(), VerifyError> {
    match command {
        Command::Classify { text, target } => cmd_classify(&text, &target, backend),
        Command::Transcribe { input, preset } => cmd_transcribe(&input, preset.into()),
        Command::Translate {
            input,
            text,
            image,
            ocr_lang,
            target,
            preset,
        } => cmd_translate(
            input.as_deref(),
            text.as_deref(),
            image.as_deref(),
            &ocr_lang,
            &target,
            preset.into(),
            backend,
            translation_backend,
        ),
        Command::Batch {
            input,
            target,
            types,
            output,
            ocr_lang,
            preset,
        } => cmd_batch(
            &input,
            &target,
            types.as_deref(),
            output.as_deref(),
            &ocr_lang,
            preset.into(),
            backend,
            translation_backend,
        ),
    }
}

// ── classify ─────────────────────────────────────────────────────

fn cmd_classify(
    text: &str,
    target: &str,
    backend: ClassifierBackend,
) -> Result<(), VerifyError> {
    let classifier = build_classifier(backend)?;
    let result = classifier.classify(text, target)?;
    if result.language.is_empty() {
        println_verify("Language detected: (none) — input empty or whitespace-only");
    } else {
        println_verify(format!(
            "Language detected: {} (confidence: {:.2}) — is_foreign={}",
            result.language, result.confidence, result.is_foreign,
        ));
    }
    Ok(())
}

// ── transcribe ───────────────────────────────────────────────────

fn cmd_transcribe(input: &std::path::Path, preset: WhisperPreset) -> Result<(), VerifyError> {
    let result = try_run_stt(input, preset)?;
    println_verify(format!(
        "Language detected: {} (confidence: {:.2})",
        result.detected_language, result.confidence
    ));
    println_verify("Transcript:");
    for seg in &result.segments {
        let start = format_ms(seg.start_ms);
        let end = format_ms(seg.end_ms);
        println_verify(format!("  [{start} - {end}] {}", seg.text));
    }
    println_verify(format!(
        "Complete. {} segment(s).",
        result.segments.len()
    ));
    Ok(())
}

fn format_ms(ms: u64) -> String {
    let total_s = ms / 1000;
    let m = total_s / 60;
    let s = total_s % 60;
    format!("{m}:{s:02}")
}

// ── translate ────────────────────────────────────────────────────

/// One pipeline step's resolved source data — what feeds the
/// classifier and (if foreign) the translator.
struct ResolvedSource {
    /// Concatenated source text — empty for OCR/STT outputs that
    /// returned nothing.
    text: String,
    /// Language hint from the upstream stage (Whisper detection,
    /// the explicit OCR language, or the classifier on text input).
    /// May be overridden by a fresh classifier pass downstream.
    upstream_lang: String,
    /// Confidence reported by the upstream stage.
    upstream_confidence: f32,
    /// Timestamped STT segments, when the source came from audio
    /// or video. Drives segment-level translation.
    segments: Option<Vec<SttSegment>>,
    /// What kind of input this was — used by the printer to label
    /// the section ("Transcript" vs "Extracted text" vs "Source").
    kind_label: &'static str,
}

#[allow(clippy::too_many_arguments)]
fn cmd_translate(
    input: Option<&std::path::Path>,
    text: Option<&str>,
    image: Option<&std::path::Path>,
    ocr_lang: &str,
    target: &str,
    preset: WhisperPreset,
    backend: ClassifierBackend,
    translation_backend: TranslationBackend,
) -> Result<(), VerifyError> {
    // Resolve the source text through the appropriate engine. The
    // pipelines diverge here:
    //   audio → preprocess → STT → classifier → NLLB
    //   video → ffmpeg-extract → STT → classifier → NLLB
    //   image → OCR → classifier → NLLB
    //   text  → classifier → NLLB
    let resolved = match (input, text, image) {
        (Some(path), None, None) => resolve_path_input(path, preset)?,
        (None, Some(t), None) => {
            let classifier = build_classifier(backend)?;
            let cr = classifier.classify(t, target)?;
            ResolvedSource {
                text: t.to_string(),
                upstream_lang: cr.language,
                upstream_confidence: cr.confidence,
                segments: None,
                kind_label: "text",
            }
        }
        (None, None, Some(img)) => resolve_image_input(img, ocr_lang)?,
        (None, None, None) => {
            return Err(VerifyError::InvalidInput(
                "verify translate requires --input <audio|video> | --text <string> | --image <path>"
                    .to_string(),
            ));
        }
        _ => {
            return Err(VerifyError::InvalidInput(
                "verify translate accepts only one of --input / --text / --image".to_string(),
            ));
        }
    };

    // For audio/video/image inputs, re-classify the extracted text
    // — the STT and OCR language hints can be coarse; fastText
    // (or whichlang) gives the canonical answer once we have text.
    let lang = if !matches!(resolved.kind_label, "text") && !resolved.text.trim().is_empty() {
        let classifier = build_classifier(backend)?;
        let cr = classifier.classify(&resolved.text, target)?;
        if cr.language.is_empty() {
            resolved.upstream_lang.clone()
        } else {
            cr.language
        }
    } else {
        resolved.upstream_lang.clone()
    };

    println_verify(format!(
        "Language detected: {lang} (confidence: {:.2})",
        resolved.upstream_confidence
    ));

    match (&resolved.segments, resolved.kind_label) {
        (Some(segs), _) => {
            println_verify("Transcript:");
            for seg in segs {
                let start = format_ms(seg.start_ms);
                let end = format_ms(seg.end_ms);
                println_verify(format!("  [{start} - {end}] {}", seg.text));
            }
        }
        (None, "image") => {
            println_verify(format!("Extracted text: {}", resolved.text));
        }
        (None, _) => {
            println_verify(format!("Source text: {}", resolved.text));
        }
    }

    if lang == target {
        println_verify(format!(
            "Source already in target language ({target}); no translation needed."
        ));
        return Ok(());
    }

    let mut engine = TranslationEngine::with_xdg_cache()?;
    engine.backend = translation_backend;
    let translation = if let Some(segs) = &resolved.segments {
        let trips: Vec<(u64, u64, String)> = segs
            .iter()
            .map(|s| (s.start_ms, s.end_ms, s.text.clone()))
            .collect();
        println_verify(format!(
            "Translating {} segment(s) {} → {} via NLLB-200 ({:?})...",
            trips.len(),
            lang,
            target,
            engine.backend
        ));
        engine.translate_segments(&trips, &lang, target)?
    } else {
        println_verify(format!(
            "Translating {lang} → {target} via NLLB-200 ({:?})...",
            engine.backend
        ));
        engine.translate(&resolved.text, &lang, target)?
    };

    print_translation(&translation);
    Ok(())
}

fn resolve_path_input(
    path: &std::path::Path,
    preset: WhisperPreset,
) -> Result<ResolvedSource, VerifyError> {
    if !path.exists() {
        return Err(VerifyError::InvalidInput(format!(
            "input file not found: {path:?}"
        )));
    }
    match detect_input_kind(path) {
        PipelineInput::Video(p) => {
            let scratch = std::env::temp_dir().join("verify").join("video-scratch");
            println_verify("Input type: Video — extracting audio track via ffmpeg...");
            let audio = extract_audio_from_video(&p, &scratch)?;
            let stt = try_run_stt(&audio, preset);
            let _ = std::fs::remove_file(&audio);
            let stt = stt?;
            Ok(ResolvedSource {
                text: stt.transcript,
                upstream_lang: stt.detected_language,
                upstream_confidence: stt.confidence,
                segments: Some(stt.segments),
                kind_label: "video",
            })
        }
        PipelineInput::Audio(p) => {
            println_verify("Input type: Audio");
            let stt = try_run_stt(&p, preset)?;
            Ok(ResolvedSource {
                text: stt.transcript,
                upstream_lang: stt.detected_language,
                upstream_confidence: stt.confidence,
                segments: Some(stt.segments),
                kind_label: "audio",
            })
        }
        PipelineInput::Image(p) => {
            // Default OCR language for auto-detected image inputs:
            // English. Examiners who know the language up front
            // should pass `--image` + `--ocr-lang`.
            resolve_image_input(&p, "en")
        }
        PipelineInput::Text(_) => {
            // detect_input_kind never returns Text from a path —
            // it falls back to Audio. This arm exists only to keep
            // the match exhaustive.
            Err(VerifyError::InvalidInput(
                "text input must be passed via --text, not --input".to_string(),
            ))
        }
    }
}

fn resolve_image_input(
    img: &std::path::Path,
    ocr_lang: &str,
) -> Result<ResolvedSource, VerifyError> {
    let tess_lang = iso_to_tesseract(ocr_lang)?;
    let engine = OcrEngine::new(tess_lang)?;
    println_verify(format!("Input type: Image — running OCR ({tess_lang})..."));
    let ocr = engine.extract_text(img)?;
    Ok(ResolvedSource {
        text: ocr.text,
        upstream_lang: ocr_lang.to_string(),
        upstream_confidence: ocr.confidence,
        segments: None,
        kind_label: "image",
    })
}

fn print_translation(translation: &TranslationResult) {
    if let Some(segs) = &translation.segments {
        println_verify("Translated transcript:");
        for seg in segs {
            let start = format_ms(seg.start_ms);
            let end = format_ms(seg.end_ms);
            println_verify(format!("  [{start} - {end}] {}", seg.translated_text));
        }
    } else {
        println_verify(format!("Translation: {}", translation.translated_text));
    }
    print_advisory(translation);
}

fn print_advisory(translation: &TranslationResult) {
    println_verify("");
    println_verify("⚠  MACHINE TRANSLATION NOTICE");
    println_verify(format!("   {MACHINE_TRANSLATION_NOTICE}"));
    println_verify(format!("   Model: {}", translation.model));
    println_verify(format!(
        "   Source language: {} ({})",
        translation.source_language,
        verify_translate::iso_to_nllb(&translation.source_language).unwrap_or("?")
    ));
    println_verify(format!(
        "   Target language: {} ({})",
        translation.target_language,
        verify_translate::iso_to_nllb(&translation.target_language).unwrap_or("?")
    ));
}

// ── batch ────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn cmd_batch(
    input: &std::path::Path,
    target: &str,
    types: Option<&[String]>,
    output: Option<&std::path::Path>,
    ocr_lang: &str,
    preset: WhisperPreset,
    backend: ClassifierBackend,
    translation_backend: TranslationBackend,
) -> Result<(), VerifyError> {
    if !input.exists() || !input.is_dir() {
        return Err(VerifyError::InvalidInput(format!(
            "batch --input must be a directory, got {input:?}"
        )));
    }
    let allowed: Option<Vec<String>> =
        types.map(|t| t.iter().map(|s| s.to_lowercase()).collect());

    let mut files: Vec<PathBuf> = Vec::new();
    walk_files(input, &mut files)?;
    files.sort();

    let mut audio_count = 0u32;
    let mut video_count = 0u32;
    let mut image_count = 0u32;
    let mut other_count = 0u32;
    for f in &files {
        match detect_input_kind(f) {
            PipelineInput::Audio(_) => audio_count += 1,
            PipelineInput::Video(_) => video_count += 1,
            PipelineInput::Image(_) => image_count += 1,
            PipelineInput::Text(_) => other_count += 1,
        }
    }
    println_verify(format!("Batch processing: {input:?}"));
    println_verify(format!(
        "Found {} files ({audio_count} audio, {video_count} video, {image_count} image{}{})",
        files.len(),
        if other_count > 0 { ", " } else { "" },
        if other_count > 0 {
            format!("{other_count} other")
        } else {
            String::new()
        },
    ));

    let mut results: Vec<BatchFileResult> = Vec::with_capacity(files.len());
    let mut processed = 0u32;
    let mut errors = 0u32;
    let mut foreign_count = 0u32;
    let mut translated_count = 0u32;
    let total = files.len() as u32;

    for (idx, file) in files.iter().enumerate() {
        let kind = detect_input_kind(file);
        let kind_label = match &kind {
            PipelineInput::Audio(_) => "audio",
            PipelineInput::Video(_) => "video",
            PipelineInput::Image(_) => "image",
            PipelineInput::Text(_) => "text",
        };
        if let Some(allow) = &allowed {
            if !allow.iter().any(|a| a == kind_label) {
                continue;
            }
        }
        println_verify(format!(
            "[{}/{}] {kind_label}: {file:?}",
            idx + 1,
            total
        ));
        match process_one_file(
            file,
            kind_label,
            target,
            ocr_lang,
            preset,
            backend,
            translation_backend,
        ) {
            Ok(r) => {
                processed += 1;
                if r.is_foreign {
                    foreign_count += 1;
                }
                if r.translated_text.is_some() {
                    translated_count += 1;
                }
                results.push(r);
            }
            Err(e) => {
                errors += 1;
                log::warn!("batch: {file:?}: {e}");
                results.push(BatchFileResult {
                    file_path: file.to_string_lossy().into_owned(),
                    input_type: kind_label.to_string(),
                    detected_language: String::new(),
                    is_foreign: false,
                    source_text: None,
                    translated_text: None,
                    segments: None,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    let report = BatchResult {
        generated_at: utc_now_iso8601(),
        total_files: total,
        processed,
        foreign_language: foreign_count,
        translated: translated_count,
        errors,
        target_language: target.to_string(),
        machine_translation_notice: MACHINE_TRANSLATION_NOTICE.to_string(),
        results,
    };
    report.assert_advisory()?;

    println_verify(format!(
        "Complete. {} processed, {} foreign-language, {} translated, {} errors.",
        processed, foreign_count, translated_count, errors,
    ));

    if let Some(out_path) = output {
        let json = serde_json::to_string_pretty(&report).map_err(|e| {
            VerifyError::Translate(format!("batch JSON serialise: {e}"))
        })?;
        std::fs::write(out_path, json)?;
        println_verify(format!("Report written to {out_path:?}"));
    }

    Ok(())
}

fn walk_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) -> Result<(), VerifyError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_files(&path, out)?;
        } else if ft.is_file() {
            out.push(path);
        }
        // Symlinks intentionally skipped — forensic discipline:
        // we don't follow links out of the evidence directory.
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_one_file(
    file: &std::path::Path,
    kind_label: &str,
    target: &str,
    ocr_lang: &str,
    preset: WhisperPreset,
    backend: ClassifierBackend,
    translation_backend: TranslationBackend,
) -> Result<BatchFileResult, VerifyError> {
    let resolved = match kind_label {
        "audio" | "video" => resolve_path_input(file, preset)?,
        "image" => resolve_image_input(file, ocr_lang)?,
        other => {
            return Err(VerifyError::InvalidInput(format!(
                "batch: unsupported input kind {other:?} for {file:?}"
            )));
        }
    };

    if resolved.text.trim().is_empty() {
        return Ok(BatchFileResult {
            file_path: file.to_string_lossy().into_owned(),
            input_type: kind_label.to_string(),
            detected_language: resolved.upstream_lang,
            is_foreign: false,
            source_text: None,
            translated_text: None,
            segments: None,
            error: None,
        });
    }

    // Re-classify the produced text — same logic as cmd_translate.
    let lang = {
        let classifier = build_classifier(backend)?;
        let cr = classifier.classify(&resolved.text, target)?;
        if cr.language.is_empty() {
            resolved.upstream_lang.clone()
        } else {
            cr.language
        }
    };
    let is_foreign = lang != target;

    if !is_foreign {
        return Ok(BatchFileResult {
            file_path: file.to_string_lossy().into_owned(),
            input_type: kind_label.to_string(),
            detected_language: lang,
            is_foreign: false,
            source_text: Some(resolved.text),
            translated_text: None,
            segments: None,
            error: None,
        });
    }

    let mut engine = TranslationEngine::with_xdg_cache()?;
    engine.backend = translation_backend;
    let translation = if let Some(segs) = &resolved.segments {
        let trips: Vec<(u64, u64, String)> = segs
            .iter()
            .map(|s| (s.start_ms, s.end_ms, s.text.clone()))
            .collect();
        engine.translate_segments(&trips, &lang, target)?
    } else {
        engine.translate(&resolved.text, &lang, target)?
    };

    let segments = translation.segments.as_ref().map(|segs| {
        segs.iter()
            .map(|s| BatchSegment {
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                source_text: s.source_text.clone(),
                translated_text: s.translated_text.clone(),
            })
            .collect::<Vec<_>>()
    });

    Ok(BatchFileResult {
        file_path: file.to_string_lossy().into_owned(),
        input_type: kind_label.to_string(),
        detected_language: lang,
        is_foreign: true,
        source_text: Some(translation.source_text.clone()),
        translated_text: Some(translation.translated_text.clone()),
        segments,
        error: None,
    })
}

/// Minimal ISO-8601 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`) without
/// pulling in chrono. Uses `SystemTime` + a manual gregorian
/// breakdown — accurate to the second, which is all the batch
/// report needs.
fn utc_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = epoch_to_ymdhms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn epoch_to_ymdhms(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let s = (secs % 60) as u32;
    let mins = secs / 60;
    let mi = (mins % 60) as u32;
    let hours = mins / 60;
    let h = (hours % 24) as u32;
    let mut days = (hours / 24) as i64;
    // Civil date algorithm by Howard Hinnant — public domain.
    days += 719_468;
    let era = days.div_euclid(146_097);
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d, h, mi, s)
}

// ── helpers ──────────────────────────────────────────────────────

/// Build the language classifier honouring `--classifier-backend`.
///
/// `fasttext` path: ensures the LID model is cached
/// (one-time network egress on first run), then loads it. If the
/// cache does not exist and the download fails (offline
/// workstation, no curl, etc.), falls back to whichlang with a
/// clear warning — no silent failure.
///
/// `whichlang` path: constructs the pure-Rust classifier with no
/// network access and no filesystem touch. This is the correct
/// choice for air-gapped deployments.
fn build_classifier(backend: ClassifierBackend) -> Result<LanguageClassifier, VerifyError> {
    match backend {
        ClassifierBackend::Whichlang => Ok(LanguageClassifier::new_whichlang()),
        ClassifierBackend::Fasttext => match build_fasttext() {
            Ok(c) => Ok(c),
            Err(e) => {
                log::warn!(
                    "fasttext classifier unavailable ({e}); falling back to whichlang \
                     (pure-Rust, 16 languages, no network)",
                );
                Ok(LanguageClassifier::new_whichlang())
            }
        },
    }
}

fn build_fasttext() -> Result<LanguageClassifier, VerifyError> {
    let mgr = ClassifierModelManager::with_xdg_cache()?;
    let path = mgr.ensure_lid_model()?;
    LanguageClassifier::load_fasttext(&path)
}

fn try_run_stt(input: &std::path::Path, preset: WhisperPreset) -> Result<SttResult, VerifyError> {
    // Validate the audio file BEFORE touching the network. An
    // examiner who types a wrong path should not accidentally
    // trigger a 150 MB / 290 MB / 3 GB Whisper download. This
    // keeps the egress truly "only when needed."
    if !input.exists() {
        return Err(VerifyError::InvalidInput(format!(
            "audio file not found: {input:?}",
        )));
    }
    let mgr = WhisperModelManager::with_xdg_cache()?;
    let paths = mgr.ensure_whisper_model(preset)?;
    let mut engine = SttEngine::load(&paths, preset)?;
    engine.transcribe(input)
}

/// Small helper so every CLI line uses the `[VERIFY]` prefix
/// consistently. Writing to stdout via `println!` here is the one
/// permitted use in the workspace — this is the CLI's own output
/// surface (not a library emitting into a pipeline), and making
/// it a single named function means every CLI line flows through
/// one place a reviewer can audit.
fn println_verify<S: AsRef<str>>(line: S) {
    println!("[VERIFY] {}", line.as_ref());
}
