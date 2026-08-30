//! Export query results to a file via DuckDB `COPY ... TO`.
//!
//! `COPY (<query>) TO '<file>'` streams the *full* result set straight to disk,
//! so exports are not capped at the 2000-row page size and never materialize
//! the whole result in memory. `COPY ... TO` only reads from the database and
//! writes an external file, so it is permitted on the read-only connection.

use crate::error::{AppError, AppResult};
use crate::guardrail::ValidatedQuery;
use crate::model::ExportInfo;
use duckdb::Connection;
use std::path::Path;

/// Export `user_sql`'s results to `dest` as CSV.
///
/// Accepting [`ValidatedQuery`] makes guardrail validation mandatory.
pub fn export(conn: &Connection, query: &ValidatedQuery, dest: &Path) -> AppResult<ExportInfo> {
    let user_sql = query.as_str();
    let path = dest.to_string_lossy().replace('\'', "''");
    let sql = format!("COPY ({user_sql}) TO '{path}' (FORMAT CSV, HEADER)");
    conn.execute_batch(&sql)
        .map_err(|e| AppError::Export(e.to_string()))?;

    Ok(ExportInfo {
        path: dest.to_string_lossy().to_string(),
        format: "CSV".to_string(),
    })
}
