//! Plain data-transfer types shared between modules and sent to the frontend.
//!
//! Keeping these in one place means the Rust modules and the JS layer agree on
//! exactly one shape for each payload.

use serde::{Deserialize, Serialize};

/// One column of a table or query result.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub name: String,
    /// DuckDB type name, e.g. `VARCHAR`, `BIGINT`, `TIMESTAMP`.
    pub data_type: String,
}

/// Optional overrides for CSV/TSV import. When a field is `None` we let DuckDB
/// auto-detect it. Sent from the "Import options" panel.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct ImportOptions {
    /// Field delimiter, e.g. `,` or `\t`. `None` => auto-detect.
    pub delimiter: Option<String>,
    /// Whether the first row is a header. `None` => auto-detect.
    pub header: Option<bool>,
}

/// Summary of one file open inside a window's environment.
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    /// Stable identity inside this window. SQL display names remain `data` or
    /// `dataN`, but the backend no longer uses paths or positions as identity.
    pub id: u64,
    pub source_path: String,
    pub file_name: String,
    /// The SQL table name this file is queried as within its window: `data`
    /// when it is the only file, otherwise `data1`, `data2`, … in open order.
    pub table: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count: u64,
    /// True if served from an existing cache (no re-ingest was needed).
    pub cached: bool,
    /// The options that produced the currently attached cache artifact.
    pub import_options: ImportOptions,
}

/// Snapshot of a single window's environment: the ordered set of files it has
/// open and the table names they are queryable as. Each window has its own.
#[derive(Debug, Clone, Serialize)]
pub struct EnvironmentInfo {
    pub files: Vec<FileInfo>,
    /// Monotonically increases after each successful workspace mutation.
    pub revision: u64,
}

/// One page of query results. Every value is rendered to text by DuckDB so the
/// grid never has to guess how to display dates, decimals, lists, etc.
#[derive(Debug, Clone, Serialize)]
pub struct PageResult {
    pub columns: Vec<ColumnInfo>,
    /// Row-major: `rows[r][c]` is a JSON string, or null for SQL NULL.
    pub rows: Vec<Vec<serde_json::Value>>,
    pub page: u64,
    pub page_size: u64,
    /// How many rows this page actually returned (< page_size on the last page).
    pub returned: u64,
}

/// A page tied to one immutable query session and workspace revision.
#[derive(Debug, Clone, Serialize)]
pub struct QueryPage {
    pub query_id: u64,
    pub workspace_revision: u64,
    pub result: PageResult,
}

/// Lazy total for one immutable query session.
#[derive(Debug, Clone, Serialize)]
pub struct QueryCount {
    pub query_id: u64,
    pub workspace_revision: u64,
    pub total_rows: u64,
}

/// Result of a successful export.
#[derive(Debug, Clone, Serialize)]
pub struct ExportInfo {
    pub path: String,
    pub format: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_contracts_keep_expected_wire_keys() {
        let environment = EnvironmentInfo {
            files: vec![FileInfo {
                id: 1,
                source_path: "/tmp/data.csv".into(),
                file_name: "data.csv".into(),
                table: "data".into(),
                columns: Vec::new(),
                row_count: 0,
                cached: false,
                import_options: ImportOptions::default(),
            }],
            revision: 2,
        };
        let json = serde_json::to_value(environment).unwrap();
        assert!(
            json.get("files").unwrap()[0]
                .get("import_options")
                .is_some()
        );
        assert_eq!(json.get("revision").unwrap(), 2);

        let page = QueryPage {
            query_id: 3,
            workspace_revision: 2,
            result: PageResult {
                columns: Vec::new(),
                rows: Vec::new(),
                page: 0,
                page_size: 2_000,
                returned: 0,
            },
        };
        let json = serde_json::to_value(page).unwrap();
        assert!(json.get("query_id").is_some());
        assert!(json.get("workspace_revision").is_some());
        assert!(json.get("result").is_some());
    }
}
