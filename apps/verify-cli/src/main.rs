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

    /// Full pipeline: classify → transcribe → translate.
    /// Sprint 1 stubs out STT and translation; Sprint 2 wires real
    /// inference.
    Translate {
        /// Path to the audio file.
        #[arg(long)]
        input: PathBuf,

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
            target,
            preset,
        } => cmd_translate(&input, &target, preset.into(), backend),
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
    let (transcript_line, detected_lang, confidence) = run_stt(input, preset);
    println_verify(format!(
        "Language detected: {} (confidence: {:.2})",
        detected_lang, confidence
    ));
    println_verify(format!("Transcript: {transcript_line}"));
    Ok(())
}

// ── translate ────────────────────────────────────────────────────

fn cmd_translate(
    input: &std::path::Path,
    target: &str,
    preset: WhisperPreset,
    backend: ClassifierBackend,
) -> Result<(), VerifyError> {
    // Step 1 — STT (Sprint 1: stub). Surface whatever the STT
    // layer gave us so the output format mirrors the final
    // Sprint 2 shape.
    let (transcript_line, detected_lang, stt_confidence) = run_stt(input, preset);

    // Step 2 — classify the transcript. Sprint 1: the stub STT
    // surfaces a sentinel string, so classify runs against that
    // marker. In Sprint 2 this receives the real transcript and
    // produces the canonical language answer.
    let classifier = build_classifier(backend)?;
    let cr = classifier.classify(&transcript_line, target)?;
    let lang = if cr.language.is_empty() {
        detected_lang.clone()
    } else {
        cr.language.clone()
    };
    let confidence = if cr.confidence > 0.0 {
        cr.confidence
    } else {
        stt_confidence
    };

    // Step 3 — translate. Sprint 1: always the sentinel.
    let translation = verify_translate::translate_stub(&transcript_line, &lang, target)?;

    println_verify(format!(
        "Language detected: {} (confidence: {:.2})",
        lang, confidence
    ));
    println_verify(format!("Transcript: {transcript_line}"));
    println_verify(format!("Translation: [{translation}]"));
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

/// Sprint 1: runs STT (stub) and surfaces the structured error
/// message inline rather than aborting the whole command. Return
/// tuple is (transcript-or-stub-marker, detected-lang, confidence).
fn run_stt(input: &std::path::Path, preset: WhisperPreset) -> (String, String, f32) {
    match try_run_stt(input, preset) {
        Ok(r) => (r.transcript, r.detected_language, r.confidence),
        Err(e) => {
            // Stub is expected in Sprint 1; log::debug so an
            // examiner running `RUST_LOG=debug` sees context
            // without flooding the default-level output.
            log::debug!("STT stub returned: {e}");
            ("[STT stub — Sprint 2]".to_string(), String::new(), 0.0)
        }
    }
}

fn try_run_stt(input: &std::path::Path, preset: WhisperPreset) -> Result<SttResult, VerifyError> {
    // Validate the audio file BEFORE touching the network. An
    // examiner who types a wrong path should not accidentally
    // trigger a 75 MB / 142 MB / 2.9 GB Whisper download. This
    // keeps the egress truly "only when needed."
    if !input.exists() {
        return Err(VerifyError::InvalidInput(format!(
            "audio file not found: {input:?}",
        )));
    }
    let mgr = WhisperModelManager::with_xdg_cache()?;
    let model_path = mgr.ensure_whisper_model(preset)?;
    let engine = SttEngine::load(&model_path, preset)?;
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
