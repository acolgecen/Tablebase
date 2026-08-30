//! Revisioned per-window dataset registry.
//!
//! Mutations are staged as a complete proposed registry and validated by
//! opening a fresh DuckDB catalog before being published. Files receive stable
//! automatic SQL abbreviations (`data`, `data1`, `data2`, …) that users can
//! replace with validated, non-reserved identifiers.

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
    relation_name: String,
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
    next_relation_index: u64,
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
            next_relation_index: 0,
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
        let mut next_relation_index = self.next_relation_index;
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
                relation_name: automatic_relation_name(next_relation_index),
                columns: artifact.columns,
                row_count: artifact.row_count,
                cached: artifact.cached,
                import_options: options.clone(),
                _lease: artifact.lease,
            });
            next_id += 1;
            next_relation_index += 1;
        }
        if proposed.len() == self.files.len() {
            return Ok(());
        }
        self.commit(proposed)?;
        self.next_dataset_id = next_id;
        self.next_relation_index = next_relation_index;
        Ok(())
    }

    pub fn configure_file(
        &mut self,
        source: &Path,
        options: &ImportOptions,
        abbreviation: &str,
    ) -> AppResult<()> {
        let identity = std::fs::canonicalize(source)?;
        let index = self
            .files
            .iter()
            .position(|file| file.identity_path == identity)
            .ok_or_else(AppError::no_data)?;
        let current = &self.files[index];
        let relation_name = validate_abbreviation(abbreviation)?;
        if self.files.iter().any(|file| {
            file.id != current.id && file.relation_name.eq_ignore_ascii_case(&relation_name)
        }) {
            return Err(AppError::InvalidAbbreviation(format!(
                "The abbreviation \"{relation_name}\" is already used by another open file."
            )));
        }
        let artifact = self.cache.materialize(source, options, true)?;
        let replacement = OpenFile {
            id: current.id,
            source_path: current.source_path.clone(),
            identity_path: current.identity_path.clone(),
            db_path: artifact.path,
            alias: current.alias.clone(),
            relation_name,
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
        let files = self
            .files
            .iter()
            .map(|file| FileInfo {
                id: file.id,
                source_path: file.source_path.to_string_lossy().to_string(),
                file_name: file
                    .source_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default(),
                table: file.relation_name.clone(),
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
    for file in files {
        let name = quote_ident(&file.relation_name);
        connection.execute_batch(&format!(
            "CREATE VIEW {name} AS SELECT * FROM {}.{}",
            file.alias,
            ingest::TABLE
        ))?;
    }
    Ok(connection)
}

fn automatic_relation_name(index: u64) -> String {
    if index == 0 {
        "data".to_string()
    } else {
        format!("data{index}")
    }
}

fn validate_abbreviation(value: &str) -> AppResult<String> {
    let abbreviation = value.trim();
    if abbreviation.is_empty() {
        return Err(AppError::InvalidAbbreviation(
            "Choose an abbreviation for this file.".into(),
        ));
    }
    if abbreviation.len() > 64 {
        return Err(AppError::InvalidAbbreviation(
            "Abbreviations must be 64 characters or fewer.".into(),
        ));
    }
    let mut characters = abbreviation.chars();
    let valid_first = characters
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_');
    if !valid_first
        || !characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(AppError::InvalidAbbreviation(
            "Use letters, numbers, or underscores, starting with a letter or underscore.".into(),
        ));
    }

    let connection = Connection::open_in_memory()?;
    let reserved: bool = connection.query_row(
        "SELECT EXISTS (
            SELECT 1 FROM duckdb_keywords()
            WHERE lower(keyword_name) = lower(?) AND keyword_category = 'reserved'
        )",
        [abbreviation],
        |row| row.get(0),
    )?;
    if reserved {
        return Err(AppError::InvalidAbbreviation(format!(
            "\"{abbreviation}\" is a reserved SQL word. Choose another abbreviation."
        )));
    }
    Ok(abbreviation.to_string())
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
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
    fn assigns_stable_automatic_names_and_supports_joins() {
        let root = temp_root("names");
        let cache = CacheStore::at(root.join("cache"), u64::MAX).unwrap();
        let a = root.join("a.csv");
        let b = root.join("b.csv");
        let c = root.join("c.csv");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&a, "id,name\n1,alice\n2,bob\n").unwrap();
        std::fs::write(&b, "id,city\n1,paris\n2,rome\n").unwrap();
        std::fs::write(&c, "id,country\n1,france\n2,italy\n").unwrap();

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
            vec!["data", "data1"]
        );
        let connection = environment.query_plan().unwrap().open_connection().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM data JOIN data1 USING (id)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        drop(connection);

        environment.remove_file(&a).unwrap();
        assert_eq!(environment.info().files[0].table, "data1");
        environment
            .add_files(&[(c, ImportOptions::default())])
            .unwrap();
        assert_eq!(environment.info().files[1].table, "data2");
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
        assert!(
            environment
                .configure_file(&source, &invalid, "data")
                .is_err()
        );
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

    #[test]
    fn accepts_custom_abbreviations_and_blocks_reserved_or_duplicate_names() {
        let root = temp_root("abbreviations");
        let cache = CacheStore::at(root.join("cache"), u64::MAX).unwrap();
        let employees = root.join("employees.csv");
        let offices = root.join("offices.csv");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&employees, "id,name\n1,alice\n").unwrap();
        std::fs::write(&offices, "id,city\n1,madrid\n").unwrap();
        let mut environment = Environment::new(cache);
        environment
            .add_files(&[
                (employees.clone(), ImportOptions::default()),
                (offices.clone(), ImportOptions::default()),
            ])
            .unwrap();
        assert_eq!(
            environment
                .info()
                .files
                .iter()
                .map(|file| file.table.as_str())
                .collect::<Vec<_>>(),
            vec!["data", "data1"]
        );

        environment
            .configure_file(&employees, &ImportOptions::default(), "emp")
            .unwrap();
        assert_eq!(environment.info().files[0].table, "emp");
        let connection = environment.query_plan().unwrap().open_connection().unwrap();
        let name: String = connection
            .query_row("SELECT name FROM emp", [], |row| row.get(0))
            .unwrap();
        assert_eq!(name, "alice");
        drop(connection);

        let revision = environment.info().revision;
        assert!(
            environment
                .configure_file(&offices, &ImportOptions::default(), "SELECT")
                .is_err()
        );
        assert!(
            environment
                .configure_file(&offices, &ImportOptions::default(), "EMP")
                .is_err()
        );
        assert_eq!(environment.info().revision, revision);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn abbreviation_validation_matches_duckdb_reserved_words() {
        assert_eq!(validate_abbreviation(" data ").unwrap(), "data");
        assert!(validate_abbreviation("select").is_err());
        assert!(validate_abbreviation("two words").is_err());
        assert!(validate_abbreviation("2files").is_err());
    }
}
