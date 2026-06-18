//! GaussDB / openGauss database connection layer for CodeRoughcollie.
//!
//! Provides connection management, EXPLAIN execution, and permission
//! validation by wrapping `tokio_opengauss`.

pub mod connection;
pub mod explain_executor;
pub mod multi_db;
pub mod placeholder;
pub mod pool;
pub mod security;

pub use connection::GaussDbConnection;
pub use explain_executor::execute_explain;
pub use placeholder::{fill_placeholders, has_placeholders};
pub use security::validate_readonly;

use async_trait::async_trait;
use cr_core::{DbConnection, DbError};

#[async_trait]
impl DbConnection for GaussDbConnection {
    /// Executes `EXPLAIN (ANALYZE, COSTS, BUFFERS, TIMING, FORMAT TEXT)` on
    /// the given SQL, returning the raw plan output.
    async fn execute_explain(&self, sql: &str) -> Result<String, DbError> {
        explain_executor::execute_explain(self.client(), sql, self.default_timeout_secs).await
    }

    /// Validates that the connection is functional and the user has the
    /// required permissions.
    async fn validate_permissions(&self) -> Result<(), DbError> {
        security::validate_readonly(self.client()).await
    }
}
