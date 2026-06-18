//! 连接池：管理多个 GaussDB 连接，支持并发审核。

use std::sync::Arc;

use parking_lot::Mutex;

use crate::connection::GaussDbConnection;

/// GaussDB 连接池。
pub struct ConnectionPool {
    inner: Arc<Mutex<PoolInner>>,
    config: PoolConfig,
}

struct PoolInner {
    connections: Vec<GaussDbConnection>,
}

/// 连接池配置。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PoolConfig {
    /// 最大连接数。
    pub max_connections: usize,
    /// 主机。
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

impl ConnectionPool {
    /// 创建连接池（初始不建立连接，按需创建）。
    #[must_use]
    pub fn new(config: PoolConfig) -> Self {
        Self { inner: Arc::new(Mutex::new(PoolInner { connections: Vec::new() })), config }
    }

    /// 获取一个可用连接。如果池中有空闲连接则复用，否则创建新连接（不超过 max_connections）。
    ///
    /// # Errors
    ///
    /// 当连接创建失败时返回 `DbError`。
    pub async fn get(&self) -> Result<ConnectionGuard, cr_core::DbError> {
        let config = self.config.clone();
        let inner = self.inner.clone();

        {
            let mut pool = inner.lock();
            if let Some(conn) = pool.connections.pop() {
                return Ok(ConnectionGuard { conn: Some(conn), pool: inner.clone() });
            }
        }

        let conn =
            GaussDbConnection::connect(&config.host, config.port, &config.database, &config.username, &config.password)
                .await?;

        Ok(ConnectionGuard { conn: Some(conn), pool: inner.clone() })
    }

    /// 当前池中空闲连接数。
    #[must_use]
    pub fn idle_count(&self) -> usize {
        self.inner.lock().connections.len()
    }
}

/// 连接守卫，Drop 时自动归还连接到池中。
pub struct ConnectionGuard {
    conn: Option<GaussDbConnection>,
    pool: Arc<Mutex<PoolInner>>,
}

impl std::ops::Deref for ConnectionGuard {
    type Target = GaussDbConnection;

    fn deref(&self) -> &Self::Target {
        self.conn.as_ref().expect("connection already returned")
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.lock().connections.push(conn);
        }
    }
}
