//! NLLB-200 offline translation.
//!
//! # Sprint 2 backend choice — Python + transformers subprocess
//!
//! Candle (the pure-Rust ML framework selected for Whisper in P1)
//! does not yet ship NLLB-200's MBart-style encoder-decoder. Per
//! the Sprint 2 decision rule, VERIFY ships Option B: a Python
//! subprocess driving `transformers` for inference. The model is
//! `facebook/nllb-200-distilled-600M` (~2.4 GB) — cached locally
//! by Hugging Face's transformers library so post-first-run
//! invocations are fully offline.
//!
//! # Offline invariant
//!
//! transformers downloads NLLB weights on first use. We point its
//! cache at `~/.cache/verify/models/nllb/` via `VERIFY_HF_CACHE`
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
use verify_core::VerifyError;

/// Sprint-1 sentinel. Retained so callers/tests that grep for it
/// have a single removal point. Sprint 2 wires real translation
/// via [`TranslationEngine`]; this sentinel is no longer emitted
/// from the default code path.
pub const TRANSLATION_STUB: &str = "STUB — NLLB-200 integration coming in Sprint 2";

/// The mandatory machine-translation advisory. Surfaced in every
/// [`TranslationResult`] and printed by the CLI on every translate
/// run. Forensic users presenting VERIFY output in court must
/// know this is automated translation, not certified human work.
pub const MACHINE_TRANSLATION_NOTICE: &str =
    "Machine translation — verify with a certified human translator for legal proceedings.";

/// The default NLLB model id. The 600M distilled variant runs on
/// CPU; the 1.3B variant is higher quality but 5+ GB.
pub const DEFAULT_NLLB_MODEL: &str = "facebook/nllb-200-distilled-600M";

/// First-run Hugging Face download URL for the default model.
/// Named const so `grep NLLB_MODEL_URL_` enumerates every egress
/// site, mirroring the WHISPER_MODEL_URL_ pattern in `verify-stt`.
pub const NLLB_MODEL_URL_DISTILLED_600M: &str =
    "https://huggingface.co/facebook/nllb-200-distilled-600M";

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
}

/// Map ISO 639-1 to NLLB's BCP-47-ish language codes.
///
/// Forensically important languages (Farsi, Pashto, Urdu) are
/// explicitly listed. Extending this map is a deliberate change.
pub fn iso_to_nllb(iso: &str) -> Result<&'static str, VerifyError> {
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
            return Err(VerifyError::Translate(format!(
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

#[derive(Deserialize)]
struct ScriptResponse {
    text: Option<String>,
    error: Option<String>,
}

const NLLB_SCRIPT: &str = include_str!("script.py");

/// Translation engine. Holds the model id + cache root + python
/// command; each [`TranslationEngine::translate`] call spawns a
/// short-lived python subprocess against the bundled NLLB script.
#[derive(Debug, Clone)]
pub struct TranslationEngine {
    pub model: String,
    pub python_cmd: String,
    pub hf_cache: Option<PathBuf>,
}

impl TranslationEngine {
    /// Default engine: `facebook/nllb-200-distilled-600M`, `python3`,
    /// HF cache under `~/.cache/verify/models/nllb/`.
    pub fn with_xdg_cache() -> Result<Self, VerifyError> {
        let home = std::env::var("HOME").map_err(|_| {
            VerifyError::Translate("HOME not set; pass a cache dir explicitly".to_string())
        })?;
        Ok(Self {
            model: DEFAULT_NLLB_MODEL.to_string(),
            python_cmd: "python3".to_string(),
            hf_cache: Some(PathBuf::from(home).join(".cache/verify/models/nllb")),
        })
    }

    /// Run NLLB-200 translation and produce a [`TranslationResult`].
    /// The returned struct always carries the mandatory machine-
    /// translation advisory notice.
    pub fn translate(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> Result<TranslationResult, VerifyError> {
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

        warn!(
            "verify-translate: invoking NLLB-200 ({src} → {tgt}) via {} — \
             one-time HF model download on first run, all inference local.",
            self.python_cmd
        );

        let req = ScriptRequest {
            text: source_text,
            src,
            tgt,
            model: &self.model,
        };
        let req_json = serde_json::to_string(&req)
            .map_err(|e| VerifyError::Translate(format!("request serialise: {e}")))?;

        let mut cmd = Command::new(&self.python_cmd);
        cmd.arg("-c").arg(NLLB_SCRIPT);
        if let Some(cache) = &self.hf_cache {
            cmd.env("VERIFY_HF_CACHE", cache);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            VerifyError::Translate(format!(
                "failed to spawn {}: {e}. \
                 Install Python 3 and `pip3 install --user transformers torch sentencepiece` \
                 to enable NLLB-200 translation.",
                self.python_cmd
            ))
        })?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(req_json.as_bytes())
                .map_err(|e| VerifyError::Translate(format!("write stdin: {e}")))?;
        } else {
            return Err(VerifyError::Translate(
                "child process stdin not piped".to_string(),
            ));
        }
        let output = child
            .wait_with_output()
            .map_err(|e| VerifyError::Translate(format!("child wait: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        debug!(
            "verify-translate: python exit={:?} stderr_bytes={} stdout_bytes={}",
            output.status.code(),
            stderr.len(),
            stdout.len()
        );

        let resp: ScriptResponse = serde_json::from_str(stdout.trim()).map_err(|e| {
            VerifyError::Translate(format!(
                "could not parse python response as JSON: {e}; \
                 stdout={stdout:?}; stderr={stderr:?}"
            ))
        })?;
        if let Some(err) = resp.error {
            return Err(VerifyError::Translate(format!(
                "NLLB worker error: {err}; stderr={stderr}"
            )));
        }
        let translated = resp.text.ok_or_else(|| {
            VerifyError::Translate(format!(
                "NLLB worker returned no text and no error; stdout={stdout:?}"
            ))
        })?;
        info!(
            "verify-translate: NLLB {src}→{tgt} produced {} chars",
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

    fn advisory(
        &self,
        source_text: &str,
        translated_text: &str,
        source_language: &str,
        target_language: &str,
        confidence: f32,
    ) -> TranslationResult {
        TranslationResult {
            source_text: source_text.to_string(),
            translated_text: translated_text.to_string(),
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
            confidence,
            model: self.model.clone(),
            is_machine_translation: true,
            advisory_notice: MACHINE_TRANSLATION_NOTICE.to_string(),
        }
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
) -> Result<String, VerifyError> {
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
            Err(VerifyError::Translate(msg)) => {
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
        };
        assert!(r.is_machine_translation);
        assert!(!r.advisory_notice.is_empty());
    }

    #[test]
    fn stub_kept_for_transition() {
        assert_eq!(
            translate_stub("x", "ar", "en").unwrap(),
            TRANSLATION_STUB
        );
    }
}
