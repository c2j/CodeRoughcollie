//! 核心 trait 定义。
//!
//! 通过 trait 抽象解耦外部依赖（M-ARCH-02），便于测试注入 Mock（R-TST-01）。

use async_trait::async_trait;

use crate::error::DbError;

/// 数据库连接抽象。
///
/// 定义在 `cr-core`（零 IO 依赖），由 `cr-db` 提供真实实现，
/// 测试时注入 Mock。
///
/// 使用 `async_trait` 因为 EXPLAIN 执行涉及网络 IO。
#[async_trait]
pub trait DbConnection: Send + Sync {
    /// 执行 `EXPLAIN` 并返回原始 TEXT 输出。
    ///
    /// # Errors
    ///
    /// 当数据库连接断开、SQL 被安全策略拒绝、或执行超时时返回 `DbError`。
    async fn execute_explain(&self, sql: &str) -> Result<String, DbError>;

    /// 校验连接用户权限（启动时调用）。
    ///
    /// 检查用户是否仅拥有 EXPLAIN 权限，不拥有 DML/DDL 权限。
    ///
    /// # Errors
    ///
    /// 当权限校验失败时返回 `DbError::PermissionDenied`。
    async fn validate_permissions(&self) -> Result<(), DbError>;
}
