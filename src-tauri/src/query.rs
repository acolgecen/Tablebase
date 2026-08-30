//! Read-only query execution, pagination, and result serialization.
//!
//! Every operation receives a fresh session connection whose source databases
//! are attached `READ_ONLY`. Results are rendered to text *by DuckDB* — every
//! output column is cast to `VARCHAR` — so the frontend grid receives uniform,
//! canonically-formatted strings and never has to guess types or worry about
//! JSON number precision.

use crate::error::AppResult;
use crate::guardrail::ValidatedQuery;
use crate::model::{ColumnInfo, PageResult};
use duckdb::{AccessMode, Config, Connection};
use std::path::Path;

/// Open a read-only connection to an ingested cache database.
pub fn open_readonly(db_path: &Path) -> AppResult<Connection> {
    let config = Config::default().access_mode(AccessMode::ReadOnly)?;
    let conn = Connection::open_with_flags(db_path, config)?;
    Ok(conn)
}

/// Column names + types of an arbitrary query, without fetching its rows.
pub fn describe(conn: &Connection, query: &ValidatedQuery) -> AppResult<Vec<ColumnInfo>> {
    let user_sql = query.as_str();
    let mut stmt = conn.prepare(&format!("DESCRIBE {user_sql}"))?;
    let rows = stmt.query_map([], |row| {
        Ok(ColumnInfo {
            name: row.get::<_, String>(0)?,      // column_name
            data_type: row.get::<_, String>(1)?, // column_type
        })
    })?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(row?);
    }
    Ok(columns)
}

/// Run one page of `user_sql`. `page` is zero-based.
///
/// Accepting [`ValidatedQuery`] makes guardrail validation mandatory.
pub fn run_page(
    conn: &Connection,
    query: &ValidatedQuery,
    page: u64,
    page_size: u64,
) -> AppResult<PageResult> {
    let user_sql = query.as_str();
    let columns = describe(conn, query)?;

    // No columns => nothing to fetch (degenerate query). Return an empty page.
    if columns.is_empty() {
        return Ok(PageResult {
            columns,
            rows: Vec::new(),
            page,
            page_size,
            returned: 0,
        });
    }

    // Cast every column to VARCHAR by name so we read uniform Option<String>.
    let select_list = columns
        .iter()
        .map(|c| {
            let ident = quote_ident(&c.name);
            format!("CAST({ident} AS VARCHAR) AS {ident}")
        })
        .collect::<Vec<_>>()
        .join(", ");

    let offset = page.saturating_mul(page_size);
    let paged = format!(
        "SELECT {select_list} FROM ({user_sql}) AS _tb_src LIMIT {page_size} OFFSET {offset}"
    );

    let mut stmt = conn.prepare(&paged)?;
    let ncols = columns.len();
    let mut result_rows = stmt.query([])?;

    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    while let Some(row) = result_rows.next()? {
        let mut record = Vec::with_capacity(ncols);
        for i in 0..ncols {
            let value: Option<String> = row.get(i)?;
            record.push(match value {
                Some(text) => serde_json::Value::String(text),
                None => serde_json::Value::Null,
            });
        }
        rows.push(record);
    }

    let returned = rows.len() as u64;
    Ok(PageResult {
        columns,
        rows,
        page,
        page_size,
        returned,
    })
}

/// Total number of rows the full (unpaginated) query would return.
///
/// Run lazily by the frontend after the first page, since on multi-GB inputs
/// this is a full scan.
pub fn count_rows(conn: &Connection, query: &ValidatedQuery) -> AppResult<u64> {
    let user_sql = query.as_str();
    let sql = format!("SELECT count(*) FROM ({user_sql}) AS _tb_src");
    let count: i64 = conn.query_row(&sql, [], |r| r.get(0))?;
    Ok(count.max(0) as u64)
}

/// Quote an identifier for safe embedding, doubling any inner double-quotes.
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ImportOptions;
    use crate::{export, guardrail, ingest};
    use std::io::Write;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tablebase_test_{}_{name}", std::process::id()));
        p
    }

    #[test]
    fn end_to_end_ingest_query_paginate_export() {
        // --- a tiny CSV ---
        let csv = temp_path("data.csv");
        {
            let mut f = std::fs::File::create(&csv).unwrap();
            writeln!(f, "id,name,score").unwrap();
            writeln!(f, "1,alice,9.5").unwrap();
            writeln!(f, "2,bob,7.0").unwrap();
            writeln!(f, "3,carol,8.25").unwrap();
        }
        let db = temp_path("data.duckdb");
        let _ = std::fs::remove_file(&db);

        // --- ingest ---
        let res = ingest::ingest(&csv, &db, &ImportOptions::default()).unwrap();
        assert_eq!(res.row_count, 3);
        assert_eq!(res.columns.len(), 3);
        assert_eq!(res.columns[0].name, "id");

        let conn = open_readonly(&db).unwrap();

        // --- query + describe + serialization ---
        let sql = "SELECT name, score FROM data WHERE score > 8 ORDER BY score DESC";
        let sql = guardrail::validate(sql).unwrap();
        let page = run_page(&conn, &sql, 0, 10).unwrap();
        assert_eq!(page.returned, 2);
        assert_eq!(page.columns.len(), 2);
        assert_eq!(page.rows[0][0], serde_json::json!("alice"));

        // --- count ---
        assert_eq!(count_rows(&conn, &sql).unwrap(), 2);

        // --- pagination (page size 1) ---
        let order = guardrail::validate("SELECT id FROM data ORDER BY id").unwrap();
        let p0 = run_page(&conn, &order, 0, 1).unwrap();
        let p1 = run_page(&conn, &order, 1, 1).unwrap();
        assert_eq!(p0.rows[0][0], serde_json::json!("1"));
        assert_eq!(p1.rows[0][0], serde_json::json!("2"));

        // --- export (also proves COPY ... TO works on the read-only connection) ---
        let out = temp_path("out.csv");
        let _ = std::fs::remove_file(&out);
        export::export(&conn, &sql, &out).unwrap();
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(written.contains("alice"), "export missing data: {written}");

        // --- cleanup ---
        drop(conn);
        for p in [&csv, &db, &out] {
            let _ = std::fs::remove_file(p);
        }
    }
}
