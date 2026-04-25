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
use verify_core::VerifyError;
use verify_stt::{
    ModelManager as WhisperModelManager, SttEngine, SttResult, WhisperPreset,
};
use verify_ocr::{iso_to_tesseract, OcrEngine};
use verify_translate::{TranslationEngine, MACHINE_TRANSLATION_NOTICE};

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

    /// Full pipeline: classify → transcribe (if audio) → translate.
    /// Sprint 2 wires real Whisper STT and NLLB-200 translation;
    /// every translation output carries a mandatory machine-
    /// translation advisory notice.
    Translate {
        /// Path to an audio file. Mutually exclusive with --text /
        /// --image.
        #[arg(long, conflicts_with_all = ["text", "image"])]
        input: Option<PathBuf>,

        /// Inline text input (skip STT/OCR entirely).
        #[arg(long, conflicts_with_all = ["input", "image"])]
        text: Option<String>,

        /// Path to an image file (PNG/JPG/TIFF/...). Routes through
        /// Tesseract OCR before classification + translation.
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

    match run(command, cli.classifier_backend) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Always surface errors on stderr — never panic.
            eprintln!("[VERIFY] error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command, backend: ClassifierBackend) -> Result<(), VerifyError> {
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

#[allow(clippy::too_many_arguments)]
fn cmd_translate(
    input: Option<&std::path::Path>,
    text: Option<&str>,
    image: Option<&std::path::Path>,
    ocr_lang: &str,
    target: &str,
    preset: WhisperPreset,
    backend: ClassifierBackend,
) -> Result<(), VerifyError> {
    // Resolve source text from one of three input modes. The
    // pipelines diverge here:
    //   audio → preprocess → STT → classifier → NLLB
    //   image → OCR        → classifier → NLLB
    //   text  → classifier → NLLB
    let (source_text, source_lang, source_confidence, segments) =
        match (input, text, image) {
            (Some(path), None, None) => {
                let stt = try_run_stt(path, preset)?;
                (
                    stt.transcript.clone(),
                    stt.detected_language.clone(),
                    stt.confidence,
                    Some(stt.segments),
                )
            }
            (None, Some(t), None) => {
                let classifier = build_classifier(backend)?;
                let cr = classifier.classify(t, target)?;
                (t.to_string(), cr.language.clone(), cr.confidence, None)
            }
            (None, None, Some(img)) => {
                let tess_lang = iso_to_tesseract(ocr_lang)?;
                let engine = OcrEngine::new(tess_lang)?;
                println_verify(format!("Running OCR ({tess_lang}) on {img:?}..."));
                let ocr = engine.extract_text(img)?;
                println_verify(format!("Extracted text: {}", ocr.text));
                (ocr.text, ocr_lang.to_string(), ocr.confidence, None)
            }
            (None, None, None) => {
                return Err(VerifyError::InvalidInput(
                    "verify translate requires --input <audio> | --text <string> | --image <path>"
                        .to_string(),
                ));
            }
            _ => {
                return Err(VerifyError::InvalidInput(
                    "verify translate accepts only one of --input / --text / --image".to_string(),
                ));
            }
        };

    // For audio + image inputs, also re-classify the extracted
    // text — STT and OCR language hints can be coarse; the text
    // classifier (fastText / whichlang) gives the canonical answer.
    let lang = if (input.is_some() || image.is_some()) && !source_text.trim().is_empty() {
        let classifier = build_classifier(backend)?;
        let cr = classifier.classify(&source_text, target)?;
        if cr.language.is_empty() {
            source_lang
        } else {
            cr.language
        }
    } else {
        source_lang
    };

    println_verify(format!(
        "Language detected: {lang} (confidence: {:.2})",
        source_confidence
    ));

    if let Some(segs) = &segments {
        println_verify("Transcript:");
        for seg in segs {
            let start = format_ms(seg.start_ms);
            let end = format_ms(seg.end_ms);
            println_verify(format!("  [{start} - {end}] {}", seg.text));
        }
    } else {
        println_verify(format!("Source text: {source_text}"));
    }

    if lang == target {
        println_verify(format!(
            "Source already in target language ({target}); no translation needed."
        ));
        return Ok(());
    }

    let engine = TranslationEngine::with_xdg_cache()?;
    let translation = engine.translate(&source_text, &lang, target)?;

    println_verify(format!(
        "Translating {} → {} via {}...",
        lang, target, translation.model
    ));
    println_verify(format!("Translation: {}", translation.translated_text));

    // Mandatory advisory — every translation output is labeled.
    println_verify("");
    println_verify("⚠  MACHINE TRANSLATION NOTICE");
    println_verify(format!("   {MACHINE_TRANSLATION_NOTICE}"));
    println_verify(format!("   Model: {}", translation.model));
    println_verify(format!(
        "   Source language: {} ({})",
        translation.source_language,
        verify_translate::iso_to_nllb(&translation.source_language)
            .unwrap_or("?")
    ));
    println_verify(format!(
        "   Target language: {} ({})",
        translation.target_language,
        verify_translate::iso_to_nllb(&translation.target_language)
            .unwrap_or("?")
    ));
    Ok(())
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
