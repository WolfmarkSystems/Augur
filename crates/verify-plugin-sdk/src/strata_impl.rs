//! Real `strata_plugin_sdk::StrataPlugin` implementation for VERIFY.
//!
//! Compiled only with `--features strata`. Pulls the upstream SDK
//! in via a sibling-workspace path dependency
//! (`../../../strata/crates/strata-plugin-sdk`).
//!
//! # Plugin behavior
//!
//! Given a `PluginContext` rooted at materialized evidence, VERIFY:
//! 1. Walks the root path for audio / video / image files,
//! 2. Runs the standard pipeline on each (Whisper/OCR → fastText
//!    classifier → NLLB-200),
//! 3. Emits one `ArtifactRecord` per foreign-language translation.
//!
//! # Forensic safety invariant
//!
//! Strata's `ArtifactRecord` has no `is_advisory` field; the
//! advisory is encoded in two places so it survives any export:
//!  - The artifact `title` is prefixed with `"[MT — review by a
//!    certified human translator] "`.
//!  - The advisory notice + machine-translation flag live inside
//!    `raw_data` JSON (`is_machine_translation`, `advisory_notice`).
//!
//! Both halves are populated by [`record_from_translation`] and
//! covered by the unit tests below.

use crate::{Confidence, MACHINE_TRANSLATION_NOTICE};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use strata_plugin_sdk::{
    Artifact, ArtifactCategory, ArtifactRecord, ForensicValue, PluginCapability, PluginContext,
    PluginError, PluginOutput, PluginResult, PluginSummary, PluginTier, PluginType, StrataPlugin,
};
use verify_classifier::LanguageClassifier;
use verify_core::pipeline::{detect_input_kind, PipelineInput};
use verify_ocr::{iso_to_tesseract, OcrEngine};
use verify_stt::{
    extract_audio_from_video, ModelManager as WhisperModelManager, SttEngine, SttResult,
    WhisperPreset,
};
use verify_translate::{TranslationEngine, TranslationResult};

const ADVISORY_TITLE_PREFIX: &str = "[MT — review by a certified human translator] ";
const VERIFY_SUBCATEGORY: &str = "Foreign Language Translation";
/// Confidence score in Strata's 0-100 scale. Machine translation
/// always lands at Medium (50) so analysts can sort/filter.
const MT_CONFIDENCE: u8 = 50;
/// Target language used when the plugin is invoked without a
/// `target_language` config override.
const DEFAULT_TARGET_LANGUAGE: &str = "en";

impl StrataPlugin for crate::VerifyStrataPlugin {
    fn name(&self) -> &str {
        "VERIFY"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn supported_inputs(&self) -> Vec<String> {
        // VERIFY processes audio / video / image artefacts on the
        // materialized evidence tree. We claim the broad inputs
        // here; the per-file walk filters by extension.
        vec![
            "audio".into(),
            "video".into(),
            "image".into(),
            "directory".into(),
        ]
    }

    fn plugin_type(&self) -> PluginType {
        PluginType::Analyzer
    }

    fn capabilities(&self) -> Vec<PluginCapability> {
        vec![PluginCapability::ArtifactExtraction]
    }

    fn description(&self) -> &str {
        "Foreign-language detection and translation. Surfaces foreign-language audio, \
         video, and image content as machine-translated artifacts. All output is \
         labeled as machine translation; review by a certified human translator is \
         required before legal use."
    }

    fn required_tier(&self) -> PluginTier {
        // Translation is a Pro feature in line with other analyzer
        // plugins; the CSAM Sentinel Free carve-out doesn't apply.
        PluginTier::Professional
    }

    fn run(&self, context: PluginContext) -> PluginResult {
        // The legacy `run` returns Vec<Artifact>. We synthesize
        // the simpler Artifact form (HashMap-backed) — analysts
        // will see the rich version when `execute` is called.
        let target = target_language_from(&context);
        let records = walk_and_translate(&PathBuf::from(&context.root_path), &target)
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;
        let mut artifacts = Vec::with_capacity(records.len());
        for r in &records {
            let mut data: HashMap<String, String> = HashMap::new();
            data.insert("title".into(), r.title.clone());
            data.insert("detail".into(), r.detail.clone());
            if let Some(rd) = &r.raw_data {
                data.insert("raw_data".into(), rd.to_string());
            }
            artifacts.push(Artifact {
                category: VERIFY_SUBCATEGORY.into(),
                timestamp: None,
                source: r.source_path.clone(),
                data,
            });
        }
        Ok(artifacts)
    }

    fn execute(&self, context: PluginContext) -> Result<PluginOutput, PluginError> {
        let started = std::time::Instant::now();
        let target = target_language_from(&context);
        let records = walk_and_translate(&PathBuf::from(&context.root_path), &target)
            .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

        // Forensic invariant — every artifact emitted by this
        // plugin must carry the advisory in title + raw_data.
        for r in &records {
            assert_advisory_invariant(r).map_err(PluginError::Internal)?;
        }

        let total = records.len();
        Ok(PluginOutput {
            plugin_name: self.name().to_string(),
            plugin_version: self.version().to_string(),
            executed_at: String::new(),
            duration_ms: started.elapsed().as_millis() as u64,
            artifacts: records,
            summary: PluginSummary {
                total_artifacts: total,
                suspicious_count: 0,
                categories_populated: vec![ArtifactCategory::Communications.as_str().to_string()],
                headline: format!("VERIFY: {total} foreign-language artifact(s) translated"),
            },
            warnings: vec![],
        })
    }
}

fn target_language_from(ctx: &PluginContext) -> String {
    ctx.config
        .get("target_language")
        .cloned()
        .unwrap_or_else(|| DEFAULT_TARGET_LANGUAGE.to_string())
}

/// Convert a [`TranslationResult`] into a Strata
/// [`ArtifactRecord`]. The advisory survives in two places: the
/// `title` prefix and the `raw_data` JSON.
pub fn record_from_translation(
    file_path: &Path,
    translation: &TranslationResult,
) -> ArtifactRecord {
    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<unnamed>".to_string());
    let advisory_notice = if translation.advisory_notice.is_empty() {
        MACHINE_TRANSLATION_NOTICE.to_string()
    } else {
        translation.advisory_notice.clone()
    };
    let raw = serde_json::json!({
        "source_text": translation.source_text,
        "translated_text": translation.translated_text,
        "source_language": translation.source_language,
        "target_language": translation.target_language,
        "model": translation.model,
        "is_machine_translation": translation.is_machine_translation,
        "advisory_notice": advisory_notice,
        "segments": translation.segments,
    });
    ArtifactRecord {
        category: ArtifactCategory::Communications,
        subcategory: VERIFY_SUBCATEGORY.into(),
        timestamp: None,
        title: format!("{ADVISORY_TITLE_PREFIX}VERIFY Translation: {file_name}"),
        detail: translation.translated_text.clone(),
        source_path: file_path.to_string_lossy().into_owned(),
        forensic_value: ForensicValue::High,
        mitre_technique: None,
        is_suspicious: false,
        raw_data: Some(raw),
        confidence: MT_CONFIDENCE,
    }
}

fn assert_advisory_invariant(r: &ArtifactRecord) -> Result<(), String> {
    if !r.title.starts_with(ADVISORY_TITLE_PREFIX) {
        return Err(format!(
            "advisory missing from artifact title: {:?}",
            r.title
        ));
    }
    let raw = r
        .raw_data
        .as_ref()
        .ok_or_else(|| "advisory_notice missing — raw_data is None".to_string())?;
    let advisory = raw
        .get("advisory_notice")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if advisory.is_empty() {
        return Err("advisory_notice empty in raw_data — forensic invariant violation".into());
    }
    let is_mt = raw
        .get("is_machine_translation")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_mt {
        return Err("is_machine_translation must be true on every VERIFY artifact".into());
    }
    Ok(())
}

/// Walk a directory, run the per-file pipeline, and produce one
/// [`ArtifactRecord`] per foreign-language translation. Errors on
/// individual files are logged and skipped — they do not abort the
/// whole walk.
pub fn walk_and_translate(
    root: &Path,
    target_language: &str,
) -> Result<Vec<ArtifactRecord>, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    walk_files(root, &mut files).map_err(|e| e.to_string())?;
    files.sort();

    let classifier = build_classifier();
    let mut translation_engine = match TranslationEngine::with_xdg_cache() {
        Ok(e) => e,
        Err(e) => return Err(format!("translation engine init: {e}")),
    };
    // Strata-mode default: keep Auto so ct2 is used when cached;
    // fall back to transformers automatically.
    translation_engine.backend = Default::default();

    let mut records: Vec<ArtifactRecord> = Vec::new();
    for file in &files {
        match process_one_file(file, target_language, &classifier, &translation_engine) {
            Ok(Some(record)) => records.push(record),
            Ok(None) => {} // not foreign-language; nothing to emit
            Err(e) => log::warn!("verify-strata-plugin: skipping {file:?}: {e}"),
        }
    }
    Ok(records)
}

/// Public entry point used by integration tests / CLI tooling that
/// wants to drive the plugin walk without going through Strata's
/// `PluginContext`.
pub fn run_on_directory(root: &Path, target: &str) -> Result<Vec<ArtifactRecord>, String> {
    walk_and_translate(root, target)
}

fn build_classifier() -> LanguageClassifier {
    use verify_classifier::ModelManager;
    if let Ok(mgr) = ModelManager::with_xdg_cache() {
        if let Ok(p) = mgr.ensure_lid_model() {
            if let Ok(c) = LanguageClassifier::load_fasttext(&p) {
                return c;
            }
        }
    }
    log::warn!(
        "verify-strata-plugin: fasttext unavailable; falling back to whichlang \
         (16 languages, pure-Rust, no network)."
    );
    LanguageClassifier::new_whichlang()
}

fn process_one_file(
    file: &Path,
    target: &str,
    classifier: &LanguageClassifier,
    engine: &TranslationEngine,
) -> Result<Option<ArtifactRecord>, String> {
    let kind = detect_input_kind(file);
    let resolved = match &kind {
        PipelineInput::Audio(_) => resolve_audio(file)?,
        PipelineInput::Video(_) => resolve_video(file)?,
        PipelineInput::Image(_) => resolve_image(file, target)?,
        PipelineInput::Text(_) => return Ok(None),
    };
    if resolved.text.trim().is_empty() {
        return Ok(None);
    }
    let cr = classifier
        .classify(&resolved.text, target)
        .map_err(|e| e.to_string())?;
    let lang = if cr.language.is_empty() {
        resolved.upstream_lang
    } else {
        cr.language
    };
    if lang == target {
        return Ok(None);
    }
    let translation = if let Some(segs) = resolved.segments {
        let trips: Vec<(u64, u64, String)> = segs
            .into_iter()
            .map(|s| (s.start_ms, s.end_ms, s.text))
            .collect();
        engine
            .translate_segments(&trips, &lang, target)
            .map_err(|e| e.to_string())?
    } else {
        engine
            .translate(&resolved.text, &lang, target)
            .map_err(|e| e.to_string())?
    };
    Ok(Some(record_from_translation(file, &translation)))
}

struct ResolvedSource {
    text: String,
    upstream_lang: String,
    segments: Option<Vec<verify_stt::SttSegment>>,
}

fn resolve_audio(file: &Path) -> Result<ResolvedSource, String> {
    let stt = run_stt(file).map_err(|e| e.to_string())?;
    Ok(ResolvedSource {
        text: stt.transcript,
        upstream_lang: stt.detected_language,
        segments: Some(stt.segments),
    })
}

fn resolve_video(file: &Path) -> Result<ResolvedSource, String> {
    let scratch = std::env::temp_dir().join("verify").join("strata-video-scratch");
    let audio = extract_audio_from_video(file, &scratch).map_err(|e| e.to_string())?;
    let stt = run_stt(&audio);
    let _ = std::fs::remove_file(&audio);
    let stt = stt.map_err(|e| e.to_string())?;
    Ok(ResolvedSource {
        text: stt.transcript,
        upstream_lang: stt.detected_language,
        segments: Some(stt.segments),
    })
}

fn resolve_image(file: &Path, ocr_lang: &str) -> Result<ResolvedSource, String> {
    let tess = iso_to_tesseract(ocr_lang).map_err(|e| e.to_string())?;
    let engine = OcrEngine::new(tess).map_err(|e| e.to_string())?;
    let r = engine.extract_text(file).map_err(|e| e.to_string())?;
    Ok(ResolvedSource {
        text: r.text,
        upstream_lang: ocr_lang.to_string(),
        segments: None,
    })
}

fn run_stt(file: &Path) -> Result<SttResult, verify_core::VerifyError> {
    let mgr = WhisperModelManager::with_xdg_cache()?;
    let paths = mgr.ensure_whisper_model(WhisperPreset::Balanced)?;
    let mut engine = SttEngine::load(&paths, WhisperPreset::Balanced)?;
    engine.transcribe(file)
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            walk_files(&path, out)?;
        } else if ft.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

// Suppress clippy on the Confidence import — used by tests below.
#[allow(dead_code)]
fn _unused_confidence_alias() -> Confidence {
    Confidence::Medium
}

#[cfg(test)]
mod tests {
    use super::*;
    use verify_translate::{DEFAULT_NLLB_MODEL, MACHINE_TRANSLATION_NOTICE};

    fn fixture() -> TranslationResult {
        TranslationResult {
            source_text: "مرحبا".into(),
            translated_text: "Hello".into(),
            source_language: "ar".into(),
            target_language: "en".into(),
            confidence: 0.85,
            model: DEFAULT_NLLB_MODEL.into(),
            is_machine_translation: true,
            advisory_notice: MACHINE_TRANSLATION_NOTICE.into(),
            segments: None,
        }
    }

    #[test]
    fn strata_record_carries_advisory_in_title_and_raw_data() {
        let r = record_from_translation(Path::new("/evidence/audio/clip.mp3"), &fixture());
        assert!(
            r.title.starts_with(ADVISORY_TITLE_PREFIX),
            "title must lead with the MT advisory prefix; got {:?}",
            r.title
        );
        assert!(r.title.contains("clip.mp3"));
        assert_eq!(r.confidence, MT_CONFIDENCE);
        assert_eq!(r.category, ArtifactCategory::Communications);
        assert_eq!(r.forensic_value, ForensicValue::High);
        let raw = r.raw_data.as_ref().expect("raw_data populated");
        assert_eq!(
            raw.get("is_machine_translation").and_then(|v| v.as_bool()),
            Some(true)
        );
        let adv = raw
            .get("advisory_notice")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(!adv.is_empty(), "advisory_notice in raw_data must be non-empty");
    }

    #[test]
    fn assert_advisory_invariant_passes_for_well_formed_records() {
        let r = record_from_translation(Path::new("/evidence/x.mp3"), &fixture());
        assert!(assert_advisory_invariant(&r).is_ok());
    }

    #[test]
    fn assert_advisory_invariant_rejects_missing_prefix() {
        let mut r = record_from_translation(Path::new("/evidence/x.mp3"), &fixture());
        r.title = "VERIFY Translation: clip.mp3".into();
        assert!(assert_advisory_invariant(&r).is_err());
    }

    #[test]
    fn plugin_metadata_via_real_trait() {
        let p = crate::VerifyStrataPlugin::new();
        assert_eq!(<crate::VerifyStrataPlugin as StrataPlugin>::name(&p), "VERIFY");
        assert_eq!(p.required_tier(), PluginTier::Professional);
        assert!(matches!(p.plugin_type(), PluginType::Analyzer));
        assert!(p
            .capabilities()
            .iter()
            .any(|c| matches!(c, PluginCapability::ArtifactExtraction)));
    }
}
