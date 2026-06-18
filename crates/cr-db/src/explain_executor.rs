//! EXPLAIN execution on GaussDB.
//!
//! Prepends `EXPLAIN (ANALYZE, COSTS, BUFFERS, TIMING, FORMAT TEXT)` to the
//! user-supplied SQL and returns the raw TEXT output.

use std::time::Duration;

use tokio_opengauss::{Client, SimpleQueryMessage};

use cr_core::DbError;

use crate::placeholder::fill_placeholders;

/// Executes `EXPLAIN (ANALYZE, COSTS, BUFFERS, TIMING, FORMAT TEXT)` on the
/// given SQL and returns the raw plan text.
///
/// Parameter placeholders (`#{…}`, `${…}`, `?`, `$N`) are automatically
/// replaced with type-inferred defaults via [`fill_placeholders`] before
/// being sent to the database.
///
/// The call is guarded by `tokio::time::timeout` — if the query does not
/// complete within `timeout_secs` the future is cancelled and the function
/// returns [`DbError::SecurityRejected`].
///
/// # Errors
///
/// * [`DbError::SecurityRejected`] — the query timed out.
/// * [`DbError::ConnectionFailed`] — the query failed at the database level.
pub async fn execute_explain(client: &Client, sql: &str, timeout_secs: u64) -> Result<String, DbError> {
    let filled_sql = fill_placeholders(sql);
    let explain_sql = format!("EXPLAIN (ANALYZE, COSTS, BUFFERS, TIMING, FORMAT TEXT) {filled_sql}");

    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), client.simple_query(&explain_sql)).await;

    match result {
        Ok(Ok(messages)) => {
            let mut lines: Vec<String> = Vec::new();
            for msg in messages {
                match msg {
                    SimpleQueryMessage::Row(row) => {
                        if let Some(text) = row.get(0) {
                            lines.push(text.to_string());
                        }
                    }
                    SimpleQueryMessage::CommandComplete(_) | SimpleQueryMessage::RowDescription(_) => {}
                    _ => {}
                }
            }
            Ok(lines.join("\n"))
        }
        Ok(Err(e)) => Err(DbError::ConnectionFailed {
            host: String::new(),
            port: 0,
            reason: format!("EXPLAIN execution failed: {e}"),
        }),
        Err(_elapsed) => {
            Err(DbError::SecurityRejected { reason: format!("EXPLAIN timed out after {timeout_secs} seconds") })
        }
    }
}
