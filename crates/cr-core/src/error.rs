//! CodeRoughcollie 核心错误类型。
//!
//! 遵循 M-ERR-01：库代码使用 `thiserror` 定义具体错误类型，禁止 `anyhow`。

use serde::Serialize;

/// CodeRoughcollie 统一的错误类型。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RoughcollieError {
    /// 配置解析失败。
    #[error("配置错误: {0}")]
    Config(String),

    /// SQL 语法解析失败。
    #[error("SQL 解析错误 (行 {line}, 列 {col}): {message}")]
    Parse {
        /// 错误所在行号（1-based）。
        line: usize,
        /// 错误所在列号（1-based）。
        col: usize,
        /// 错误描述。
        message: String,
    },

    /// 数据库连接或执行错误。
    #[error("数据库错误: {0}")]
    Database(#[from] DbError),

    /// ogexplain-analyzer 规则引擎错误。
    #[error("规则引擎错误: {0}")]
    RuleEngine(String),

    /// EXPLAIN 执行超时。
    #[error("EXPLAIN 超时 ({timeout_sec}s): {sql}")]
    ExplainTimeout {
        /// 配置的超时秒数。
        timeout_sec: u64,
        /// 被超时的 SQL 文本。
        sql: String,
    },

    /// 插件加载错误。
    #[error("插件加载错误: {0}")]
    Plugin(String),

    /// IO 错误（文件读写等）。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 数据库层错误。
#[derive(Debug, thiserror::Error, Serialize)]
#[non_exhaustive]
pub enum DbError {
    /// 连接失败。
    #[error("连接 GaussDB 失败 ({host}:{port}): {reason}")]
    ConnectionFailed {
        /// 数据库主机地址。
        host: String,
        /// 数据库端口。
        port: u16,
        /// 失败原因。
        reason: String,
    },

    /// 权限不足。
    #[error("权限不足: 用户 '{user}' 缺少 {required_priv} 权限")]
    PermissionDenied {
        /// 数据库用户名。
        user: String,
        /// 所需权限。
        required_priv: String,
    },

    /// EXPLAIN 被安全策略拒绝。
    #[error("SQL 被安全策略拒绝: {reason}")]
    SecurityRejected {
        /// 拒绝原因。
        reason: String,
    },
}
