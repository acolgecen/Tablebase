//! Revisioned per-window dataset registry.
//!
//! Mutations are staged as a complete proposed registry and validated by
//! opening a fresh DuckDB catalog before being published. Visible SQL names
//! intentionally retain the existing `data` / `data1`... behavior, while stable
//! dataset IDs and immutable cache leases provide reliable internal identity.

use crate::cache::{CacheLease, CacheStore};
use crate::error::{AppError, AppResult};
use crate::ingest;
use crate::model::{ColumnInfo, EnvironmentInfo, FileInfo, ImportOptions};
use duckdb::Connection;
use std::path::{Path, PathBuf};

#[derive(Clone)]
struct OpenFile {
    id: u64,
    source_path: PathBuf,
    identity_path: PathBuf,
    db_path: PathBuf,
    alias: String,
    columns: Vec<ColumnInfo>,
    row_count: u64,
    cached: bool,
    import_options: ImportOptions,
    _lease: CacheLease,
}

pub struct Environment {
    cache: CacheStore,
    files: Vec<OpenFile>,
    revision: u64,
    next_dataset_id: u64,
}

/// Immutable execution plan captured by a query session. Cloned leases keep
/// backing databases available even if the workspace later changes.
#[derive(Clone)]
pub struct QueryPlan {
    pub revision: u64,
    files: Vec<OpenFile>,
}

impl QueryPlan {
    pub fn open_connection(&self) -> AppResult<Connection> {
        build_connection(&self.files)
    }
}

impl Environment {
    pub fn new(cache: CacheStore) -> Self {
        Self {
            cache,
            files: Vec::new(),
            revision: 0,
            next_dataset_id: 0,
        }
    }

    pub fn query_plan(&self) -> AppResult<QueryPlan> {
        if self.files.is_empty() {
            return Err(AppError::no_data());
        }
        Ok(QueryPlan {
            revision: self.revision,
            files: self.files.clone(),
        })
    }

    /// Add a picker batch atomically: either every valid new dataset is
    /// published in order, or the registry remains unchanged.
    pub fn add_files(&mut self, sources: &[(PathBuf, ImportOptions)]) -> AppResult<()> {
        let mut proposed = self.files.clone();
        let mut next_id = self.next_dataset_id;
        for (source, options) in sources {
            let identity = std::fs::canonicalize(source)?;
            if proposed.iter().any(|file| file.identity_path == identity) {
                continue;
            }
            let artifact = self.cache.materialize(source, options, false)?;
            proposed.push(OpenFile {
                id: next_id,
                source_path: source.clone(),
                identity_path: identity,
                db_path: artifact.path,
                alias: format!("src_{next_id}"),
                columns: artifact.columns,
                row_count: artifact.row_count,
                cached: artifact.cached,
                import_options: options.clone(),
                _lease: artifact.lease,
            });
            next_id += 1;
        }
        if proposed.len() == self.files.len() {
            return Ok(());
        }
        self.commit(proposed)?;
        self.next_dataset_id = next_id;
        Ok(())
    }

    pub fn reimport_file(&mut self, source: &Path, options: &ImportOptions) -> AppResult<()> {
        let identity = std::fs::canonicalize(source)?;
        let index = self
            .files
            .iter()
            .position(|file| file.identity_path == identity)
            .ok_or_else(AppError::no_data)?;
        let current = &self.files[index];
        let artifact = self.cache.materialize(source, options, true)?;
        let replacement = OpenFile {
            id: current.id,
            source_path: current.source_path.clone(),
            identity_path: current.identity_path.clone(),
            db_path: artifact.path,
            alias: current.alias.clone(),
            columns: artifact.columns,
            row_count: artifact.row_count,
            cached: artifact.cached,
            import_options: options.clone(),
            _lease: artifact.lease,
        };
        let mut proposed = self.files.clone();
        proposed[index] = replacement;
        self.commit(proposed)
    }

    pub fn remove_file(&mut self, source: &Path) -> AppResult<()> {
        let identity = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
        let index = self
            .files
            .iter()
            .position(|file| file.identity_path == identity || file.source_path == source)
            .ok_or_else(AppError::no_data)?;
        let mut proposed = self.files.clone();
        proposed.remove(index);
        self.commit(proposed)
    }

    pub fn info(&self) -> EnvironmentInfo {
        let names = view_names(self.files.len());
        let files = self
            .files
            .iter()
            .zip(names)
            .map(|(file, table)| FileInfo {
                id: file.id,
                source_path: file.source_path.to_string_lossy().to_string(),
                file_name: file
                    .source_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
                table,
                columns: file.columns.clone(),
                row_count: file.row_count,
                cached: file.cached,
                import_options: file.import_options.clone(),
            })
            .collect();
        EnvironmentInfo {
            files,
            revision: self.revision,
        }
    }

    fn commit(&mut self, proposed: Vec<OpenFile>) -> AppResult<()> {
        let connection = build_connection(&proposed)?;
        drop(connection);
        self.files = proposed;
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}

fn build_connection(files: &[OpenFile]) -> AppResult<Connection> {
    let connection = Connection::open_in_memory()?;
    for file in files {
        let path = file.db_path.to_string_lossy().replace('\'', "''");
        connection.execute_batch(&format!("ATTACH '{path}' AS {} (READ_ONLY)", file.alias))?;
    }
    for (file, name) in files.iter().zip(view_names(files.len())) {
        connection.execute_batch(&format!(
            "CREATE VIEW {name} AS SELECT * FROM {}.{}",
            file.alias,
            ingest::TABLE
        ))?;
    }
    Ok(connection)
}

fn view_names(count: usize) -> Vec<String> {
    match count {
        0 => Vec::new(),
        1 => vec!["data".to_string()],
        _ => (1..=count).map(|index| format!("data{index}")).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tablebase_env_{}_{}_{}",
            std::process::id(),
            name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn preserves_visible_names_and_supports_joins() {
        let root = temp_root("names");
        let cache = CacheStore::at(root.join("cache"), u64::MAX).unwrap();
        let a = root.join("a.csv");
        let b = root.join("b.csv");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&a, "id,name\n1,alice\n2,bob\n").unwrap();
        std::fs::write(&b, "id,city\n1,paris\n2,rome\n").unwrap();

        let mut environment = Environment::new(cache);
        environment
            .add_files(&[(a.clone(), ImportOptions::default())])
            .unwrap();
        assert_eq!(environment.info().files[0].table, "data");

        environment
            .add_files(&[(b.clone(), ImportOptions::default())])
            .unwrap();
        assert_eq!(
            environment
                .info()
                .files
                .iter()
                .map(|file| file.table.as_str())
                .collect::<Vec<_>>(),
            vec!["data1", "data2"]
        );
        let connection = environment.query_plan().unwrap().open_connection().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM data1 JOIN data2 USING (id)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        drop(connection);

        environment.remove_file(&a).unwrap();
        assert_eq!(environment.info().files[0].table, "data");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_reimport_leaves_workspace_unchanged() {
        let root = temp_root("rollback");
        let cache = CacheStore::at(root.join("cache"), u64::MAX).unwrap();
        let source = root.join("data.csv");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&source, "id,name\n1,alice\n").unwrap();
        let mut environment = Environment::new(cache);
        environment
            .add_files(&[(source.clone(), ImportOptions::default())])
            .unwrap();
        let before = environment.info();
        let invalid = ImportOptions {
            delimiter: Some("too long".into()),
            header: None,
        };
        assert!(environment.reimport_file(&source, &invalid).is_err());
        let after = environment.info();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.files[0].row_count, before.files[0].row_count);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_batch_add_publishes_none_of_the_batch() {
        let root = temp_root("batch_rollback");
        let cache = CacheStore::at(root.join("cache"), u64::MAX).unwrap();
        let valid = root.join("valid.csv");
        let missing = root.join("missing.csv");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&valid, "id\n1\n").unwrap();
        let mut environment = Environment::new(cache);
        let batch = vec![
            (valid, ImportOptions::default()),
            (missing, ImportOptions::default()),
        ];
        assert!(environment.add_files(&batch).is_err());
        assert!(environment.info().files.is_empty());
        assert_eq!(environment.info().revision, 0);
        std::fs::remove_dir_all(root).unwrap();
    }
}
