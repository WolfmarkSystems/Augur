//! AUGUR Installer Wizard — Tauri 2 desktop app.
//!
//! Sprint 11. Standalone application that downloads the AUGUR
//! model bundle for an examiner, sets up bundled binaries, and
//! hands off to the main AUGUR app. See `AUGUR_SPRINT_11.md` for
//! the spec.

pub mod bundled;
pub mod download;
pub mod installer;
pub mod profiles;
pub mod python;

use serde_json::Value as JsonValue;
use tauri::{AppHandle, Emitter};

use crate::profiles::Profile;

#[tauri::command]
async fn get_profile_components(profile: Profile) -> Result<Vec<JsonValue>, String> {
    let comps = profiles::components_for_profile(&profile);
    Ok(comps
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "name": c.name,
                "description": c.description,
                "sizeDisplay": c.size_display,
                "sizeBytes": c.size_bytes,
                "isBundled": c.is_bundled,
                "componentType": format!("{:?}", c.component_type),
                "downloadUrl": c.download_url,
            })
        })
        .collect())
}

#[tauri::command]
async fn get_total_size(profile: Profile) -> u64 {
    profiles::total_size_for_profile(&profile)
}

#[tauri::command]
async fn check_existing_installation(app: AppHandle) -> JsonValue {
    installer::check_existing(&app).await
}

#[tauri::command]
async fn start_installation(app: AppHandle, profile: Profile) -> Result<(), String> {
    let components = profiles::components_for_profile(&profile);
    let total = components.len();

    for (idx, component) in components.iter().enumerate() {
        let _ = app.emit(
            "install-component-start",
            serde_json::json!({
                "id": component.id,
                "index": idx,
                "total": total,
            }),
        );

        if let Err(e) = installer::install_one(&app, component).await {
            let _ = app.emit(
                "install-component-error",
                serde_json::json!({
                    "id": component.id,
                    "index": idx,
                    "error": e,
                }),
            );
            return Err(e);
        }

        let _ = app.emit(
            "install-component-done",
            serde_json::json!({
                "id": component.id,
                "index": idx,
            }),
        );
    }

    installer::write_manifest(&app, &profile).await?;
    let _ = app.emit("install-complete", serde_json::json!({}));
    Ok(())
}

#[tauri::command]
async fn launch_augur(app: AppHandle) -> Result<(), String> {
    // Launch the main AUGUR.app via macOS `open`. Falls through
    // gracefully on non-macOS hosts; in that case the installer
    // simply exits and the user runs `augur` from the shell.
    #[cfg(target_os = "macos")]
    {
        let augur_path = "/Applications/AUGUR.app";
        tokio::process::Command::new("open")
            .arg(augur_path)
            .spawn()
            .map_err(|e| format!("Could not launch AUGUR: {e}"))?;
    }
    app.exit(0);
    Ok(())
}

/// Entry point shared by the binary (`src/main.rs`) and any
/// future test harnesses. Splitting the Tauri builder out of
/// `main` lets us unit-test the command surface in isolation.
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_profile_components,
            get_total_size,
            check_existing_installation,
            start_installation,
            launch_augur,
        ])
        .run(tauri::generate_context!())
        .expect("AUGUR Installer failed to launch");
}
