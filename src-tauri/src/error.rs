//! Unified error type for the whole backend.
//!
//! Every fallible operation returns [`AppResult`]. The error serializes to a
//! small `{ "kind": ..., "message": ... }` object so the frontend can tell a
//! guardrail rejection apart from, say, a disk error and present it nicely.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    /// Filesystem / dialog / path problems.
    Io(String),
    /// Anything DuckDB rejected (parse, plan, execution).
    Database(String),
    /// The user's SQL was blocked by the read-only guardrail.
    Guardrail(String),
    /// An operation needs an open file but the window has none loaded.
    NoData(String),
    /// Cache directory / eviction problems.
    Cache(String),
    /// Export (COPY ... TO) failures.
    Export(String),
    /// A superseded query was interrupted or its session expired.
    QuerySession(String),
    /// A requested file abbreviation is invalid, reserved, or already used.
    #[serde(rename = "invalid_abbreviation")]
    InvalidAbbreviation(String),
    /// Anything else we didn't model explicitly.
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    /// A poisoned internal `Mutex` (a worker thread panicked while holding it).
    pub fn lock_poisoned() -> Self {
        AppError::Internal("internal state lock was poisoned".into())
    }

    /// The window has no file open but the operation needs one.
    pub fn no_data() -> Self {
        AppError::NoData("No file is open in this window. Use \"Open\" first.".into())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, msg) = match self {
            AppError::Io(m) => ("io", m),
            AppError::Database(m) => ("database", m),
            AppError::Guardrail(m) => ("guardrail", m),
            AppError::NoData(m) => ("no_data", m),
            AppError::Cache(m) => ("cache", m),
            AppError::Export(m) => ("export", m),
            AppError::QuerySession(m) => ("query_session", m),
            AppError::InvalidAbbreviation(m) => ("invalid_abbreviation", m),
            AppError::Internal(m) => ("internal", m),
        };
        write!(f, "[{kind}] {msg}")
    }
}

impl std::error::Error for AppError {}

impl From<duckdb::Error> for AppError {
    fn from(e: duckdb::Error) -> Self {
        AppError::Database(e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviation_errors_have_a_stable_frontend_kind() {
        let value = serde_json::to_value(AppError::InvalidAbbreviation("reserved".into())).unwrap();
        assert_eq!(value["kind"], "invalid_abbreviation");
        assert_eq!(value["message"], "reserved");
    }
}
