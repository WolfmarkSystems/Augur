//! Sprint 10 P3 — SeamlessM4T integration.
//!
//! SeamlessM4T (Meta AI) handles code-switched input — text where
//! a speaker switches languages mid-sentence — far better than the
//! single-language NLLB-200 path. It is also a unified model:
//! given audio + a target language it produces a translation in one
//! inference pass without a separate STT step.
//!
//! The engine talks to the model via a bundled python subprocess
//! (`seamless_worker.py`) — same shape as the NLLB worker so the
//! offline-first contract holds. The model is loaded from the HF
//! cache populated by `augur install full`.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use augur_core::AugurError;
use serde::Deserialize;

use crate::{TranslationResult, MACHINE_TRANSLATION_NOTICE};

/// Bundled python worker script. Same pattern as the NLLB workers.
const SEAMLESS_SCRIPT: &str = include_str!("seamless_worker.py");

pub const DEFAULT_SEAMLESS_MODEL: &str = "facebook/seamless-m4t-medium";

/// Sprint 10 P3 — additional advisory line that fires alongside
/// (never replacing) the mandatory machine-translation advisory
/// whenever a translation is produced by SeamlessM4T. Discloses
/// the model origin so the examiner record is complete.
pub const SEAMLESS_ADVISORY: &str = "Translation produced by SeamlessM4T (Meta AI, open weights). \
     Code-switching may have been detected — content may contain multiple languages. \
     Verify with a certified human translator before using in legal proceedings.";

/// User-facing engine selector for `augur translate --engine …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranslationEngineKind {
    /// Default — NLLB-200 (transformers / ctranslate2).
    Nllb,
    /// SeamlessM4T — best for code-switched content; requires the
    /// `seamless-m4t-medium` model installed.
    Seamless,
    /// Auto — defer to [`select_engine`].
    Auto,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CodeSwitchAnalysis {
    pub is_code_switched: bool,
    pub confidence: f32,
    pub languages_detected: Vec<String>,
    pub switch_count: u32,
}

/// Sprint 10 P3 — heuristic code-switch detector. Counts script
/// runs (Latin / Arabic / Cyrillic / CJK) and the number of
/// transitions between them. Two or more distinct scripts AND at
/// least one transition flag the input as code-switched.
pub fn detect_code_switching(text: &str) -> CodeSwitchAnalysis {
    let mut scripts: Vec<&'static str> = Vec::new();
    let mut switches: u32 = 0;
    let mut last: Option<&'static str> = None;
    for ch in text.chars() {
        let s = char_script(ch);
        if s == "other" {
            continue;
        }
        if !scripts.contains(&s) {
            scripts.push(s);
        }
        match last {
            Some(prev) if prev != s => switches += 1,
            _ => {}
        }
        last = Some(s);
    }
    let is_cs = scripts.len() >= 2 && switches >= 1;
    let confidence = if !is_cs {
        0.0
    } else {
        // More switches relative to text length → higher
        // confidence, capped at 0.95.
        let total_alpha = text.chars().filter(|c| c.is_alphabetic()).count() as f32;
        let ratio = if total_alpha == 0.0 {
            0.0
        } else {
            (switches as f32 / total_alpha).min(0.5) * 1.9
        };
        ratio.clamp(0.3, 0.95)
    };
    CodeSwitchAnalysis {
        is_code_switched: is_cs,
        confidence,
        languages_detected: scripts.iter().map(|s| (*s).to_string()).collect(),
        switch_count: switches,
    }
}

fn char_script(c: char) -> &'static str {
    if c.is_ascii_alphabetic() {
        "latin"
    } else {
        let cp = c as u32;
        if (0x0600..=0x06FF).contains(&cp) || (0x0750..=0x077F).contains(&cp) {
            "arabic"
        } else if (0x0400..=0x04FF).contains(&cp) {
            "cyrillic"
        } else if (0x4E00..=0x9FFF).contains(&cp) {
            "cjk"
        } else if (0x0590..=0x05FF).contains(&cp) {
            "hebrew"
        } else if (0x0900..=0x097F).contains(&cp) {
            "devanagari"
        } else if c.is_alphabetic() {
            // Other Latin-extended (accented chars) classify as
            // latin so "café" doesn't read as a script switch.
            "latin"
        } else {
            "other"
        }
    }
}

/// Sprint 10 P3 — select the best engine for the given input.
///
/// 1. Seamless is unavailable on disk → Nllb.
/// 2. Classification confidence is low (< 0.75) → Seamless
///    (likely code-switched / mixed-language).
/// 3. Detected primary language is Arabic AND the text has
///    significant Latin content → Seamless (Arabic/English
///    code-switching is the high-value case).
/// 4. Otherwise → Nllb.
pub fn select_engine(
    text: &str,
    detected_language: &str,
    classification_confidence: f32,
    seamless_installed: bool,
) -> TranslationEngineKind {
    if !seamless_installed {
        return TranslationEngineKind::Nllb;
    }
    if classification_confidence < 0.75 {
        return TranslationEngineKind::Seamless;
    }
    if detected_language == "ar" && latin_ratio(text) > 0.15 {
        return TranslationEngineKind::Seamless;
    }
    TranslationEngineKind::Nllb
}

fn latin_ratio(text: &str) -> f32 {
    let total = text.chars().filter(|c| c.is_alphabetic()).count();
    if total == 0 {
        return 0.0;
    }
    let latin = text
        .chars()
        .filter(|c| c.is_alphabetic() && char_script(*c) == "latin")
        .count();
    latin as f32 / total as f32
}

#[derive(Debug, Clone)]
pub struct SeamlessEngine {
    pub model: String,
    pub python_cmd: String,
    pub hf_cache: Option<PathBuf>,
}

impl SeamlessEngine {
    pub fn with_xdg_cache() -> Result<Self, AugurError> {
        let home = std::env::var("HOME").map_err(|_| {
            AugurError::Translate("HOME not set; pass a cache dir explicitly".to_string())
        })?;
        Ok(Self {
            model: DEFAULT_SEAMLESS_MODEL.to_string(),
            python_cmd: "python3".to_string(),
            hf_cache: Some(PathBuf::from(home).join(".cache/augur/models/seamless")),
        })
    }

    /// Translate text through SeamlessM4T. Always wraps the
    /// output in the mandatory MT advisory + the SeamlessM4T-
    /// specific origin disclosure.
    pub fn translate(
        &self,
        source_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> Result<TranslationResult, AugurError> {
        if source_text.trim().is_empty() {
            return Ok(self.advisory(source_text, "", source_language, target_language));
        }
        if let Some(cache) = &self.hf_cache {
            std::fs::create_dir_all(cache)?;
        }
        let request = serde_json::json!({
            "task": "translate_text",
            "model_dir": self.hf_cache.as_ref().map(|p| p.to_string_lossy().to_string()),
            "model_id": self.model,
            "text": source_text,
            "src_lang": source_language,
            "tgt_lang": target_language,
        });
        let translated = self.invoke(request.to_string().as_bytes())?;
        Ok(self.advisory(source_text, &translated, source_language, target_language))
    }

    fn invoke(&self, stdin_bytes: &[u8]) -> Result<String, AugurError> {
        let mut child = Command::new(&self.python_cmd)
            .arg("-c")
            .arg(SEAMLESS_SCRIPT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AugurError::Translate(format!("could not spawn {}: {e}", self.python_cmd))
            })?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(stdin_bytes)?;
        }
        let output = child.wait_with_output().map_err(|e| {
            AugurError::Translate(format!("seamless worker wait failed: {e}"))
        })?;
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.status.success() {
            return Err(AugurError::Translate(format!(
                "seamless worker exited {:?}; stderr={stderr}",
                output.status.code()
            )));
        }
        let resp: SeamlessResponse = serde_json::from_str(stdout.trim()).map_err(|e| {
            AugurError::Translate(format!(
                "could not parse seamless response: {e}; stdout={stdout:?}; stderr={stderr:?}"
            ))
        })?;
        if let Some(err) = resp.error {
            return Err(AugurError::Translate(format!(
                "seamless worker error: {err}; stderr={stderr}"
            )));
        }
        resp.translation.ok_or_else(|| {
            AugurError::Translate(format!(
                "seamless worker returned no translation; stdout={stdout:?}"
            ))
        })
    }

    fn advisory(
        &self,
        source_text: &str,
        translated_text: &str,
        source_language: &str,
        target_language: &str,
    ) -> TranslationResult {
        // Mandatory MT notice first, SeamlessM4T disclosure second
        // — the MT advisory is never replaced.
        let notice = format!("{} {}", MACHINE_TRANSLATION_NOTICE, SEAMLESS_ADVISORY);
        TranslationResult {
            source_text: source_text.to_string(),
            translated_text: translated_text.to_string(),
            source_language: source_language.to_string(),
            target_language: target_language.to_string(),
            confidence: 0.0,
            model: self.model.clone(),
            is_machine_translation: true,
            advisory_notice: notice,
            segments: None,
        }
    }
}

#[derive(Deserialize)]
struct SeamlessResponse {
    translation: Option<String>,
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_switching_detected_in_arabic_english_mix() {
        let text = "قال إنه going to the market غداً";
        let analysis = detect_code_switching(text);
        assert!(analysis.is_code_switched, "should flag mixed-script text");
        assert!(analysis.languages_detected.contains(&"latin".to_string()));
        assert!(analysis.languages_detected.contains(&"arabic".to_string()));
    }

    #[test]
    fn pure_arabic_not_flagged_as_code_switched() {
        let analysis = detect_code_switching("مرحبا بالعالم كيف حالك");
        assert!(!analysis.is_code_switched);
    }

    #[test]
    fn engine_auto_picks_seamless_for_low_confidence() {
        let kind = select_engine("any", "ar", 0.60, true);
        assert_eq!(kind, TranslationEngineKind::Seamless);
    }

    #[test]
    fn engine_falls_back_to_nllb_when_seamless_missing() {
        let kind = select_engine("any", "ar", 0.30, false);
        assert_eq!(kind, TranslationEngineKind::Nllb);
    }

    #[test]
    fn seamless_advisory_preserves_mt_advisory() {
        let eng = SeamlessEngine {
            model: "facebook/seamless-m4t-medium".into(),
            python_cmd: "python3".into(),
            hf_cache: None,
        };
        let r = eng.advisory("ar", "en source", "ar", "en");
        assert!(
            r.advisory_notice.contains(MACHINE_TRANSLATION_NOTICE),
            "MT advisory must always be present"
        );
        assert!(
            r.advisory_notice.contains("SeamlessM4T"),
            "Seamless origin must be disclosed"
        );
        assert!(r.is_machine_translation);
    }
}
