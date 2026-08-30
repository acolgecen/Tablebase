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
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Manager, WindowEvent};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new().expect("could not initialize Tablebase state"))
        .menu(|app| {
            let new_window =
                MenuItem::with_id(app, "new_window", "New Window", true, Some("CmdOrCtrl+N"))?;
            let close_query_tab = MenuItem::with_id(
                app,
                "close_query_tab",
                "Close Query Tab",
                true,
                Some("CmdOrCtrl+W"),
            )?;
            let quit = MenuItem::with_id(
                app,
                "quit_confirmed",
                "Quit Tablebase",
                true,
                Some("CmdOrCtrl+Q"),
            )?;
            let file_separator = PredefinedMenuItem::separator(app)?;
            let file_menu = Submenu::with_items(
                app,
                "File",
                true,
                &[
                    &new_window,
                    &file_separator,
                    &close_query_tab,
                    #[cfg(not(target_os = "macos"))]
                    &quit,
                ],
            )?;

            let undo = PredefinedMenuItem::undo(app, None)?;
            let redo = PredefinedMenuItem::redo(app, None)?;
            let edit_separator = PredefinedMenuItem::separator(app)?;
            let cut = PredefinedMenuItem::cut(app, None)?;
            let copy = PredefinedMenuItem::copy(app, None)?;
            let paste = PredefinedMenuItem::paste(app, None)?;
            let select_all = PredefinedMenuItem::select_all(app, None)?;
            let edit_menu = Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &undo,
                    &redo,
                    &edit_separator,
                    &cut,
                    &copy,
                    &paste,
                    &select_all,
                ],
            )?;

            let minimize = PredefinedMenuItem::minimize(app, None)?;
            let maximize = PredefinedMenuItem::maximize(app, None)?;
            let window_menu = Submenu::with_id_and_items(
                app,
                tauri::menu::WINDOW_SUBMENU_ID,
                "Window",
                true,
                &[&minimize, &maximize],
            )?;

            #[cfg(target_os = "macos")]
            let app_menu = {
                let about = PredefinedMenuItem::about(app, None, None)?;
                let first_separator = PredefinedMenuItem::separator(app)?;
                let services = PredefinedMenuItem::services(app, None)?;
                let second_separator = PredefinedMenuItem::separator(app)?;
                let hide = PredefinedMenuItem::hide(app, None)?;
                let hide_others = PredefinedMenuItem::hide_others(app, None)?;
                let third_separator = PredefinedMenuItem::separator(app)?;
                Submenu::with_items(
                    app,
                    "Tablebase",
                    true,
                    &[
                        &about,
                        &first_separator,
                        &services,
                        &second_separator,
                        &hide,
                        &hide_others,
                        &third_separator,
                        &quit,
                    ],
                )?
            };

            Menu::with_items(
                app,
                &[
                    #[cfg(target_os = "macos")]
                    &app_menu,
                    &file_menu,
                    &edit_menu,
                    &window_menu,
                ],
            )
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "new_window" => {
                let _ = commands::new_window(app.clone());
            }
            "close_query_tab" => {
                if let Some(window) = app
                    .webview_windows()
                    .into_values()
                    .find(|window| window.is_focused().unwrap_or(false))
                {
                    let _ =
                        window.eval("window.dispatchEvent(new Event('tablebase:close-query-tab'))");
                }
            }
            "quit_confirmed" => commands::request_quit(app.clone()),
            _ => {}
        })
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
