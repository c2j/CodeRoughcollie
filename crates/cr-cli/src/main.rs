use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// CodeRoughcollie — GaussDB/openGauss 代码审核工具。
#[derive(Parser)]
#[command(name = "coderc", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 执行代码审核。
    Audit {
        /// Baseline 分支名（如 `main`、`origin/main`）。
        #[arg(long)]
        baseline: String,

        /// 待审核文件列表（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        files: Vec<PathBuf>,

        /// 输出格式：markdown / json / sarif。
        #[arg(long, default_value = "markdown")]
        output_format: String,

        /// 输出文件路径。
        #[arg(long)]
        output_path: Option<PathBuf>,

        /// 强制禁用 EXPLAIN，仅静态规则。
        #[arg(long)]
        no_db: bool,

        /// 数据库主机（覆盖配置文件）。
        #[arg(long)]
        db_host: Option<String>,

        /// 数据库名称（覆盖配置文件）。
        #[arg(long)]
        db_name: Option<String>,

        /// 数据库用户名（覆盖配置文件）。
        #[arg(long)]
        db_user: Option<String>,

        /// 密码环境变量名（覆盖配置文件）。
        #[arg(long)]
        db_password_env: Option<String>,
    },
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Audit {
            baseline,
            files,
            output_format,
            output_path,
            no_db,
            db_host,
            db_name,
            db_user,
            db_password_env,
        } => run_audit(
            &baseline,
            &files,
            &output_format,
            output_path.as_deref(),
            no_db,
            db_host,
            db_name,
            db_user,
            db_password_env,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_audit(
    baseline: &str,
    files: &[PathBuf],
    output_format: &str,
    output_path: Option<&std::path::Path>,
    no_db: bool,
    db_host: Option<String>,
    db_name: Option<String>,
    db_user: Option<String>,
    db_password_env: Option<String>,
) {
    let config = load_config();

    let audit_files = if files.is_empty() {
        match cr_git::changed_files(baseline) {
            Ok(f) => {
                f.into_iter().filter(|cf| cf.is_sql() || cf.is_mybatis_xml()).map(|cf| cf.path).collect::<Vec<_>>()
            }
            Err(e) => {
                tracing::error!(error = %e, "获取变更文件失败");
                std::process::exit(2);
            }
        }
    } else {
        files.to_vec()
    };

    tracing::info!(file_count = audit_files.len(), "开始审核");

    let rt = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");
    let (all_findings, degraded) = rt.block_on(audit_all_files(
        &audit_files,
        &config,
        no_db,
        db_host.as_deref(),
        db_name.as_deref(),
        db_user.as_deref(),
        db_password_env.as_deref(),
    ));

    let format = cr_report::ReportFormat::parse(output_format).unwrap_or(cr_report::ReportFormat::Markdown);

    let mut report = cr_report::render(&all_findings, format);
    if degraded {
        report = format!("> ⚠️ **[EXPLAIN 降级：静态分析]**\n\n{report}");
    }

    match output_path {
        Some(path) => {
            std::fs::write(path, &report).expect("写入报告文件失败");
            tracing::info!(path = %path.display(), "报告已写入");
        }
        None => println!("{report}"),
    }

    let counts = cr_core::scoring::count_by_severity(&all_findings);
    tracing::info!(critical = counts.critical, warning = counts.warning, info = counts.info, "审核完成");

    if counts.has_critical() {
        std::process::exit(1);
    }
}

fn load_config() -> cr_config::Config {
    let config_path = std::path::Path::new(".roughcollie.toml");
    if config_path.exists() {
        match cr_config::Config::load_from_file(config_path) {
            Ok(c) => {
                tracing::info!("已加载 .roughcollie.toml");
                c
            }
            Err(e) => {
                tracing::warn!(error = %e, "解析 .roughcollie.toml 失败，使用默认配置");
                cr_config::Config::default()
            }
        }
    } else {
        cr_config::Config::default()
    }
}

async fn audit_all_files(
    files: &[PathBuf],
    config: &cr_config::Config,
    no_db: bool,
    db_host: Option<&str>,
    db_name: Option<&str>,
    db_user: Option<&str>,
    db_password_env: Option<&str>,
) -> (Vec<cr_core::Finding>, bool) {
    let mut all_findings = Vec::new();
    let mut degraded = false;

    let db_enabled = config.database.enabled && !no_db;
    let db_conn = if db_enabled {
        let host = db_host.unwrap_or(&config.database.host);
        let database = db_name.unwrap_or(&config.database.database);
        let user = db_user.unwrap_or(&config.database.username);
        let password = db_password_env
            .and_then(|env_var| std::env::var(env_var).ok())
            .or_else(|| config.database.password_env.as_ref().and_then(|env_var| std::env::var(env_var).ok()))
            .unwrap_or_default();

        tracing::info!(host = host, db = database, "连接 GaussDB 中");
        match cr_db::GaussDbConnection::connect(host, config.database.port, database, user, &password).await {
            Ok(conn) => {
                tracing::info!("GaussDB 连接成功");
                Some(conn)
            }
            Err(e) => {
                tracing::warn!(error = %e, "GaussDB 连接失败，降级为静态分析");
                degraded = true;
                None
            }
        }
    } else {
        None
    };

    for file_path in files {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %file_path.display(), error = %e, "读取文件失败，跳过");
                continue;
            }
        };

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let mut findings = match ext {
            "sql" => audit_sql_file(&content, &db_conn, config).await,
            "xml" => {
                tracing::debug!(path = %file_path.display(), "MyBatis XML 安全扫描中");
                cr_audit_static::java_security::audit_mybatis_xml(&content)
            }
            "java" => {
                tracing::debug!(path = %file_path.display(), "Java 安全扫描中");
                cr_audit_static::java_security::audit_java_source(&content)
            }
            _ => {
                tracing::debug!(path = %file_path.display(), ext = ext, "未知文件类型，尝试 SQL 审核");
                audit_sql_file(&content, &db_conn, config).await
            }
        };

        tracing::info!(
            path = %file_path.display(),
            findings = findings.len(),
            "文件审核完成"
        );
        all_findings.append(&mut findings);
    }

    (all_findings, degraded)
}

async fn audit_sql_file(
    sql: &str,
    db_conn: &Option<cr_db::GaussDbConnection>,
    config: &cr_config::Config,
) -> Vec<cr_core::Finding> {
    let mut findings = Vec::new();

    tracing::debug!("静态审核中");
    findings.extend(cr_audit_static::audit_sql(sql));

    tracing::debug!("复杂度审核中");
    findings.extend(cr_audit_complexity::audit_complexity(
        sql,
        None,
        config.rules.complexity.warning_delta,
        config.rules.complexity.critical_delta,
    ));

    if let Some(_conn) = db_conn {
        // EXPLAIN analysis temporarily disabled — ogexplain-analyzer#12
        // tracing::debug!("EXPLAIN 审核中");
        // ... cr_audit_explain::analyze_explain_text ...
        tracing::debug!("EXPLAIN 审核已禁用（ogexplain-core 兼容性问题）");
    }

    findings
}
