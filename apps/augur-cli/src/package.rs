//! Evidence-package export.
//!
//! Sprint 9 P3 — bundle a batch result into a self-contained ZIP
//! the examiner can hand to a prosecutor / co-agency / archive.
//! Layout (per the spec):
//!
//! ```text
//! case-001-verify/
//! ├── MANIFEST.json          ← package metadata + per-file SHA-256
//! ├── REPORT.html            ← rendered HTML batch report
//! ├── REPORT.json            ← full BatchResult JSON
//! ├── CHAIN_OF_CUSTODY.txt   ← who / when / what
//! ├── translations/
//! │   ├── recording_001.mp3.en.txt
//! │   └── ...
//! └── original/              ← only when --include-originals
//! ```
//!
//! # Forensic safety invariants
//!
//! - `MANIFEST.json` always carries the
//!   `machine_translation_notice`. Stripping it would defeat the
//!   advisory. [`Manifest::assert_advisory`] enforces the
//!   invariant before the writer hands the bytes to the ZIP.
//! - `CHAIN_OF_CUSTODY.txt` includes the same notice in the
//!   prose form an investigator reads on first opening.
//! - Hashes use SHA-256. Originals are read once, hashed, and
//!   only re-read when `include_originals = true`.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use augur_core::pipeline::{BatchFileResult, BatchResult};
use augur_core::report::{render_batch_html, ReportConfig};
use augur_core::AugurError;
use augur_translate::MACHINE_TRANSLATION_NOTICE;

/// One row in `MANIFEST.json`'s `files` array.
#[derive(Debug, Clone, Serialize)]
pub struct ManifestFile {
    pub original_name: String,
    pub original_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub language: String,
    pub translated: bool,
    /// Path *inside* the ZIP for the per-file translation. Empty
    /// when `translated = false`.
    pub translation_file: String,
}

/// Top-level shape of `MANIFEST.json`. The advisory invariant is
/// pinned by [`Manifest::assert_advisory`] before write.
#[derive(Debug, Clone, Serialize)]
pub struct Manifest {
    pub package_version: String,
    pub created_at: String,
    pub examiner: Option<String>,
    pub agency: Option<String>,
    pub case_number: Option<String>,
    pub augur_version: String,
    pub source_directory: String,
    pub file_count: u32,
    pub translated_count: u32,
    /// Forensic invariant: same advisory the report carries,
    /// restated at the manifest level so any consumer parsing
    /// only the manifest still sees it.
    pub machine_translation_notice: String,
    pub files: Vec<ManifestFile>,
}

impl Manifest {
    /// Refuse to write a manifest with translations but no
    /// advisory. Mirrors `BatchResult::assert_advisory` at the
    /// package layer.
    pub fn assert_advisory(&self) -> Result<(), AugurError> {
        if self.translated_count > 0 && self.machine_translation_notice.is_empty() {
            return Err(AugurError::Translate(
                "MANIFEST.json missing machine_translation_notice — \
                 forensic invariant violation"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

/// Render the chain-of-custody plaintext block from a finalised
/// batch report + report config + system metadata.
/// Sprint 17 P2 — human-readable summary of the examiner-flagged
/// segments. Lands at `review/REVIEW_REQUIRED.txt` in the
/// package. Always closes with the MT advisory in prose.
pub fn render_review_required_txt(
    flags: &[serde_json::Value],
    report: &BatchResult,
) -> String {
    let mut out = String::new();
    out.push_str("AUGUR Evidence Package — Human Review Required\n");
    out.push_str("===============================================\n");
    out.push_str(&format!(
        "{} segment{} have been flagged by the examiner for human review.\n\n",
        flags.len(),
        if flags.len() == 1 { "" } else { "s" }
    ));
    out.push_str(
        "This package contains machine translations (NLLB-200, Meta AI; \
         SeamlessM4T, Meta AI; CAMeL Tools, Carnegie Mellon Univ.). \
         The flagged segments below require verification by a certified \
         human linguist before use in legal proceedings.\n\n",
    );
    let pick_str = |obj: &serde_json::Value, a: &str, b: &str| -> String {
        obj.get(a)
            .and_then(|v| v.as_str())
            .or_else(|| obj.get(b).and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string()
    };
    let pick_u64 = |obj: &serde_json::Value, a: &str, b: &str| -> u64 {
        obj.get(a)
            .and_then(|v| v.as_u64())
            .or_else(|| obj.get(b).and_then(|v| v.as_u64()))
            .unwrap_or(0)
    };
    for flag in flags {
        let idx = pick_u64(flag, "segmentIndex", "segment_index");
        let path = pick_str(flag, "filePath", "file_path");
        let note = pick_str(flag, "examinerNote", "examiner_note");
        let status = if flag.get("reviewStatus").is_some() {
            pick_str(flag, "reviewStatus", "reviewStatus")
        } else {
            pick_str(flag, "review_status", "review_status")
        };
        // Try to find the segment text in the report.
        let mut original = String::new();
        let mut translation = String::new();
        for r in &report.results {
            if !path.is_empty() && r.file_path != path {
                continue;
            }
            if let Some(segs) = &r.segments {
                // BatchSegment has no public index field; use
                // position to match the flag's segmentIndex.
                if let Some(s) = segs.get(idx as usize) {
                    original = s.source_text.clone();
                    translation = s.translated_text.clone();
                    break;
                }
            }
            if r.file_path == path
                || (path.is_empty() && original.is_empty() && r.translated_text.is_some())
            {
                if let Some(t) = &r.translated_text {
                    translation = t.clone();
                }
                if let Some(o) = &r.source_text {
                    original = o.clone();
                }
            }
        }
        out.push_str(&format!(
            "Segment {} (file: {}):\n  Original: {original}\n  MT Translation: {translation}\n",
            idx + 1,
            if path.is_empty() { "(unknown)" } else { &path }
        ));
        if !note.is_empty() {
            out.push_str(&format!("  Examiner note: {note}\n"));
        }
        let status_s = if status.is_empty() { "needs_review" } else { &status };
        out.push_str(&format!(
            "  Status: {}\n\n",
            status_s.replace('_', " ").to_uppercase()
        ));
    }
    out.push_str("MACHINE TRANSLATION NOTICE:\n");
    out.push_str(
        "All translations in this package are machine-generated and have \
         not been reviewed by a certified human translator. Flagged segments \
         are of particular concern. Verify ALL translations before use in \
         legal proceedings.\n",
    );
    out
}

pub fn render_chain_of_custody(
    report: &BatchResult,
    config: &ReportConfig,
    source: &Path,
) -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("AUGUR Evidence Package — Chain of Custody\n");
    out.push_str("==========================================\n");
    out.push_str(&format!(
        "Package created: {}\n",
        report.generated_at,
    ));
    out.push_str(&format!(
        "Examiner: {}\n",
        config.examiner_name.as_deref().unwrap_or("(not configured)")
    ));
    if let Some(b) = &config.examiner_badge {
        out.push_str(&format!("Badge: {b}\n"));
    }
    if let Some(a) = &config.agency_name {
        out.push_str(&format!("Agency: {a}\n"));
    }
    if let Some(c) = &config.case_number {
        out.push_str(&format!("Case Number: {c}\n"));
    }
    if let Some(c) = &config.classification {
        out.push_str(&format!("Classification: {c}\n"));
    }
    out.push_str(&format!(
        "System: {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
    ));
    out.push_str(&format!(
        "AUGUR version: {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    out.push('\n');
    out.push_str(&format!("Source directory: {}\n", source.display()));
    out.push_str(&format!("Files processed: {}\n", report.processed));
    out.push_str(&format!(
        "Foreign-language files: {}\n",
        report.foreign_language
    ));
    out.push_str(&format!("Translated files: {}\n", report.translated));
    out.push_str(&format!("Errors: {}\n", report.errors));
    if let Some(s) = &report.summary {
        if !s.languages_detected.is_empty() {
            out.push_str("Languages detected: ");
            for (i, (k, v)) in s.languages_detected.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("{k} ({v})"));
            }
            out.push('\n');
        }
    }
    out.push('\n');
    out.push_str("MACHINE TRANSLATION NOTICE\n");
    out.push_str("--------------------------\n");
    out.push_str("All translations in this package were produced by AUGUR,\n");
    out.push_str("an automated machine translation system. They have NOT been\n");
    out.push_str("reviewed by a certified human translator. For legal\n");
    out.push_str("proceedings, verify all translations with a qualified human\n");
    out.push_str("linguist.\n");
    out.push('\n');
    out.push_str(&format!("({MACHINE_TRANSLATION_NOTICE})\n\n"));
    out.push_str("Original files: SHA-256 hashes listed in MANIFEST.json\n");
    out.push_str("Translation files: Generated by NLLB-200-distilled-600M\n");
    out
}

/// Compute the SHA-256 of a file's contents. Reads in 64 KiB
/// chunks so memory stays bounded for large evidence files.
pub fn sha256_of_path(path: &Path) -> Result<String, AugurError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Build the in-memory `Manifest` for a finalised report. The
/// caller still has to assemble the ZIP — see [`write_package`].
pub fn build_manifest(
    report: &BatchResult,
    config: &ReportConfig,
    source: &Path,
) -> Result<Manifest, AugurError> {
    let mut files: Vec<ManifestFile> = Vec::with_capacity(report.results.len());
    for r in &report.results {
        let original_path = PathBuf::from(&r.file_path);
        let original_name = original_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| r.file_path.clone());
        let (sha256, size_bytes) = if original_path.exists() {
            let sha = sha256_of_path(&original_path)?;
            let size = std::fs::metadata(&original_path).map(|m| m.len()).unwrap_or(0);
            (sha, size)
        } else {
            // Missing originals are not fatal — record an empty
            // hash and zero size; the consumer can flag it.
            (String::new(), 0)
        };
        let translated = r.translated_text.is_some();
        let translation_file = if translated {
            format!("translations/{original_name}.{}.txt", report.target_language)
        } else {
            String::new()
        };
        files.push(ManifestFile {
            original_name,
            original_path: r.file_path.clone(),
            sha256,
            size_bytes,
            language: r.detected_language.clone(),
            translated,
            translation_file,
        });
    }
    let manifest = Manifest {
        package_version: "1.0".into(),
        created_at: report.generated_at.clone(),
        examiner: config.examiner_name.clone(),
        agency: config.agency_name.clone(),
        case_number: config.case_number.clone(),
        augur_version: env!("CARGO_PKG_VERSION").to_string(),
        source_directory: source.to_string_lossy().into_owned(),
        file_count: report.total_files,
        translated_count: report.translated,
        machine_translation_notice: MACHINE_TRANSLATION_NOTICE.to_string(),
        files,
    };
    manifest.assert_advisory()?;
    Ok(manifest)
}

/// Assemble the ZIP. Writer takes ownership of the output path.
/// The structure mirrors the spec layout.
pub fn write_package(
    output_zip: &Path,
    report: &BatchResult,
    config: &ReportConfig,
    source: &Path,
    include_originals: bool,
) -> Result<Manifest, AugurError> {
    let manifest = build_manifest(report, config, source)?;
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| {
        AugurError::Translate(format!("MANIFEST.json serialize: {e}"))
    })?;
    let chain = render_chain_of_custody(report, config, source);
    let html = render_batch_html(report, config);
    let report_json = serde_json::to_string_pretty(report).map_err(|e| {
        AugurError::Translate(format!("REPORT.json serialize: {e}"))
    })?;

    if let Some(parent) = output_zip.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = std::fs::File::create(output_zip)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    write_zip_entry(&mut zip, "MANIFEST.json", manifest_json.as_bytes(), opts)?;
    write_zip_entry(&mut zip, "CHAIN_OF_CUSTODY.txt", chain.as_bytes(), opts)?;
    write_zip_entry(&mut zip, "REPORT.html", html.as_bytes(), opts)?;
    write_zip_entry(&mut zip, "REPORT.json", report_json.as_bytes(), opts)?;

    // Per-file translation .txt artifacts.
    for (mf, r) in manifest.files.iter().zip(report.results.iter()) {
        if !mf.translated {
            continue;
        }
        let body = build_translation_text(r);
        write_zip_entry(&mut zip, &mf.translation_file, body.as_bytes(), opts)?;
    }

    if include_originals {
        for r in &report.results {
            let path = PathBuf::from(&r.file_path);
            if !path.exists() {
                continue;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| r.file_path.clone());
            let bytes = std::fs::read(&path)?;
            write_zip_entry(
                &mut zip,
                &format!("original/{name}"),
                &bytes,
                opts,
            )?;
        }
    }

    // Sprint 17 P2 — examiner-flagged segments, sourced from
    // the optional AUGUR_FLAGS_JSON env var. The schema accepts
    // both the desktop GUI's camelCase shape and the snake_case
    // shape used elsewhere; either round-trips into the
    // REVIEW_REQUIRED.txt summary and the structured JSON.
    if let Some(flags_path) = std::env::var_os("AUGUR_FLAGS_JSON") {
        let flags_path = std::path::PathBuf::from(flags_path);
        if flags_path.exists() {
            match std::fs::read(&flags_path) {
                Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                    Ok(parsed) => {
                        let flags_arr: Vec<serde_json::Value> = parsed
                            .get("flags")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        if !flags_arr.is_empty() {
                            let txt = render_review_required_txt(&flags_arr, report);
                            write_zip_entry(
                                &mut zip,
                                "review/REVIEW_REQUIRED.txt",
                                txt.as_bytes(),
                                opts,
                            )?;
                            let body = serde_json::to_string_pretty(
                                &serde_json::json!({
                                    "machine_translation_notice": MACHINE_TRANSLATION_NOTICE,
                                    "case_number": config
                                        .case_number
                                        .clone()
                                        .unwrap_or_default(),
                                    "flagged_segments_count": flags_arr.len(),
                                    "flagged_segments": flags_arr,
                                }),
                            )
                            .map_err(|e| {
                                AugurError::Translate(format!(
                                    "review/flagged_segments.json serialize: {e}"
                                ))
                            })?;
                            write_zip_entry(
                                &mut zip,
                                "review/flagged_segments.json",
                                body.as_bytes(),
                                opts,
                            )?;
                        }
                    }
                    Err(e) => log::warn!(
                        "AUGUR_FLAGS_JSON pointed at unparseable JSON ({e}); review/ skipped"
                    ),
                },
                Err(e) => log::warn!(
                    "AUGUR_FLAGS_JSON could not be read ({e}); review/ skipped"
                ),
            }
        }
    }

    zip.finish().map_err(|e| {
        AugurError::Translate(format!("zip finalise: {e}"))
    })?;
    Ok(manifest)
}

fn write_zip_entry(
    zip: &mut zip::ZipWriter<std::fs::File>,
    name: &str,
    body: &[u8],
    opts: zip::write::SimpleFileOptions,
) -> Result<(), AugurError> {
    zip.start_file(name, opts).map_err(|e| {
        AugurError::Translate(format!("zip start_file({name}): {e}"))
    })?;
    zip.write_all(body)
        .map_err(|e| AugurError::Translate(format!("zip write({name}): {e}")))?;
    Ok(())
}

fn build_translation_text(r: &BatchFileResult) -> String {
    let mut body = String::with_capacity(512);
    body.push_str("# AUGUR translation\n");
    body.push_str(&format!("# Source file: {}\n", r.file_path));
    body.push_str(&format!(
        "# Detected language: {}\n",
        if r.detected_language.is_empty() {
            "(unknown)"
        } else {
            r.detected_language.as_str()
        }
    ));
    if let Some(adv) = &r.confidence_advisory {
        body.push_str(&format!("# Confidence advisory: {adv}\n"));
    }
    body.push_str(&format!("# {MACHINE_TRANSLATION_NOTICE}\n"));
    body.push_str("\n--- SOURCE ---\n");
    body.push_str(r.source_text.as_deref().unwrap_or("(no source text)"));
    body.push_str("\n\n--- TRANSLATION ---\n");
    body.push_str(r.translated_text.as_deref().unwrap_or("(no translation)"));
    body.push('\n');
    if let Some(segs) = &r.segments {
        body.push_str("\n--- SEGMENTS ---\n");
        for s in segs {
            body.push_str(&format!(
                "[{} - {} ms] {} → {}\n",
                s.start_ms, s.end_ms, s.source_text, s.translated_text
            ));
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use augur_core::pipeline::{BatchFileResult, BatchResult, BatchSummary};

    fn fixture_report() -> BatchResult {
        BatchResult {
            generated_at: "2026-04-26T10:00:00Z".into(),
            total_files: 2,
            processed: 2,
            foreign_language: 1,
            translated: 1,
            errors: 0,
            target_language: "en".into(),
            machine_translation_notice: MACHINE_TRANSLATION_NOTICE.to_string(),
            results: vec![
                BatchFileResult {
                    file_path: "/ev/clip.mp3".into(),
                    input_type: "audio".into(),
                    detected_language: "ar".into(),
                    is_foreign: true,
                    confidence_tier: "HIGH".into(),
                    confidence_advisory: None,
                    source_text: Some("مرحبا".into()),
                    translated_text: Some("Hello".into()),
                    segments: None,
                    error: None,
                },
                BatchFileResult {
                    file_path: "/ev/notes.png".into(),
                    input_type: "image".into(),
                    detected_language: "en".into(),
                    is_foreign: false,
                    confidence_tier: "HIGH".into(),
                    confidence_advisory: None,
                    source_text: Some("English notes".into()),
                    translated_text: None,
                    segments: None,
                    error: None,
                },
            ],
            summary: Some(BatchSummary {
                total_files: 2,
                processed: 2,
                foreign_language_files: 1,
                translated_files: 1,
                errors: 0,
                languages_detected: {
                    let mut m = std::collections::BTreeMap::new();
                    m.insert("ar".into(), 1);
                    m.insert("en".into(), 1);
                    m
                },
                processing_time_seconds: 1.0,
                machine_translation_notice: MACHINE_TRANSLATION_NOTICE.to_string(),
            }),
            language_groups: Vec::new(),
            dominant_language: None,
        }
    }

    #[test]
    fn package_manifest_includes_mt_notice() {
        let mut cfg = ReportConfig::blank();
        cfg.examiner_name = Some("D. Examiner".into());
        cfg.case_number = Some("2026-001".into());
        let m = build_manifest(&fixture_report(), &cfg, Path::new("/evidence")).unwrap();
        assert!(!m.machine_translation_notice.is_empty());
        assert_eq!(m.translated_count, 1);
        assert_eq!(m.case_number.as_deref(), Some("2026-001"));
        assert_eq!(m.examiner.as_deref(), Some("D. Examiner"));
    }

    #[test]
    fn package_manifest_sha256_correct() {
        // Write a real file and verify the manifest's SHA-256
        // matches a hand-computed digest of the same content.
        let dir = std::env::temp_dir().join(format!(
            "augur-pkg-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("clip.mp3");
        let content = b"augur-pkg-test-content";
        std::fs::write(&path, content).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(content);
        let expected = hex_lower(&hasher.finalize());

        let actual = sha256_of_path(&path).unwrap();
        assert_eq!(actual, expected);

        // Also exercise build_manifest end-to-end: a report
        // pointing at this file produces a manifest entry whose
        // sha256 matches.
        let mut report = fixture_report();
        report.results[0].file_path = path.to_string_lossy().into_owned();
        let cfg = ReportConfig::blank();
        let m = build_manifest(&report, &cfg, &dir).unwrap();
        assert_eq!(m.files[0].sha256, expected);
        assert_eq!(m.files[0].size_bytes, content.len() as u64);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_chain_of_custody_present() {
        let mut cfg = ReportConfig::blank();
        cfg.examiner_name = Some("D. Examiner".into());
        cfg.case_number = Some("2026-001".into());
        cfg.agency_name = Some("Wolfmark Systems".into());
        let chain = render_chain_of_custody(&fixture_report(), &cfg, Path::new("/evidence"));
        assert!(chain.contains("Chain of Custody"));
        assert!(chain.contains("D. Examiner"));
        assert!(chain.contains("2026-001"));
        assert!(chain.contains("Wolfmark Systems"));
        assert!(chain.contains("MACHINE TRANSLATION NOTICE"));
        assert!(chain.contains("2026-04-26T10:00:00Z"));
    }

    #[test]
    fn manifest_advisory_invariant_rejects_empty_notice() {
        let mut m = Manifest {
            package_version: "1.0".into(),
            created_at: "2026-04-26T10:00:00Z".into(),
            examiner: None,
            agency: None,
            case_number: None,
            augur_version: env!("CARGO_PKG_VERSION").to_string(),
            source_directory: "/evidence".into(),
            file_count: 1,
            translated_count: 1,
            machine_translation_notice: String::new(),
            files: vec![],
        };
        assert!(m.assert_advisory().is_err());
        m.machine_translation_notice = "advisory present".into();
        assert!(m.assert_advisory().is_ok());
    }

    #[test]
    fn write_package_round_trip_produces_zip_with_required_entries() {
        let dir = std::env::temp_dir().join(format!(
            "augur-pkg-zip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let report = {
            let mut r = fixture_report();
            // Strip the file_path that doesn't exist on this
            // host (build_manifest tolerates missing files).
            r.results[0].file_path = "/nonexistent/clip.mp3".into();
            r.results[1].file_path = "/nonexistent/notes.png".into();
            r
        };
        let cfg = ReportConfig::blank();
        let zip_path = dir.join("case.zip");
        write_package(&zip_path, &report, &cfg, &dir, false).unwrap();

        // Re-open and confirm the four required entries are there.
        let f = std::fs::File::open(&zip_path).unwrap();
        let archive = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = archive.file_names().map(String::from).collect();
        assert!(names.contains(&"MANIFEST.json".to_string()));
        assert!(names.contains(&"CHAIN_OF_CUSTODY.txt".to_string()));
        assert!(names.contains(&"REPORT.html".to_string()));
        assert!(names.contains(&"REPORT.json".to_string()));
        // Per-translation file for the foreign-language entry.
        assert!(names
            .iter()
            .any(|n| n.starts_with("translations/clip.mp3")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
