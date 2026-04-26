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
) -> Result<String, String> {
    let dest = PathBuf::from(&output_path);
    match format.as_str() {
        "html" => export_html(&dest, &case_number, &source_lang, &target_lang, dialect.as_deref(), &segments).await,
        "json" => export_json(&dest, &case_number, &source_lang, &target_lang, dialect.as_deref(), &segments).await,
        "zip" => export_zip(&dest, &case_number, &source_lang, &target_lang, dialect.as_deref(), &segments).await,
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

async fn export_html(
    dest: &Path,
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
) -> Result<String, String> {
    let html = render_html(case, src, tgt, dialect, segments);
    tokio::fs::write(dest, html)
        .await
        .map_err(|e| format!("write {dest:?}: {e}"))?;
    Ok(dest.display().to_string())
}

async fn export_json(
    dest: &Path,
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
) -> Result<String, String> {
    let body = serde_json::json!({
        "mt_advisory": MT_ADVISORY,
        "augur_version": env!("CARGO_PKG_VERSION"),
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "case_number": case,
        "source_language": src,
        "target_language": tgt,
        "dialect": dialect,
        "segments": segments,
    });
    let pretty = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    tokio::fs::write(dest, pretty)
        .await
        .map_err(|e| format!("write {dest:?}: {e}"))?;
    Ok(dest.display().to_string())
}

async fn export_zip(
    dest: &Path,
    case: &str,
    src: &str,
    tgt: &str,
    dialect: Option<&str>,
    segments: &[Value],
) -> Result<String, String> {
    let html = render_html(case, src, tgt, dialect, segments);
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "mt_advisory": MT_ADVISORY,
        "case_number": case,
        "source_language": src,
        "target_language": tgt,
        "dialect": dialect,
        "segments": segments,
    }))
    .map_err(|e| e.to_string())?;
    let manifest = serde_json::to_string_pretty(&serde_json::json!({
        "augur_version": env!("CARGO_PKG_VERSION"),
        "case_number": case,
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "machine_translation_notice": MT_ADVISORY,
        "segment_count": segments.len(),
        "files": [
            "REPORT.html",
            "REPORT.json",
            "MANIFEST.json",
            "CHAIN_OF_CUSTODY.txt",
            "translations/",
        ],
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
        let html = render_html("CASE-1", "ar", "en", None, &[]);
        let count = html.matches(MT_ADVISORY).count();
        assert!(count >= 2, "MT advisory must appear in header AND footer");
    }
}
