//! Application services: use-case orchestration independent of Tauri dialogs.

use crate::error::{AppError, AppResult};
use crate::guardrail;
use crate::model::{EnvironmentInfo, ExportInfo, ImportOptions, QueryCount, QueryPage};
use crate::state::AppState;
use std::path::{Path, PathBuf};

pub fn add_paths(
    state: &AppState,
    window: &str,
    paths: Vec<PathBuf>,
) -> AppResult<EnvironmentInfo> {
    let environment = state.env(window)?;
    let mut environment = environment.lock().map_err(|_| AppError::lock_poisoned())?;
    let sources = paths
        .into_iter()
        .map(|path| (path, ImportOptions::default()))
        .collect::<Vec<_>>();
    environment.add_files(&sources)?;
    Ok(environment.info())
}

pub fn close_path(state: &AppState, window: &str, path: &Path) -> AppResult<EnvironmentInfo> {
    let environment = state.env(window)?;
    let mut environment = environment.lock().map_err(|_| AppError::lock_poisoned())?;
    environment.remove_file(path)?;
    Ok(environment.info())
}

pub fn configure_path(
    state: &AppState,
    window: &str,
    path: &Path,
    options: &ImportOptions,
    abbreviation: &str,
) -> AppResult<EnvironmentInfo> {
    let environment = state.env(window)?;
    let mut environment = environment.lock().map_err(|_| AppError::lock_poisoned())?;
    environment.configure_file(path, options, abbreviation)?;
    Ok(environment.info())
}

pub fn environment_info(state: &AppState, window: &str) -> AppResult<EnvironmentInfo> {
    let environment = state.env(window)?;
    let environment = environment.lock().map_err(|_| AppError::lock_poisoned())?;
    Ok(environment.info())
}

pub fn start_query(
    state: &AppState,
    window: &str,
    sql: &str,
    page_size: u64,
) -> AppResult<QueryPage> {
    let query = guardrail::validate(sql)?;
    let plan = {
        let environment = state.env(window)?;
        let environment = environment.lock().map_err(|_| AppError::lock_poisoned())?;
        environment.query_plan()?
    };
    let session = state.sessions.create(window, plan, query)?;
    match session.page(0, page_size) {
        Ok(result) => Ok(QueryPage {
            query_id: session.id,
            workspace_revision: session.workspace_revision,
            result,
        }),
        Err(error) => {
            state.sessions.cancel(window, session.id);
            Err(error)
        }
    }
}

pub fn fetch_page(
    state: &AppState,
    window: &str,
    query_id: u64,
    page: u64,
    page_size: u64,
) -> AppResult<QueryPage> {
    let session = state.sessions.get(window, query_id)?;
    Ok(QueryPage {
        query_id,
        workspace_revision: session.workspace_revision,
        result: session.page(page, page_size)?,
    })
}

pub fn count_query(state: &AppState, window: &str, query_id: u64) -> AppResult<QueryCount> {
    let session = state.sessions.get(window, query_id)?;
    Ok(QueryCount {
        query_id,
        workspace_revision: session.workspace_revision,
        total_rows: session.count()?,
    })
}

pub fn export_query(
    state: &AppState,
    window: &str,
    query_id: u64,
    destination: &Path,
) -> AppResult<ExportInfo> {
    state.sessions.get(window, query_id)?.export(destination)
}

pub fn cancel_query(state: &AppState, window: &str, query_id: u64) {
    state.sessions.cancel(window, query_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::CacheStore;
    use crate::state::AppConfig;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn setup(name: &str) -> (PathBuf, AppState, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "tablebase_app_{}_{}_{}",
            std::process::id(),
            name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("data.csv");
        std::fs::write(&source, "id,name\n1,alice\n2,bob\n").unwrap();
        let config = AppConfig {
            default_page_size: 2_000,
            cache_cap_bytes: u64::MAX,
        };
        let cache = CacheStore::at(root.join("cache"), config.cache_cap_bytes).unwrap();
        (root, AppState::with_cache(cache, config), source)
    }

    #[test]
    fn query_session_pages_counts_and_exports_one_snapshot() {
        let (root, state, source) = setup("session");
        add_paths(&state, "main", vec![source]).unwrap();
        let first = start_query(&state, "main", "SELECT * FROM data ORDER BY id", 1).unwrap();
        assert_eq!(first.result.returned, 1);
        let second = fetch_page(&state, "main", first.query_id, 1, 1).unwrap();
        assert_eq!(second.result.rows[0][1], serde_json::json!("bob"));
        let count = count_query(&state, "main", first.query_id).unwrap();
        assert_eq!(count.total_rows, 2);
        let destination = root.join("out.csv");
        export_query(&state, "main", first.query_id, &destination).unwrap();
        assert!(
            std::fs::read_to_string(destination)
                .unwrap()
                .contains("alice")
        );
        cancel_query(&state, "main", first.query_id);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_new_query_does_not_destroy_previous_session() {
        let (root, state, source) = setup("invalid");
        add_paths(&state, "main", vec![source]).unwrap();
        let valid = start_query(&state, "main", "SELECT * FROM data", 10).unwrap();
        assert!(start_query(&state, "main", "DELETE FROM data", 10).is_err());
        assert_eq!(
            count_query(&state, "main", valid.query_id)
                .unwrap()
                .total_rows,
            2
        );
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelled_session_rejects_new_work() {
        let (root, state, source) = setup("cancel");
        add_paths(&state, "main", vec![source]).unwrap();
        let query = start_query(&state, "main", "SELECT * FROM data", 10).unwrap();
        cancel_query(&state, "main", query.query_id);
        assert!(count_query(&state, "main", query.query_id).is_err());
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn query_sessions_persist_until_explicitly_cancelled() {
        let (root, state, source) = setup("persistent_sessions");
        add_paths(&state, "main", vec![source]).unwrap();
        let first = start_query(&state, "main", "SELECT * FROM data", 10).unwrap();
        for id in 0..12 {
            start_query(
                &state,
                "main",
                &format!("SELECT * FROM data WHERE id >= {id}"),
                10,
            )
            .unwrap();
        }
        assert_eq!(
            count_query(&state, "main", first.query_id)
                .unwrap()
                .total_rows,
            2
        );
        cancel_query(&state, "main", first.query_id);
        assert!(count_query(&state, "main", first.query_id).is_err());
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
}
