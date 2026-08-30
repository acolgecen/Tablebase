//! Read-only SQL guardrail (layer 1 & 2 of defense-in-depth).
//!
//! Before any user SQL reaches DuckDB we:
//!   1. parse it and require **exactly one** statement (blocks
//!      `SELECT ...; DROP ...` style injection), and
//!   2. require that statement to be a read-only query (`SELECT` / `WITH`).
//!
//! Layer 3 is the immutable session catalog: every source database is attached
//! `READ_ONLY`. We parse with the PostgreSQL dialect because DuckDB's dialect is
//! Postgres-flavoured; this errs on the side of rejecting anything we cannot
//! confidently prove is read-only.

use crate::error::{AppError, AppResult};
use sqlparser::ast::Statement;
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

/// Proof that a SQL string passed the single-statement, read-only guardrail.
/// Query and export infrastructure accept this type instead of raw strings.
#[derive(Debug, Clone)]
pub struct ValidatedQuery(String);

impl ValidatedQuery {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Returns `Ok(())` only if `sql` is a single read-only query.
pub fn validate(sql: &str) -> AppResult<ValidatedQuery> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(AppError::Guardrail("The query is empty.".into()));
    }

    let dialect = PostgreSqlDialect {};
    let statements = Parser::parse_sql(&dialect, trimmed)
        .map_err(|e| AppError::Guardrail(format!("Could not parse SQL: {e}")))?;

    match statements.len() {
        1 => {}
        0 => return Err(AppError::Guardrail("No SQL statement found.".into())),
        n => {
            return Err(AppError::Guardrail(format!(
                "Only a single statement is allowed (found {n}). Remove extra ';'-separated statements."
            )));
        }
    }

    match &statements[0] {
        // `WITH ... SELECT` also parses to `Query`; a `WITH ... INSERT` would
        // parse to `Statement::Insert` and be rejected below.
        Statement::Query(_) => Ok(ValidatedQuery(trimmed.to_string())),
        other => Err(AppError::Guardrail(format!(
            "Only read-only SELECT queries are allowed; '{}' is blocked.",
            statement_kind(other)
        ))),
    }
}

/// A short human label for a rejected statement, used in error messages.
fn statement_kind(stmt: &Statement) -> &'static str {
    match stmt {
        Statement::Insert(_) => "INSERT",
        Statement::Update { .. } => "UPDATE",
        Statement::Delete(_) => "DELETE",
        Statement::CreateTable(_) => "CREATE TABLE",
        Statement::CreateView { .. } => "CREATE VIEW",
        Statement::AlterTable { .. } => "ALTER TABLE",
        Statement::Drop { .. } => "DROP",
        Statement::Truncate { .. } => "TRUNCATE",
        Statement::Copy { .. } => "COPY",
        Statement::Call(_) => "CALL",
        Statement::Pragma { .. } => "PRAGMA",
        Statement::AttachDuckDBDatabase { .. } => "ATTACH",
        _ => "this statement",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_select() {
        assert!(validate("SELECT * FROM data").is_ok());
    }

    #[test]
    fn allows_cte() {
        assert!(validate("WITH x AS (SELECT 1 AS a) SELECT a FROM x").is_ok());
    }

    #[test]
    fn blocks_write_statements() {
        for sql in [
            "INSERT INTO data VALUES (1)",
            "UPDATE data SET a = 1",
            "DELETE FROM data",
            "DROP TABLE data",
            "CREATE TABLE t (a INT)",
        ] {
            assert!(validate(sql).is_err(), "should reject: {sql}");
        }
    }

    #[test]
    fn blocks_stacked_statements() {
        assert!(validate("SELECT 1; DROP TABLE data").is_err());
    }
}
