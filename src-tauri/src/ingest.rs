//! CSV/TSV -> DuckDB ingestion.
//!
//! Streams a source file into a persistent columnar DuckDB table named
//! [`TABLE`] using DuckDB's native `read_csv`. The whole file never needs to
//! fit in memory, so multi-GB inputs work. This is the only module that opens
//! a *read-write* DuckDB connection; everything downstream is read-only.

use crate::error::AppResult;
use crate::model::{ColumnInfo, ImportOptions};
use duckdb::Connection;
use std::path::Path;

/// The table every file is loaded into.
pub const TABLE: &str = "data";

/// What an ingest produced.
pub struct IngestResult {
    pub columns: Vec<ColumnInfo>,
    pub row_count: u64,
}

/// Ingest `source` into a fresh DuckDB file at `db_path`, returning its schema.
pub fn ingest(source: &Path, db_path: &Path, options: &ImportOptions) -> AppResult<IngestResult> {
    // Start clean so a previous partial/failed ingest can't leave stale schema.
    if db_path.exists() {
        let _ = std::fs::remove_file(db_path);
    }

    let conn = Connection::open(db_path)?; // read-write

    let src = sql_str(&source.to_string_lossy());
    let mut args: Vec<String> = vec![format!("'{src}'"), "auto_detect=true".into()];
    if let Some(delim) = &options.delimiter {
        args.push(format!("delim='{}'", sql_str(delim)));
    }
    if let Some(header) = options.header {
        args.push(format!("header={header}"));
    }

    let create = format!(
        "CREATE TABLE {TABLE} AS SELECT * FROM read_csv({})",
        args.join(", ")
    );
    conn.execute_batch(&create)?;

    let columns = columns_of(&conn, TABLE)?;
    let row_count: i64 =
        conn.query_row(&format!("SELECT count(*) FROM {TABLE}"), [], |r| r.get(0))?;

    Ok(IngestResult {
        columns,
        row_count: row_count.max(0) as u64,
    })
}

/// Read a table's column names and types via `PRAGMA table_info`.
pub fn columns_of(conn: &Connection, table: &str) -> AppResult<Vec<ColumnInfo>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info('{}')", sql_str(table)))?;
    let rows = stmt.query_map([], |row| {
        Ok(ColumnInfo {
            name: row.get::<_, String>(1)?,      // column 1: name
            data_type: row.get::<_, String>(2)?, // column 2: type
        })
    })?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    Ok(columns)
}

/// Escape a value for embedding inside a single-quoted SQL string literal.
fn sql_str(value: &str) -> String {
    value.replace('\'', "''")
}
