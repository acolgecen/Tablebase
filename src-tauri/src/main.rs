// Hide the extra console window on Windows in release builds (harmless on macOS).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tablebase — query CSV/TSV files with read-only SQL, backed by DuckDB.
//!
//! Module map (one responsibility each):
//!   - `error`       unified error type sent to the frontend
//!   - `model`       serializable DTOs shared with the UI
//!   - `cache`       on-disk cache keying + LRU eviction (shared across windows)
//!   - `ingest`      CSV/TSV -> DuckDB (the only read-write path)
//!   - `guardrail`   read-only SQL validation
//!   - `query`       read-only execution, pagination, serialization
//!   - `export`      COPY results to CSV
//!   - `environment` one window's isolated set of open files
//!   - `state`       per-window environment map + config
//!   - `commands`    thin Tauri command handlers

mod application;
mod cache;
mod commands;
mod environment;
mod error;
mod export;
mod guardrail;
mod ingest;
mod model;
mod query;
mod query_session;
mod state;

use state::AppState;
use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new().expect("could not initialize Tablebase state"))
        // Each window is its own environment. When one is destroyed we drop just
        // that window's data (never touching the others); when the *last* window
        // closes there is nothing left to show, so the app exits.
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Destroyed) {
                let app = window.app_handle();
                app.state::<AppState>().remove_env(window.label());
                if app.webview_windows().is_empty() {
                    app.exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_files,
            commands::close_file,
            commands::configure_file,
            commands::env_info,
            commands::start_query,
            commands::fetch_page,
            commands::count_query,
            commands::cancel_query,
            commands::export_results,
            commands::new_window,
            commands::quit,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tablebase");
}
