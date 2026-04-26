//! NLLB-200 offline translation.
//!
//! # Sprint 2 backend choice — Python + transformers subprocess
//!
//! Candle (the pure-Rust ML framework selected for Whisper in P1)
//! does not yet ship NLLB-200's MBart-style encoder-decoder. Per
//! the Sprint 2 decision rule, AUGUR ships Option B: a Python
//! subprocess driving `transformers` for inference. The model is
//! `facebook/nllb-200-distilled-600M` (~2.4 GB) — cached locally
//! by Hugging Face's transformers library so post-first-run
//! invocations are fully offline.
//!
//! # Offline invariant
//!
//! transformers downloads NLLB weights on first use. We point its
//! cache at `~/.cache/augur/models/nllb/` via `AUGUR_HF_CACHE`
//! / `HF_HOME` so the egress is auditable. No source text is sent
//! to any server: NLLB inference runs locally in the spawned
//! Python process.
//!
//! # Forensic safety — the advisory notice
//!
//! Every [`TranslationResult`] carries `is_machine_translation =
//! true` and a non-empty `advisory_notice`. Machine translation in
//! a forensic / legal context must always be labeled as such; the
//! examiner cannot suppress this with a flag. See
//! [`MACHINE_TRANSLATION_NOTICE`].

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use augur_core::AugurError;

pub mod seamless;
pub use seamless::{
    detect_code_switching, select_engine, CodeSwitchAnalysis, SeamlessEngine,
    TranslationEngineKind, DEFAULT_SEAMLESS_MODEL, SEAMLESS_ADVISORY,
};

/// Sprint-1 sentinel. Retained so callers/tests that grep for it
/// have a single removal point. Sprint 2 wires real translation
/// via [`TranslationEngine`]; this sentinel is no longer emitted
/// from the default code path.
pub const TRANSLATION_STUB: &str = "STUB — NLLB-200 integration coming in Sprint 2";

/// The mandatory machine-translation advisory. Surfaced in every
/// [`TranslationResult`] and printed by the CLI on every translate
/// run. Forensic users presenting AUGUR output in court must
/// know this is automated translation, not certified human work.
pub const MACHINE_TRANSLATION_NOTICE: &str =
    "Machine translation — verify with a certified human translator for legal proceedings.";

/// Sprint 6 P4 — appended to the advisory notice when the source
/// language is Persian/Farsi (`fa`). Both whichlang and
/// `lid.176.ftz` confuse Pashto with Farsi at the model level
/// (Sprint 5 P1 probe); examiners working in contexts where
/// Pashto is plausible must verify with a human linguist. See
/// `docs/LANGUAGE_LIMITATIONS.md` for the full rationale.
pub const FARSI_PASHTO_ADVISORY: &str =
    "Note: Automated tools may confuse Farsi (fa) with Pashto (ps). \
     Verify language identification if this is critical evidence.";

/// The default NLLB model id. The 600M distilled variant runs on
/// CPU; the 1.3B variant is higher quality but 5+ GB.
pub const DEFAULT_NLLB_MODEL: &str = "facebook/nllb-200-distilled-600M";

/// First-run Hugging Face download URL for the default model.
/// Named const so `grep NLLB_MODEL_URL_` enumerates every egress
/// site, mirroring the WHISPER_MODEL_URL_ pattern in `augur-stt`.
pub const NLLB_MODEL_URL_DISTILLED_600M: &str =
    "https://huggingface.co/facebook/nllb-200-distilled-600M";

/// One translated chunk of audio/video, with timestamps preserved
/// from the upstream STT segment.
#[derive(Debug, Clone, Serialize)]
pub struct TranslatedSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source_text: String,
    pub translated_text: String,
}

/// Result of a single translation call.
#[derive(Debug, Clone)]
pub struct TranslationResult {
    pub source_text: String,
    pub translated_text: String,
    /// ISO 639-1 code (e.g. "ar").
    pub source_language: String,
    /// ISO 639-1 code (e.g. "en").
    pub target_language: String,
    pub confidence: f32,
    /// Hugging Face model id, e.g. `"facebook/nllb-200-distilled-600M"`.
    pub model: String,
    /// Always `true`. Forensic safety: machine translation must
    /// always be labeled.
    pub is_machine_translation: bool,
    /// Human-readable advisory notice. Always non-empty.
    pub advisory_notice: String,
    /// Per-segment translations when the source came from STT
    /// (audio / video). `None` for plain text or OCR inputs that
    /// have no upstream timestamps.
    pub segments: Option<Vec<TranslatedSegment>>,
}

/// Map ISO 639-1 to NLLB's BCP-47-ish language codes.
///
/// Forensically important languages (Farsi, Pashto, Urdu) are
/// explicitly listed. Extending this map is a deliberate change.
pub fn iso_to_nllb(iso: &str) -> Result<&'static str, AugurError> {
    Ok(match iso {
        "ar" => "ara_Arab",
        "zh" => "zho_Hans",
        "ru" => "rus_Cyrl",
        "es" => "spa_Latn",
        "fr" => "fra_Latn",
        "de" => "deu_Latn",
        "fa" => "pes_Arab",
        "ps" => "pbt_Arab",
        "ur" => "urd_Arab",
        "ko" => "kor_Hang",
        "ja" => "jpn_Jpan",
        "vi" => "vie_Latn",
        "tr" => "tur_Latn",
        "pt" => "por_Latn",
        "it" => "ita_Latn",
        "nl" => "nld_Latn",
        "he" => "heb_Hebr",
        "hi" => "hin_Deva",
        "id" => "ind_Latn",
        "pl" => "pol_Latn",
        "uk" => "ukr_Cyrl",
        "en" => "eng_Latn",
        other => {
            return Err(AugurError::Translate(format!(
                "unsupported language code {other:?} — extend iso_to_nllb to add it"
            )));
        }
    })
}

#[derive(Serialize)]
struct ScriptRequest<'a> {
    text: &'a str,
    src: &'a str,
    tgt: &'a str,
    model: &'a str,
}

#[derive(Serialize)]
struct Ct2ScriptRequest<'a> {
    text: &'a str,
    src: &'a str,
    tgt: &'a str,
    model: &'a str,
    ct2_dir: &'a str,
}

/// Translation backend.
///
/// `Auto` (default) prefers the ctranslate2 worker when its
/// converted model exists at `<hf_cache>/ct2/`; otherwise it falls
/// back to the transformers worker. `Ctranslate2` forces ct2 (and
/// triggers a one-time conversion on first use). `Transformers`
/// forces the Sprint 2 transformers backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backend {
    #[default]
    Auto,
    Transformers,
    Ctranslate2,
}

#[derive(Deserialize)]
struct ScriptResponse {
    text: Option<String>,
    error: Option<String>,
}

const NLLB_SCRIPT: &str = include_str!("script.py");
const NLLB_SCRIPT_CT2: &str = include_str!("script_ct2.py");

/// Translation engine. Holds the model id + cache root + python
/// command; each [`TranslationEngine::translate`] call spawns a
/// short-lived python subprocess against one of the bundled NLLB
/// worker scripts (transformers or ctranslate2).
#[derive(Debug, Clone)]
pub struct TranslationEngine {
    pub model: String,
    pub python_cmd: String,
    pub hf_cache: Option<PathBuf>,
    pub backend: Backend,
}

impl TranslationEngine {
    /// Default engine: `facebook/nllb-200-distilled-600M`, `python3`,
    /// HF cache under `~/.cache/augur/models/nllb/`, backend `Auto`.
    pub fn with_xdg_cache() -> Result<Self, AugurError> {
        let home = std::env::var("HOME").map_err(|_| {
            AugurError::Translate("HOME not set; pass a cache dir explicitly".to_string())
        })?;
        Ok(Self {
            model: DEFAULT_NLLB_MODEL.to_string(),
            python_cmd: "python3".to_string(),
            hf_cache: Some(PathBuf::from(home).join(".cache/augur/models/nllb")),
            backend: Backend::default(),
        })
    }

    /// CT2 model directory: `<hf_cache>/ct2/`. Returns `None` when
    /// no cache root is configured.
    pub fn ct2_dir(&self) -> Option<PathBuf> {
        self.hf_cache.as_ref().map(|p| p.join("ct2"))
    }

    /// Decide which backend to actually use this call. `Auto`
    /// prefers ct2 when its converted model exists on disk;
    /// otherwise transformers. The other variants pass through.
    fn pick_backend(&self) -> Backend {
        match self.backend {
            Backend::Transformers => Backend::Transformers,
            Backend::Ctranslate2 => Backend::Ctranslate2,
            Backend::Auto => match self.ct2_dir() {
                Some(p) if p.exists() && p.is_dir() => Backend::Ctranslate2,
                _ => Backend::Transformers,
            },
        }
    }

    /// Run NLLB-200 translation and produce a [`TranslationResult`].
    /// The returned struct always carries the mandatory machine-
    /// translation advisory notice.
    pub fn translate(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> Result<TranslationResult, AugurError> {
        if source_text.trim().is_empty() {
            return Ok(self.advisory(
                source_text,
                "",
                source_language,
                target_language,
                0.0,
            ));
        }

        let src = iso_to_nllb(source_language)?;
        let tgt = iso_to_nllb(target_language)?;

        if let Some(cache) = &self.hf_cache {
            std::fs::create_dir_all(cache)?;
        }

        let chosen = self.pick_backend();
        debug!("augur-translate: backend selection → {chosen:?}");
        let translated = match chosen {
            Backend::Ctranslate2 => match self.run_ct2(source_text, src, tgt) {
                Ok(t) => t,
                Err(e) if self.backend == Backend::Auto => {
                    warn!(
                        "augur-translate: ctranslate2 backend failed ({e}); \
                         falling back to transformers."
                    );
                    self.run_transformers(source_text, src, tgt)?
                }
                Err(e) => return Err(e),
            },
            Backend::Transformers => self.run_transformers(source_text, src, tgt)?,
            // Auto only resolves to Transformers/Ct2 above; this
            // arm is unreachable but keeps the match exhaustive.
            Backend::Auto => self.run_transformers(source_text, src, tgt)?,
        };
        info!(
            "augur-translate: NLLB {src}→{tgt} produced {} chars",
            translated.len()
        );

        // NLLB does not expose a per-sentence confidence; we report
        // a fixed value tagged as machine-derived. The advisory
        // notice carries the real semantic.
        Ok(self.advisory(
            source_text,
            &translated,
            source_language,
            target_language,
            0.85,
        ))
    }

    fn run_transformers(&self, text: &str, src: &str, tgt: &str) -> Result<String, AugurError> {
        warn!(
            "augur-translate: invoking NLLB-200 ({src} → {tgt}) via transformers — \
             one-time HF model download on first run, all inference local."
        );
        let req = ScriptRequest {
            text,
            src,
            tgt,
            model: &self.model,
        };
        let req_json = serde_json::to_string(&req)
            .map_err(|e| AugurError::Translate(format!("request serialise: {e}")))?;
        self.run_python_worker(NLLB_SCRIPT, &req_json, "transformers")
    }

    fn run_ct2(&self, text: &str, src: &str, tgt: &str) -> Result<String, AugurError> {
        let ct2_dir = self.ct2_dir().ok_or_else(|| {
            AugurError::Translate(
                "ctranslate2 backend requires a configured hf_cache (got None)".to_string(),
            )
        })?;
        warn!(
            "augur-translate: invoking NLLB-200 ({src} → {tgt}) via ctranslate2 \
             (model dir {ct2_dir:?}) — one-time conversion on first run, all inference local."
        );
        if let Some(parent) = ct2_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let req = Ct2ScriptRequest {
            text,
            src,
            tgt,
            model: &self.model,
            ct2_dir: ct2_dir.to_str().ok_or_else(|| {
                AugurError::Translate(format!("ct2_dir not UTF-8: {ct2_dir:?}"))
            })?,
        };
        let req_json = serde_json::to_string(&req)
            .map_err(|e| AugurError::Translate(format!("request serialise: {e}")))?;
        self.run_python_worker(NLLB_SCRIPT_CT2, &req_json, "ctranslate2")
    }

    fn run_python_worker(
        &self,
        script: &str,
        req_json: &str,
        backend_name: &str,
    ) -> Result<String, AugurError> {
        let mut cmd = Command::new(&self.python_cmd);
        cmd.arg("-c").arg(script);
        if let Some(cache) = &self.hf_cache {
            cmd.env("AUGUR_HF_CACHE", cache);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AugurError::Translate(format!(
                "failed to spawn {} for {backend_name}: {e}. \
                 Install Python 3 and the required packages \
                 (`pip3 install --user transformers torch sentencepiece` \
                 plus `ctranslate2` for the ct2 backend) to enable NLLB-200.",
                self.python_cmd
            ))
        })?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(req_json.as_bytes())
                .map_err(|e| AugurError::Translate(format!("write stdin: {e}")))?;
        } else {
            return Err(AugurError::Translate(
                "child process stdin not piped".to_string(),
            ));
        }
        let output = child
            .wait_with_output()
            .map_err(|e| AugurError::Translate(format!("child wait: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        debug!(
            "augur-translate: {backend_name} exit={:?} stderr_bytes={} stdout_bytes={}",
            output.status.code(),
            stderr.len(),
            stdout.len()
        );

        let resp: ScriptResponse = serde_json::from_str(stdout.trim()).map_err(|e| {
            AugurError::Translate(format!(
                "could not parse {backend_name} response as JSON: {e}; \
                 stdout={stdout:?}; stderr={stderr:?}"
            ))
        })?;
        if let Some(err) = resp.error {
            return Err(AugurError::Translate(format!(
                "{backend_name} worker error: {err}; stderr={stderr}"
            )));
        }
        resp.text.ok_or_else(|| {
            AugurError::Translate(format!(
                "{backend_name} worker returned no text and no error; stdout={stdout:?}"
            ))
        })
    }

    fn advisory(
        &self,
        source_text: &str,
        translated_text: &str,
        source_language: &str,
        target_language: &str,
        confidence: f32,
    ) -> TranslationResult {
        let mut notice = MACHINE_TRANSLATION_NOTICE.to_string();
        // Sprint 6 P4 — Pashto/Persian disambiguation advisory.
        // Both `whichlang` and `lid.176.ftz` confuse Pashto with
        // Farsi at the model level (Sprint 5 P1 probe). Whenever
        // the source is reported as Farsi, append a one-line
        // disambiguation hint *in addition to* the mandatory
        // machine-translation advisory — never replacing it.
        if source_language == "fa" {
            notice.push(' ');
            notice.push_str(FARSI_PASHTO_ADVISORY);
        }
        TranslationResult {
            source_text: source_text.to_string(),
            translated_text: translated_text.to_string(),
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
            confidence,
            model: self.model.clone(),
            is_machine_translation: true,
            advisory_notice: notice,
            segments: None,
        }
    }

    /// Translate a list of timestamped STT segments, producing both
    /// a full concatenated translation and per-segment timestamped
    /// translations. Each segment is translated independently — this
    /// is what gives examiners a translated transcript that lines up
    /// in time with the original audio.
    ///
    /// Empty-text segments are skipped (they preserve their
    /// timestamps in the resulting list with empty `translated_text`).
    pub fn translate_segments(
        &self,
        segments: &[(u64, u64, String)],
        source_language: &str,
        target_language: &str,
    ) -> Result<TranslationResult, AugurError> {
        let mut translated: Vec<TranslatedSegment> = Vec::with_capacity(segments.len());
        for (start_ms, end_ms, text) in segments {
            if text.trim().is_empty() {
                translated.push(TranslatedSegment {
                    start_ms: *start_ms,
                    end_ms: *end_ms,
                    source_text: text.clone(),
                    translated_text: String::new(),
                });
                continue;
            }
            let r = self.translate(text, source_language, target_language)?;
            translated.push(TranslatedSegment {
                start_ms: *start_ms,
                end_ms: *end_ms,
                source_text: text.clone(),
                translated_text: r.translated_text,
            });
        }
        let full_source = segments
            .iter()
            .map(|(_, _, t)| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let full_translated = translated
            .iter()
            .map(|t| t.translated_text.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let mut out = self.advisory(
            &full_source,
            &full_translated,
            source_language,
            target_language,
            0.85,
        );
        out.segments = Some(translated);
        Ok(out)
    }
}

/// Sprint-1 stub kept for transitional callers. Returns a structured
/// `TranslationResult` carrying the sentinel as the translated text
/// so the advisory invariant still holds. Prefer
/// [`TranslationEngine::translate`].
pub fn translate_stub(
    source_text: &str,
    source_language: &str,
    target_language: &str,
) -> Result<String, AugurError> {
    let _ = (source_text, source_language, target_language);
    Ok(TRANSLATION_STUB.to_string())
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_to_nllb_maps_forensic_languages_correctly() {
        assert_eq!(iso_to_nllb("ar").unwrap(), "ara_Arab");
        assert_eq!(iso_to_nllb("fa").unwrap(), "pes_Arab");
        assert_eq!(iso_to_nllb("ps").unwrap(), "pbt_Arab");
        assert_eq!(iso_to_nllb("ur").unwrap(), "urd_Arab");
        assert_eq!(iso_to_nllb("en").unwrap(), "eng_Latn");
    }

    #[test]
    fn unsupported_language_returns_clear_error() {
        match iso_to_nllb("xx") {
            Err(AugurError::Translate(msg)) => {
                assert!(msg.contains("unsupported"), "msg: {msg}");
            }
            other => panic!("expected Translate error, got {other:?}"),
        }
    }

    #[test]
    fn machine_translation_advisory_always_present() {
        // Load-bearing invariant: every TranslationResult emitted
        // via the public API must carry is_machine_translation =
        // true and a non-empty advisory_notice. A tool that surfaces
        // MT output without labeling it is dangerous in a legal
        // context; this test fails the build if the invariant slips.
        let engine = TranslationEngine {
            model: DEFAULT_NLLB_MODEL.to_string(),
            python_cmd: "python3".to_string(),
            hf_cache: None,
            backend: Backend::Auto,
        };
        // Use the empty-input fast path — it never spawns python,
        // so the test is hermetic but still exercises the result-
        // builder path that every translation call goes through.
        let r = engine.translate("", "ar", "en").expect("empty path");
        assert!(r.is_machine_translation, "MT flag must be true");
        assert!(
            !r.advisory_notice.is_empty(),
            "advisory_notice must be non-empty — forensic invariant"
        );
        assert_eq!(r.advisory_notice, MACHINE_TRANSLATION_NOTICE);
        assert_eq!(r.source_language, "ar");
        assert_eq!(r.target_language, "en");
        assert_eq!(r.model, DEFAULT_NLLB_MODEL);
    }

    #[test]
    fn farsi_detection_includes_disambiguation_advisory() {
        // Sprint 6 P4 acceptance test. When source_language is
        // "fa", the advisory must include the Pashto/Persian
        // disambiguation hint AND must still carry the mandatory
        // machine-translation notice (the language advisory
        // augments, never replaces it).
        let engine = TranslationEngine {
            model: DEFAULT_NLLB_MODEL.into(),
            python_cmd: "python3".into(),
            hf_cache: None,
            backend: Backend::Auto,
        };
        let r = engine.advisory("ساده", "simple", "fa", "en", 0.8);
        assert!(r.is_machine_translation);
        assert!(
            r.advisory_notice.contains("Machine translation"),
            "MT notice missing: {}",
            r.advisory_notice
        );
        assert!(
            r.advisory_notice.contains("Pashto"),
            "fa-disambiguation missing: {}",
            r.advisory_notice
        );
        assert!(
            r.advisory_notice.contains("Farsi"),
            "fa-disambiguation missing Farsi: {}",
            r.advisory_notice
        );
    }

    #[test]
    fn language_limitations_doc_exists() {
        // Sprint 6 P4 acceptance. Path is relative to this crate's
        // manifest dir → workspace root → docs/.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/LANGUAGE_LIMITATIONS.md");
        assert!(
            path.exists(),
            "docs/LANGUAGE_LIMITATIONS.md is missing at {path:?}"
        );
        let content = std::fs::read_to_string(&path).expect("read doc");
        assert!(!content.is_empty());
        // Spot-check that it actually documents the fa/ps
        // confusion — the whole point of this file.
        assert!(content.contains("Pashto"));
        assert!(content.contains("Farsi"));
    }

    #[test]
    fn non_farsi_source_does_not_get_disambiguation_advisory() {
        let engine = TranslationEngine {
            model: DEFAULT_NLLB_MODEL.into(),
            python_cmd: "python3".into(),
            hf_cache: None,
            backend: Backend::Auto,
        };
        let r = engine.advisory("مرحبا", "Hello", "ar", "en", 0.9);
        assert!(r.is_machine_translation);
        assert!(r.advisory_notice.contains("Machine translation"));
        // Arabic source must NOT trigger the fa/ps disambiguation
        // — that note is specific to detected Farsi.
        assert!(
            !r.advisory_notice.contains("Pashto"),
            "fa-disambiguation leaked to non-fa source: {}",
            r.advisory_notice
        );
    }

    #[test]
    fn translation_result_fields_round_trip() {
        let r = TranslationResult {
            source_text: "مرحبا".into(),
            translated_text: "Hello".into(),
            source_language: "ar".into(),
            target_language: "en".into(),
            confidence: 0.9,
            model: DEFAULT_NLLB_MODEL.into(),
            is_machine_translation: true,
            advisory_notice: MACHINE_TRANSLATION_NOTICE.into(),
            segments: None,
        };
        assert!(r.is_machine_translation);
        assert!(!r.advisory_notice.is_empty());
    }

    #[test]
    fn translated_segments_preserve_timestamps_for_empty_inputs() {
        // The empty-input fast path is hermetic: it never spawns
        // python and so is safe to call in unit tests. Verifies
        // that timestamp fields round-trip through the segment
        // list — this is the load-bearing invariant for the video
        // pipeline (examiners must know WHEN each phrase was said).
        let engine = TranslationEngine {
            model: DEFAULT_NLLB_MODEL.into(),
            python_cmd: "python3".into(),
            hf_cache: None,
            backend: Backend::Auto,
        };
        let segs = vec![
            (0u64, 1_500u64, String::new()),
            (1_500u64, 3_000u64, "   ".into()),
        ];
        let r = engine
            .translate_segments(&segs, "ar", "en")
            .expect("empty-segment fast path");
        assert!(r.is_machine_translation);
        assert!(!r.advisory_notice.is_empty());
        let out = r.segments.expect("segments populated");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].start_ms, 0);
        assert_eq!(out[0].end_ms, 1_500);
        assert_eq!(out[1].start_ms, 1_500);
        assert_eq!(out[1].end_ms, 3_000);
    }

    #[test]
    fn backend_auto_falls_back_to_transformers_when_ct2_dir_absent() {
        // Auto backend prefers ct2 when its directory exists; with
        // a fresh temp cache (no ct2 subdir), it must select
        // transformers — that is the documented graceful-fallback
        // behavior the Sprint 3 spec calls out.
        let tmp = std::env::temp_dir()
            .join(format!("augur-translate-backend-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let engine = TranslationEngine {
            model: DEFAULT_NLLB_MODEL.into(),
            python_cmd: "python3".into(),
            hf_cache: Some(tmp.clone()),
            backend: Backend::Auto,
        };
        assert_eq!(engine.pick_backend(), Backend::Transformers);

        // Once we materialise the ct2 directory, Auto picks ct2.
        std::fs::create_dir_all(tmp.join("ct2")).expect("mkdir ct2");
        assert_eq!(engine.pick_backend(), Backend::Ctranslate2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn explicit_backends_override_auto() {
        let engine = TranslationEngine {
            model: DEFAULT_NLLB_MODEL.into(),
            python_cmd: "python3".into(),
            hf_cache: None,
            backend: Backend::Transformers,
        };
        assert_eq!(engine.pick_backend(), Backend::Transformers);
        let engine = TranslationEngine {
            model: DEFAULT_NLLB_MODEL.into(),
            python_cmd: "python3".into(),
            hf_cache: None,
            backend: Backend::Ctranslate2,
        };
        assert_eq!(engine.pick_backend(), Backend::Ctranslate2);
    }

    #[test]
    fn stub_kept_for_transition() {
        assert_eq!(
            translate_stub("x", "ar", "en").unwrap(),
            TRANSLATION_STUB
        );
    }
}
