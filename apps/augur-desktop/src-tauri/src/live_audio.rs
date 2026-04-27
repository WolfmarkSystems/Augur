//! Sprint 19 P2 — drive `augur live` from the desktop GUI.
//!
//! `start_live_translation` spawns the CLI with stdin piped so
//! we can stop it cleanly by closing the pipe (the CLI's
//! background stdin-watcher thread sets the global stop flag
//! the instant stdin closes). `stop_live_translation` drops the
//! handle held in shared state, which closes stdin.

use std::sync::Arc;

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::pipeline::find_augur_binary;

#[derive(Default)]
pub struct LiveSessionState {
    /// The running `augur live` child. Holding it lets us close
    /// stdin (and SIGKILL on stop) atomically.
    child: Arc<Mutex<Option<Child>>>,
}

#[tauri::command]
pub async fn start_live_translation(
    app: AppHandle,
    state: tauri::State<'_, LiveSessionState>,
    target_lang: String,
    chunk_duration_ms: u64,
) -> Result<(), String> {
    {
        let guard = state.child.lock().await;
        if guard.is_some() {
            return Err("Live session already running".into());
        }
    }
    let augur = find_augur_binary()
        .ok_or_else(|| "AUGUR CLI not found.".to_string())?;
    let mut cmd = Command::new(&augur);
    cmd.arg("live")
        .arg("--target")
        .arg(&target_lang)
        .arg("--chunk-ms")
        .arg(chunk_duration_ms.to_string())
        .arg("--format")
        .arg("ndjson")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start `augur live`: {e}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "no stdout from `augur live`".to_string())?;
    let app_for_task = app.clone();
    let child_arc = Arc::clone(&state.child);
    {
        let mut guard = child_arc.lock().await;
        *guard = Some(child);
    }
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let json: Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(e) => {
                            log::warn!("non-JSON live line {line:?} ({e})");
                            continue;
                        }
                    };
                    match json
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                    {
                        "live_started" => {
                            let _ = app_for_task.emit("live-started", &json);
                        }
                        "live_segment" => {
                            let _ = app_for_task.emit("live-segment", &json);
                        }
                        "live_chunk_error" => {
                            let _ = app_for_task.emit("live-chunk-error", &json);
                        }
                        "live_stopped" => {
                            let _ = app_for_task.emit("live-stopped", &json);
                            break;
                        }
                        other => log::warn!("unknown live event type: {other:?}"),
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    log::warn!("live stdout error: {e}");
                    break;
                }
            }
        }
        // Drain anything left and reap the child.
        let mut guard = child_arc.lock().await;
        if let Some(mut c) = guard.take() {
            // Closing stdin is the documented stop signal; if
            // it's still open here (the child exited on its
            // own) the drop is a no-op.
            drop(c.stdin.take());
            // Best-effort wait so we don't leak zombies.
            let _ = c.wait().await;
        }
        let _ = app_for_task.emit("live-stopped", serde_json::json!({}));
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_live_translation(
    state: tauri::State<'_, LiveSessionState>,
) -> Result<(), String> {
    let mut guard = state.child.lock().await;
    if let Some(child) = guard.as_mut() {
        // Drop stdin → the CLI's stdin-watcher sets STOP and
        // the chunk loop exits cleanly.
        drop(child.stdin.take());
        // Give the CLI a moment to flush the `live_stopped`
        // event, then ensure we kill if it's still alive.
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(2_000),
            child.wait(),
        )
        .await;
        // Force-kill if still running.
        let _ = child.start_kill();
    }
    *guard = None;
    Ok(())
}
