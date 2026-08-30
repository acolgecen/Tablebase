//! Thin Tauri adapters. Native dialogs live here; application behavior is in
//! `application`, which can be exercised without a webview or OS picker.

use crate::application;
use crate::error::{AppError, AppResult};
use crate::model::{EnvironmentInfo, ExportInfo, ImportOptions, QueryCount, QueryPage};
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

static QUIT_CONFIRMATION_OPEN: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn add_files(app: AppHandle, window: WebviewWindow) -> AppResult<EnvironmentInfo> {
    let label = window.label().to_string();
    run_off_main(move || {
        let picked = app
            .dialog()
            .file()
            .add_filter("Tabular data", &["csv", "tsv", "txt"])
            .blocking_pick_files();
        let paths = picked
            .unwrap_or_default()
            .into_iter()
            .map(|file| {
                file.into_path().map_err(|error| {
                    AppError::Io(format!("could not read the selected path: {error}"))
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        application::add_paths(&app.state::<AppState>(), &label, paths)
    })
    .await
}

#[tauri::command]
pub async fn close_file(
    app: AppHandle,
    window: WebviewWindow,
    path: String,
) -> AppResult<EnvironmentInfo> {
    let label = window.label().to_string();
    run_off_main(move || {
        application::close_path(
            &app.state::<AppState>(),
            &label,
            std::path::Path::new(&path),
        )
    })
    .await
}

#[tauri::command]
pub async fn configure_file(
    app: AppHandle,
    window: WebviewWindow,
    path: String,
    options: ImportOptions,
    abbreviation: String,
) -> AppResult<EnvironmentInfo> {
    let label = window.label().to_string();
    run_off_main(move || {
        application::configure_path(
            &app.state::<AppState>(),
            &label,
            std::path::Path::new(&path),
            &options,
            &abbreviation,
        )
    })
    .await
}

#[tauri::command]
pub async fn env_info(app: AppHandle, window: WebviewWindow) -> AppResult<EnvironmentInfo> {
    let label = window.label().to_string();
    run_off_main(move || application::environment_info(&app.state::<AppState>(), &label)).await
}

#[tauri::command]
pub async fn start_query(
    app: AppHandle,
    window: WebviewWindow,
    sql: String,
    page_size: Option<u64>,
) -> AppResult<QueryPage> {
    let label = window.label().to_string();
    run_off_main(move || {
        let state = app.state::<AppState>();
        let page_size = normalized_page_size(&state, page_size);
        application::start_query(&state, &label, &sql, page_size)
    })
    .await
}

#[tauri::command]
pub async fn fetch_page(
    app: AppHandle,
    window: WebviewWindow,
    query_id: u64,
    page: u64,
    page_size: Option<u64>,
) -> AppResult<QueryPage> {
    let label = window.label().to_string();
    run_off_main(move || {
        let state = app.state::<AppState>();
        let page_size = normalized_page_size(&state, page_size);
        application::fetch_page(&state, &label, query_id, page, page_size)
    })
    .await
}

#[tauri::command]
pub async fn count_query(
    app: AppHandle,
    window: WebviewWindow,
    query_id: u64,
) -> AppResult<QueryCount> {
    let label = window.label().to_string();
    run_off_main(move || application::count_query(&app.state::<AppState>(), &label, query_id)).await
}

#[tauri::command]
pub async fn cancel_query(app: AppHandle, window: WebviewWindow, query_id: u64) {
    application::cancel_query(&app.state::<AppState>(), window.label(), query_id);
}

#[tauri::command]
pub async fn export_results(
    app: AppHandle,
    window: WebviewWindow,
    query_id: u64,
) -> AppResult<Option<ExportInfo>> {
    let label = window.label().to_string();
    run_off_main(move || {
        let picked = app
            .dialog()
            .file()
            .add_filter("CSV", &["csv"])
            .set_file_name("results.csv")
            .blocking_save_file();
        let Some(picked) = picked else {
            return Ok(None);
        };
        let destination = picked
            .into_path()
            .map_err(|error| AppError::Io(format!("could not read the save path: {error}")))?;
        application::export_query(&app.state::<AppState>(), &label, query_id, &destination)
            .map(Some)
    })
    .await
}

#[tauri::command]
pub fn new_window(app: AppHandle) -> AppResult<()> {
    let label = app.state::<AppState>().next_window_label();
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
        .title("Tablebase")
        .inner_size(1280.0, 820.0)
        .min_inner_size(900.0, 600.0)
        .build()
        .map_err(|error| AppError::Internal(format!("could not create a new window: {error}")))?;
    Ok(())
}

#[tauri::command]
pub fn quit(app: AppHandle) {
    request_quit(app);
}

/// Ask once before destroying every window and its retained query tabs.
pub fn request_quit(app: AppHandle) {
    if QUIT_CONFIRMATION_OPEN.swap(true, Ordering::AcqRel) {
        return;
    }
    let dialog_app = app.clone();
    app.dialog()
        .message("Are you sure you want to quit? All open query tabs and results will be closed.")
        .title("Quit Tablebase?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Quit".into(),
            "Cancel".into(),
        ))
        .show(move |confirmed| {
            QUIT_CONFIRMATION_OPEN.store(false, Ordering::Release);
            if confirmed {
                dialog_app.exit(0);
            }
        });
}

fn normalized_page_size(state: &AppState, page_size: Option<u64>) -> u64 {
    page_size
        .unwrap_or(state.config.default_page_size)
        .clamp(1, 100_000)
}

async fn run_off_main<T, F>(function: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(function)
        .await
        .map_err(|error| AppError::Internal(format!("background task failed: {error}")))?
}
