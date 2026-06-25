use std::time::{Duration, Instant};

use gaussdb::Client;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PasswordSource {
    Plaintext,
    EnvVar(String),
    None,
}

fn classify_password_source(db: &cr_config::DatabaseConfig) -> PasswordSource {
    if db.password.is_some() {
        PasswordSource::Plaintext
    } else if let Some(ref env_var) = db.password_env {
        PasswordSource::EnvVar(env_var.clone())
    } else {
        PasswordSource::None
    }
}

fn resolve_password(db: &cr_config::DatabaseConfig) -> String {
    db.password_env
        .as_ref()
        .and_then(|env_var| std::env::var(env_var).ok())
        .or_else(|| db.password.clone())
        .unwrap_or_default()
}

fn status_mark(ok: bool) -> &'static str {
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
    match (is_tty, ok) {
        (true, true) => "✓",
        (true, false) => "✗",
        (false, true) => "[OK]",
        (false, false) => "[FAIL]",
    }
}

async fn tcp_probe(host: &str, port: u16) -> Result<Duration, String> {
    let start = Instant::now();
    match tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect((host, port)),
    )
    .await
    {
        Ok(Ok(_stream)) => Ok(start.elapsed()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timeout after 3s".to_string()),
    }
}

async fn query_scalar(client: &Client, sql: &str) -> Option<String> {
    match client.query_one(sql, &[]).await {
        Ok(row) => row.try_get::<_, String>(0).ok(),
        Err(_) => None,
    }
}

pub(crate) fn run_doctor(
    config_path: Option<&std::path::Path>,
    db_filter: Option<&str>,
    verbose: bool,
) -> i32 {
    let path = config_path.unwrap_or_else(|| std::path::Path::new(".roughcollie.toml"));
    let config = match cr_config::Config::load_from_file(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("配置文件加载失败 ({}): {e}", path.display());
            return 2;
        }
    };

    let databases: Vec<(&str, &cr_config::DatabaseConfig)> = if let Some(name) = db_filter {
        match config.databases.get(name) {
            Some(db) => vec![(name, db)],
            None => {
                eprintln!("配置中未找到数据库 '{name}'");
                eprintln!("可用: {}", config.databases.keys().cloned().collect::<Vec<_>>().join(", "));
                return 2;
            }
        }
    } else {
        config
            .databases
            .iter()
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    };

    let total = databases.len();
    eprintln!("coderc doctor — DB 连通性诊断\n");

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("tokio runtime 初始化失败: {e}");
            return 2;
        }
    };

    let mut healthy = 0usize;
    for (idx, (label, db)) in databases.iter().enumerate() {
        eprintln!("[{}/{}] {} ({})", idx + 1, total, label, if db.enabled { "enabled" } else { "disabled" });
        eprintln!("  Config:");
        eprintln!("    host:port        {}:{}", db.host, db.port);
        eprintln!("    user@database    {}@{}", db.username, db.database);
        let source = classify_password_source(db);
        let source_str = match &source {
            PasswordSource::Plaintext => "plaintext (config file)".to_string(),
            PasswordSource::EnvVar(name) => {
                let state = if std::env::var(name).is_ok() { "set" } else { "UNSET" };
                format!("env var (${name}) [{state}]")
            }
            PasswordSource::None => "none".to_string(),
        };
        eprintln!("    password source  {source_str}");
        eprintln!("    ssl_mode         {}", db.ssl_mode);
        eprintln!("    auth_method      {}", db.auth_method);

        if !db.enabled {
            eprintln!("  Skipped (disabled)\n");
            continue;
        }

        match rt.block_on(tcp_probe(&db.host, db.port)) {
            Ok(elapsed) => {
                eprintln!("  TCP probe:         {} ({:.0?})", status_mark(true), elapsed);
            }
            Err(reason) => {
                eprintln!("  TCP probe:         {} ({reason})", status_mark(false));
                eprintln!("  Auth:              skipped (TCP failed)\n");
                continue;
            }
        }

        let password = resolve_password(db);
        let conn_result = rt.block_on(cr_db::GaussDbConnection::connect(
            &db.host,
            db.port,
            &db.database,
            &db.username,
            &password,
        ));

        match conn_result {
            Ok(conn) => {
                eprintln!(
                    "  Auth:              {} connected as {}@{}",
                    status_mark(true),
                    db.username,
                    db.database
                );
                healthy += 1;

                if verbose {
                    let client = conn.client();
                    if let Some(version) = rt.block_on(query_scalar(client, "SELECT version()")) {
                        eprintln!("  Server:            {version}");
                    }
                    if let Some(user) = rt.block_on(query_scalar(client, "SELECT current_user")) {
                        eprintln!("  current_user:      {user}");
                    }
                    if let Some(cur_db) = rt.block_on(query_scalar(client, "SELECT current_database()")) {
                        eprintln!("  current_database:  {cur_db}");
                    }
                    if let Some(pet) = rt.block_on(query_scalar(client, "SHOW password_encryption_type")) {
                        let encryption_name = match pet.as_str() {
                            "0" => "MD5",
                            "1" => "SHA256+MD5",
                            "2" => "SHA256",
                            "3" => "SM3",
                            _ => "unknown",
                        };
                        eprintln!("  password_encryption_type: {pet} ({encryption_name})");
                    }
                    if let Some(ssl) = rt.block_on(query_scalar(client, "SHOW ssl")) {
                        eprintln!("  ssl:               {ssl}");
                    }
                }
            }
            Err(e) => {
                eprintln!("  Auth:              {} {e}", status_mark(false));
            }
        }
        eprintln!();
    }

    eprintln!("Summary: {healthy}/{total} healthy");
    if healthy == total { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db() -> cr_config::DatabaseConfig {
        cr_config::DatabaseConfig::default()
    }

    fn with_password(mut db: cr_config::DatabaseConfig, pw: &str) -> cr_config::DatabaseConfig {
        db.password = Some(pw.into());
        db
    }

    fn with_password_env(mut db: cr_config::DatabaseConfig, env: &str) -> cr_config::DatabaseConfig {
        db.password_env = Some(env.into());
        db
    }

    #[test]
    fn classify_plaintext_when_password_set() {
        let db = with_password(make_db(), "secret");
        assert_eq!(classify_password_source(&db), PasswordSource::Plaintext);
    }

    #[test]
    fn classify_envvar_when_only_password_env_set() {
        let db = with_password_env(make_db(), "DB_PASS");
        assert_eq!(classify_password_source(&db), PasswordSource::EnvVar("DB_PASS".into()));
    }

    #[test]
    fn classify_plaintext_wins_over_envvar() {
        let db = with_password(with_password_env(make_db(), "DB_PASS"), "direct");
        assert_eq!(classify_password_source(&db), PasswordSource::Plaintext);
    }

    #[test]
    fn classify_none_when_neither_set() {
        let db = make_db();
        assert_eq!(classify_password_source(&db), PasswordSource::None);
    }
}
