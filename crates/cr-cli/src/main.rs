use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use encoding_rs::Encoding;

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
        ///
        /// 可选。仅在未指定 --files/--dir 时用于 `git diff` 发现变更文件。
        #[arg(long)]
        baseline: Option<String>,

        /// 待审核文件列表（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        files: Vec<PathBuf>,

        /// 待审核目录列表（逗号分隔，递归遍历，尊重 .gitignore）。
        #[arg(long, value_delimiter = ',')]
        dir: Vec<PathBuf>,

        /// 输出格式：markdown / json / sarif / csv。
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
            dir,
            output_format,
            output_path,
            no_db,
            db_host,
            db_name,
            db_user,
            db_password_env,
        } => run_audit(
            baseline.as_deref(),
            &files,
            &dir,
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
/// Read a file with automatic encoding detection.
///
/// Tries: UTF-8 → GB18030 → Shift_JIS → EUC-JP → EUC-KR → BIG5 →
/// ISO-2022-JP → UTF-16LE/BE → lossy UTF-8 fallback.
fn read_file_with_encoding(path: &std::path::Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;

    if let Ok(s) = String::from_utf8(bytes.clone()) {
        return Ok(s);
    }

    let encodings: &[(&'static Encoding, &'static str)] = &[
        (encoding_rs::GB18030, "GB18030"),
        (encoding_rs::SHIFT_JIS, "Shift_JIS"),
        (encoding_rs::EUC_JP, "EUC-JP"),
        (encoding_rs::EUC_KR, "EUC-KR"),
        (encoding_rs::BIG5, "BIG5"),
        (encoding_rs::ISO_2022_JP, "ISO-2022-JP"),
        (encoding_rs::UTF_16LE, "UTF-16LE"),
        (encoding_rs::UTF_16BE, "UTF-16BE"),
    ];

    for (encoding, name) in encodings {
        let (cow, _, had_errors) = encoding.decode(&bytes);
        if !had_errors {
            tracing::debug!(path = %path.display(), encoding = name, "detected file encoding");
            return Ok(cow.into_owned());
        }
    }

    tracing::debug!(path = %path.display(), "falling back to lossy UTF-8");
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[allow(clippy::too_many_arguments)]
fn run_audit(
    baseline: Option<&str>,
    files: &[PathBuf],
    dirs: &[PathBuf],
    output_format: &str,
    output_path: Option<&std::path::Path>,
    no_db: bool,
    db_host: Option<String>,
    db_name: Option<String>,
    db_user: Option<String>,
    db_password_env: Option<String>,
) {
    let config = load_config();

    if let Some(name) = &config.project.name {
        tracing::info!(project = %name, "审核项目");
    }

    let effective_baseline = baseline.or(config.project.baseline.as_deref());
    let repo_path = Path::new(config.project.git_repo.as_deref().unwrap_or("."));

    let user_explicit_sources = !files.is_empty() || !dirs.is_empty();

    // 告警场景 1：baseline 与显式文件源共存
    if user_explicit_sources && baseline.is_some() {
        tracing::warn!("--baseline 在指定 --files/--dir 时不会用于文件发现，仅用于报告展示");
    }

    let mut audit_files: Vec<PathBuf> = Vec::new();
    if !files.is_empty() {
        audit_files.extend(files.iter().cloned());
    }
    if !dirs.is_empty() {
        match cr_git::walk_directory(dirs) {
            Ok(walked) => audit_files.extend(walked),
            Err(e) => {
                tracing::error!(error = %e, "目录遍历失败");
                std::process::exit(2);
            }
        }
    }
    audit_files.sort();
    audit_files.dedup();

    if !user_explicit_sources {
        // 没有 --files/--dir，必须靠 git diff，此时 baseline 必填
        let baseline = match effective_baseline {
            Some(b) => b,
            None => {
                tracing::error!("未指定 --files/--dir，且未提供 --baseline 或配置 project.baseline，无法确定审核范围。请提供 --baseline 分支名或使用 --files/--dir 显式指定文件");
                std::process::exit(2);
            }
        };

        // 预检 baseline 有效性
        if let Err(e) = cr_git::validate_baseline(baseline, repo_path) {
            tracing::error!(error = %e, baseline = %baseline, "baseline 分支验证失败");
            std::process::exit(2);
        }

        audit_files = match cr_git::changed_files(baseline, repo_path) {
            Ok(f) => {
                // 告警场景 2：git diff 返回零文件
                if f.is_empty() {
                    tracing::warn!(baseline = %baseline, "相对于 baseline 未发现变更文件，可能 baseline 分支名有误或确实无变更");
                }
                f.into_iter()
                    .filter(|cf| file_matches_project_type(cf, config.project.project_type))
                    .map(|cf| cf.path)
                    .collect()
            }
            Err(e) => {
                tracing::error!(error = %e, "获取变更文件失败");
                std::process::exit(2);
            }
        };
    }

    tracing::info!(file_count = audit_files.len(), "开始审核");

    let rt = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");
    let (all_findings, degraded, skipped) = rt.block_on(audit_all_files(
        &audit_files,
        &config,
        no_db,
        db_host.as_deref(),
        db_name.as_deref(),
        db_user.as_deref(),
        db_password_env.as_deref(),
    ));

    let format = cr_report::ReportFormat::parse(output_format).unwrap_or(cr_report::ReportFormat::Markdown);

    let severity_counts = cr_core::scoring::count_by_severity(&all_findings);
    let hs = cr_core::scoring::health_score(&all_findings);
    let hg = cr_core::scoring::HealthGrade::from_score(hs);

    let ctx = cr_report::RenderContext::new(
        all_findings,
        severity_counts,
        hs,
        hg,
        effective_baseline.unwrap_or("unknown").to_string(),
        degraded,
    )
    .with_skipped_files(skipped);

    let report = cr_report::render(&ctx, format);

    match output_path {
        Some(path) => {
            std::fs::write(path, &report).expect("写入报告文件失败");
            tracing::info!(path = %path.display(), "报告已写入");
        }
        None => println!("{report}"),
    }

    tracing::info!(
        critical = ctx.severity_counts.critical,
        warning = ctx.severity_counts.warning,
        info = ctx.severity_counts.info,
        skipped = ctx.skipped_files.len(),
        health_score = ctx.health_score,
        "审核完成"
    );

    if ctx.severity_counts.has_critical() {
        std::process::exit(1);
    }
}

fn file_matches_project_type(cf: &cr_git::ChangedFile, project_type: Option<cr_config::ProjectType>) -> bool {
    let accepted = match project_type {
        Some(cr_config::ProjectType::GaussdbSql) => cf.is_sql(),
        Some(cr_config::ProjectType::Java) => cf.is_java() || cf.is_xml(),
        _ => cf.is_sql() || cf.is_xml() || cf.is_java(),
    };
    if !accepted {
        tracing::debug!(path = %cf.path.display(), ?project_type, "文件被 project_type 过滤");
    }
    accepted
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

/// 审核所有文件，返回（发现列表，是否降级，跳过的文件列表）。
async fn audit_all_files(
    files: &[PathBuf],
    config: &cr_config::Config,
    no_db: bool,
    db_host: Option<&str>,
    db_name: Option<&str>,
    db_user: Option<&str>,
    db_password_env: Option<&str>,
) -> (Vec<cr_core::Finding>, bool, Vec<String>) {
    let mut all_findings = Vec::new();
    let mut degraded = false;
    let mut skipped: Vec<String> = Vec::new();

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
        let content = match read_file_with_encoding(file_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %file_path.display(), error = %e, "读取文件失败，跳过");
                continue;
            }
        };

        let file_path_str = file_path.to_string_lossy().to_string();
        let kind = cr_audit_static::detect(file_path, &content);
        let mut findings = match kind {
            cr_audit_static::FileKind::Sql => {
                tracing::debug!(path = %file_path.display(), "SQL 审核中");
                audit_sql_file(&content, &file_path_str, &db_conn, config).await
            }
            cr_audit_static::FileKind::Java => {
                tracing::debug!(path = %file_path.display(), "Java 安全扫描中");
                cr_audit_static::java_security::audit_java_source(&content, &file_path_str)
            }
            cr_audit_static::FileKind::MyBatisXml => {
                tracing::debug!(path = %file_path.display(), "MyBatis XML 安全扫描中");
                cr_audit_static::java_security::audit_mybatis_xml(&content, &file_path_str)
            }
            cr_audit_static::FileKind::Unsupported => {
                tracing::warn!(path = %file_path.display(), "不支持审核此文件类型，已跳过");
                skipped.push(file_path_str.clone());
                continue;
            }
            _ => {
                tracing::warn!(path = %file_path.display(), "未知文件类型，已跳过");
                skipped.push(file_path_str.clone());
                continue;
            }
        };

        tracing::info!(
            path = %file_path.display(),
            findings = findings.len(),
            "文件审核完成"
        );
        all_findings.append(&mut findings);
    }

    (all_findings, degraded, skipped)
}

async fn audit_sql_file(
    sql: &str,
    file_path: &str,
    db_conn: &Option<cr_db::GaussDbConnection>,
    config: &cr_config::Config,
) -> Vec<cr_core::Finding> {
    let mut findings = Vec::new();

    tracing::debug!("静态审核中");
    findings.extend(cr_audit_static::audit_sql(sql, file_path));

    tracing::debug!("复杂度审核中");
    findings.extend(cr_audit_complexity::audit_complexity(
        sql,
        file_path,
        None,
        config.rules.complexity.warning_delta,
        config.rules.complexity.critical_delta,
    ));

    if let Some(conn) = db_conn {
        tracing::debug!("EXPLAIN 审核中");
        let timeout = config.database.explain.timeout_seconds;
        match cr_db::execute_explain(conn.client(), sql, timeout).await {
            Ok(explain_text) => match cr_audit_explain::analyze_explain_text(&explain_text, "inline") {
                Ok(explain_findings) => findings.extend(explain_findings),
                Err(e) => {
                    tracing::warn!(error = %e, "EXPLAIN 解析失败，跳过执行计划审核");
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "EXPLAIN 执行失败，该 SQL 仅静态审核");
            }
        }
    }

    findings
}
