//! GaussDB connection management.
//!
//! Provides [`GaussDbConnection`], which wraps a `tokio_opengauss::Client`
//! and implements the [`cr_core::DbConnection`] trait.

use std::time::Duration;

use tokio_opengauss::{Client, Config, NoTls};

use cr_core::DbError;

/// Default timeout for establishing a TCP connection (seconds).
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default timeout for EXPLAIN execution (seconds).
const DEFAULT_EXPLAIN_TIMEOUT_SECS: u64 = 30;

/// A connection to a GaussDB / openGauss database.
///
/// Wraps a [`tokio_opengauss::Client`] and stores connection metadata
/// for error reporting.
#[derive(Debug)]
pub struct GaussDbConnection {
    /// The underlying asynchronous database client.
    client: Client,
    /// Database host address.
    host: String,
    /// Database port number.
    port: u16,
    /// Database user name.
    user: String,
    /// Default timeout for EXPLAIN execution in seconds.
    pub(crate) default_timeout_secs: u64,
}

impl GaussDbConnection {
    /// Opens a new connection to a GaussDB database.
    ///
    /// Builds a [`Config`] from the provided parameters, connects via
    /// `NoTls`, and spawns the connection handler on the tokio runtime.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::ConnectionFailed`] when the TCP or authentication
    /// handshake fails.
    pub async fn connect(host: &str, port: u16, db: &str, user: &str, password: &str) -> Result<Self, DbError> {
        let host_owned = host.to_string();
        let user_owned = user.to_string();

        let mut config = Config::new();
        config.user(user);
        config.password(password.as_bytes());
        config.dbname(db);
        config.host(host);
        config.port(port);
        config.connect_timeout(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS));

        let (client, connection) = config.connect(NoTls).await.map_err(|e| DbError::ConnectionFailed {
            host: host_owned.clone(),
            port,
            reason: e.to_string(),
        })?;

        // The connection future must be spawned to drive protocol communication.
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(%e, "GaussDB connection handler terminated");
            }
        });

        Ok(Self {
            client,
            host: host_owned,
            port,
            user: user_owned,
            default_timeout_secs: DEFAULT_EXPLAIN_TIMEOUT_SECS,
        })
    }

    /// Returns a shared reference to the underlying [`Client`].
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Returns the database host address.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Returns the database port number.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the database user name.
    pub fn user(&self) -> &str {
        &self.user
    }
}
