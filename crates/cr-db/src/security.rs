//! Permission validation for EXPLAIN-only database access.

use gaussdb::{Client, SimpleQueryMessage};

use cr_core::DbError;

use crate::connection::full_error_chain;

/// Validates that the connection is usable for read-only queries.
///
/// Gets the current database user for error reporting, then verifies the
/// connection is functional by executing `SELECT 1`.
///
/// # Errors
///
/// Returns [`DbError::PermissionDenied`] when the connection validation
/// query does not produce a result row.
/// Returns [`DbError::ConnectionFailed`] when the validation queries
/// themselves fail at the database level.
pub async fn validate_readonly(client: &Client) -> Result<(), DbError> {
    let user = current_user(client).await?;
    verify_connection(client, &user).await
}

async fn current_user(client: &Client) -> Result<String, DbError> {
    let messages = client.simple_query("SELECT current_user").await.map_err(|e| DbError::ConnectionFailed {
        host: String::new(),
        port: 0,
        reason: format!("Failed to query current user: {}", full_error_chain(&e)),
    })?;

    let user = messages
        .iter()
        .find_map(|msg| if let SimpleQueryMessage::Row(row) = msg { row.get(0).map(String::from) } else { None })
        .unwrap_or_else(|| String::from("unknown"));

    Ok(user)
}

async fn verify_connection(client: &Client, user: &str) -> Result<(), DbError> {
    let messages = client.simple_query("SELECT 1").await.map_err(|e| DbError::ConnectionFailed {
        host: String::new(),
        port: 0,
        reason: format!("Connection validation failed: {}", full_error_chain(&e)),
    })?;

    let ok = messages.iter().any(|msg| matches!(msg, SimpleQueryMessage::Row(_)));

    if !ok {
        return Err(DbError::PermissionDenied { user: user.to_string(), required_priv: String::from("CONNECT") });
    }

    Ok(())
}
