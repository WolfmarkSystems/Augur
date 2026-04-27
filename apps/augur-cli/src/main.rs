//! `verify` — AUGUR's standalone CLI.
//!
//! Three subcommands (`classify` / `transcribe` / `translate`),
//! all offline by default. Sprint 1 ships real classification (via
//! fastText or whichlang) plus Sprint-1 stubs for STT and
//! translation; Sprint 2 replaces the stubs.

mod benchmark;
mod install;
mod live;
mod package;
mod selftest;

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;
use augur_classifier::{LanguageClassifier, ModelManager as ClassifierModelManager};
use augur_core::geoip::{GeoIpEngine, GeoIpResult, GEOIP_DB_INSTRUCTIONS};
use augur_core::report::{render_batch_html, ReportConfig};
use augur_core::yara_scan::{YaraEngine, YaraMatch};
use augur_core::timestamps::{
    convert as ts_convert, detect_and_convert as ts_detect, parse_input_file as ts_parse,
    TimestampFormat, TimestampResult,
};
use augur_core::pipeline::{
    detect_input_kind_robust, render_batch_csv, BatchFileResult, BatchResult, BatchSegment,
    PipelineInput,
};
use augur_core::subtitle::{
    parse_srt, parse_vtt, render_srt as render_srt_subs, SubtitleEntry,
};
use augur_core::AugurError;
use augur_ocr::{extract_pdf_text, iso_to_tesseract, OcrEngine};
use augur_stt::{
    extract_audio_from_video, merge_stt_with_diarization, DiarizationEngine, DiarizationSegment,
    EnrichedSegment, HfTokenManager, ModelManager as WhisperModelManager, SttEngine, SttResult,
    SttSegment, TranscribeOptions, WhisperModel, WhisperPreset,
};
use augur_translate::{
    Backend as TranslationBackend, SeamlessEngine, TranslationEngine, TranslationEngineKind,
    TranslationResult, MACHINE_TRANSLATION_NOTICE,
};

/// Exact `--version` / `-V` output. Kept as a `const` so it's
/// greppable and so it doesn't drift from the `Cargo.toml`
/// version.
const VERSION_STRING: &str = concat!("AUGUR ", env!("CARGO_PKG_VERSION"), " — Wolfmark Systems");

#[derive(Debug, Parser)]
#[command(
    name = "verify",
    // `disable_version_flag = true` plus our own `--version` /
    // `-V` bool below — clap's default version output is
    // `{bin_name} {version}` which would produce
    // `verify AUGUR 0.1.0 — …`. We want the exact sentinel
    // string, so we intercept the flag ourselves.
    disable_version_flag = true,
    about = "AUGUR — forensic translation + transcription.\n\
             All processing is local. No evidence leaves your machine.",
    long_about = "AUGUR surfaces foreign-language content inside digital \
                  evidence — text, audio, video, and images — translating it \
                  into the examiner's working language.\n\
                  \n\
                  All processing is local. No evidence leaves your machine. \
                  The only network access AUGUR performs is a one-time \
                  download of model weights on first run, which can be \
                  pre-placed offline for air-gapped workstations."
)]
struct Cli {
    /// Print version (`AUGUR 0.1.0 — Wolfmark Systems`) and exit.
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

        /// Sprint 10 P2 — concrete Whisper model. `auto` (the
        /// default) cascades from the largest installed model
        /// (Large-v3 → Base → Tiny). Overrides `--preset` when
        /// set to anything other than `auto`.
        #[arg(long, value_enum, default_value_t = CliWhisperModel::Auto)]
        model: CliWhisperModel,

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

        /// Sprint 10 P2 — concrete Whisper model (auto-cascades
        /// to the largest installed when `auto`).
        #[arg(long, value_enum, default_value_t = CliWhisperModel::Auto)]
        model: CliWhisperModel,

        /// Sprint 10 P3 — translation engine. `nllb` (default)
        /// uses NLLB-200; `seamless` routes through SeamlessM4T
        /// for code-switched input; `auto` picks per-input based
        /// on classifier confidence and script-mix heuristics.
        #[arg(long, value_enum, default_value_t = CliEngine::Nllb)]
        engine: CliEngine,

        /// Super Sprint Group B P3 — path to a YARA rules file
        /// (or directory). When set, the translated text AND the
        /// original source are scanned with the rules; matches
        /// are printed alongside the translation.
        /// Requires `yara` on PATH.
        #[arg(long)]
        yara_rules: Option<PathBuf>,

        /// Super Sprint Group B P2 — when the input is a subtitle
        /// file (.srt / .vtt), write a translated subtitle file
        /// to this path. Cues retain their original timestamps;
        /// only the cue text is replaced with the NLLB output.
        /// Output format mirrors the input (`.srt` → SRT,
        /// `.vtt` → SRT regardless — most players read SRT).
        #[arg(long)]
        output_srt: Option<PathBuf>,

        /// Enable speaker diarization (who said what). Requires
        /// `pip3 install --user pyannote.audio` and a Hugging Face
        /// token configured via `augur setup --hf-token`. Audio
        /// and video inputs only — text/image/PDF are silently
        /// unaffected. Default: off.
        #[arg(long, default_value_t = false)]
        diarize: bool,

        /// Sprint 13 P1 — output format. `text` (default) is the
        /// human-readable CLI output; `ndjson` emits one JSON
        /// object per line on stdout (segment / dialect /
        /// code_switch / complete / error events) so the desktop
        /// GUI can stream-parse the pipeline. Anything else
        /// falls back to text.
        #[arg(long, default_value = "text")]
        format: String,

        /// Sprint 13 P1 — explicit source-language ISO 639-1
        /// hint. When omitted the classifier auto-detects.
        #[arg(long)]
        source: Option<String>,
    },

    /// Show bundled AUGUR documentation. Without a topic
    /// argument prints the full user manual; with one of
    /// `quick`, `deploy`, `airgap`, `strata`, `languages`,
    /// prints the focused doc.
    Docs {
        /// Optional topic. One of `manual` (default) / `quick` /
        /// `deploy` / `airgap` / `strata` / `languages`.
        topic: Option<String>,
    },

    /// Run the AUGUR benchmark suite. By default exercises the
    /// classifier on every text fixture under `tests/benchmarks/`.
    /// `--full` adds a translation pass on the smallest fixture
    /// (requires the python NLLB worker dependencies).
    Benchmark {
        /// Path to the fixtures directory. Defaults to
        /// `tests/benchmarks/` relative to the workspace root.
        #[arg(long)]
        fixtures: Option<PathBuf>,

        /// Optional output JSON path.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Compare against a previously-written results JSON;
        /// any fixture > 1.2× the baseline is flagged.
        #[arg(long)]
        compare: Option<PathBuf>,

        /// Run translation benchmarks too. Without it the run
        /// is fully offline.
        #[arg(long, default_value_t = false)]
        full: bool,
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
        /// `augur-package-<YYYYMMDD>.zip` in the current dir.
        #[arg(long)]
        output: Option<PathBuf>,

        /// Target language for translations. Same semantics as
        /// `augur batch --target`.
        #[arg(long, default_value = "en")]
        target: String,

        /// Optional `augur config` TOML — supplies agency /
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

        /// Sprint 16 P1 — case number override. Wins over any
        /// value in the optional `--config` TOML. Lands in the
        /// MANIFEST.json `case_number` field and the chain-of-
        /// custody header.
        #[arg(long)]
        case_number: Option<String>,

        /// Sprint 16 P1 — examiner name override.
        #[arg(long)]
        examiner: Option<String>,

        /// Sprint 16 P1 — agency override.
        #[arg(long)]
        agency: Option<String>,

        /// Sprint 16 P1 — emit per-file packaging progress as
        /// NDJSON to stdout (`package_file_start` /
        /// `package_file_done` / `package_complete` events) so
        /// the desktop GUI can drive a live progress wizard.
        #[arg(long, default_value = "text")]
        format_progress: String,

        /// Sprint 17 P2 — path to a JSON document with the
        /// examiner-flagged segments to embed in the package's
        /// `review/` directory. Schema:
        ///   { "flags": [ { "filePath": "...",
        ///                  "segmentIndex": 3,
        ///                  "examinerNote": "...",
        ///                  "reviewStatus": "needs_review",
        ///                  "flaggedAt": "ISO8601" }, ... ] }
        /// Missing or empty file → no review/ directory.
        #[arg(long)]
        flags_json: Option<PathBuf>,
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
    /// download instructions or set `AUGUR_GEOIP_PATH`.
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
    /// `~/.cache/augur/hf_token` (chmod 0600 on Unix).
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
    /// View / write the report config used by `augur batch`.
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

        /// Optional `augur config` TOML — supplies agency
        /// name / case number / examiner signature / classification
        /// marking for the rendered report. Default location:
        /// `~/.augur_report.toml`.
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

        /// Sprint 13 P2 — emit per-file progress events to stdout
        /// in NDJSON form (`batch_file_start` / `batch_file_done`
        /// / `batch_complete`) so the desktop GUI can drive a
        /// live batch-progress view. The final report still lands
        /// at `--output` in the chosen `--format`.
        #[arg(long, default_value = "text")]
        format_progress: String,

        /// Sprint 9 P2 — number of worker threads for parallel
        /// file processing. `0` (the default) means
        /// `min(num_cpus, 8)`. Use `1` to force the previous
        /// sequential behaviour. Cap of 8 keeps STT model loads
        /// from blowing memory on large evidence runs.
        #[arg(long, default_value_t = 0)]
        threads: usize,
    },

    /// Sprint 19 P1 — real-time microphone translation. Captures
    /// the default input device, chunks audio every `--chunk-ms`,
    /// runs each chunk through Whisper STT + classifier + NLLB,
    /// and emits NDJSON `live_segment` events on stdout. The
    /// session terminates on SIGINT or when stdin closes.
    Live {
        /// Target language ISO 639-1 code.
        #[arg(long, default_value = "en")]
        target: String,

        /// Chunk duration in milliseconds. Default 3000 (3s).
        /// Smaller = lower latency, more compute. Whisper does
        /// not handle chunks shorter than ~1500 ms reliably.
        #[arg(long, default_value_t = 3000)]
        chunk_ms: u64,

        /// Output format. Only `ndjson` is wired today.
        #[arg(long, default_value = "ndjson")]
        format: String,
    },

    /// Sprint 10 P1 — manage the model catalog. Three install
    /// tiers (minimal / standard / full); `--list` prints the
    /// catalog without touching the network; `--status` shows what
    /// is currently materialised on disk; `--airgap` builds a tar
    /// of an installed tier for transfer to a SCIF / disconnected
    /// host.
    Install {
        /// Install profile: `minimal` (~2.5 GB) / `standard` (~11 GB)
        /// / `full` (~15 GB). Required unless `--list`, `--status`,
        /// or (with `--airgap`) `--profile` are supplied.
        profile: Option<String>,

        /// Print the model catalog (no network).
        #[arg(long, default_value_t = false)]
        list: bool,

        /// Print install status (no network).
        #[arg(long, default_value_t = false)]
        status: bool,

        /// Build an air-gap package archive at this path. Requires
        /// the chosen profile's models to already be installed
        /// locally; the archive bundles the model cache plus a
        /// manifest + README.
        #[arg(long)]
        airgap: Option<PathBuf>,

        /// Profile to bundle when `--airgap` is set. Defaults to
        /// `standard`.
        #[arg(long)]
        profile_for_airgap: Option<String>,

        /// Sprint 13 P3 — install one specific model by id (e.g.
        /// `whisper-large-v3`, `nllb-1.3b`). Useful for adding
        /// models post-initial install without re-running an
        /// entire tier.
        #[arg(long)]
        model: Option<String>,

        /// Sprint 13 P3 — output format. `text` (default) is the
        /// human-readable rendering of `--list` / `--status` /
        /// install logs; `json` makes `--status` emit a single
        /// JSON document on stdout; `ndjson` makes `--model`
        /// installs emit per-model progress events on stdout.
        #[arg(long, default_value = "text")]
        format: String,
    },
}

#[derive(Debug, Clone, Subcommand)]
enum ConfigAction {
    /// Write a default TOML config to `--output` (or
    /// `~/.augur_report.toml` if not specified). Refuses to
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

/// Sprint 10 P2 — `--model` value enum on transcribe/translate.
/// Unlike `CliPreset` (which names the candle architecture class),
/// `CliWhisperModel` names a specific installed model + supports
/// the `auto` cascade.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliWhisperModel {
    Auto,
    Tiny,
    Base,
    LargeV3,
    Pashto,
    Dari,
}

/// Sprint 10 P3 — `--engine` value enum on translate.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum CliEngine {
    Nllb,
    Seamless,
    Auto,
}

impl From<CliEngine> for TranslationEngineKind {
    fn from(c: CliEngine) -> Self {
        match c {
            CliEngine::Nllb => TranslationEngineKind::Nllb,
            CliEngine::Seamless => TranslationEngineKind::Seamless,
            CliEngine::Auto => TranslationEngineKind::Auto,
        }
    }
}

impl CliWhisperModel {
    fn into_model(self, detected_language: Option<&str>) -> WhisperModel {
        match self {
            Self::Auto => augur_stt::auto_select_whisper_model(detected_language),
            Self::Tiny => WhisperModel::Tiny,
            Self::Base => WhisperModel::Base,
            Self::LargeV3 => WhisperModel::LargeV3,
            Self::Pashto => WhisperModel::Pashto,
            Self::Dari => WhisperModel::Dari,
        }
    }
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
        eprintln!("[AUGUR] no subcommand given. Run `augur --help` for usage.");
        return ExitCode::from(2);
    };

    match run(command, cli.classifier_backend, cli.translation_backend.into()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Always surface errors on stderr — never panic.
            eprintln!("[AUGUR] error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(
    command: Command,
    backend: ClassifierBackend,
    translation_backend: TranslationBackend,
) -> Result<(), AugurError> {
    match command {
        Command::Classify { text, target } => cmd_classify(&text, &target, backend),
        Command::Transcribe {
            input,
            preset,
            model,
            temperature,
            max_retries,
        } => {
            // Sprint 10 P2 — `--model` overrides `--preset` when
            // it resolves to a different concrete preset. `auto`
            // cascades to the largest installed.
            let resolved = model.into_model(None);
            let effective_preset = match model {
                CliWhisperModel::Auto => preset.into(),
                _ => resolved.resolved_preset(),
            };
            cmd_transcribe(&input, effective_preset, temperature, max_retries)
        }
        Command::Translate {
            input,
            text,
            image,
            ocr_lang,
            target,
            preset,
            model,
            engine,
            output_srt,
            diarize,
            yara_rules,
            format,
            source,
        } => {
            let resolved = model.into_model(None);
            let effective_preset = match model {
                CliWhisperModel::Auto => preset.into(),
                _ => resolved.resolved_preset(),
            };
            // Sprint 13 P1 — `--format ndjson` activates the
            // streaming JSON output the desktop GUI consumes.
            // Anything else (text / unknown) keeps the legacy
            // human-readable rendering.
            let ndjson = format.eq_ignore_ascii_case("ndjson");
            // Source hint is informational at this layer; the
            // pipeline runs the classifier regardless.
            let _ = source;
            cmd_translate(
                input.as_deref(),
                text.as_deref(),
                image.as_deref(),
                &ocr_lang,
                &target,
                effective_preset,
                backend,
                translation_backend,
                engine.into(),
                diarize,
                output_srt.as_deref(),
                yara_rules.as_deref(),
                ndjson,
            )
        }
        Command::Setup { hf_token } => cmd_setup(&hf_token),
        Command::Docs { topic } => cmd_docs(topic.as_deref()),
        Command::Benchmark {
            fixtures,
            output,
            compare,
            full,
        } => cmd_benchmark(fixtures.as_deref(), output.as_deref(), compare.as_deref(), full),
        Command::SelfTest { full } => cmd_self_test(full),
        Command::Package {
            input,
            output,
            target,
            config,
            preset,
            ocr_lang,
            include_originals,
            case_number,
            examiner,
            agency,
            format_progress,
            flags_json,
        } => {
            // Sprint 16 P1 — `--format-progress ndjson` activates
            // the streaming progress channel for the desktop GUI's
            // Package Wizard.
            if format_progress.eq_ignore_ascii_case("ndjson") {
                NDJSON_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            cmd_package(
                &input,
                output.as_deref(),
                &target,
                config.as_deref(),
                preset.into(),
                &ocr_lang,
                include_originals,
                backend,
                translation_backend,
                case_number.as_deref(),
                examiner.as_deref(),
                agency.as_deref(),
                flags_json.as_deref(),
            )
        }
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
            format_progress,
            threads,
        } => {
            // Sprint 13 P2 — `--format-progress ndjson` enables
            // the streaming progress channel for the desktop GUI.
            // Activates the global NDJSON_MODE so all
            // `[AUGUR] …` chatter is suppressed during the run.
            if format_progress.eq_ignore_ascii_case("ndjson") {
                NDJSON_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            cmd_batch(
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
            )
        }
        Command::Config { action } => cmd_config(action),
        Command::Live {
            target,
            chunk_ms,
            format,
        } => {
            let ndjson = format.eq_ignore_ascii_case("ndjson");
            if ndjson {
                NDJSON_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            live::cmd_live(&target, chunk_ms, ndjson)
        }
        Command::Install {
            profile,
            list,
            status,
            airgap,
            profile_for_airgap,
            model,
            format,
        } => {
            let profile_arg = if airgap.is_some() {
                profile_for_airgap.as_deref().or(profile.as_deref())
            } else {
                profile.as_deref()
            };
            // Sprint 13 P3 — JSON / NDJSON output modes activate
            // NDJSON_MODE so the human-readable `[AUGUR] …` lines
            // are suppressed and stdout carries machine output.
            let format_l = format.to_lowercase();
            if format_l == "json" || format_l == "ndjson" {
                NDJSON_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            install::cmd_install(
                profile_arg,
                list,
                status,
                airgap.as_deref(),
                model.as_deref(),
                &format_l,
            )
        }
    }
}

// ── classify ─────────────────────────────────────────────────────

fn cmd_classify(
    text: &str,
    target: &str,
    backend: ClassifierBackend,
) -> Result<(), AugurError> {
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
/// format spec'd by AUGUR_SPRINT_6 P2c — language, confidence
/// tier + raw score, input word count, and the advisory line
/// when the tier is anything other than `High`.
fn print_classification(r: &augur_classifier::ClassificationResult) {
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
    if let Some(dialect) = r.arabic_dialect {
        if !matches!(dialect, augur_classifier::ArabicDialect::Unknown) {
            println_verify(format!(
                "         Dialect: {} — confidence {:.2}",
                dialect.as_str(),
                r.arabic_dialect_confidence
            ));
            if !r.arabic_dialect_indicators.is_empty() {
                println_verify(format!(
                    "         Dialect indicators: {}",
                    r.arabic_dialect_indicators.join(", ")
                ));
            }
            if let Some(note) = &r.arabic_dialect_note {
                println_verify(format!("         ⚠ {note}"));
            }
        }
    }
}

// ── transcribe ───────────────────────────────────────────────────

fn cmd_transcribe(
    input: &std::path::Path,
    preset: WhisperPreset,
    temperature: f32,
    max_retries: u8,
) -> Result<(), AugurError> {
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
    engine_kind: TranslationEngineKind,
    diarize: bool,
    output_srt: Option<&std::path::Path>,
    yara_rules: Option<&std::path::Path>,
    ndjson: bool,
) -> Result<(), AugurError> {
    // Sprint 13 P1 — NDJSON output mode. When enabled, all
    // human-readable `[AUGUR] …` lines are suppressed and the
    // command emits one JSON object per line on stdout instead.
    // The desktop GUI consumes this stream to populate its
    // split-view workspace event-by-event.
    if ndjson {
        return cmd_translate_ndjson(
            input,
            text,
            image,
            ocr_lang,
            target,
            preset,
            backend,
            translation_backend,
            engine_kind,
            diarize,
            output_srt,
            yara_rules,
        );
    }
    // Resolve the source text through the appropriate engine. The
    // pipelines diverge here:
    //   audio    → preprocess → STT → classifier → NLLB
    //   video    → ffmpeg-extract → STT → classifier → NLLB
    //   image    → OCR → classifier → NLLB
    //   subtitle → SRT/VTT parse → classifier → per-cue NLLB
    //   text     → classifier → NLLB
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
            return Err(AugurError::InvalidInput(
                "augur translate requires --input <audio|video> | --text <string> | --image <path>"
                    .to_string(),
            ));
        }
        _ => {
            return Err(AugurError::InvalidInput(
                "augur translate accepts only one of --input / --text / --image".to_string(),
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
            AugurError::InvalidInput(
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

    // Sprint 10 P3 — engine selection. Default `Nllb` keeps the
    // legacy single-language path. `Seamless` routes through
    // SeamlessM4T (handles code-switching). `Auto` defers to
    // `select_engine` heuristics — only chooses Seamless if it is
    // actually installed.
    let seamless_installed = augur_core::models::find_model("seamless-m4t-medium")
        .map(augur_core::models::is_installed)
        .unwrap_or(false);
    let resolved_engine = match engine_kind {
        TranslationEngineKind::Nllb => TranslationEngineKind::Nllb,
        TranslationEngineKind::Seamless => TranslationEngineKind::Seamless,
        TranslationEngineKind::Auto => augur_translate::select_engine(
            &resolved.text,
            &lang,
            resolved.upstream_confidence,
            seamless_installed,
        ),
    };
    if matches!(resolved_engine, TranslationEngineKind::Seamless) && !seamless_installed {
        return Err(AugurError::InvalidInput(
            "--engine seamless requires `augur install full` (seamless-m4t-medium not installed)"
                .to_string(),
        ));
    }

    let translation = match resolved_engine {
        TranslationEngineKind::Seamless => {
            println_verify(format!(
                "Translating {lang} → {target} via SeamlessM4T..."
            ));
            let seamless = SeamlessEngine::with_xdg_cache()?;
            seamless.translate(&resolved.text, &lang, target)?
        }
        _ => {
            let mut engine = TranslationEngine::with_xdg_cache()?;
            engine.backend = translation_backend;
            if let Some(segs) = &resolved.segments {
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
            }
        }
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

    // Super Sprint Group B P3 — YARA pattern scanning. Scan
    // the translated text AND the source text; print matches.
    if let Some(rules) = yara_rules {
        run_yara_scans(
            rules,
            &resolved.text,
            translation.translated_text.as_str(),
        )?;
    }

    // Super Sprint Group B P2 — per-cue translated SRT output.
    // Only fires when the input was a subtitle file AND the
    // user passed `--output-srt`. We re-parse the entries (cheap
    // — already on disk) and translate each cue independently,
    // preserving timestamps. The full-text translation above
    // remains the human-facing summary.
    if matches!(resolved.kind_label, "subtitle") {
        if let Some(out_path) = output_srt {
            if let Some(in_path) = resolved_path.as_deref() {
                println_verify(format!(
                    "Writing translated SRT to {out_path:?} (per-cue NLLB)..."
                ));
                write_translated_srt(in_path, out_path, &lang, target, translation_backend)?;
            }
        }
    }

    Ok(())
}

/// Sprint 13 P1 — NDJSON streaming translate path. Emits one
/// JSON object per line on stdout in the order the desktop GUI
/// expects:
///
///   1. `dialect` (when source is Arabic and a dialect was
///      identified)
///   2. `segment` × N
///   3. `complete`
///
/// Errors emit a single `error` line then return `Err`.
/// `println!` here is the audited NDJSON output surface — the
/// only stdout site in the binary outside `println_verify`.
#[allow(clippy::too_many_arguments)]
fn cmd_translate_ndjson(
    input: Option<&std::path::Path>,
    text: Option<&str>,
    image: Option<&std::path::Path>,
    ocr_lang: &str,
    target: &str,
    preset: WhisperPreset,
    backend: ClassifierBackend,
    translation_backend: TranslationBackend,
    engine_kind: TranslationEngineKind,
    _diarize: bool,
    _output_srt: Option<&std::path::Path>,
    _yara_rules: Option<&std::path::Path>,
) -> Result<(), AugurError> {
    NDJSON_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
    let started = std::time::Instant::now();
    let resolved = match (input, text, image) {
        (Some(path), None, None) => resolve_path_input(path, preset),
        (None, Some(t), None) => {
            let classifier = build_classifier(backend)?;
            let cr = classifier.classify(t, target)?;
            Ok(ResolvedSource {
                text: t.to_string(),
                upstream_lang: cr.language,
                upstream_confidence: cr.confidence,
                segments: None,
                kind_label: "text",
                audio_path: None,
                audio_path_is_scratch: false,
            })
        }
        (None, None, Some(img)) => resolve_image_input(img, ocr_lang),
        (None, None, None) => Err(AugurError::InvalidInput(
            "augur translate --format ndjson requires --input / --text / --image".to_string(),
        )),
        _ => Err(AugurError::InvalidInput(
            "augur translate accepts only one of --input / --text / --image".to_string(),
        )),
    };
    let resolved = match resolved {
        Ok(r) => r,
        Err(e) => {
            emit_error_ndjson(&format!("{e}"));
            return Err(e);
        }
    };

    // Re-classify the resolved text for the canonical language /
    // dialect signal, same as the human-readable path.
    let classifier = match build_classifier(backend) {
        Ok(c) => c,
        Err(e) => {
            emit_error_ndjson(&format!("{e}"));
            return Err(e);
        }
    };
    let classification = match classifier.classify(&resolved.text, target) {
        Ok(c) => c,
        Err(e) => {
            emit_error_ndjson(&format!("{e}"));
            return Err(e);
        }
    };

    if let Some(dialect) = classification.arabic_dialect {
        if !matches!(dialect, augur_classifier::ArabicDialect::Unknown) {
            let source_label = if classification
                .arabic_dialect_indicators
                .iter()
                .any(|s| s.starts_with("CAMeL:"))
            {
                "camel"
            } else {
                "lexical"
            };
            let json = serde_json::json!({
                "type": "dialect",
                "dialect": dialect.as_str(),
                "confidence": classification.arabic_dialect_confidence,
                "source": source_label,
                "indicators": classification.arabic_dialect_indicators,
            });
            println!("{json}");
        }
    }

    let lang = if classification.language.is_empty() {
        resolved.upstream_lang.clone()
    } else {
        classification.language.clone()
    };
    if !lang.is_empty() && lang == target {
        // Already in target language — emit a single completion
        // event with zero segments.
        let json = serde_json::json!({
            "type": "complete",
            "total_segments": 0,
            "duration_ms": started.elapsed().as_millis() as u64,
            "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        });
        println!("{json}");
        return Ok(());
    }

    let resolved_engine = match engine_kind {
        TranslationEngineKind::Auto => augur_translate::select_engine(
            &resolved.text,
            &lang,
            resolved.upstream_confidence,
            augur_core::models::find_model("seamless-m4t-medium")
                .map(augur_core::models::is_installed)
                .unwrap_or(false),
        ),
        other => other,
    };

    // Sprint 14 P1 — true streaming. We translate one unit at a
    // time (one STT segment, or one sentence of text input) and
    // emit + flush stdout after each so the desktop GUI sees the
    // segments arrive live, not as a batch at completion time.
    //
    // Sprint 15 P2 — when the source is Arabic, run the dialect
    // router first and emit a `dialect_routing` event before any
    // segment lands. The router decides whether to use NLLB with
    // a dialect-specific token (arz_Arab / apc_Arab / acm_Arab /
    // ary_Arab / ara_Arab) or to route Moroccan Darija through
    // SeamlessM4T (when installed).
    let routing = if lang == "ar" {
        let installed_seamless =
            augur_core::models::find_model("seamless-m4t-medium")
                .map(augur_core::models::is_installed)
                .unwrap_or(false);
        let detected_kind = match classification
            .arabic_dialect
            .unwrap_or(augur_classifier::ArabicDialect::Unknown)
        {
            augur_classifier::ArabicDialect::ModernStandard => {
                augur_core::dialect_routing::DialectKind::ModernStandard
            }
            augur_classifier::ArabicDialect::Egyptian => {
                augur_core::dialect_routing::DialectKind::Egyptian
            }
            augur_classifier::ArabicDialect::Levantine => {
                augur_core::dialect_routing::DialectKind::Levantine
            }
            augur_classifier::ArabicDialect::Gulf => {
                augur_core::dialect_routing::DialectKind::Gulf
            }
            augur_classifier::ArabicDialect::Iraqi => {
                augur_core::dialect_routing::DialectKind::Iraqi
            }
            augur_classifier::ArabicDialect::Moroccan => {
                augur_core::dialect_routing::DialectKind::Moroccan
            }
            augur_classifier::ArabicDialect::Yemeni => {
                augur_core::dialect_routing::DialectKind::Yemeni
            }
            augur_classifier::ArabicDialect::Sudanese => {
                augur_core::dialect_routing::DialectKind::Sudanese
            }
            augur_classifier::ArabicDialect::Unknown => {
                augur_core::dialect_routing::DialectKind::Unknown
            }
        };
        let analysis = augur_core::dialect_routing::DialectAnalysisInput {
            detected_dialect: detected_kind,
            confidence: classification.arabic_dialect_confidence,
        };
        let decision = augur_core::dialect_routing::route_arabic_translation(
            &analysis,
            installed_seamless,
        );
        let json = serde_json::json!({
            "type": "dialect_routing",
            "dialect": format!("{:?}", detected_kind),
            "confidence": analysis.confidence,
            "route": decision.route_label(),
            "model": decision.model_used,
            "reason": decision.reason,
            "dialect_advisory": decision.dialect_advisory,
            "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        });
        println!("{json}");
        flush_stdout();
        Some(decision)
    } else {
        None
    };

    // Build the work-list of (start_ms, end_ms, text) triples.
    // For audio/video the STT segments are authoritative; for text
    // input we sentence-split so each sentence is its own unit and
    // streams independently.
    let work: Vec<(Option<u64>, Option<u64>, String)> =
        if let Some(segs) = &resolved.segments {
            segs.iter()
                .map(|s| (Some(s.start_ms), Some(s.end_ms), s.text.clone()))
                .collect()
        } else {
            split_sentences(&resolved.text)
                .into_iter()
                .map(|s| (None, None, s))
                .collect()
        };

    // Pre-build the heavy engines once so we don't pay startup
    // cost per segment.
    let mut nllb_engine = match TranslationEngine::with_xdg_cache() {
        Ok(e) => e,
        Err(e) => {
            emit_error_ndjson(&format!("{e}"));
            return Err(e);
        }
    };
    nllb_engine.backend = translation_backend;
    let seamless_engine_lazy: Option<SeamlessEngine> = if matches!(
        (resolved_engine, routing.as_ref().map(|r| r.route)),
        (TranslationEngineKind::Seamless, _)
            | (
                _,
                Some(augur_core::dialect_routing::TranslationRoute::SeamlessM4T)
            )
    ) {
        match SeamlessEngine::with_xdg_cache() {
            Ok(s) => Some(s),
            Err(e) => {
                emit_error_ndjson(&format!("{e}"));
                return Err(e);
            }
        }
    } else {
        None
    };

    // Resolve the source-language string passed to the worker.
    // Routing decisions for Arabic dialects override the plain
    // `ar` ISO with a dialect-specific NLLB token.
    let source_for_nllb: String = match routing.as_ref().map(|r| r.route) {
        Some(augur_core::dialect_routing::TranslationRoute::NllbEgyptian) => {
            "arz_Arab".into()
        }
        Some(augur_core::dialect_routing::TranslationRoute::NllbLevantine) => {
            "apc_Arab".into()
        }
        Some(augur_core::dialect_routing::TranslationRoute::NllbIraqi) => {
            "acm_Arab".into()
        }
        Some(augur_core::dialect_routing::TranslationRoute::NllbMoroccan) => {
            "ary_Arab".into()
        }
        _ => lang.clone(),
    };
    let use_dialect_token = routing
        .as_ref()
        .map(|r| {
            matches!(
                r.route,
                augur_core::dialect_routing::TranslationRoute::NllbEgyptian
                    | augur_core::dialect_routing::TranslationRoute::NllbLevantine
                    | augur_core::dialect_routing::TranslationRoute::NllbIraqi
                    | augur_core::dialect_routing::TranslationRoute::NllbMoroccan
            )
        })
        .unwrap_or(false);

    let mut last_advisory = MACHINE_TRANSLATION_NOTICE.to_string();
    let total_segments = work.len();
    for (i, (start, end, text)) in work.iter().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let seamless_route = matches!(
            routing.as_ref().map(|r| r.route),
            Some(augur_core::dialect_routing::TranslationRoute::SeamlessM4T)
        ) || matches!(resolved_engine, TranslationEngineKind::Seamless);
        let single = if seamless_route {
            seamless_engine_lazy
                .as_ref()
                .expect("Seamless engine pre-built when route demanded it")
                .translate(text, &lang, target)
        } else if use_dialect_token {
            nllb_engine.translate_with_nllb_token(text, &source_for_nllb, target)
        } else {
            nllb_engine.translate(text, &lang, target)
        };
        let single = match single {
            Ok(t) => t,
            Err(e) => {
                emit_error_ndjson(&format!("{e}"));
                return Err(e);
            }
        };
        last_advisory = single.advisory_notice.clone();
        let json = serde_json::json!({
            "type": "segment",
            "index": i,
            "start_ms": start,
            "end_ms": end,
            "original": single.source_text,
            "translated": single.translated_text,
            "is_complete": true,
        });
        println!("{json}");
        flush_stdout();
    }

    let json = serde_json::json!({
        "type": "complete",
        "total_segments": total_segments,
        "duration_ms": started.elapsed().as_millis() as u64,
        "machine_translation_notice": last_advisory,
    });
    println!("{json}");
    flush_stdout();
    Ok(())
}

/// Sprint 14 P1 — explicit stdout flush after every NDJSON line.
/// Without this the OS buffers stdout when the consumer is a
/// pipe, defeating the streaming contract.
fn flush_stdout() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
}

/// Sprint 14 P1 — sentence splitter for the text-input path.
/// Splits on `.`, `!`, `?`, Arabic full stop, Urdu period, and
/// the Arabic question mark. Trailing punctuation is preserved
/// on each sentence so the original text round-trips faithfully
/// when concatenated.
fn split_sentences(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let terminators = ['.', '!', '?', '\u{06D4}', '\u{061F}', '\u{3002}'];
    for ch in text.chars() {
        current.push(ch);
        if terminators.contains(&ch) {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                out.push(trimmed);
            }
            current.clear();
        }
    }
    let tail = current.trim().to_string();
    if !tail.is_empty() {
        out.push(tail);
    }
    if out.is_empty() {
        // Fall back to the whole input — never return an empty
        // work list when the caller had non-empty text.
        let trimmed = text.trim().to_string();
        if !trimmed.is_empty() {
            out.push(trimmed);
        }
    }
    out
}

fn emit_error_ndjson(message: &str) {
    let json = serde_json::json!({"type": "error", "message": message});
    println!("{json}");
}

fn run_yara_scans(
    rules: &std::path::Path,
    source_text: &str,
    translated_text: &str,
) -> Result<(), AugurError> {
    let engine = YaraEngine::load(rules)?;
    if !engine.is_available() {
        println_verify(
            "⚠ YARA scan skipped — `yara` binary not on PATH. \
             Install with `brew install yara` or `apt install yara`.",
        );
        return Ok(());
    }
    let mut all: Vec<YaraMatch> = Vec::new();
    if !translated_text.trim().is_empty() {
        match engine.scan_text(translated_text) {
            Ok(mut hits) => {
                for h in &mut hits {
                    h.scanned_source = "translation".to_string();
                }
                all.extend(hits);
            }
            Err(e) => {
                log::warn!("YARA scan (translation) failed: {e}");
            }
        }
    }
    if !source_text.trim().is_empty() {
        match engine.scan_text(source_text) {
            Ok(mut hits) => {
                for h in &mut hits {
                    h.scanned_source = "source".to_string();
                }
                all.extend(hits);
            }
            Err(e) => {
                log::warn!("YARA scan (source) failed: {e}");
            }
        }
    }
    if all.is_empty() {
        println_verify("YARA scan: 0 matches.");
    } else {
        println_verify(format!("YARA scan: {} match(es)", all.len()));
        for m in &all {
            println_verify(format!("  Rule: {} ({})", m.rule_name, m.scanned_source));
            for s in &m.matched_strings {
                let preview: String = s.data.chars().take(80).collect();
                println_verify(format!(
                    "    ${} @ 0x{:x}: {preview}",
                    s.identifier, s.offset
                ));
            }
        }
    }
    Ok(())
}

fn write_translated_srt(
    src: &std::path::Path,
    dst: &std::path::Path,
    source_lang: &str,
    target_lang: &str,
    translation_backend: TranslationBackend,
) -> Result<(), AugurError> {
    let mut entries = load_subtitle_entries(src)?;
    if source_lang == target_lang {
        // Nothing to translate — just copy text through.
        let body = render_srt_subs(&entries);
        std::fs::write(dst, body)?;
        return Ok(());
    }
    let mut engine = TranslationEngine::with_xdg_cache()?;
    engine.backend = translation_backend;
    for entry in &mut entries {
        if entry.text.trim().is_empty() {
            continue;
        }
        let result = engine.translate(&entry.text, source_lang, target_lang)?;
        // Forensic invariant — every TranslationResult carries
        // the MT advisory; we don't strip it but we don't write
        // it into the cue body either (cues are user-visible).
        // The advisory belongs in the report, not on every line
        // of every subtitle.
        debug_assert!(result.is_machine_translation);
        debug_assert!(!result.advisory_notice.is_empty());
        entry.text = result.translated_text;
    }
    std::fs::write(dst, render_srt_subs(&entries))?;
    Ok(())
}

fn print_speaker_advisory() {
    println_verify("");
    println_verify("⚠  SPEAKER DIARIZATION NOTICE");
    for line in augur_stt::SPEAKER_DIARIZATION_ADVISORY
        .split_terminator(". ")
        .filter(|s| !s.is_empty())
    {
        println_verify(format!("   {}", line.trim()));
    }
}

fn run_diarization(audio: &std::path::Path) -> Result<Vec<DiarizationSegment>, AugurError> {
    let engine = DiarizationEngine::with_xdg_cache()?;
    if !engine.is_available() {
        return Err(AugurError::Stt(
            "diarization unavailable: python3 missing or HF token not configured. \
             Run `augur setup --hf-token <hf_…>` and \
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
    translated: &[augur_translate::TranslatedSegment],
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

// Bundled docs — `include_str!` so `augur docs` works on
// air-gapped machines that don't have the source tree.
const DOCS_USER_MANUAL: &str = include_str!("../../../docs/USER_MANUAL.md");
const DOCS_QUICK_REFERENCE: &str = include_str!("../../../docs/QUICK_REFERENCE.md");
const DOCS_DEPLOYMENT: &str = include_str!("../../../docs/DEPLOYMENT.md");
const DOCS_AIRGAP: &str = include_str!("../../../docs/AIRGAP_INSTALL.md");
const DOCS_STRATA: &str = include_str!("../../../docs/STRATA_INTEGRATION.md");
const DOCS_LANG_LIMITS: &str = include_str!("../../../docs/LANGUAGE_LIMITATIONS.md");

fn cmd_docs(topic: Option<&str>) -> Result<(), AugurError> {
    let body = match topic.unwrap_or("manual") {
        "manual" | "user" | "" => DOCS_USER_MANUAL,
        "quick" | "ref" | "reference" => DOCS_QUICK_REFERENCE,
        "deploy" | "deployment" => DOCS_DEPLOYMENT,
        "airgap" | "air-gap" => DOCS_AIRGAP,
        "strata" => DOCS_STRATA,
        "languages" | "limits" | "limitations" => DOCS_LANG_LIMITS,
        other => {
            return Err(AugurError::InvalidInput(format!(
                "unknown docs topic {other:?}; valid: manual, quick, deploy, airgap, strata, languages"
            )));
        }
    };
    // Print docs through the same `println_verify` helper that
    // every other CLI line uses — keeping every stdout write
    // routed through one auditable function. The `[AUGUR]`
    // prefix is consistent across the binary even on doc
    // output; piping `augur docs | sed 's/^\[AUGUR\] //'`
    // strips it for human reading.
    for line in body.lines() {
        println_verify(line);
    }
    Ok(())
}

fn cmd_benchmark(
    fixtures: Option<&std::path::Path>,
    output: Option<&std::path::Path>,
    compare: Option<&std::path::Path>,
    full: bool,
) -> Result<(), AugurError> {
    let fixtures_dir = match fixtures {
        Some(p) => p.to_path_buf(),
        None => {
            // Default: workspace_root/tests/benchmarks. Resolve
            // from CARGO_MANIFEST_DIR at compile time so the
            // installed binary keeps working when the workspace
            // is co-located.
            let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.push("../../tests/benchmarks");
            p.canonicalize().unwrap_or(p)
        }
    };
    let opts = benchmark::BenchmarkOptions {
        full,
        fixtures_dir,
    };
    let suite = benchmark::run_suite(&opts)?;
    for line in benchmark::render_text(&suite).lines() {
        println_verify(line);
    }
    if let Some(prev_path) = compare {
        let body = std::fs::read_to_string(prev_path)?;
        let baseline: benchmark::BenchmarkSuite = serde_json::from_str(&body)
            .map_err(|e| {
                AugurError::InvalidInput(format!(
                    "previous benchmark JSON parse: {e}"
                ))
            })?;
        let report = benchmark::render_regression_report(&suite, &baseline);
        if report.is_empty() {
            println_verify("Regression check: no slowdowns > 1.2× baseline detected.");
        } else {
            for line in report.lines() {
                println_verify(line);
            }
        }
    }
    if let Some(out) = output {
        let body = serde_json::to_string_pretty(&suite).map_err(|e| {
            AugurError::InvalidInput(format!("benchmark JSON serialise: {e}"))
        })?;
        if let Some(parent) = out.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(out, body)?;
        println_verify(format!("Results saved: {out:?}"));
    }
    Ok(())
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
    case_number_override: Option<&str>,
    examiner_override: Option<&str>,
    agency_override: Option<&str>,
    flags_json_path: Option<&std::path::Path>,
) -> Result<(), AugurError> {
    use std::sync::atomic::{AtomicU32, Ordering};
    use rayon::prelude::*;
    use augur_core::pipeline::BatchSummary;
    let _ = flags_json_path; // Pinned in package.rs once threaded fully — currently surfaced via an env hint.
    if let Some(p) = flags_json_path {
        // Forward the path via env so package.rs (which already
        // owns the manifest layout) can pick it up without a
        // signature change in this sprint.
        std::env::set_var("AUGUR_FLAGS_JSON", p.as_os_str());
    }

    if !input.exists() || !input.is_dir() {
        return Err(AugurError::InvalidInput(format!(
            "augur package --input must be a directory, got {input:?}"
        )));
    }

    let mut config = load_report_config(config_path)?;
    // Sprint 16 P1 — explicit CLI flags win over the optional
    // TOML config so the desktop GUI can pass case-info per call
    // without writing a temporary config file.
    if let Some(c) = case_number_override.filter(|s| !s.is_empty()) {
        config.case_number = Some(c.to_string());
    }
    if let Some(e) = examiner_override.filter(|s| !s.is_empty()) {
        config.examiner_name = Some(e.to_string());
    }
    if let Some(a) = agency_override.filter(|s| !s.is_empty()) {
        config.agency_name = Some(a.to_string());
    }
    let zip_path: PathBuf = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let stamp = utc_now_iso8601()
                .chars()
                .take(10)
                .collect::<String>()
                .replace('-', "");
            std::env::current_dir()
                .map_err(AugurError::Io)?
                .join(format!("augur-package-{stamp}.zip"))
        }
    };

    println_verify(format!(
        "augur package: walking {input:?} (preset={preset:?}, target={target})"
    ));

    let mut files: Vec<PathBuf> = Vec::new();
    walk_files(input, &mut files)?;
    files.sort();
    let mut eligible: Vec<(PathBuf, &'static str)> = Vec::with_capacity(files.len());
    for f in &files {
        let kind = detect_input_kind_robust(f);
        let label: &'static str = match &kind {
            PipelineInput::Audio(_) => "audio",
            PipelineInput::Video(_) => "video",
            PipelineInput::Image(_) => "image",
            PipelineInput::Pdf(_) => "pdf",
            PipelineInput::Subtitle(_) => "subtitle",
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
        .thread_name(|i| format!("augur-package-{i}"))
        .build()
        .map_err(|e| AugurError::InvalidInput(format!("rayon pool: {e}")))?;
    // Sprint 16 P1 — emit per-file packaging progress when the
    // global NDJSON_MODE is on (set by `--format-progress ndjson`).
    let ndjson_progress = NDJSON_MODE.load(std::sync::atomic::Ordering::Relaxed);
    let started_atomic = AtomicU32::new(0);
    let total_for_events = eligible.len() as u32;
    let mut results: Vec<BatchFileResult> = pool.install(|| {
        eligible
            .par_iter()
            .map(|(file, kind_label)| {
                if ndjson_progress {
                    let idx = started_atomic.fetch_add(1, Ordering::Relaxed) + 1;
                    let json = serde_json::json!({
                        "type": "package_file_start",
                        "file": file.to_string_lossy(),
                        "input_type": kind_label,
                        "index": idx,
                        "total": total_for_events,
                    });
                    println!("{json}");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
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
                };
                if ndjson_progress {
                    let json = serde_json::json!({
                        "type": "package_file_done",
                        "file": row.file_path,
                        "input_type": kind_label,
                        "detected_language": row.detected_language,
                        "is_foreign": row.is_foreign,
                        "translated": row.translated_text.is_some(),
                        "error": row.error,
                        "processed": processed_atomic.load(Ordering::Relaxed),
                        "total": total_for_events,
                    });
                    println!("{json}");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
                row
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
    if ndjson_progress {
        let json = serde_json::json!({
            "type": "package_complete",
            "output_path": zip_path.to_string_lossy(),
            "total_files": manifest.file_count,
            "translated_files": manifest.translated_count,
            "errors": errors,
            "case_number": config.case_number.clone().unwrap_or_default(),
            "examiner": config.examiner_name.clone().unwrap_or_default(),
            "agency": config.agency_name.clone().unwrap_or_default(),
            "size_bytes": std::fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0),
            "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        });
        println!("{json}");
        use std::io::Write;
        let _ = std::io::stdout().flush();
    }
    Ok(())
}

fn cmd_self_test(full: bool) -> Result<(), AugurError> {
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
        Err(AugurError::InvalidInput(
            "self-test reported one or more failures — see check list above".to_string(),
        ))
    }
}

fn cmd_timestamp(
    value: Option<i64>,
    input: Option<&std::path::Path>,
    format: Option<&str>,
) -> Result<(), AugurError> {
    let chosen = match format {
        Some(f) => Some(TimestampFormat::from_str(f).ok_or_else(|| {
            AugurError::InvalidInput(format!(
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
        return Err(AugurError::InvalidInput(
            "augur timestamp requires <value> or --input <file>".to_string(),
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
) -> Result<(), AugurError> {
    if setup {
        println_verify("MaxMind GeoLite2 setup");
        for line in GEOIP_DB_INSTRUCTIONS.split('\n') {
            println_verify(format!("  {line}"));
        }
        if let Some(p) = augur_core::geoip::configured_db_path() {
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
        return Err(AugurError::InvalidInput(
            "augur geoip requires <IP> or --input <file> or --setup".to_string(),
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

fn cmd_setup(token: &str) -> Result<(), AugurError> {
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
) -> Result<ResolvedSource, AugurError> {
    if !path.exists() {
        return Err(AugurError::InvalidInput(format!(
            "input file not found: {path:?}"
        )));
    }
    match detect_input_kind_robust(path) {
        PipelineInput::Video(p) => {
            let scratch = std::env::temp_dir().join("augur").join("video-scratch");
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
        PipelineInput::Subtitle(p) => {
            println_verify(format!("Input type: Subtitle ({:?})", p.extension()));
            resolve_subtitle_input(&p)
        }
        PipelineInput::Text(_) => {
            // detect_input_kind never returns Text from a path —
            // it falls back to Audio. This arm exists only to keep
            // the match exhaustive.
            Err(AugurError::InvalidInput(
                "text input must be passed via --text, not --input".to_string(),
            ))
        }
    }
}

/// Sprint Group B P2 — parse `.srt`/`.vtt` into a string the
/// classifier can chew on. The CLI keeps the parsed entries
/// alongside (read separately when `--output-srt` is set) via
/// [`load_subtitle_entries`]; here we just produce a
/// concatenation for classification purposes.
fn resolve_subtitle_input(path: &std::path::Path) -> Result<ResolvedSource, AugurError> {
    let entries = load_subtitle_entries(path)?;
    let text = entries
        .iter()
        .map(|e| e.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(ResolvedSource {
        text,
        upstream_lang: String::new(),
        upstream_confidence: 0.0,
        segments: None,
        kind_label: "subtitle",
        audio_path: None,
        audio_path_is_scratch: false,
    })
}

fn load_subtitle_entries(path: &std::path::Path) -> Result<Vec<SubtitleEntry>, AugurError> {
    let body = std::fs::read_to_string(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("srt") => parse_srt(&body),
        Some("vtt") => parse_vtt(&body),
        other => Err(AugurError::InvalidInput(format!(
            "subtitle input expects .srt/.vtt; got {other:?}"
        ))),
    }
}

fn resolve_pdf_input(
    pdf: &std::path::Path,
    ocr_lang: &str,
) -> Result<ResolvedSource, AugurError> {
    let scratch = std::env::temp_dir().join("augur").join("pdf-scratch");
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
) -> Result<ResolvedSource, AugurError> {
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
        augur_translate::iso_to_nllb(&translation.source_language).unwrap_or("?")
    ));
    println_verify(format!(
        "   Target language: {} ({})",
        translation.target_language,
        augur_translate::iso_to_nllb(&translation.target_language).unwrap_or("?")
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
) -> Result<(), AugurError> {
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
        return Err(AugurError::InvalidInput(format!(
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
    let mut subtitle_count = 0u32;
    let mut other_count = 0u32;
    for f in &files {
        match detect_input_kind_robust(f) {
            PipelineInput::Audio(_) => audio_count += 1,
            PipelineInput::Video(_) => video_count += 1,
            PipelineInput::Image(_) => image_count += 1,
            PipelineInput::Pdf(_) => pdf_count += 1,
            PipelineInput::Subtitle(_) => subtitle_count += 1,
            PipelineInput::Text(_) => other_count += 1,
        }
    }
    let _ = subtitle_count; // surfaced via the per-file dispatch below
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
        let kind = detect_input_kind_robust(file);
        let kind_label: &'static str = match &kind {
            PipelineInput::Audio(_) => "audio",
            PipelineInput::Video(_) => "video",
            PipelineInput::Image(_) => "image",
            PipelineInput::Pdf(_) => "pdf",
            PipelineInput::Subtitle(_) => "subtitle",
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
        .thread_name(|i| format!("augur-batch-{i}"))
        .build()
        .map_err(|e| AugurError::InvalidInput(format!("rayon pool: {e}")))?;

    let progress_path_ref = progress_path.as_deref();
    let target_ref = target;

    // Sprint 13 P2 — emit a `batch_file_start` NDJSON event
    // before each file when the global NDJSON_MODE is on.
    let ndjson_progress = NDJSON_MODE.load(std::sync::atomic::Ordering::Relaxed);
    let started_atomic = std::sync::atomic::AtomicU32::new(0);

    let mut results: Vec<BatchFileResult> = pool.install(|| {
        eligible
            .par_iter()
            .map(|(file, kind_label)| {
                if ndjson_progress {
                    let idx = started_atomic.fetch_add(1, Ordering::Relaxed) + 1;
                    let json = serde_json::json!({
                        "type": "batch_file_start",
                        "file": file.to_string_lossy(),
                        "input_type": kind_label,
                        "index": idx,
                        "total": total,
                    });
                    println!("{json}");
                }
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
                if ndjson_progress {
                    let json = serde_json::json!({
                        "type": "batch_file_done",
                        "file": row.file_path,
                        "input_type": kind_label,
                        "detected_language": row.detected_language,
                        "is_foreign": row.is_foreign,
                        "translated": row.translated_text.is_some(),
                        "error": row.error,
                        "processed": processed_atomic.load(Ordering::Relaxed),
                        "total": total,
                    });
                    println!("{json}");
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

    if ndjson_progress {
        let json = serde_json::json!({
            "type": "batch_complete",
            "total_files": total,
            "processed": processed,
            "foreign_files": foreign_count,
            "translated": translated_count,
            "errors": errors,
            "elapsed_seconds": elapsed,
            "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
        });
        println!("{json}");
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
) -> Result<(), AugurError> {
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
        .map_err(|e| AugurError::Translate(format!("progress JSON serialise: {e}")))?;
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
) -> Result<(), AugurError> {
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
                AugurError::Translate(format!("batch JSON serialise: {e}"))
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
                .map_err(|e| AugurError::Translate(format!("batch JSON serialise: {e}")))?
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

fn load_report_config(path: Option<&std::path::Path>) -> Result<ReportConfig, AugurError> {
    let path = match path {
        Some(p) => p.to_path_buf(),
        None => default_config_path()?,
    };
    if !path.exists() {
        return Ok(ReportConfig::blank());
    }
    ReportConfig::load(&path)
}

fn default_config_path() -> Result<PathBuf, AugurError> {
    let home = std::env::var("HOME").map_err(|_| {
        AugurError::InvalidInput("HOME not set; pass --config explicitly".to_string())
    })?;
    Ok(PathBuf::from(home).join(".augur_report.toml"))
}

fn cmd_config(action: ConfigAction) -> Result<(), AugurError> {
    match action {
        ConfigAction::Init { output, force } => {
            let path = match output {
                Some(p) => p,
                None => default_config_path()?,
            };
            if path.exists() && !force {
                return Err(AugurError::InvalidInput(format!(
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
                    return Err(AugurError::InvalidInput(format!(
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

fn walk_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) -> Result<(), AugurError> {
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
) -> Result<BatchFileResult, AugurError> {
    let resolved = match kind_label {
        "audio" | "video" => resolve_path_input(file, preset)?,
        "image" => resolve_image_input(file, ocr_lang)?,
        "pdf" => resolve_pdf_input(file, ocr_lang)?,
        "subtitle" => resolve_subtitle_input(file)?,
        other => {
            return Err(AugurError::InvalidInput(format!(
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
fn build_classifier(backend: ClassifierBackend) -> Result<LanguageClassifier, AugurError> {
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

fn build_fasttext() -> Result<LanguageClassifier, AugurError> {
    let mgr = ClassifierModelManager::with_xdg_cache()?;
    let path = mgr.ensure_lid_model()?;
    LanguageClassifier::load_fasttext(&path)
}

fn try_run_stt(input: &std::path::Path, preset: WhisperPreset) -> Result<SttResult, AugurError> {
    let options = TranscribeOptions {
        preset,
        ..TranscribeOptions::default()
    };
    try_run_stt_with(input, &options)
}

fn try_run_stt_with(
    input: &std::path::Path,
    options: &TranscribeOptions,
) -> Result<SttResult, AugurError> {
    // Validate the audio file BEFORE touching the network. An
    // examiner who types a wrong path should not accidentally
    // trigger a 150 MB / 290 MB / 3 GB Whisper download. This
    // keeps the egress truly "only when needed."
    if !input.exists() {
        return Err(AugurError::InvalidInput(format!(
            "audio file not found: {input:?}",
        )));
    }
    let mgr = WhisperModelManager::with_xdg_cache()?;
    let paths = mgr.ensure_whisper_model(options.preset)?;
    let mut engine = SttEngine::load(&paths, options.preset)?;
    engine.transcribe_with_options(input, options)
}

/// Small helper so every CLI line uses the `[AUGUR]` prefix
/// consistently. Writing to stdout via `println!` here is the one
/// permitted use in the workspace — this is the CLI's own output
/// surface (not a library emitting into a pipeline), and making
/// it a single named function means every CLI line flows through
/// one place a reviewer can audit.
/// Sprint 13 P1 — when NDJSON mode is active, every
/// `[AUGUR] …` line is suppressed so stdout carries the
/// machine-readable stream alone. Set by `cmd_translate_ndjson`
/// (and any future NDJSON-emitting subcommand) before any work
/// runs.
pub(crate) static NDJSON_MODE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn println_verify<S: AsRef<str>>(line: S) {
    if NDJSON_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        // Surface the message via `log::info!` so verbose dev
        // runs (`RUST_LOG=info`) still see it; just keep stdout
        // clean for the JSON consumer.
        log::info!("{}", line.as_ref());
        return;
    }
    println!("[AUGUR] {}", line.as_ref());
}

/// Sprint 10 — public alias used by sibling modules
/// (`install.rs`, future GUI shims). The original name is kept
/// to avoid touching ~100 internal call sites.
pub(crate) fn println_augur<S: AsRef<str>>(line: S) {
    println_verify(line)
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
            "augur-progress-test-{}-{}",
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
