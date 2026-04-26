//! Report customization — agency / case / examiner header
//! metadata, plus an HTML rendering of the batch report.
//!
//! Sprint 7 P2. Forensic agencies want batch reports that carry
//! their agency name, case number, examiner signature, and
//! classification marking. The metadata is configured via a TOML
//! file (created by `verify config init`); a missing config still
//! produces a valid report with no metadata block.
//!
//! # Forensic safety invariant
//!
//! `include_mt_advisory` is ignored — the machine-translation
//! advisory is non-suppressible at every output surface (Sprint 2
//! decision; see CLAUDE.md). The field exists in the TOML schema
//! only so a config file written by an examiner cannot accidentally
//! disable it; the deserializer pins it to `true` regardless of
//! the on-disk value.

use crate::error::VerifyError;
use crate::pipeline::BatchResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level config schema. Mirrors the TOML layout — the
/// `[report]` and `[output]` tables flatten onto this struct.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ReportConfig {
    pub agency_name: Option<String>,
    pub case_number: Option<String>,
    pub examiner_name: Option<String>,
    pub examiner_badge: Option<String>,
    pub classification: Option<String>,
    pub report_title: Option<String>,
    pub logo_path: Option<PathBuf>,
    /// Forensic invariant — always rendered as `true` regardless
    /// of what the on-disk TOML says. Surfaced here for
    /// completeness so anyone reading the schema sees the
    /// machine-translation advisory is structurally non-
    /// suppressible.
    #[serde(default = "always_true")]
    pub include_mt_advisory: bool,
    /// Whether to include `confidence_tier` columns / sections in
    /// the rendered report. Default `true`.
    #[serde(default = "always_true")]
    pub include_confidence_tiers: bool,
    /// Whether the HTML report appends the
    /// `LANGUAGE_LIMITATIONS.md` content as a footer. Default
    /// `true`.
    #[serde(default = "always_true")]
    pub include_language_limitations: bool,
}

fn always_true() -> bool {
    true
}

impl ReportConfig {
    /// Default ("blank") config. The TOML written by
    /// `verify config init` contains the same fields so an
    /// examiner can fill them in.
    pub fn blank() -> Self {
        Self {
            agency_name: None,
            case_number: None,
            examiner_name: None,
            examiner_badge: None,
            classification: None,
            report_title: None,
            logo_path: None,
            include_mt_advisory: true,
            include_confidence_tiers: true,
            include_language_limitations: true,
        }
    }

    pub fn load(path: &Path) -> Result<Self, VerifyError> {
        let body = std::fs::read_to_string(path)?;
        Self::from_toml_str(&body)
    }

    pub fn from_toml_str(s: &str) -> Result<Self, VerifyError> {
        let parsed: TomlConfig = toml::from_str(s).map_err(|e| {
            VerifyError::InvalidInput(format!("config TOML parse: {e}"))
        })?;
        let mut cfg: ReportConfig = parsed.report.unwrap_or_default();
        if let Some(out) = parsed.output {
            // Output settings overlay onto the report config —
            // an examiner can keep `[report]` for the static
            // metadata and `[output]` for booleans.
            if let Some(v) = out.include_confidence_tiers {
                cfg.include_confidence_tiers = v;
            }
            if let Some(v) = out.include_language_limitations {
                cfg.include_language_limitations = v;
            }
        }
        // Forensic invariant — pin even if the TOML attempts
        // `include_mt_advisory = false`. The advisory is
        // non-suppressible.
        cfg.include_mt_advisory = true;
        Ok(cfg)
    }

    /// Serialize back to TOML for `verify config init` /
    /// `verify config show`. Always emits the full schema with
    /// defaults filled in so the user can see every knob.
    pub fn to_toml_string(&self) -> Result<String, VerifyError> {
        let wrapped = TomlConfig {
            report: Some(self.clone()),
            output: Some(TomlOutput {
                include_confidence_tiers: Some(self.include_confidence_tiers),
                include_language_limitations: Some(self.include_language_limitations),
            }),
        };
        toml::to_string_pretty(&wrapped).map_err(|e| {
            VerifyError::InvalidInput(format!("config TOML serialize: {e}"))
        })
    }

    /// Build the JSON `report_metadata` block surfaced inside a
    /// batch report. Returns `None` when no agency-side metadata
    /// is configured (the caller can omit the block in that case
    /// to keep the report minimal).
    pub fn metadata_json(&self, generated_at: &str) -> Option<serde_json::Value> {
        if self.agency_name.is_none()
            && self.case_number.is_none()
            && self.examiner_name.is_none()
            && self.classification.is_none()
            && self.report_title.is_none()
        {
            return None;
        }
        Some(serde_json::json!({
            "agency": self.agency_name,
            "case_number": self.case_number,
            "examiner": self.examiner_name,
            "badge": self.examiner_badge,
            "classification": self.classification,
            "report_title": self.report_title,
            "generated_at": generated_at,
            "verify_version": env!("CARGO_PKG_VERSION"),
        }))
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TomlConfig {
    #[serde(default)]
    report: Option<ReportConfig>,
    #[serde(default)]
    output: Option<TomlOutput>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TomlOutput {
    include_confidence_tiers: Option<bool>,
    include_language_limitations: Option<bool>,
}

// ── HTML rendering ──────────────────────────────────────────────

const HTML_HEAD: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>VERIFY — Foreign Language Analysis</title>
<style>
body { font-family: system-ui, sans-serif; max-width: 1100px; margin: 1.5rem auto; padding: 0 1rem; color: #222; }
h1 { font-size: 1.5rem; margin: 0 0 0.25rem 0; }
.classification { color: #b00; font-weight: 600; text-align: center; padding: 0.4rem; border: 1px solid #b00; margin-bottom: 1rem; }
.advisory { color: #b00; background: #ffeaea; border: 1px solid #b00; padding: 0.6rem 0.8rem; margin: 1rem 0; }
.meta { background: #f6f6f6; padding: 0.6rem 0.9rem; border-radius: 4px; margin-bottom: 1rem; }
.meta dt { font-weight: 600; }
.meta dl { margin: 0; display: grid; grid-template-columns: 9rem 1fr; gap: 0.2rem 0.8rem; }
table { border-collapse: collapse; width: 100%; margin: 0.5rem 0; }
th, td { padding: 0.4rem 0.6rem; border: 1px solid #ccc; text-align: left; vertical-align: top; }
th { background: #eee; }
tr.foreign { background: #fffaef; }
tr.errored { background: #ffecec; }
.tier-HIGH { color: #060; }
.tier-MEDIUM { color: #a60; }
.tier-LOW { color: #b00; }
small.advisory-line { color: #a60; }
footer { margin-top: 2rem; padding-top: 1rem; border-top: 1px solid #ccc; color: #555; font-size: 0.85rem; }
</style>
</head>
<body>
"#;

const HTML_TAIL: &str = "</body>\n</html>\n";

/// Render a `BatchResult` plus optional `ReportConfig` to a
/// self-contained HTML document. The MT advisory is rendered
/// twice (top and bottom) so a printed copy carries it on the
/// first and last visible page.
pub fn render_batch_html(report: &BatchResult, config: &ReportConfig) -> String {
    let mut out = String::with_capacity(8 * 1024 + report.results.len() * 200);
    out.push_str(HTML_HEAD);

    if let Some(cls) = &config.classification {
        out.push_str(&format!(
            "<div class=\"classification\">{}</div>\n",
            html_escape(cls)
        ));
    }

    let title = config
        .report_title
        .as_deref()
        .unwrap_or("VERIFY Foreign Language Analysis Report");
    out.push_str(&format!("<h1>{}</h1>\n", html_escape(title)));

    // Always-visible MT advisory at the top.
    out.push_str(&format!(
        "<div class=\"advisory\">⚠ MACHINE TRANSLATION NOTICE — {}</div>\n",
        html_escape(&report.machine_translation_notice),
    ));

    // Metadata block.
    if config.agency_name.is_some()
        || config.case_number.is_some()
        || config.examiner_name.is_some()
    {
        out.push_str("<div class=\"meta\"><dl>\n");
        push_meta_row(&mut out, "Agency", config.agency_name.as_deref());
        push_meta_row(&mut out, "Case Number", config.case_number.as_deref());
        push_meta_row(&mut out, "Examiner", config.examiner_name.as_deref());
        push_meta_row(&mut out, "Badge", config.examiner_badge.as_deref());
        push_meta_row(
            &mut out,
            "Generated",
            Some(report.generated_at.as_str()),
        );
        push_meta_row(
            &mut out,
            "Target language",
            Some(report.target_language.as_str()),
        );
        out.push_str("</dl></div>\n");
    }

    // Summary table.
    if let Some(s) = &report.summary {
        out.push_str("<h2>Summary</h2>\n");
        out.push_str("<table>\n<tr><th>Total files</th><td>");
        out.push_str(&s.total_files.to_string());
        out.push_str("</td></tr>\n<tr><th>Processed</th><td>");
        out.push_str(&s.processed.to_string());
        out.push_str("</td></tr>\n<tr><th>Foreign-language</th><td>");
        out.push_str(&s.foreign_language_files.to_string());
        out.push_str("</td></tr>\n<tr><th>Translated</th><td>");
        out.push_str(&s.translated_files.to_string());
        out.push_str("</td></tr>\n<tr><th>Errors</th><td>");
        out.push_str(&s.errors.to_string());
        out.push_str("</td></tr>\n<tr><th>Languages detected</th><td>");
        for (i, (k, v)) in s.languages_detected.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{}: {}", html_escape(k), v));
        }
        out.push_str(&format!(
            "</td></tr>\n<tr><th>Processing time</th><td>{:.1} s</td></tr>\n",
            s.processing_time_seconds
        ));
        out.push_str("</table>\n");
    }

    // Sprint 8 P2 — language summary block (counts + dominant
    // foreign language) when the report carries grouping info.
    if !report.language_groups.is_empty() {
        out.push_str("<h2>Language summary</h2>\n<table>\n");
        out.push_str("<tr><th>Code</th><th>Language</th><th>Files</th><th>Words</th></tr>\n");
        for g in &report.language_groups {
            out.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                html_escape(&g.language_code),
                html_escape(&g.language_name),
                g.file_count,
                g.total_words,
            ));
        }
        out.push_str("</table>\n");
        if let Some(dom) = &report.dominant_language {
            out.push_str(&format!(
                "<p>Dominant foreign language: <strong>{}</strong></p>\n",
                html_escape(dom)
            ));
        }
    }

    // Per-language sections (Sprint 8 P2). When more than one
    // language is detected, render each group with its own
    // heading + per-group MT advisory so a printed copy still
    // makes the advisory unmissable per section.
    if report.language_groups.len() > 1 {
        for g in &report.language_groups {
            out.push_str(&format!(
                "<h2>{} Evidence ({} files)</h2>\n",
                html_escape(&g.language_name),
                g.file_count,
            ));
            out.push_str(&format!(
                "<div class=\"advisory\">⚠ MACHINE TRANSLATION NOTICE — {}</div>\n",
                html_escape(&report.machine_translation_notice),
            ));
            push_results_table(&mut out, &g.files, config);
        }
    }

    // Per-file results (full table — emitted always, including
    // when per-language sections rendered above; the global
    // table is the canonical row-by-row reference).
    out.push_str("<h2>Per-file results</h2>\n");
    push_results_table(&mut out, &report.results, config);

    // Bottom advisory — always present.
    out.push_str(&format!(
        "<div class=\"advisory\">⚠ MACHINE TRANSLATION NOTICE — {}</div>\n",
        html_escape(&report.machine_translation_notice),
    ));

    if config.include_language_limitations {
        out.push_str("<footer><p>For known classifier limitations \
                      (Pashto/Persian confusion, short-text reliability) \
                      see <code>docs/LANGUAGE_LIMITATIONS.md</code> in \
                      the VERIFY repository.</p></footer>\n");
    }

    out.push_str(HTML_TAIL);
    out
}

/// Helper used by both the global per-file table and the
/// per-language sections (Sprint 8 P2).
fn push_results_table(
    out: &mut String,
    files: &[crate::pipeline::BatchFileResult],
    config: &ReportConfig,
) {
    out.push_str("<table>\n<tr><th>File</th><th>Type</th><th>Lang</th>");
    if config.include_confidence_tiers {
        out.push_str("<th>Confidence</th>");
    }
    out.push_str("<th>Foreign?</th><th>Source</th><th>Translation</th></tr>\n");
    for r in files {
        let row_class = if r.error.is_some() {
            "errored"
        } else if r.is_foreign {
            "foreign"
        } else {
            ""
        };
        out.push_str(&format!("<tr class=\"{row_class}\"><td>"));
        out.push_str(&html_escape(&r.file_path));
        out.push_str("</td><td>");
        out.push_str(&html_escape(&r.input_type));
        out.push_str("</td><td>");
        out.push_str(&html_escape(&r.detected_language));
        out.push_str("</td>");
        if config.include_confidence_tiers {
            let tier = r.confidence_tier.as_str();
            out.push_str(&format!(
                "<td><span class=\"tier-{tier}\">{}</span>",
                html_escape(tier)
            ));
            if let Some(adv) = &r.confidence_advisory {
                out.push_str(&format!(
                    "<br><small class=\"advisory-line\">{}</small>",
                    html_escape(adv)
                ));
            }
            out.push_str("</td>");
        }
        out.push_str(&format!(
            "<td>{}</td><td>{}</td><td>{}</td></tr>\n",
            if r.is_foreign { "yes" } else { "no" },
            html_escape(r.source_text.as_deref().unwrap_or("")),
            html_escape(
                r.translated_text
                    .as_deref()
                    .or(r.error.as_deref())
                    .unwrap_or("")
            ),
        ));
    }
    out.push_str("</table>\n");
}

fn push_meta_row(buf: &mut String, label: &str, value: Option<&str>) {
    if let Some(v) = value {
        buf.push_str(&format!(
            "<dt>{}</dt><dd>{}</dd>\n",
            html_escape(label),
            html_escape(v)
        ));
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{BatchFileResult, BatchResult};

    fn empty_report() -> BatchResult {
        BatchResult {
            generated_at: "2026-04-26T08:15:32Z".into(),
            total_files: 1,
            processed: 1,
            foreign_language: 1,
            translated: 1,
            errors: 0,
            target_language: "en".into(),
            machine_translation_notice: "MT — verify with human translator.".into(),
            language_groups: Vec::new(),
            dominant_language: None,
            results: vec![BatchFileResult {
                file_path: "/ev/a.mp3".into(),
                input_type: "audio".into(),
                detected_language: "ar".into(),
                is_foreign: true,
                confidence_tier: "HIGH".into(),
                confidence_advisory: None,
                source_text: Some("مرحبا".into()),
                translated_text: Some("Hello".into()),
                segments: None,
                error: None,
            }],
            summary: None,
        }
    }

    #[test]
    fn report_config_loads_from_toml() {
        let toml_str = r#"
            [report]
            agency_name = "Wolfmark Systems"
            case_number = "2026-001"
            examiner_name = "D. Examiner"
            examiner_badge = "12345"
            classification = "UNCLASSIFIED // FOR OFFICIAL USE ONLY"
            report_title = "VERIFY Foreign Language Analysis Report"

            [output]
            include_confidence_tiers = true
            include_language_limitations = true
        "#;
        let cfg = ReportConfig::from_toml_str(toml_str).expect("parse");
        assert_eq!(cfg.agency_name.as_deref(), Some("Wolfmark Systems"));
        assert_eq!(cfg.case_number.as_deref(), Some("2026-001"));
        assert_eq!(cfg.examiner_name.as_deref(), Some("D. Examiner"));
        assert!(cfg.include_confidence_tiers);
        assert!(cfg.include_language_limitations);
    }

    #[test]
    fn config_pins_mt_advisory_true_even_if_user_writes_false() {
        // Forensic invariant — `include_mt_advisory = false` in
        // the on-disk TOML must NOT disable the advisory. The
        // loader pins it back to true.
        let toml_str = r#"
            [report]
            include_mt_advisory = false
        "#;
        let cfg = ReportConfig::from_toml_str(toml_str).expect("parse");
        assert!(
            cfg.include_mt_advisory,
            "MT advisory must remain true regardless of TOML"
        );
    }

    #[test]
    fn json_metadata_includes_agency_when_configured() {
        let mut cfg = ReportConfig::blank();
        cfg.agency_name = Some("Wolfmark Systems".into());
        cfg.case_number = Some("2026-001".into());
        let meta = cfg
            .metadata_json("2026-04-26T00:00:00Z")
            .expect("metadata present");
        let serialized = serde_json::to_string(&meta).unwrap();
        assert!(serialized.contains("Wolfmark Systems"));
        assert!(serialized.contains("2026-001"));
        assert!(serialized.contains("verify_version"));
    }

    #[test]
    fn json_metadata_none_when_blank() {
        assert!(ReportConfig::blank()
            .metadata_json("2026-04-26T00:00:00Z")
            .is_none());
    }

    #[test]
    fn html_report_contains_mt_advisory() {
        // Forensic invariant — the advisory appears in the
        // rendered HTML even with a blank config.
        let html = render_batch_html(&empty_report(), &ReportConfig::blank());
        assert!(html.contains("MACHINE TRANSLATION NOTICE"));
        // It appears twice (top + bottom).
        let count = html.matches("MACHINE TRANSLATION NOTICE").count();
        assert_eq!(count, 2, "advisory must appear top + bottom");
    }

    #[test]
    fn html_report_renders_classification_marking() {
        let mut cfg = ReportConfig::blank();
        cfg.classification = Some("UNCLASSIFIED // FOR OFFICIAL USE ONLY".into());
        let html = render_batch_html(&empty_report(), &cfg);
        assert!(html.contains("UNCLASSIFIED // FOR OFFICIAL USE ONLY"));
        assert!(html.contains("class=\"classification\""));
    }

    #[test]
    fn html_escapes_user_supplied_strings() {
        let mut cfg = ReportConfig::blank();
        cfg.agency_name = Some("Acme<script>alert(1)</script>".into());
        let html = render_batch_html(&empty_report(), &cfg);
        assert!(!html.contains("<script>"), "user strings must be escaped");
        assert!(html.contains("Acme&lt;script&gt;"));
    }

    #[test]
    fn html_renders_per_language_sections_with_advisory_each() {
        // Sprint 8 P2 — when the report carries multiple language
        // groups, the HTML renderer emits a per-group section
        // with its own MT advisory line, plus the dominant-
        // language summary block.
        let mut report = empty_report();
        report.language_groups = vec![
            crate::pipeline::LanguageGroup {
                language_code: "ar".into(),
                language_name: "Arabic".into(),
                file_count: 8,
                total_words: 1247,
                files: report.results.clone(),
            },
            crate::pipeline::LanguageGroup {
                language_code: "zh".into(),
                language_name: "Chinese".into(),
                file_count: 3,
                total_words: 445,
                files: report.results.clone(),
            },
        ];
        report.dominant_language = Some("ar".into());
        let html = render_batch_html(&report, &ReportConfig::blank());
        assert!(html.contains("Arabic Evidence (8 files)"));
        assert!(html.contains("Chinese Evidence (3 files)"));
        assert!(html.contains("Language summary"));
        assert!(html.contains("Dominant foreign language"));
        // Each section emits its own advisory; combined with top
        // + bottom the count must be ≥ 4 (top + 2 sections + bottom).
        let n = html.matches("MACHINE TRANSLATION NOTICE").count();
        assert!(
            n >= 4,
            "expected ≥4 MT advisories (top + per-section + bottom), got {n}"
        );
    }

    #[test]
    fn config_round_trip_through_toml() {
        let mut cfg = ReportConfig::blank();
        cfg.agency_name = Some("X".into());
        cfg.case_number = Some("Y".into());
        let body = cfg.to_toml_string().expect("serialize");
        let parsed = ReportConfig::from_toml_str(&body).expect("re-parse");
        assert_eq!(parsed.agency_name, cfg.agency_name);
        assert_eq!(parsed.case_number, cfg.case_number);
        assert!(parsed.include_mt_advisory);
    }
}
