//! Sprint 12 P7 — report export (HTML / JSON / ZIP).
//!
//! The mandatory machine-translation advisory is emitted in
//! every output format and at every supported location:
//!   - JSON: top-level `mt_advisory` field
//!   - HTML: top-of-document banner + bottom-of-document footer
//!   - ZIP : MANIFEST.json carries the advisory + every embedded
//!     HTML / JSON copy already carries it inline + the chain
//!     of custody text spells it out in prose
//!
//! No flag, setting, or input dismisses or removes it.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

pub const MT_ADVISORY: &str =
    "Machine translation — verify with a certified human translator for legal proceedings.";

#[tauri::command]
pub async fn save_report_dialog(
    app: AppHandle,
    format: String,
    case_number: String,
) -> Result<Option<String>, String> {
    let ext = match format.as_str() {
        "html" => "html",
        "json" => "json",
        "zip" => "zip",
        _ => "html",
    };
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let safe_case = sanitize_case_number(&case_number);
    let default_name = format!("AUGUR_Report_{safe_case}_{stamp}.{ext}");
    let path = app
        .dialog()
        .file()
        .set_file_name(&default_name)
        .add_filter(format.to_uppercase().as_str(), &[ext])
        .blocking_save_file();
    Ok(path.map(|p| p.to_string()))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn export_report(
    _app: AppHandle,
    format: String,
    output_path: String,
    case_number: String,
    source_lang: String,
    target_lang: String,
    dialect: Option<String>,
    segments: Vec<Value>,
    flagged_segments: Option<Vec<Value>>,
) -> Result<String, String> {
    let dest = PathBuf::from(&output_path);
    let flags = flagged_segments.unwrap_or_default();
    match format.as_str() {
        "html" => {
            export_html(&dest, &case_number, &source_lang, &target_lang, dialect.as_deref(), &segments, &flags).await
        }
        "json" => {
            export_json(&dest, &case_number, &source_lang, &target_lang, dialect.as_deref(), &segments, &flags).await
        }
        "zip" => {
            export_zip(&dest, &case_number, &source_lang, &target_lang, dialect.as_deref(), &segments, &flags).await
        }
        other => Err(format!("Unknown format: {other}")),
    }
}

fn sanitize_case_number(case: &str) -> String {
    case.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_html(
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
    flags: &[Value],
) -> String {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
    let mut body = String::new();
    body.push_str(&format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\" />\n\
         <title>AUGUR Report — {}</title>\n\
         <style>body{{font-family:-apple-system,BlinkMacSystemFont,sans-serif;\
         max-width:880px;margin:36px auto;color:#1a1a1a;line-height:1.5;padding:0 22px}}\
         h1{{color:#085041;margin:0 0 6px 0}}\
         .meta{{color:#5b6770;font-size:13px;margin-bottom:18px}}\
         .advisory{{background:#FFF6E6;border:1px solid #BA7517;color:#5C3502;\
         padding:12px 16px;border-radius:6px;margin:18px 0;font-size:13px}}\
         .review-banner{{background:#FBE9E7;border:1px solid #B3261E;color:#700;\
         padding:12px 16px;border-radius:6px;margin:18px 0;font-size:13px;font-weight:600}}\
         .flagged-segment{{background:#FBE9E7;border-left:3px solid #B3261E;\
         padding:10px 14px;margin:10px 0;border-radius:4px;font-size:13px}}\
         .flagged-segment .timestamp{{color:#5b6770;font-size:11px}}\
         .flagged-segment .original{{margin:6px 0;color:#1a1a1a}}\
         .flagged-segment .translation{{margin:6px 0;color:#0F5038;font-style:italic}}\
         .flagged-segment .examiner-note{{font-size:12px;color:#5b6770;\
         margin-top:6px;padding:6px 8px;background:#fff;border-radius:4px}}\
         .flagged-segment .review-status{{display:inline-block;font-size:10px;\
         font-weight:700;text-transform:uppercase;padding:2px 8px;border-radius:99px;\
         background:#B3261E;color:#fff;margin-top:4px}}\
         table{{width:100%;border-collapse:collapse;margin:18px 0;font-size:14px}}\
         th,td{{text-align:left;padding:8px 10px;border-bottom:1px solid #eee;vertical-align:top}}\
         th{{background:#f6f7f7;font-weight:600;color:#5b6770}}\
         .ts{{font-variant-numeric:tabular-nums;color:#5b6770;width:80px}}\
         .src{{color:#1a1a1a}}.tgt{{color:#085041}}\
         .dialect{{background:#E1F5EE;border:1px solid #1D9E75;padding:10px 14px;\
         border-radius:6px;margin:14px 0;font-size:13px}}\
         footer{{color:#5b6770;font-size:12px;margin-top:36px;border-top:1px solid #eee;\
         padding-top:14px}}</style>\n</head>\n<body>\n",
        html_escape(case)
    ));
    body.push_str(&format!(
        "<div class=\"advisory\"><strong>Forensic notice:</strong> {}</div>\n",
        html_escape(MT_ADVISORY)
    ));
    if !flags.is_empty() {
        body.push_str(&format!(
            "<div class=\"review-banner\">⚠ {} segment{} flagged for human review — see Section 2 below.</div>\n",
            flags.len(),
            if flags.len() == 1 { "" } else { "s" }
        ));
    }
    body.push_str("<h1>AUGUR Translation Report</h1>\n");
    body.push_str(&format!(
        "<div class=\"meta\">Wolfmark Systems · Generated {stamp}<br/>\
         Case: <strong>{}</strong> · Source: <code>{}</code> → Target: <code>{}</code></div>\n",
        html_escape(case),
        html_escape(src),
        html_escape(tgt)
    ));
    if let Some(d) = dialect {
        body.push_str(&format!(
            "<div class=\"dialect\"><strong>Dialect identified:</strong> {} \
             <em>(verify with a human Arabic linguist)</em></div>\n",
            html_escape(d)
        ));
    }
    if !flags.is_empty() {
        body.push_str(
            "<section id=\"segments-requiring-review\">\n\
             <h2>2. Segments Requiring Human Review</h2>\n\
             <p>The following segments were flagged by the examiner as requiring \
             verification by a certified human linguist. Translations below are \
             marked <strong>[PENDING HUMAN REVIEW]</strong> and must not be relied on \
             without independent confirmation.</p>\n",
        );
        for flag in flags {
            let idx = flag
                .get("segmentIndex")
                .and_then(|v| v.as_u64())
                .or_else(|| flag.get("segment_index").and_then(|v| v.as_u64()))
                .unwrap_or(0);
            let seg = segments.iter().find(|s| {
                s.get("index").and_then(|v| v.as_u64()).unwrap_or(0) == idx
            });
            let original = seg
                .and_then(|s| s.get("originalText").and_then(|v| v.as_str()))
                .unwrap_or("");
            let translated = seg
                .and_then(|s| s.get("translatedText").and_then(|v| v.as_str()))
                .unwrap_or("");
            let start = seg
                .and_then(|s| s.get("startMs").and_then(|v| v.as_u64()));
            let end = seg
                .and_then(|s| s.get("endMs").and_then(|v| v.as_u64()));
            let ts_range = match (start, end) {
                (Some(s), Some(e)) => format!(
                    "{:02}:{:02} — {:02}:{:02}",
                    s / 60_000,
                    (s / 1000) % 60,
                    e / 60_000,
                    (e / 1000) % 60,
                ),
                _ => "—".into(),
            };
            let note = flag
                .get("examinerNote")
                .and_then(|v| v.as_str())
                .or_else(|| flag.get("examiner_note").and_then(|v| v.as_str()))
                .unwrap_or("");
            let status = flag
                .get("reviewStatus")
                .and_then(|v| v.as_str())
                .or_else(|| flag.get("review_status").and_then(|v| v.as_str()))
                .unwrap_or("needs_review");
            body.push_str(&format!(
                "<div class=\"flagged-segment\">\n\
                 <div class=\"timestamp\">Segment {idx} · {ts}</div>\n\
                 <div class=\"original\">{orig}</div>\n\
                 <div class=\"translation\">[PENDING HUMAN REVIEW] {trans}</div>\n",
                idx = idx + 1,
                ts = html_escape(&ts_range),
                orig = html_escape(original),
                trans = html_escape(translated),
            ));
            if !note.is_empty() {
                body.push_str(&format!(
                    "<div class=\"examiner-note\"><strong>Examiner note:</strong> {}</div>\n",
                    html_escape(note)
                ));
            }
            body.push_str(&format!(
                "<div class=\"review-status\">Status: {}</div>\n</div>\n",
                html_escape(&status.replace('_', " ").to_uppercase())
            ));
        }
        body.push_str("</section>\n<h2>3. Full Translation</h2>\n");
    }
    body.push_str("<table>\n<thead><tr><th>#</th><th>Time</th><th>Original</th><th>Translation</th></tr></thead>\n<tbody>\n");
    for (i, seg) in segments.iter().enumerate() {
        let original = seg.get("originalText").and_then(|v| v.as_str()).unwrap_or("");
        let translated = seg.get("translatedText").and_then(|v| v.as_str()).unwrap_or("");
        let start = seg.get("startMs").and_then(|v| v.as_u64());
        let ts = start
            .map(|ms| {
                let s = ms / 1000;
                format!("{:02}:{:02}", s / 60, s % 60)
            })
            .unwrap_or_default();
        body.push_str(&format!(
            "<tr><td>{}</td><td class=\"ts\">{}</td>\
             <td class=\"src\">{}</td><td class=\"tgt\">{}</td></tr>\n",
            i + 1,
            html_escape(&ts),
            html_escape(original),
            html_escape(translated)
        ));
    }
    body.push_str("</tbody></table>\n");
    body.push_str(&format!(
        "<footer><strong>Forensic notice:</strong> {}</footer>\n",
        html_escape(MT_ADVISORY)
    ));
    body.push_str("</body>\n</html>\n");
    body
}

#[allow(clippy::too_many_arguments)]
async fn export_html(
    dest: &Path,
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
    flags: &[Value],
) -> Result<String, String> {
    let html = render_html(case, src, tgt, dialect, segments, flags);
    tokio::fs::write(dest, html)
        .await
        .map_err(|e| format!("write {dest:?}: {e}"))?;
    Ok(dest.display().to_string())
}

#[allow(clippy::too_many_arguments)]
async fn export_json(
    dest: &Path,
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
    flags: &[Value],
) -> Result<String, String> {
    let body = json_body(case, src, tgt, dialect, segments, flags);
    let pretty = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    tokio::fs::write(dest, pretty)
        .await
        .map_err(|e| format!("write {dest:?}: {e}"))?;
    Ok(dest.display().to_string())
}

fn json_body(
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
    flags: &[Value],
) -> Value {
    let flag_objects: Vec<Value> = flags
        .iter()
        .map(|f| flag_to_export_shape(f, segments))
        .collect();
    serde_json::json!({
        "mt_advisory": MT_ADVISORY,
        "augur_version": env!("CARGO_PKG_VERSION"),
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "case_number": case,
        "source_language": src,
        "target_language": tgt,
        "dialect": dialect,
        "segments": segments,
        "flagged_segments_count": flag_objects.len(),
        "flagged_segments": flag_objects,
    })
}

fn flag_to_export_shape(flag: &Value, segments: &[Value]) -> Value {
    let idx = flag
        .get("segmentIndex")
        .and_then(|v| v.as_u64())
        .or_else(|| flag.get("segment_index").and_then(|v| v.as_u64()))
        .unwrap_or(0);
    let seg = segments
        .iter()
        .find(|s| s.get("index").and_then(|v| v.as_u64()).unwrap_or(0) == idx);
    serde_json::json!({
        "segment_index": idx,
        "start_ms": seg.and_then(|s| s.get("startMs").cloned()).unwrap_or(Value::Null),
        "end_ms":   seg.and_then(|s| s.get("endMs").cloned()).unwrap_or(Value::Null),
        "original": seg
            .and_then(|s| s.get("originalText").and_then(|v| v.as_str()))
            .unwrap_or(""),
        "translation": seg
            .and_then(|s| s.get("translatedText").and_then(|v| v.as_str()))
            .unwrap_or(""),
        "review_status": flag
            .get("reviewStatus")
            .and_then(|v| v.as_str())
            .or_else(|| flag.get("review_status").and_then(|v| v.as_str()))
            .unwrap_or("needs_review"),
        "examiner_note": flag
            .get("examinerNote")
            .and_then(|v| v.as_str())
            .or_else(|| flag.get("examiner_note").and_then(|v| v.as_str()))
            .unwrap_or(""),
        "flagged_at": flag
            .get("flaggedAt")
            .and_then(|v| v.as_str())
            .or_else(|| flag.get("flagged_at").and_then(|v| v.as_str()))
            .unwrap_or(""),
        "machine_translation_notice": MT_ADVISORY,
    })
}

/// Sprint 17 P2 — human-readable summary that ships under
/// `review/REVIEW_REQUIRED.txt` in the ZIP package. Always
/// closes with the MT advisory in prose.
pub fn render_review_required_txt(flags: &[Value], segments: &[Value]) -> String {
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
    for flag in flags {
        let shape = flag_to_export_shape(flag, segments);
        let idx = shape["segment_index"].as_u64().unwrap_or(0);
        let start_ms = shape["start_ms"].as_u64();
        let end_ms = shape["end_ms"].as_u64();
        let ts_range = match (start_ms, end_ms) {
            (Some(s), Some(e)) => format!(
                "{:02}:{:02} — {:02}:{:02}",
                s / 60_000,
                (s / 1000) % 60,
                e / 60_000,
                (e / 1000) % 60,
            ),
            _ => "—".into(),
        };
        let original = shape["original"].as_str().unwrap_or("");
        let translation = shape["translation"].as_str().unwrap_or("");
        let note = shape["examiner_note"].as_str().unwrap_or("");
        let status = shape["review_status"].as_str().unwrap_or("needs_review");
        out.push_str(&format!(
            "Segment {} ({ts_range}):\n  Original: {original}\n  MT Translation: {translation}\n",
            idx + 1
        ));
        if !note.is_empty() {
            out.push_str(&format!("  Examiner note: {note}\n"));
        }
        out.push_str(&format!("  Status: {}\n\n", status.replace('_', " ").to_uppercase()));
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

#[allow(clippy::too_many_arguments)]
async fn export_zip(
    dest: &Path,
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
    flags: &[Value],
) -> Result<String, String> {
    let html = render_html(case, src, tgt, dialect, segments, flags);
    let body = json_body(case, src, tgt, dialect, segments, flags);
    let json = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    let mut files_in_zip: Vec<&'static str> = vec![
        "REPORT.html",
        "REPORT.json",
        "MANIFEST.json",
        "CHAIN_OF_CUSTODY.txt",
        "translations/",
    ];
    if !flags.is_empty() {
        files_in_zip.push("review/");
    }
    let manifest = serde_json::to_string_pretty(&serde_json::json!({
        "augur_version": env!("CARGO_PKG_VERSION"),
        "case_number": case,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "machine_translation_notice": MT_ADVISORY,
        "segment_count": segments.len(),
        "flagged_segments_count": flags.len(),
        "files": files_in_zip,
    }))
    .map_err(|e| e.to_string())?;
    let chain = format!(
        "AUGUR Chain of Custody\n=========================\n\nCase number: {case}\nGenerated: {}\nSource language: {src}\nTarget language: {tgt}\nSegments: {}\n\nForensic notice: {MT_ADVISORY}\n\nThis package was produced by AUGUR (Wolfmark Systems). Every translation\nin REPORT.html and REPORT.json was produced by an offline machine-translation\npipeline. The output is NOT a substitute for review by a certified human\ntranslator. Distribute and review accordingly.\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z"),
        segments.len()
    );

    let file = std::fs::File::create(dest).map_err(|e| format!("create {dest:?}: {e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (name, body) in [
        ("REPORT.html", html.as_str()),
        ("REPORT.json", json.as_str()),
        ("MANIFEST.json", manifest.as_str()),
        ("CHAIN_OF_CUSTODY.txt", chain.as_str()),
    ] {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(body.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    for (i, seg) in segments.iter().enumerate() {
        let translated = seg.get("translatedText").and_then(|v| v.as_str()).unwrap_or("");
        let entry = format!("translations/segment_{:04}.txt", i + 1);
        zip.start_file(&entry, opts).map_err(|e| e.to_string())?;
        zip.write_all(format!("{translated}\n\n--\n{MT_ADVISORY}\n").as_bytes())
            .map_err(|e| e.to_string())?;
    }
    // Sprint 17 P2 — review/ directory only when flags exist.
    if !flags.is_empty() {
        let review_txt = render_review_required_txt(flags, segments);
        let review_json = serde_json::to_string_pretty(&serde_json::json!({
            "machine_translation_notice": MT_ADVISORY,
            "case_number": case,
            "flagged_segments_count": flags.len(),
            "flagged_segments": flags
                .iter()
                .map(|f| flag_to_export_shape(f, segments))
                .collect::<Vec<_>>(),
        }))
        .map_err(|e| e.to_string())?;
        zip.start_file("review/REVIEW_REQUIRED.txt", opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(review_txt.as_bytes())
            .map_err(|e| e.to_string())?;
        zip.start_file("review/flagged_segments.json", opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(review_json.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(dest.display().to_string())
}

#[tauri::command]
pub fn mt_advisory_text() -> &'static str {
    MT_ADVISORY
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_blocks_xss() {
        let s = html_escape("<script>alert(1)</script>");
        assert!(!s.contains("<script>"));
        assert!(s.contains("&lt;script&gt;"));
    }

    #[test]
    fn case_sanitisation_strips_path_chars() {
        assert_eq!(sanitize_case_number("CASE-2026/04"), "CASE-2026_04");
        assert_eq!(sanitize_case_number("foo bar"), "foo_bar");
    }

    #[test]
    fn html_render_includes_mt_advisory_twice() {
        let html = render_html("CASE-1", "ar", "en", None, &[], &[]);
        let count = html.matches(MT_ADVISORY).count();
        assert!(count >= 2, "MT advisory must appear in header AND footer");
    }

    fn fixture_segment(idx: u64, original: &str, translation: &str) -> Value {
        serde_json::json!({
            "index": idx,
            "originalText": original,
            "translatedText": translation,
            "startMs": 45_000_u64 + idx * 7_000,
            "endMs": 52_000_u64 + idx * 7_000,
        })
    }

    fn fixture_flag(idx: u64, note: &str) -> Value {
        serde_json::json!({
            "segmentIndex": idx,
            "flaggedAt": "2026-04-26T16:30:00Z",
            "examinerNote": note,
            "reviewStatus": "needs_review",
        })
    }

    #[test]
    fn html_report_includes_review_banner_when_flags_present() {
        let segs = vec![fixture_segment(0, "الحزمة ستصل غداً", "The package arrives tomorrow")];
        let flags = vec![fixture_flag(0, "Verify سلاح translation")];
        let html = render_html("CASE-1", "ar", "en", None, &segs, &flags);
        assert!(html.contains("Segments Requiring Human Review"));
        assert!(html.contains("PENDING HUMAN REVIEW"));
        assert!(html.contains("Verify سلاح translation"));
    }

    #[test]
    fn html_report_no_review_banner_when_no_flags() {
        let segs = vec![fixture_segment(0, "Hi", "Hi")];
        let html = render_html("CASE-1", "en", "en", None, &segs, &[]);
        assert!(!html.contains("Segments Requiring Human Review"));
        assert!(!html.contains("PENDING HUMAN REVIEW"));
    }

    #[test]
    fn json_export_includes_flagged_segments_array() {
        let segs = vec![fixture_segment(3, "src", "tgt")];
        let flags = vec![fixture_flag(3, "Note A")];
        let body = json_body("CASE-1", "ar", "en", None, &segs, &flags);
        assert_eq!(body["flagged_segments_count"], 1);
        let arr = body["flagged_segments"].as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["segment_index"], 3);
        assert_eq!(arr[0]["machine_translation_notice"], MT_ADVISORY);
    }

    #[test]
    fn review_required_txt_contains_mt_advisory() {
        let segs = vec![fixture_segment(0, "src", "tgt")];
        let flags = vec![fixture_flag(0, "")];
        let txt = render_review_required_txt(&flags, &segs);
        assert!(txt.contains("machine-generated"));
        assert!(txt.contains("certified human"));
        assert!(txt.contains("MACHINE TRANSLATION NOTICE"));
    }
}
