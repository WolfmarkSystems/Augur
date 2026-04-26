//! AUGUR Desktop GUI — Tauri 2 main app.
//!
//! Sprint 12. Standalone application; excluded from the root
//! cargo workspace. Build:
//!   cd apps/augur-desktop/src-tauri && cargo build

pub mod export;
pub mod file_load;
pub mod models;
pub mod pipeline;
pub mod state;

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .manage(state::AppState::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            file_load::open_evidence_dialog,
            file_load::detect_file_type,
            file_load::load_file_metadata,
            models::list_models,
            models::detected_profile,
            pipeline::start_translation,
            export::save_report_dialog,
            export::export_report,
            export::mt_advisory_text,
        ])
        .run(tauri::generate_context!())
        .expect("AUGUR Desktop failed to launch");
}
