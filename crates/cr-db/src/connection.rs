//! GaussDB connection management.
//!
//! Provides [`GaussDbConnection`], which wraps a `gaussdb::Client`
//! and implements the [`cr_core::DbConnection`] trait.

use std::time::Duration;

use gaussdb::{Client, Config, NoTls};

use cr_core::DbError;

/// Default timeout for establishing a TCP connection (seconds).
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Default timeout for EXPLAIN execution (seconds).
const DEFAULT_EXPLAIN_TIMEOUT_SECS: u64 = 30;

/// A connection to a GaussDB / openGauss database.
///
/// Wraps a [`gaussdb::Client`] and stores connection metadata
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
            reason: full_error_chain(&e),
        })?;

        // The connection future must be spawned to drive protocol communication.
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = full_error_chain(&e), "GaussDB connection handler terminated");
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

/// Walks the full [`std::error::Error`] source chain and returns a
/// colon-separated string like `"db error: FATAL: Invalid username/password"`.
///
/// The top-level error's `Display` text appears first, followed by each
/// source error in order. This is particularly useful for [`gaussdb::Error`]
/// whose `Display` only produces `"db error"` for the `Db` variant, hiding
/// the underlying server message in the [`gaussdb::error::DbError`] source.
pub(crate) fn full_error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(cause) = src {
        s.push_str(": ");
        s.push_str(&cause.to_string());
        src = cause.source();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    /// A leaf error with no source.
    #[derive(Debug)]
    struct TestError(&'static str);

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl Error for TestError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            None
        }
    }

    /// An error that wraps another error as its source.
    #[derive(Debug)]
    struct WrapperError {
        msg: &'static str,
        source: Box<dyn Error + 'static>,
    }

    impl std::fmt::Display for WrapperError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.msg)
        }
    }

    impl Error for WrapperError {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            Some(&*self.source)
        }
    }

    #[test]
    fn test_full_error_chain_three_levels() {
        let inner = TestError("inner error");
        let mid = WrapperError { msg: "middle error", source: Box::new(inner) };
        let outer = WrapperError { msg: "outer error", source: Box::new(mid) };

        let chain = full_error_chain(&outer);
        assert_eq!(chain, "outer error: middle error: inner error");
    }

    #[test]
    fn test_full_error_chain_single() {
        let single = TestError("just me");
        assert_eq!(full_error_chain(&single), "just me");
    }

    #[test]
    fn test_full_error_chain_empty_message() {
        let empty = TestError("");
        assert_eq!(full_error_chain(&empty), "");
    }
}
