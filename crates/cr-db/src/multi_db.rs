//! 多数据库支持。
//!
//! 提供 PostgreSQL 和 MySQL 的连接工厂。
//! GaussDB/openGauss 是默认且完整支持的后端。

use cr_core::DbConnection;

use crate::connection::GaussDbConnection;

/// 支持的数据库类型。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DatabaseType {
    /// openGauss / GaussDB（默认，完整 EXPLAIN + 专属算子诊断）。
    GaussDb,
    /// PostgreSQL（共享 ogsql-parser 基础，EXPLAIN 格式兼容）。
    Postgres,
    /// MySQL（需适配 EXPLAIN FORMAT=JSON，二期支持）。
    Mysql,
}

impl DatabaseType {
    /// 从连接字符串或配置推断数据库类型。
    #[must_use]
    pub fn from_port(port: u16) -> Self {
        match port {
            3306 => Self::Mysql,
            _ => Self::GaussDb,
        }
    }
}

/// 数据库连接参数。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ConnectionParams {
    /// 数据库类型。
    pub db_type: DatabaseType,
    /// 主机地址。
    pub host: String,
    /// 端口。
    pub port: u16,
    /// 数据库名。
    pub database: String,
    /// 用户名。
    pub username: String,
    /// 密码。
    pub password: String,
}

/// 创建数据库连接。
///
/// 根据数据库类型返回对应的连接实现。
///
/// # Errors
///
/// 当连接失败或数据库类型不支持时返回 `DbError`。
pub async fn create_connection(params: &ConnectionParams) -> Result<Box<dyn DbConnection>, cr_core::DbError> {
    match params.db_type {
        DatabaseType::GaussDb | DatabaseType::Postgres => {
            let conn = GaussDbConnection::connect(
                &params.host,
                params.port,
                &params.database,
                &params.username,
                &params.password,
            )
            .await?;
            Ok(Box::new(conn))
        }
        DatabaseType::Mysql => Err(cr_core::DbError::ConnectionFailed {
            host: params.host.clone(),
            port: params.port,
            reason: "MySQL 支持尚未实现，请使用 GaussDB/PostgreSQL".into(),
        }),
    }
}
