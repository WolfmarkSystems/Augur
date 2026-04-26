//! `verify` — VERIFY's standalone CLI.
//!
//! Three subcommands (`classify` / `transcribe` / `translate`),
//! all offline by default. Sprint 1 ships real classification (via
//! fastText or whichlang) plus Sprint-1 stubs for STT and
//! translation; Sprint 2 replaces the stubs.

mod package;
mod selftest;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;
use verify_classifier::{LanguageClassifier, ModelManager as ClassifierModelManager};
use verify_core::geoip::{GeoIpEngine, GeoIpResult, GEOIP_DB_INSTRUCTIONS};
use verify_core::report::{render_batch_html, ReportConfig};
use verify_core::timestamps::{
    convert as ts_convert, detect_and_convert as ts_detect, parse_input_file as ts_parse,
    TimestampFormat, TimestampResult,
};
use verify_core::pipeline::{
    detect_input_kind, render_batch_csv, BatchFileResult, BatchResult, BatchSegment,
    PipelineInput,
};
use verify_core::VerifyError;
use verify_ocr::{extract_pdf_text, iso_to_tesseract, OcrEngine};
use verify_stt::{
    extract_audio_from_video, merge_stt_with_diarization, DiarizationEngine, DiarizationSegment,
    EnrichedSegment, HfTokenManager, ModelManager as WhisperModelManager, SttEngine, SttResult,
    SttSegment, TranscribeOptions, WhisperPreset,
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

    /// Language-identification backend.
    ///
    /// `whichlang` (default) — 16 major languages, pure-Rust,
    /// embedded weights, no network, no model download.
    ///
    /// `fasttext` (production-ready as of Sprint 5) — 176
    /// languages via `fasttext-pure-rs` reading Meta's
    /// `lid.176.ftz`. Requires a one-time ~900 KB model
    /// download on first run. Recommended when broader language
    /// coverage matters (forensic targets in Persian, Urdu, etc.
    /// that whichlang does not cover). Pashto confuses with
    /// Persian at the model level — corroborate with metadata
    /// when the case hinges on the distinction.
    #[arg(long, value_enum, default_value_t = ClassifierBackend::Whichlang, global = true)]
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

        /// Initial decoding temperature. `0.0` is greedy (default).
        /// Higher values introduce sampling and are used by the
        /// retry loop on hard audio.
        #[arg(long, default_value_t = 0.0)]
        temperature: f32,

        /// Maximum number of *additional* temperature retries on
        /// segments that look like hallucinations (per OpenAI's
        /// reference). Each retry bumps temperature by 0.2.
        #[arg(long, default_value_t = 5)]
        max_retries: u8,
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

        /// Enable speaker diarization (who said what). Requires
        /// `pip3 install --user pyannote.audio` and a Hugging Face
        /// token configured via `verify setup --hf-token`. Audio
        /// and video inputs only — text/image/PDF are silently
        /// unaffected. Default: off.
        #[arg(long, default_value_t = false)]
        diarize: bool,
    },

    /// Build an evidence-export ZIP from a previously-run batch
    /// report (or an evidence directory plus a fresh classify
    /// pass). The package contains MANIFEST.json (with SHA-256
    /// hashes of every original), CHAIN_OF_CUSTODY.txt, the
    /// rendered HTML/JSON reports, and per-file translation
    /// `.txt` artifacts. By default, source files are NOT copied
    /// into the package — pass `--include-originals` to include
    /// them.
    Package {
        /// Evidence directory to package. The CLI runs a batch
        /// pass internally to gather classifications and
        /// translations, then assembles the ZIP.
        #[arg(long)]
        input: PathBuf,

        /// Output ZIP path. Defaults to
        /// `verify-package-<YYYYMMDD>.zip` in the current dir.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Target language for translations. Same semantics as
        /// `verify batch --target`.
        #[arg(long, default_value = "en")]
        target: String,

        /// Optional `verify config` TOML — supplies agency /
        /// case / examiner metadata for the manifest and chain
        /// of custody.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Whisper preset for any audio/video files in the
        /// evidence directory.
        #[arg(long, value_enum, default_value_t = CliPreset::Balanced)]
        preset: CliPreset,

        /// OCR language hint for image files.
        #[arg(long, default_value = "en")]
        ocr_lang: String,

        /// Include the original source files inside the ZIP. Off
        /// by default — large evidence directories make zipping
        /// originals impractical, and the manifest's SHA-256
        /// hashes already provide integrity verification against
        /// the originals at their canonical location.
        #[arg(long, default_value_t = false)]
        include_originals: bool,
    },

    /// Convert a forensic timestamp (Unix / Apple / Windows /
    /// WebKit / HFS+) into a human-readable UTC time. With no
    /// `--format`, all plausible interpretations are listed.
    Timestamp {
        /// The integer timestamp value. Accepts negative values.
        /// Mutually exclusive with `--input`.
        #[arg(conflicts_with = "input")]
        value: Option<i64>,

        /// File of "<value> [label]" lines (one per line). `#`
        /// prefix or blank line = ignored.
        #[arg(long, conflicts_with = "value")]
        input: Option<PathBuf>,

        /// Force a specific format. Default behaviour is to list
        /// every plausible interpretation.
        #[arg(long)]
        format: Option<String>,
    },

    /// Geolocate one or more IP addresses against a MaxMind
    /// GeoLite2-City database. The database is NOT auto-
    /// downloaded (MaxMind license); pass `--setup` for the
    /// download instructions or set `VERIFY_GEOIP_PATH`.
    Geoip {
        /// IP address to look up. Mutually exclusive with
        /// `--input` and `--setup`.
        #[arg(conflicts_with_all = ["input", "setup"])]
        ip: Option<String>,

        /// File with one IP address per line (blank lines and
        /// `#`-prefixed comments ignored).
        #[arg(long, conflicts_with_all = ["ip", "setup"])]
        input: Option<PathBuf>,

        /// Print MaxMind GeoLite2 setup instructions and exit.
        #[arg(long, conflicts_with_all = ["ip", "input"])]
        setup: bool,
    },

    /// Pre-deployment readiness check — runs a battery of
    /// classification, model-cache, and tooling checks and reports
    /// whether the installation is ready for casework. Default
    /// form is fully offline; `--full` opts into running real
    /// translation inference end-to-end.
    SelfTest {
        /// Trigger the inference checks (Whisper / NLLB end-to-end).
        /// Requires Python + transformers installed for the
        /// translation arm; without them that check downgrades to
        /// `Skip`, never `Fail`.
        #[arg(long, default_value_t = false)]
        full: bool,
    },

    /// One-time setup commands. Currently writes a Hugging Face
    /// access token used by the optional speaker-diarization
    /// feature (`--diarize`); the token lives at
    /// `~/.cache/verify/hf_token` (chmod 0600 on Unix).
    Setup {
        /// Hugging Face access token (`hf_…`). Get one at
        /// https://huggingface.co/settings/tokens. Accept the
        /// pyannote model terms at
        /// https://huggingface.co/pyannote/speaker-diarization-3.1
        /// before first use.
        #[arg(long)]
        hf_token: String,
    },

    /// Process an entire directory of evidence files. Walks the
    /// folder, classifies each file, and translates the foreign-
    /// language ones. Optionally writes a consolidated JSON report.
    /// View / write the report config used by `verify batch`.
    /// The config carries agency name, case number, examiner
    /// signature, and classification marking.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    Batch {
        /// Path to the evidence directory.
        #[arg(long)]
        input: PathBuf,

        /// Target language — ISO 639-1 code.
        #[arg(long, default_value = "en")]
        target: String,

        /// Comma-separated list of input kinds to include
        /// (`audio,video,image,pdf`). Default: all four.
        #[arg(long, value_delimiter = ',')]
        types: Option<Vec<String>>,

        /// Optional output path for the report. Format is
        /// inferred from extension: `.csv` → CSV, anything else
        /// → JSON. The CSV form has one row per file with the
        /// columns enumerated in `BATCH_CSV_HEADER`.
        #[arg(long)]
        output: Option<PathBuf>,

        /// OCR language hint for image files (ISO 639-1).
        #[arg(long, default_value = "en")]
        ocr_lang: String,

        /// Whisper model preset for audio/video files.
        #[arg(long, value_enum, default_value_t = CliPreset::Balanced)]
        preset: CliPreset,

        /// Optional `verify config` TOML — supplies agency
        /// name / case number / examiner signature / classification
        /// marking for the rendered report. Default location:
        /// `~/.verify_report.toml`.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Output format. `auto` infers from `--output` extension
        /// (`.csv` → CSV, `.html`/`.htm` → HTML, else JSON).
        #[arg(long, value_enum, default_value_t = CliReportFormat::Auto)]
        format: CliReportFormat,

        /// Sprint 8 P2 — translate every detected foreign-language
        /// file (any non-`--target` language) rather than skipping
        /// non-foreign files. Default behavior is unchanged when
        /// the flag is absent (only files whose detected language
        /// differs from `--target` get translated).
        #[arg(long, default_value_t = false)]
        all_foreign: bool,

        /// Sprint 9 P2 — number of worker threads for parallel
        /// file processing. `0` (the default) means
        /// `min(num_cpus, 8)`. Use `1` to force the previous
        /// sequential behaviour. Cap of 8 keeps STT model loads
        /// from blowing memory on large evidence runs.
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum ConfigAction {
    /// Write a default TOML config to `--output` (or
    /// `~/.verify_report.toml` if not specified). Refuses to
    /// overwrite an existing file unless `--force` is set.
    Init {
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Print the current config to stdout (TOML form).
    Show {
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Set a single config field. Recognized keys:
    /// `agency_name` / `case_number` / `examiner_name` /
    /// `examiner_badge` / `classification` / `report_title`.
    Set {
        key: String,
        value: String,
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliReportFormat {
    Auto,
    Json,
    Csv,
    Html,
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
        Command::Transcribe {
            input,
            preset,
            temperature,
            max_retries,
        } => cmd_transcribe(&input, preset.into(), temperature, max_retries),
        Command::Translate {
            input,
            text,
            image,
            ocr_lang,
            target,
            preset,
            diarize,
        } => cmd_translate(
            input.as_deref(),
            text.as_deref(),
            image.as_deref(),
            &ocr_lang,
            &target,
            preset.into(),
            backend,
            translation_backend,
            diarize,
        ),
        Command::Setup { hf_token } => cmd_setup(&hf_token),
        Command::SelfTest { full } => cmd_self_test(full),
        Command::Package {
            input,
            output,
            target,
            config,
            preset,
            ocr_lang,
            include_originals,
        } => cmd_package(
            &input,
            output.as_deref(),
            &target,
            config.as_deref(),
            preset.into(),
            &ocr_lang,
            include_originals,
            backend,
            translation_backend,
        ),
        Command::Geoip { ip, input, setup } => cmd_geoip(ip.as_deref(), input.as_deref(), setup),
        Command::Timestamp {
            value,
            input,
            format,
        } => cmd_timestamp(value, input.as_deref(), format.as_deref()),
        Command::Batch {
            input,
            target,
            types,
            output,
            ocr_lang,
            preset,
            config,
            format,
            all_foreign,
            threads,
        } => cmd_batch(
            &input,
            &target,
            types.as_deref(),
            output.as_deref(),
            &ocr_lang,
            preset.into(),
            backend,
            translation_backend,
            config.as_deref(),
            format,
            all_foreign,
            threads,
        ),
        Command::Config { action } => cmd_config(action),
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
        print_classification(&result);
    }
    Ok(())
}

/// Render a `ClassificationResult` to the CLI in the multi-line
/// format spec'd by VERIFY_SPRINT_6 P2c — language, confidence
/// tier + raw score, input word count, and the advisory line
/// when the tier is anything other than `High`.
fn print_classification(r: &verify_classifier::ClassificationResult) {
    println_verify(format!(
        "Language detected: {} (target: {}) — is_foreign={}",
        r.language, r.target_language, r.is_foreign,
    ));
    println_verify(format!(
        "         Confidence: {} ({:.2})",
        r.confidence_tier.as_str(),
        r.confidence,
    ));
    println_verify(format!(
        "         Input: {} word(s)",
        r.input_word_count
    ));
    if let Some(adv) = &r.advisory {
        println_verify(format!("         ⚠ {adv}"));
    }
    if let Some(note) = &r.disambiguation_note {
        println_verify(format!("         ⚠ {note}"));
    }
}

// ── transcribe ───────────────────────────────────────────────────

fn cmd_transcribe(
    input: &std::path::Path,
    preset: WhisperPreset,
    temperature: f32,
    max_retries: u8,
) -> Result<(), VerifyError> {
    let options = TranscribeOptions {
        preset,
        temperature,
        max_temperature_retries: max_retries,
        ..TranscribeOptions::default()
    };
    let result = try_run_stt_with(input, &options)?;
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
    /// Sprint 8 P3 — the path pyannote should read for speaker
    /// diarization. `Some(input)` for audio, `Some(scratch_wav)`
    /// for video (scratch survives until diarization completes —
    /// see cleanup in `cmd_translate`). `None` for text / image /
    /// PDF inputs that have no audio track.
    audio_path: Option<PathBuf>,
    /// `true` when `audio_path` is a temp file the CLI owns and
    /// must remove after diarization runs. `false` for audio
    /// inputs (the original file belongs to the examiner).
    audio_path_is_scratch: bool,
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
    diarize: bool,
) -> Result<(), VerifyError> {
    // Resolve the source text through the appropriate engine. The
    // pipelines diverge here:
    //   audio → preprocess → STT → classifier → NLLB
    //   video → ffmpeg-extract → STT → classifier → NLLB
    //   image → OCR → classifier → NLLB
    //   text  → classifier → NLLB
    let resolved_path: Option<std::path::PathBuf> = input.map(|p| p.to_path_buf());
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
                audio_path: None,
                audio_path_is_scratch: false,
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

    // Optional diarization step. Only applies when the source had
    // STT segments (audio / video). Text / image / PDF inputs
    // ignore the flag entirely — there's no audio to attribute.
    // Sprint 8 P3: prefer the scratch WAV the video resolver
    // extracted (pyannote reads audio, not video containers).
    let diarization_segments: Option<Vec<DiarizationSegment>> = if diarize
        && resolved.segments.is_some()
    {
        let audio_for_pyannote = resolved.audio_path.as_deref().or(resolved_path.as_deref());
        let path = audio_for_pyannote.ok_or_else(|| {
            VerifyError::InvalidInput(
                "--diarize requires --input <audio|video>".to_string(),
            )
        })?;
        Some(run_diarization(path)?)
    } else {
        if diarize {
            println_verify(
                "--diarize ignored: input does not produce timestamped audio segments.",
            );
        }
        None
    };

    // Clean up the video scratch WAV. Diarization is already done
    // (or was skipped); the scratch is no longer needed.
    if resolved.audio_path_is_scratch {
        if let Some(p) = &resolved.audio_path {
            let _ = std::fs::remove_file(p);
        }
    }

    match (&resolved.segments, &diarization_segments, resolved.kind_label) {
        (Some(stt), Some(diar), _) => {
            let merged = merge_stt_with_diarization(stt, diar);
            println_verify(format!(
                "Transcript ({} speaker(s) detected):",
                count_speakers(diar)
            ));
            for seg in &merged {
                let start = format_ms(seg.start_ms);
                let end = format_ms(seg.end_ms);
                println_verify(format!(
                    "  [{start} - {end}] {}: {}",
                    seg.speaker_id, seg.text
                ));
            }
        }
        (Some(stt), None, _) => {
            println_verify("Transcript:");
            for seg in stt {
                let start = format_ms(seg.start_ms);
                let end = format_ms(seg.end_ms);
                println_verify(format!("  [{start} - {end}] {}", seg.text));
            }
        }
        (None, _, "image") => {
            println_verify(format!("Extracted text: {}", resolved.text));
        }
        (None, _, _) => {
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

    if let (Some(diar), Some(translated)) = (&diarization_segments, &translation.segments) {
        let enriched = build_enriched(translated, diar);
        println_verify("Translated transcript:");
        for seg in &enriched {
            let start = format_ms(seg.start_ms);
            let end = format_ms(seg.end_ms);
            let text = seg.translated_text.as_deref().unwrap_or("");
            println_verify(format!(
                "  [{start} - {end}] {}: {text}",
                seg.speaker_id
            ));
        }
        print_advisory(&translation);
        // Sprint 8 P3 — speaker advisory always fires alongside
        // (NOT instead of) the MT advisory whenever the
        // transcript carries diarization-derived speaker labels.
        print_speaker_advisory();
    } else {
        print_translation(&translation);
    }
    Ok(())
}

fn print_speaker_advisory() {
    println_verify("");
    println_verify("⚠  SPEAKER DIARIZATION NOTICE");
    for line in verify_stt::SPEAKER_DIARIZATION_ADVISORY
        .split_terminator(". ")
        .filter(|s| !s.is_empty())
    {
        println_verify(format!("   {}", line.trim()));
    }
}

fn run_diarization(audio: &std::path::Path) -> Result<Vec<DiarizationSegment>, VerifyError> {
    let engine = DiarizationEngine::with_xdg_cache()?;
    if !engine.is_available() {
        return Err(VerifyError::Stt(
            "diarization unavailable: python3 missing or HF token not configured. \
             Run `verify setup --hf-token <hf_…>` and \
             `pip3 install --user pyannote.audio`."
                .to_string(),
        ));
    }
    println_verify("Running pyannote speaker diarization...");
    engine.diarize(audio)
}

fn count_speakers(segments: &[DiarizationSegment]) -> usize {
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for s in segments {
        seen.insert(&s.speaker_id);
    }
    seen.len()
}

fn build_enriched(
    translated: &[verify_translate::TranslatedSegment],
    diar: &[DiarizationSegment],
) -> Vec<EnrichedSegment> {
    translated
        .iter()
        .map(|t| {
            let speaker = best_speaker(t.start_ms, t.end_ms, diar);
            EnrichedSegment {
                start_ms: t.start_ms,
                end_ms: t.end_ms,
                text: t.source_text.clone(),
                speaker_id: speaker,
                translated_text: Some(t.translated_text.clone()),
            }
        })
        .collect()
}

fn best_speaker(start_ms: u64, end_ms: u64, diar: &[DiarizationSegment]) -> String {
    let mut best: Option<(u64, &str)> = None;
    for d in diar {
        let lo = start_ms.max(d.start_ms);
        let hi = end_ms.min(d.end_ms);
        if hi <= lo {
            continue;
        }
        let overlap = hi - lo;
        match &best {
            Some((cur, _)) if *cur >= overlap => {}
            _ => best = Some((overlap, d.speaker_id.as_str())),
        }
    }
    best.map(|(_, s)| s.to_string())
        .unwrap_or_else(|| "UNKNOWN".to_string())
}

#[allow(clippy::too_many_arguments)]
fn cmd_package(
    input: &std::path::Path,
    output: Option<&std::path::Path>,
    target: &str,
    config_path: Option<&std::path::Path>,
    preset: WhisperPreset,
    ocr_lang: &str,
    include_originals: bool,
    backend: ClassifierBackend,
    translation_backend: TranslationBackend,
) -> Result<(), VerifyError> {
    use std::sync::atomic::{AtomicU32, Ordering};
    use rayon::prelude::*;
    use verify_core::pipeline::BatchSummary;

    if !input.exists() || !input.is_dir() {
        return Err(VerifyError::InvalidInput(format!(
            "verify package --input must be a directory, got {input:?}"
        )));
    }

    let config = load_report_config(config_path)?;
    let zip_path: PathBuf = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let stamp = utc_now_iso8601()
                .chars()
                .take(10)
                .collect::<String>()
                .replace('-', "");
            std::env::current_dir()
                .map_err(VerifyError::Io)?
                .join(format!("verify-package-{stamp}.zip"))
        }
    };

    println_verify(format!(
        "verify package: walking {input:?} (preset={preset:?}, target={target})"
    ));

    let mut files: Vec<PathBuf> = Vec::new();
    walk_files(input, &mut files)?;
    files.sort();
    let mut eligible: Vec<(PathBuf, &'static str)> = Vec::with_capacity(files.len());
    for f in &files {
        let kind = detect_input_kind(f);
        let label: &'static str = match &kind {
            PipelineInput::Audio(_) => "audio",
            PipelineInput::Video(_) => "video",
            PipelineInput::Image(_) => "image",
            PipelineInput::Pdf(_) => "pdf",
            PipelineInput::Text(_) => "text",
        };
        eligible.push((f.clone(), label));
    }

    let processed_atomic = AtomicU32::new(0);
    let errors_atomic = AtomicU32::new(0);
    let foreign_atomic = AtomicU32::new(0);
    let translated_atomic = AtomicU32::new(0);
    let started = std::time::Instant::now();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(resolve_thread_count(0))
        .thread_name(|i| format!("verify-package-{i}"))
        .build()
        .map_err(|e| VerifyError::InvalidInput(format!("rayon pool: {e}")))?;
    let mut results: Vec<BatchFileResult> = pool.install(|| {
        eligible
            .par_iter()
            .map(|(file, kind_label)| {
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
                        processed_atomic.fetch_add(1, Ordering::Relaxed);
                        if r.is_foreign {
                            foreign_atomic.fetch_add(1, Ordering::Relaxed);
                        }
                        if r.translated_text.is_some() {
                            translated_atomic.fetch_add(1, Ordering::Relaxed);
                        }
                        r
                    }
                    Err(e) => {
                        errors_atomic.fetch_add(1, Ordering::Relaxed);
                        log::warn!("package: {file:?}: {e}");
                        BatchFileResult {
                            file_path: file.to_string_lossy().into_owned(),
                            input_type: kind_label.to_string(),
                            detected_language: String::new(),
                            is_foreign: false,
                            confidence_tier: String::new(),
                            confidence_advisory: None,
                            source_text: None,
                            translated_text: None,
                            segments: None,
                            error: Some(e.to_string()),
                        }
                    }
                }
            })
            .collect()
    });
    results.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    let processed = processed_atomic.load(Ordering::Relaxed);
    let errors = errors_atomic.load(Ordering::Relaxed);
    let foreign_count = foreign_atomic.load(Ordering::Relaxed);
    let translated_count = translated_atomic.load(Ordering::Relaxed);

    let elapsed = started.elapsed().as_secs_f64();
    let mut report = BatchResult {
        generated_at: utc_now_iso8601(),
        total_files: files.len() as u32,
        processed,
        foreign_language: foreign_count,
        translated: translated_count,
        errors,
        target_language: target.to_string(),
        machine_translation_notice: MACHINE_TRANSLATION_NOTICE.to_string(),
        results,
        summary: None,
        language_groups: Vec::new(),
        dominant_language: None,
    };
    let summary: BatchSummary = report.build_summary(elapsed, MACHINE_TRANSLATION_NOTICE);
    report.summary = Some(summary);
    report.build_language_groups();
    report.assert_advisory()?;

    let manifest = package::write_package(
        &zip_path,
        &report,
        &config,
        input,
        include_originals,
    )?;
    println_verify(format!(
        "Package written to {zip_path:?} ({} files, {} translated, {} errors)",
        manifest.file_count, manifest.translated_count, errors,
    ));
    if !include_originals {
        println_verify(
            "(originals NOT included — pass --include-originals to bundle source files)",
        );
    }
    Ok(())
}

fn cmd_self_test(full: bool) -> Result<(), VerifyError> {
    println_verify("Running self-test...");
    println_verify("");
    let result = selftest::run_self_test(full)?;
    for c in &result.checks {
        let line = format!(
            "{} [{}] {}: {}",
            c.status.glyph(),
            c.status.label(),
            c.name,
            c.message,
        );
        println_verify(line);
    }
    println_verify("");
    let summary = format!(
        "Self-test {label} ({} passed, {} failed, {} skipped, {} warnings)",
        result.passed,
        result.failed,
        result.skipped,
        result.warnings,
        label = if result.ready_for_casework {
            "PASSED"
        } else {
            "FAILED"
        }
    );
    println_verify(summary);
    if result.ready_for_casework {
        println_verify("This installation is ready for casework.");
        Ok(())
    } else {
        Err(VerifyError::InvalidInput(
            "self-test reported one or more failures — see check list above".to_string(),
        ))
    }
}

fn cmd_timestamp(
    value: Option<i64>,
    input: Option<&std::path::Path>,
    format: Option<&str>,
) -> Result<(), VerifyError> {
    let chosen = match format {
        Some(f) => Some(TimestampFormat::from_str(f).ok_or_else(|| {
            VerifyError::InvalidInput(format!(
                "unknown timestamp format {f:?}; valid: unix-seconds, \
                 unix-ms, unix-us, unix-ns, apple-coredata, apple-ns, \
                 windows-filetime, webkit, hfs-plus, cocoa-date"
            ))
        })?),
        None => None,
    };
    if let Some(v) = value {
        run_timestamp(v, None, chosen);
    } else if let Some(path) = input {
        let body = std::fs::read_to_string(path)?;
        let entries = ts_parse(&body)?;
        for (idx, (val, label)) in entries.iter().enumerate() {
            if idx > 0 {
                println_verify("");
            }
            run_timestamp(*val, label.as_deref(), chosen);
        }
    } else {
        return Err(VerifyError::InvalidInput(
            "verify timestamp requires <value> or --input <file>".to_string(),
        ));
    }
    Ok(())
}

fn run_timestamp(value: i64, label: Option<&str>, format: Option<TimestampFormat>) {
    let header = match label {
        Some(l) => format!("Timestamp: {value} ({l})"),
        None => format!("Timestamp: {value}"),
    };
    println_verify(header);
    let results: Vec<TimestampResult> = match format {
        Some(fmt) => vec![ts_convert(value, fmt)],
        None => ts_detect(value),
    };
    if results.is_empty() {
        println_verify("  (no plausible interpretation in supported range)");
        return;
    }
    println_verify(format!(
        "  {:<22} {:<10} {}",
        "Format", "Confidence", "UTC"
    ));
    println_verify(format!(
        "  {:<22} {:<10} {}",
        "----------------------", "----------", "------------------------"
    ));
    for r in &results {
        let utc = if r.utc.is_empty() {
            "(out of range)".to_string()
        } else {
            r.utc.clone()
        };
        println_verify(format!(
            "  {:<22} {:<10} {utc}",
            r.format.as_str(),
            r.confidence,
        ));
    }
}

fn cmd_geoip(
    ip: Option<&str>,
    input: Option<&std::path::Path>,
    setup: bool,
) -> Result<(), VerifyError> {
    if setup {
        println_verify("MaxMind GeoLite2 setup");
        for line in GEOIP_DB_INSTRUCTIONS.split('\n') {
            println_verify(format!("  {line}"));
        }
        if let Some(p) = verify_core::geoip::configured_db_path() {
            println_verify(format!("Currently configured: {p:?}"));
        } else {
            println_verify("Currently configured: (none — install per the above)");
        }
        return Ok(());
    }
    let engine = GeoIpEngine::with_xdg_cache()?;
    if let Some(addr) = ip {
        let r = engine.lookup(addr)?;
        print_geoip(&r);
    } else if let Some(path) = input {
        let body = std::fs::read_to_string(path)?;
        for line in body.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            match engine.lookup(trimmed) {
                Ok(r) => print_geoip(&r),
                Err(e) => println_verify(format!("{trimmed}: {e}")),
            }
            println_verify("");
        }
    } else {
        return Err(VerifyError::InvalidInput(
            "verify geoip requires <IP> or --input <file> or --setup".to_string(),
        ));
    }
    Ok(())
}

fn print_geoip(r: &GeoIpResult) {
    println_verify(format!("GeoIP: {}", r.ip));
    if r.is_private {
        println_verify("  Private: Yes (RFC 1918 / loopback / link-local — no public geolocation)");
        return;
    }
    let country = match (&r.country_code, &r.country_name) {
        (Some(code), Some(name)) => format!("{name} ({code})"),
        (Some(code), None) => code.clone(),
        (None, Some(name)) => name.clone(),
        (None, None) => "(unknown)".into(),
    };
    println_verify(format!("  Country: {country}"));
    if let Some(c) = &r.city {
        println_verify(format!("  City: {c}"));
    }
    if let (Some(lat), Some(lon)) = (r.latitude, r.longitude) {
        println_verify(format!("  Coords: {lat:.4}, {lon:.4}"));
    }
    println_verify("  Private: No");
}

fn cmd_setup(token: &str) -> Result<(), VerifyError> {
    let mgr = HfTokenManager::with_xdg_cache()?;
    mgr.save(token)?;
    println_verify(format!(
        "Hugging Face token written to {:?} (chmod 0600 on Unix).",
        mgr.token_path
    ));
    println_verify(
        "Next: install pyannote (`pip3 install --user pyannote.audio`) and \
         accept the model terms at \
         https://huggingface.co/pyannote/speaker-diarization-3.1.",
    );
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
            let stt = match try_run_stt(&audio, preset) {
                Ok(r) => r,
                Err(e) => {
                    let _ = std::fs::remove_file(&audio);
                    return Err(e);
                }
            };
            // Keep the scratch WAV alive — diarization (if the
            // examiner passes --diarize) needs to read it. The
            // CLI cleans it up after the diarization step
            // (or unconditionally on the non-diarize path).
            Ok(ResolvedSource {
                text: stt.transcript,
                upstream_lang: stt.detected_language,
                upstream_confidence: stt.confidence,
                segments: Some(stt.segments),
                kind_label: "video",
                audio_path: Some(audio),
                audio_path_is_scratch: true,
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
                audio_path: Some(p),
                audio_path_is_scratch: false,
            })
        }
        PipelineInput::Image(p) => {
            // Default OCR language for auto-detected image inputs:
            // English. Examiners who know the language up front
            // should pass `--image` + `--ocr-lang`.
            resolve_image_input(&p, "en")
        }
        PipelineInput::Pdf(p) => {
            println_verify("Input type: PDF — extracting text layer (OCR fallback if scanned)...");
            resolve_pdf_input(&p, "en")
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

fn resolve_pdf_input(
    pdf: &std::path::Path,
    ocr_lang: &str,
) -> Result<ResolvedSource, VerifyError> {
    let scratch = std::env::temp_dir().join("verify").join("pdf-scratch");
    let text = extract_pdf_text(pdf, &scratch, ocr_lang)?;
    Ok(ResolvedSource {
        text,
        upstream_lang: ocr_lang.to_string(),
        upstream_confidence: 0.0,
        segments: None,
        kind_label: "pdf",
        audio_path: None,
        audio_path_is_scratch: false,
    })
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
        audio_path: None,
        audio_path_is_scratch: false,
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
    config_path: Option<&std::path::Path>,
    format: CliReportFormat,
    all_foreign: bool,
    threads: usize,
) -> Result<(), VerifyError> {
    let config = load_report_config(config_path)?;
    let resolved_threads = resolve_thread_count(threads);
    if all_foreign {
        // The Sprint 8 default already classifies and translates
        // every non-`--target` file. The flag is plumbed for
        // examiner-intent clarity and to surface a leading log
        // line so the run banner reflects what the operator
        // asked for.
        println_verify(
            "Mode: --all-foreign — every detected non-target language will be translated.",
        );
    }
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
    let mut pdf_count = 0u32;
    let mut other_count = 0u32;
    for f in &files {
        match detect_input_kind(f) {
            PipelineInput::Audio(_) => audio_count += 1,
            PipelineInput::Video(_) => video_count += 1,
            PipelineInput::Image(_) => image_count += 1,
            PipelineInput::Pdf(_) => pdf_count += 1,
            PipelineInput::Text(_) => other_count += 1,
        }
    }
    println_verify(format!("Batch processing: {input:?}"));
    println_verify(format!(
        "Found {} files ({audio_count} audio, {video_count} video, {image_count} image, \
         {pdf_count} pdf{}{})",
        files.len(),
        if other_count > 0 { ", " } else { "" },
        if other_count > 0 {
            format!("{other_count} other")
        } else {
            String::new()
        },
    ));

    let total = files.len() as u32;
    let started = std::time::Instant::now();

    // Sprint 6 P1c — progress file for long batches. When `--output`
    // is set, we write `<output>.progress.json` after each file so
    // an examiner can `tail` it during a multi-hour run.
    let progress_path: Option<PathBuf> = output.map(|p| {
        let mut q = p.to_path_buf();
        let name = q
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "report".to_string());
        q.set_file_name(format!("{name}.progress.json"));
        q
    });

    // Sprint 9 P2 — pre-filter the file list per `--types` so the
    // rayon iterator processes only the eligible inputs. Each
    // entry pairs the file path with its kind_label so we don't
    // call `detect_input_kind` twice.
    let mut eligible: Vec<(PathBuf, &'static str)> = Vec::with_capacity(files.len());
    for file in &files {
        let kind = detect_input_kind(file);
        let kind_label: &'static str = match &kind {
            PipelineInput::Audio(_) => "audio",
            PipelineInput::Video(_) => "video",
            PipelineInput::Image(_) => "image",
            PipelineInput::Pdf(_) => "pdf",
            PipelineInput::Text(_) => "text",
        };
        if let Some(allow) = &allowed {
            if !allow.iter().any(|a| a == kind_label) {
                continue;
            }
        }
        eligible.push((file.clone(), kind_label));
    }

    println_verify(format!(
        "Processing {} eligible file(s) with {} worker thread(s)...",
        eligible.len(),
        resolved_threads,
    ));

    use rayon::prelude::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    // Live counters incremented from worker threads. Atomic so
    // we can build the progress snapshot without locking the
    // results vec on every file.
    let processed_atomic = AtomicU32::new(0);
    let errors_atomic = AtomicU32::new(0);
    let foreign_atomic = AtomicU32::new(0);
    let translated_atomic = AtomicU32::new(0);
    // Recent paths buffer for the progress JSON. Tiny lock —
    // contention is bounded to the sub-millisecond push.
    let recent: Mutex<Vec<String>> = Mutex::new(Vec::with_capacity(8));

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(resolved_threads)
        .thread_name(|i| format!("verify-batch-{i}"))
        .build()
        .map_err(|e| VerifyError::InvalidInput(format!("rayon pool: {e}")))?;

    let progress_path_ref = progress_path.as_deref();
    let target_ref = target;

    let mut results: Vec<BatchFileResult> = pool.install(|| {
        eligible
            .par_iter()
            .map(|(file, kind_label)| {
                let row = match process_one_file(
                    file,
                    kind_label,
                    target,
                    ocr_lang,
                    preset,
                    backend,
                    translation_backend,
                ) {
                    Ok(r) => {
                        processed_atomic.fetch_add(1, Ordering::Relaxed);
                        if r.is_foreign {
                            foreign_atomic.fetch_add(1, Ordering::Relaxed);
                        }
                        if r.translated_text.is_some() {
                            translated_atomic.fetch_add(1, Ordering::Relaxed);
                        }
                        r
                    }
                    Err(e) => {
                        errors_atomic.fetch_add(1, Ordering::Relaxed);
                        log::warn!("batch: {file:?}: {e}");
                        BatchFileResult {
                            file_path: file.to_string_lossy().into_owned(),
                            input_type: kind_label.to_string(),
                            detected_language: String::new(),
                            is_foreign: false,
                            confidence_tier: String::new(),
                            confidence_advisory: None,
                            source_text: None,
                            translated_text: None,
                            segments: None,
                            error: Some(e.to_string()),
                        }
                    }
                };

                // Per-file progress update — best-effort, lock-
                // protected, never panics back into rayon.
                if let Some(pp) = progress_path_ref {
                    if let Ok(mut recent_guard) = recent.lock() {
                        if recent_guard.len() >= 8 {
                            recent_guard.remove(0);
                        }
                        recent_guard.push(row.file_path.clone());
                        let snapshot_recent: Vec<String> = recent_guard.clone();
                        drop(recent_guard);
                        let _ = write_progress_snapshot(
                            pp,
                            target_ref,
                            total,
                            processed_atomic.load(Ordering::Relaxed),
                            foreign_atomic.load(Ordering::Relaxed),
                            translated_atomic.load(Ordering::Relaxed),
                            errors_atomic.load(Ordering::Relaxed),
                            &snapshot_recent,
                            started.elapsed().as_secs_f64(),
                        );
                    }
                }
                row
            })
            .collect()
    });

    // par_iter().collect() returns results in the same order as
    // the input slice (rayon promise) — sort to be defensive.
    results.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    let processed = processed_atomic.load(Ordering::Relaxed);
    let errors = errors_atomic.load(Ordering::Relaxed);
    let foreign_count = foreign_atomic.load(Ordering::Relaxed);
    let translated_count = translated_atomic.load(Ordering::Relaxed);

    let elapsed = started.elapsed().as_secs_f64();
    let mut report = BatchResult {
        generated_at: utc_now_iso8601(),
        total_files: total,
        processed,
        foreign_language: foreign_count,
        translated: translated_count,
        errors,
        target_language: target.to_string(),
        machine_translation_notice: MACHINE_TRANSLATION_NOTICE.to_string(),
        results,
        summary: None,
        language_groups: Vec::new(),
        dominant_language: None,
    };
    let summary = report.build_summary(elapsed, MACHINE_TRANSLATION_NOTICE);
    report.summary = Some(summary);
    report.build_language_groups();
    report.assert_advisory()?;

    println_verify(format!(
        "Complete. {} processed, {} foreign-language, {} translated, {} errors.",
        processed, foreign_count, translated_count, errors,
    ));

    if let Some(out_path) = output {
        write_batch_report(out_path, &report, &config, format)?;
        println_verify(format!("Report written to {out_path:?}"));
        // The progress file is intentionally NOT removed — examiners
        // may want it as evidence of a long-run audit trail. Note its
        // path explicitly.
        if let Some(pp) = &progress_path {
            println_verify(format!("Progress snapshots: {pp:?}"));
        }
    }

    Ok(())
}

/// Resolve the user-facing `--threads` flag into a concrete pool
/// size. `0` (the default) → `min(num_cpus, 8)`; any other value
/// passes through. The 8-thread cap keeps STT model loads (each
/// pulls ~150 MB of safetensors into memory) from blowing past
/// reasonable budgets on parallel large-evidence runs.
fn resolve_thread_count(requested: usize) -> usize {
    if requested == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4)
    } else {
        requested
    }
}

#[allow(clippy::too_many_arguments)]
fn write_progress_snapshot(
    path: &std::path::Path,
    target: &str,
    total: u32,
    processed: u32,
    foreign_count: u32,
    translated_count: u32,
    errors: u32,
    recent: &[String],
    elapsed_secs: f64,
) -> Result<(), VerifyError> {
    // Sprint 9 P2 — thread-safe variant of the Sprint 6 progress
    // writer. The serialisation work happens on the caller's
    // thread; `recent` is a pre-cloned snapshot so we don't hold
    // the Mutex across the JSON write.
    let snapshot = serde_json::json!({
        "generated_at": utc_now_iso8601(),
        "target_language": target,
        "total_files": total,
        "processed": processed,
        "foreign_language": foreign_count,
        "translated": translated_count,
        "errors": errors,
        "elapsed_seconds": elapsed_secs,
        "recent_files": recent,
        "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        "complete": false,
    });
    let body = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| VerifyError::Translate(format!("progress JSON serialise: {e}")))?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, body)?;
    Ok(())
}

/// Render and write a [`BatchResult`] to `out_path`. Format
/// honours the explicit `--format` flag; `Auto` picks by
/// extension (`.csv` → CSV, `.html`/`.htm` → HTML, else JSON).
/// JSON output gets the optional `report_metadata` block from
/// the loaded report config.
fn write_batch_report(
    out_path: &std::path::Path,
    report: &BatchResult,
    config: &ReportConfig,
    format: CliReportFormat,
) -> Result<(), VerifyError> {
    let resolved = match format {
        CliReportFormat::Auto => match out_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("csv") => CliReportFormat::Csv,
            Some("html") | Some("htm") => CliReportFormat::Html,
            _ => CliReportFormat::Json,
        },
        explicit => explicit,
    };
    let body = match resolved {
        CliReportFormat::Csv => render_batch_csv(report),
        CliReportFormat::Html => render_batch_html(report, config),
        CliReportFormat::Json | CliReportFormat::Auto => {
            // Auto can't reach here — the match above resolves it.
            // We still serialise JSON; metadata block is woven in
            // at the top when `config` carries any agency fields.
            let mut value = serde_json::to_value(report).map_err(|e| {
                VerifyError::Translate(format!("batch JSON serialise: {e}"))
            })?;
            if let Some(meta) = config.metadata_json(&report.generated_at) {
                if let serde_json::Value::Object(map) = &mut value {
                    let mut prefixed = serde_json::Map::new();
                    prefixed.insert("report_metadata".into(), meta);
                    for (k, v) in map.iter() {
                        prefixed.insert(k.clone(), v.clone());
                    }
                    value = serde_json::Value::Object(prefixed);
                }
            }
            serde_json::to_string_pretty(&value)
                .map_err(|e| VerifyError::Translate(format!("batch JSON serialise: {e}")))?
        }
    };
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(out_path, body)?;
    Ok(())
}

fn load_report_config(path: Option<&std::path::Path>) -> Result<ReportConfig, VerifyError> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path()?,
    };
    if !path.exists() {
        return Ok(ReportConfig::blank());
    }
    ReportConfig::load(&path)
}

fn default_config_path() -> Result<PathBuf, VerifyError> {
    let home = std::env::var("HOME").map_err(|_| {
        VerifyError::InvalidInput("HOME not set; pass --config explicitly".to_string())
    })?;
    Ok(PathBuf::from(home).join(".verify_report.toml"))
}

fn cmd_config(action: ConfigAction) -> Result<(), VerifyError> {
    match action {
        ConfigAction::Init { output, force } => {
            let path = match output {
                Some(p) => p,
                None => default_config_path()?,
            };
            if path.exists() && !force {
                return Err(VerifyError::InvalidInput(format!(
                    "{path:?} already exists; pass --force to overwrite"
                )));
            }
            let cfg = ReportConfig::blank();
            let body = cfg.to_toml_string()?;
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(&path, body)?;
            println_verify(format!("Wrote default config to {path:?}"));
        }
        ConfigAction::Show { path } => {
            let cfg = load_report_config(path.as_deref())?;
            let body = cfg.to_toml_string()?;
            // Show via the CLI helper so output stays prefixed.
            for line in body.lines() {
                println_verify(line);
            }
        }
        ConfigAction::Set { key, value, path } => {
            let target = match path {
                Some(p) => p,
                None => default_config_path()?,
            };
            let mut cfg = if target.exists() {
                ReportConfig::load(&target)?
            } else {
                ReportConfig::blank()
            };
            match key.as_str() {
                "agency_name" => cfg.agency_name = Some(value),
                "case_number" => cfg.case_number = Some(value),
                "examiner_name" => cfg.examiner_name = Some(value),
                "examiner_badge" => cfg.examiner_badge = Some(value),
                "classification" => cfg.classification = Some(value),
                "report_title" => cfg.report_title = Some(value),
                other => {
                    return Err(VerifyError::InvalidInput(format!(
                        "unknown config key {other:?}; valid: agency_name, \
                         case_number, examiner_name, examiner_badge, \
                         classification, report_title"
                    )));
                }
            }
            let body = cfg.to_toml_string()?;
            std::fs::write(&target, body)?;
            println_verify(format!("Updated {key} in {target:?}"));
        }
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
        "pdf" => resolve_pdf_input(file, ocr_lang)?,
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
            confidence_tier: String::new(),
            confidence_advisory: None,
            source_text: None,
            translated_text: None,
            segments: None,
            error: None,
        });
    }

    // Re-classify the produced text — same logic as cmd_translate.
    let classifier = build_classifier(backend)?;
    let cr = classifier.classify(&resolved.text, target)?;
    let lang = if cr.language.is_empty() {
        resolved.upstream_lang.clone()
    } else {
        cr.language.clone()
    };
    let confidence_tier = cr.confidence_tier.as_str().to_string();
    let confidence_advisory = cr.advisory.clone();
    let is_foreign = lang != target;

    if !is_foreign {
        return Ok(BatchFileResult {
            file_path: file.to_string_lossy().into_owned(),
            input_type: kind_label.to_string(),
            detected_language: lang,
            is_foreign: false,
            confidence_tier,
            confidence_advisory,
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
        confidence_tier,
        confidence_advisory,
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
    let options = TranscribeOptions {
        preset,
        ..TranscribeOptions::default()
    };
    try_run_stt_with(input, &options)
}

fn try_run_stt_with(
    input: &std::path::Path,
    options: &TranscribeOptions,
) -> Result<SttResult, VerifyError> {
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
    let paths = mgr.ensure_whisper_model(options.preset)?;
    let mut engine = SttEngine::load(&paths, options.preset)?;
    engine.transcribe_with_options(input, options)
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn default_classifier_backend_is_whichlang() {
        // Sprint 4 P1: fasttext 0.8 is binary-incompatible with
        // lid.176.ftz; whichlang must be the default backend.
        let cli = Cli::parse_from(["verify", "classify", "--text", "x"]);
        assert!(matches!(cli.classifier_backend, ClassifierBackend::Whichlang));
    }

    #[test]
    fn fasttext_backend_still_selectable_for_research() {
        let cli = Cli::parse_from([
            "verify",
            "--classifier-backend",
            "fasttext",
            "classify",
            "--text",
            "x",
        ]);
        assert!(matches!(cli.classifier_backend, ClassifierBackend::Fasttext));
    }

    #[test]
    fn resolve_thread_count_zero_means_auto_capped_at_8() {
        // Default `0` → some positive value, capped at 8.
        let n = resolve_thread_count(0);
        assert!(
            (1..=8).contains(&n),
            "auto-resolved thread count {n} out of bounds"
        );
    }

    #[test]
    fn resolve_thread_count_passes_through_explicit_value() {
        // Explicit values pass through — including `1` for forcing
        // sequential behaviour and large values for power users.
        assert_eq!(resolve_thread_count(1), 1);
        assert_eq!(resolve_thread_count(4), 4);
        assert_eq!(resolve_thread_count(16), 16);
    }

    #[test]
    fn parallel_batch_progress_snapshot_is_well_formed() {
        // Sprint 9 P2 — the threaded progress writer must produce
        // valid JSON with the MT notice and the recent_files
        // array, regardless of which worker thread invoked it.
        let dir = std::env::temp_dir().join(format!(
            "verify-progress-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rpt.progress.json");
        let recent = vec!["/ev/a.mp3".to_string(), "/ev/b.mp3".to_string()];
        write_progress_snapshot(&path, "en", 10, 5, 3, 2, 0, &recent, 12.5)
            .expect("write");
        let body = std::fs::read_to_string(&path).expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("parse");
        assert_eq!(parsed["target_language"], "en");
        assert_eq!(parsed["total_files"], 10);
        assert_eq!(parsed["processed"], 5);
        assert!(!parsed["machine_translation_notice"]
            .as_str()
            .unwrap_or("")
            .is_empty());
        assert_eq!(parsed["recent_files"].as_array().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_translation_backend_is_auto() {
        let cli = Cli::parse_from(["verify", "classify", "--text", "x"]);
        assert!(matches!(
            cli.translation_backend,
            CliTranslationBackend::Auto
        ));
    }
}
