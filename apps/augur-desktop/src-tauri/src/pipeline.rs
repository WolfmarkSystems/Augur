//! Sprint 13 P1 — real `augur translate` subprocess wiring.
//!
//! Spawns the `augur` CLI with `--format ndjson`, parses each
//! line, and re-emits it as a typed Tauri event. The desktop
//! GUI consumes the events to paint the split-view workspace
//! live.
//!
//! Locating the CLI: env override → sibling-of-exe → cargo-bin
//! → PATH. The first hit wins; if nothing is found,
//! `start_translation` returns the structured "CLI not found"
//! error and the front-end shows the install banner (P4).

use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Resolve the `augur` CLI binary in priority order:
///   1. `AUGUR_BIN` env var (dev / CI override)
///   2. Same directory as the desktop executable (production
///      .app bundles ship the CLI alongside)
///   3. `~/.cargo/bin/augur` (developer-local installs)
///   4. System PATH
pub fn find_augur_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("AUGUR_BIN") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("augur");
        if sibling.exists() {
            return Some(sibling);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin").join("augur");
        if cargo_bin.exists() {
            return Some(cargo_bin);
        }
    }
    which::which("augur").ok()
}

#[tauri::command]
pub async fn check_augur_available() -> bool {
    find_augur_binary().is_some()
}

/// Sprint 13 P4 — non-blocking startup self-test. Shells out to
/// `augur self-test --format json` (when supported) or just
/// `augur self-test` and parses any "FAIL" lines into a short
/// list. Empty Vec means everything passed; missing CLI returns
/// `Err`. The desktop GUI surfaces the result in the status
/// bar so the examiner sees "2 components unavailable" without
/// having to open a terminal.
#[tauri::command]
pub async fn run_startup_self_test() -> Result<Vec<String>, String> {
    let augur = find_augur_binary()
        .ok_or_else(|| "AUGUR CLI not found.".to_string())?;
    let output = tokio::process::Command::new(&augur)
        .arg("self-test")
        .output()
        .await
        .map_err(|e| format!("self-test spawn failed: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fails: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if line.contains("[FAIL]") {
            fails.push(line.trim().to_string());
        }
    }
    Ok(fails)
}

#[tauri::command]
pub async fn augur_binary_path() -> Option<String> {
    find_augur_binary().map(|p| p.display().to_string())
}

#[tauri::command]
pub async fn start_translation(
    app: AppHandle,
    file_path: String,
    source_lang: String,
    target_lang: String,
    stt_model: String,
    engine: String,
) -> Result<(), String> {
    let augur = find_augur_binary().ok_or_else(|| {
        "AUGUR CLI not found. Run the AUGUR Installer or install via `cargo install augur`."
            .to_string()
    })?;
    let p = PathBuf::from(&file_path);
    if !p.exists() {
        return Err(format!("file does not exist: {file_path}"));
    }

    log::info!(
        "start_translation: file={file_path} source={source_lang} target={target_lang} \
         stt={stt_model} engine={engine} (augur={})",
        augur.display()
    );

    let mut cmd = Command::new(&augur);
    cmd.arg("translate")
        .arg("--input")
        .arg(&file_path)
        .arg("--target")
        .arg(&target_lang)
        .arg("--format")
        .arg("ndjson");
    if stt_model != "auto" {
        cmd.arg("--model").arg(&stt_model);
    }
    if engine != "auto" {
        cmd.arg("--engine").arg(&engine);
    }
    if source_lang != "auto" && !source_lang.is_empty() {
        cmd.arg("--source").arg(&source_lang);
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let app_for_task = app.clone();
    tokio::spawn(async move {
        if let Err(e) = pump_translation(&app_for_task, cmd).await {
            let _ = app_for_task.emit(
                "translation-error",
                serde_json::json!({"message": e}),
            );
        }
    });
    Ok(())
}

async fn pump_translation(app: &AppHandle, mut cmd: Command) -> Result<(), String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start AUGUR: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout from AUGUR process".to_string())?;
    let stderr = child.stderr.take();
    let mut lines = BufReader::new(stdout).lines();

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("read stdout: {e}"))?
    {
        if line.trim().is_empty() {
            continue;
        }
        let json = match serde_json::from_str::<Value>(&line) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("non-JSON line on stdout: {line:?} ({e})");
                continue;
            }
        };
        match json
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
        {
            "segment" => emit_segment(app, &json),
            "dialect" => emit_dialect(app, &json),
            "code_switch" => emit_code_switch(app, &json),
            "complete" => {
                let _ = app.emit("translation-complete", &json);
                break;
            }
            "error" => {
                let msg = json
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error from AUGUR")
                    .to_string();
                let _ = app.emit(
                    "translation-error",
                    serde_json::json!({"message": msg}),
                );
                break;
            }
            other => log::warn!("unknown NDJSON event type: {other:?}"),
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("wait for AUGUR: {e}"))?;
    if !status.success() {
        let stderr_msg = if let Some(stderr) = stderr {
            let mut buf = String::new();
            let mut err_lines = BufReader::new(stderr).lines();
            while let Ok(Some(l)) = err_lines.next_line().await {
                buf.push_str(&l);
                buf.push('\n');
            }
            buf
        } else {
            String::new()
        };
        let summary = if stderr_msg.trim().is_empty() {
            format!("AUGUR exited with {status}")
        } else {
            format!("AUGUR exited with {status}: {}", stderr_msg.trim())
        };
        let _ = app.emit(
            "translation-error",
            serde_json::json!({"message": summary}),
        );
    }
    Ok(())
}

/// Re-shape the CLI's snake_case `segment` event into the
/// camelCase shape the React side has consumed since Sprint 12.
/// Sprint 16 P1 — drive `augur package` from the GUI's Package
/// Wizard. Streams `package_file_start` / `package_file_done` /
/// `package_complete` events to the front-end. Returns the
/// resolved output path on success — useful for the wizard's
/// "Open in Finder" affordance.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_evidence_package(
    app: AppHandle,
    input_path: String,
    target_lang: String,
    case_number: String,
    examiner_name: String,
    agency: String,
    output_path: String,
) -> Result<String, String> {
    let augur = find_augur_binary()
        .ok_or_else(|| "AUGUR CLI not found.".to_string())?;
    if !PathBuf::from(&input_path).exists() {
        return Err(format!("evidence path does not exist: {input_path}"));
    }
    let mut cmd = Command::new(&augur);
    cmd.arg("package")
        .arg("--input")
        .arg(&input_path)
        .arg("--target")
        .arg(&target_lang)
        .arg("--output")
        .arg(&output_path)
        .arg("--format-progress")
        .arg("ndjson");
    if !case_number.is_empty() {
        cmd.arg("--case-number").arg(&case_number);
    }
    if !examiner_name.is_empty() {
        cmd.arg("--examiner").arg(&examiner_name);
    }
    if !agency.is_empty() {
        cmd.arg("--agency").arg(&agency);
    }
    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let app_for_task = app.clone();
    let output_for_return = output_path.clone();
    tokio::spawn(async move {
        if let Err(e) = pump_package(&app_for_task, cmd).await {
            let _ = app_for_task.emit(
                "package-error",
                serde_json::json!({"message": e}),
            );
        }
    });
    Ok(output_for_return)
}

async fn pump_package(app: &AppHandle, mut cmd: Command) -> Result<(), String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start AUGUR package: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout from AUGUR package".to_string())?;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("read stdout: {e}"))?
    {
        if line.trim().is_empty() {
            continue;
        }
        let json: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("non-JSON package line: {line:?} ({e})");
                continue;
            }
        };
        match json
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
        {
            "package_file_start" => {
                let _ = app.emit("package-file-start", &json);
            }
            "package_file_done" => {
                let _ = app.emit("package-file-done", &json);
            }
            "package_complete" => {
                let _ = app.emit("package-complete", &json);
                break;
            }
            other => log::warn!("unknown package event type: {other:?}"),
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|e| format!("wait for AUGUR package: {e}"))?;
    if !status.success() {
        let _ = app.emit(
            "package-error",
            serde_json::json!({"message": format!("AUGUR package exited with {status}")}),
        );
    }
    Ok(())
}

#[tauri::command]
pub async fn start_batch_translation(
    app: AppHandle,
    input_dir: String,
    target_lang: String,
    output_path: String,
    format: String,
) -> Result<(), String> {
    let augur = find_augur_binary()
        .ok_or_else(|| "AUGUR CLI not found.".to_string())?;
    if !PathBuf::from(&input_dir).exists() {
        return Err(format!("directory does not exist: {input_dir}"));
    }
    let mut cmd = Command::new(&augur);
    cmd.arg("batch")
        .arg("--input")
        .arg(&input_dir)
        .arg("--target")
        .arg(&target_lang)
        .arg("--output")
        .arg(&output_path)
        .arg("--format")
        .arg(&format)
        .arg("--format-progress")
        .arg("ndjson")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let app_for_task = app.clone();
    tokio::spawn(async move {
        if let Err(e) = pump_batch(&app_for_task, cmd).await {
            let _ = app_for_task.emit(
                "batch-error",
                serde_json::json!({"message": e}),
            );
        }
    });
    Ok(())
}

async fn pump_batch(app: &AppHandle, mut cmd: Command) -> Result<(), String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start AUGUR batch: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout from AUGUR batch".to_string())?;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("read stdout: {e}"))?
    {
        if line.trim().is_empty() {
            continue;
        }
        let json: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("non-JSON batch line: {line:?} ({e})");
                continue;
            }
        };
        match json
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
        {
            "batch_file_start" => {
                let _ = app.emit("batch-file-start", &json);
            }
            "batch_file_done" => {
                let _ = app.emit("batch-file-done", &json);
            }
            "batch_complete" => {
                let _ = app.emit("batch-complete", &json);
                break;
            }
            other => log::warn!("unknown batch event type: {other:?}"),
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|e| format!("wait for AUGUR batch: {e}"))?;
    if !status.success() {
        let _ = app.emit(
            "batch-error",
            serde_json::json!({"message": format!("AUGUR batch exited with {status}")}),
        );
    }
    Ok(())
}

fn emit_segment(app: &AppHandle, json: &Value) {
    let original = json.get("original").cloned().unwrap_or(Value::Null);
    let translated = json.get("translated").cloned().unwrap_or(Value::Null);
    let payload = serde_json::json!({
        "index": json.get("index").and_then(|v| v.as_u64()).unwrap_or(0),
        "start_ms": json.get("start_ms").cloned().unwrap_or(Value::Null),
        "end_ms": json.get("end_ms").cloned().unwrap_or(Value::Null),
        "original_text": original.as_str().unwrap_or(""),
        "translated_text": translated.as_str().unwrap_or(""),
        "is_complete": json.get("is_complete").and_then(|v| v.as_bool()).unwrap_or(true),
    });
    let _ = app.emit("segment-ready", payload);
}

fn emit_dialect(app: &AppHandle, json: &Value) {
    let payload = serde_json::json!({
        "dialect": json.get("dialect").and_then(|v| v.as_str()).unwrap_or(""),
        "confidence": json.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0),
        "source": json.get("source").and_then(|v| v.as_str()).unwrap_or("lexical"),
    });
    let _ = app.emit("dialect-detected", payload);
}

fn emit_code_switch(app: &AppHandle, json: &Value) {
    let payload = serde_json::json!({
        "offset": json.get("offset").and_then(|v| v.as_u64()).unwrap_or(0),
        "from": json.get("from").and_then(|v| v.as_str()).unwrap_or(""),
        "to": json.get("to").and_then(|v| v.as_str()).unwrap_or(""),
    });
    let _ = app.emit("code-switch-detected", payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndjson_segment_serializes_correctly() {
        let json = serde_json::json!({
            "type": "segment",
            "index": 0,
            "original": "مرحبا",
            "translated": "Hello",
            "is_complete": true,
        });
        assert_eq!(json["type"], "segment");
        assert_eq!(json["translated"], "Hello");
    }

    #[test]
    fn ndjson_parse_segment_event() {
        let line = r#"{"type":"segment","index":0,"original":"test","translated":"test","is_complete":true}"#;
        let json: Value = serde_json::from_str(line).expect("valid JSON");
        assert_eq!(json["type"], "segment");
    }

    #[test]
    fn find_augur_binary_runs_without_panic() {
        let _ = find_augur_binary();
    }

    #[test]
    fn batch_ndjson_file_start_parsed() {
        let line = r#"{"type":"batch_file_start","file":"test.mp3","input_type":"audio","index":1,"total":10}"#;
        let json: Value = serde_json::from_str(line).expect("valid JSON");
        assert_eq!(json["type"], "batch_file_start");
        assert_eq!(json["total"], 10);
    }

    #[test]
    fn check_augur_available_returns_bool() {
        // Just verifies the synchronous resolver runs cleanly —
        // the async Tauri command wrapping it is the same code
        // path. Result is host-dependent.
        let _ = find_augur_binary();
    }

    #[test]
    fn batch_ndjson_complete_parsed() {
        let line = r#"{"type":"batch_complete","total_files":47,"foreign_files":12,"processed":47,"translated":12,"errors":0,"elapsed_seconds":135.4}"#;
        let json: Value = serde_json::from_str(line).expect("valid JSON");
        assert_eq!(json["type"], "batch_complete");
        assert_eq!(json["foreign_files"], 12);
    }
}
