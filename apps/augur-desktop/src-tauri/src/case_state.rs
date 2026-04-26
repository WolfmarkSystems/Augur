//! Sprint 16 P2 — persistent case state.
//!
//! Stores the current case number / examiner / agency plus a
//! capped recent-files list at
//! `~/Library/Application Support/AUGUR/case_state.json`.
//! All writes are atomic-via-tempfile so a crash mid-write does
//! not leave a partial JSON document on disk.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const RECENT_CAP: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecentFile {
    pub path: String,
    pub opened_at: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub file_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CaseState {
    #[serde(default)]
    pub case_number: String,
    #[serde(default)]
    pub examiner_name: String,
    #[serde(default)]
    pub agency: String,
    #[serde(default)]
    pub recent_files: Vec<RecentFile>,
    #[serde(default)]
    pub last_output_dir: String,
    /// Sprint 17 P1 — per-file flagged segments. Map keyed by
    /// the absolute file path of the evidence the flags belong
    /// to. JSON-serialized so the on-disk shape stays stable
    /// across schema additions.
    #[serde(default)]
    pub flagged_segments: std::collections::BTreeMap<String, Vec<serde_json::Value>>,
}

fn case_state_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?
        .join("AUGUR");
    Ok(dir.join("case_state.json"))
}

pub fn load(app: &AppHandle) -> CaseState {
    let path = match case_state_path(app) {
        Ok(p) => p,
        Err(_) => return CaseState::default(),
    };
    match std::fs::read_to_string(&path) {
        Ok(body) => serde_json::from_str(&body).unwrap_or_default(),
        Err(_) => CaseState::default(),
    }
}

pub fn save(app: &AppHandle, state: &CaseState) -> Result<(), String> {
    let path = case_state_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }
    let body = serde_json::to_string_pretty(state)
        .map_err(|e| format!("serialise: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("write {tmp:?}: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename: {e}"))?;
    Ok(())
}

pub fn cap_recent(files: &mut Vec<RecentFile>) {
    if files.len() > RECENT_CAP {
        files.drain(0..files.len() - RECENT_CAP);
    }
}

#[tauri::command]
pub fn get_case_state(app: AppHandle) -> serde_json::Value {
    serde_json::to_value(load(&app)).unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn set_case_info(
    app: AppHandle,
    case_number: String,
    examiner_name: String,
    agency: String,
) -> Result<(), String> {
    let mut state = load(&app);
    state.case_number = case_number;
    state.examiner_name = examiner_name;
    state.agency = agency;
    save(&app, &state)
}

#[tauri::command]
pub fn add_recent_file(
    app: AppHandle,
    path: String,
    source_lang: String,
    target_lang: String,
    file_type: String,
) -> Result<(), String> {
    let mut state = load(&app);
    // Move-to-front semantics: drop any existing entry with the
    // same path, then push the new one.
    state.recent_files.retain(|r| r.path != path);
    state.recent_files.push(RecentFile {
        path,
        opened_at: chrono::Utc::now().to_rfc3339(),
        source_lang,
        target_lang,
        file_type,
    });
    cap_recent(&mut state.recent_files);
    save(&app, &state)
}

#[tauri::command]
pub fn save_segment_flags(
    app: AppHandle,
    file_path: String,
    flags: Vec<serde_json::Value>,
) -> Result<(), String> {
    let mut state = load(&app);
    if flags.is_empty() {
        state.flagged_segments.remove(&file_path);
    } else {
        state.flagged_segments.insert(file_path, flags);
    }
    save(&app, &state)
}

#[tauri::command]
pub fn get_segment_flags(
    app: AppHandle,
    file_path: String,
) -> Vec<serde_json::Value> {
    let state = load(&app);
    state
        .flagged_segments
        .get(&file_path)
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_capped_at_ten() {
        let mut v: Vec<RecentFile> = (0..15)
            .map(|i| RecentFile {
                path: format!("/{i}"),
                opened_at: "0".into(),
                source_lang: "en".into(),
                target_lang: "ar".into(),
                file_type: "audio".into(),
            })
            .collect();
        cap_recent(&mut v);
        assert_eq!(v.len(), RECENT_CAP);
        // Oldest entries (indices 0..5) should have been dropped,
        // newest (5..15) preserved.
        assert_eq!(v[0].path, "/5");
        assert_eq!(v[9].path, "/14");
    }

    #[test]
    fn case_state_round_trips_through_json() {
        let mut s = CaseState {
            case_number: "2026-042".into(),
            examiner_name: "D. Examiner".into(),
            agency: "Wolfmark Systems".into(),
            ..Default::default()
        };
        s.recent_files.push(RecentFile {
            path: "/evidence/foo.mp3".into(),
            opened_at: "2026-04-26T16:00:00Z".into(),
            source_lang: "ar".into(),
            target_lang: "en".into(),
            file_type: "audio".into(),
        });
        let body = serde_json::to_string(&s).expect("serialise");
        let back: CaseState = serde_json::from_str(&body).expect("parse");
        assert_eq!(back.case_number, "2026-042");
        assert_eq!(back.examiner_name, "D. Examiner");
        assert_eq!(back.recent_files.len(), 1);
        assert_eq!(back.recent_files[0].path, "/evidence/foo.mp3");
    }

    #[test]
    fn empty_flag_list_removes_file_entry() {
        let mut s = CaseState::default();
        s.flagged_segments
            .insert("/foo".into(), vec![serde_json::json!({"x": 1})]);
        s.flagged_segments.insert("/bar".into(), Vec::new());
        // Mirror save_segment_flags's empty-erase semantic for
        // the unit test (the actual command goes through Tauri).
        let drop_empty = |state: &mut CaseState, path: &str, flags: Vec<serde_json::Value>| {
            if flags.is_empty() {
                state.flagged_segments.remove(path);
            } else {
                state.flagged_segments.insert(path.to_string(), flags);
            }
        };
        drop_empty(&mut s, "/foo", Vec::new());
        assert!(!s.flagged_segments.contains_key("/foo"));
    }
}
