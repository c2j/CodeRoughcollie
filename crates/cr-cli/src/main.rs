use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use encoding_rs::Encoding;

mod doctor;
mod manifest;

/// CodeRoughcollie — GaussDB/openGauss 代码审核工具。
#[derive(Parser)]
#[command(name = "coderc", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// 执行代码审核。
    Audit {
        /// 配置文件路径（缺省为 .roughcollie.toml）。
        #[arg(long)]
        config: Option<PathBuf>,

        /// 审核指定项目（缺省则审核配置中所有项目）。
        #[arg(long)]
        project: Option<String>,

        /// 全量审核（扫描项目 git_repo 下所有文件，而非仅 git diff 变更）。
        #[arg(long)]
        full: bool,

        /// Baseline 分支名（如 `main`、`origin/main`）。
        ///
        /// 仅 --project 模式生效。覆盖配置文件中的 project.baseline。
        #[arg(long)]
        baseline: Option<String>,

        /// 待审核文件列表（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        files: Vec<PathBuf>,

        /// 待审核目录列表（逗号分隔，递归遍历，尊重 .gitignore）。
        #[arg(long, value_delimiter = ',')]
        dir: Vec<PathBuf>,

        /// 待审核清单文件（CSV：project,branch,files）。提供时按清单逐条 pull + 审核。
        #[arg(
            long,
            conflicts_with = "project",
            conflicts_with = "files",
            conflicts_with = "dir",
            conflicts_with = "baseline"
        )]
        manifest: Option<PathBuf>,

        /// 输出格式：markdown / json / sarif / csv。
        #[arg(long, default_value = "markdown")]
        output_format: String,

        /// 输出文件路径。
        #[arg(long)]
        output_path: Option<PathBuf>,

        /// 强制禁用 EXPLAIN，仅静态规则。
        #[arg(long)]
        no_db: bool,

        /// 启用行级 diff-aware 过滤（需配合 baseline；仅保留落在新增/变更行内的 findings）。
        ///
        /// 默认为文件级过滤（一旦文件出现在 diff 中即整体审核）。开启后可显著降低
        /// 历史代码告警噪声，但依赖 git 仓库可用且 finding 路径与 diff 路径一致。
        #[arg(long)]
        diff_aware: bool,

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
    /// 诊断数据库连通性（TCP / 认证 / 服务端信息）。
    #[command(alias = "db-check")]
    Doctor {
        /// 配置文件路径（缺省为 .roughcollie.toml）。
        #[arg(long)]
        config: Option<PathBuf>,

        /// 仅诊断指定数据库（缺省则诊断全部启用的数据库）。
        #[arg(long)]
        db: Option<String>,

        /// 显示服务端版本、GUC 参数等详细信息。
        #[arg(short, long)]
        verbose: bool,
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
            config: config_path,
            project,
            full,
            baseline,
            files,
            dir,
            manifest,
            output_format,
            output_path,
            no_db,
            diff_aware,
            db_host,
            db_name,
            db_user,
            db_password_env,
        } => {
            run_audit(
                config_path,
                project,
                full,
                baseline,
                files,
                dir,
                manifest,
                output_format,
                output_path,
                no_db,
                diff_aware,
                db_host,
                db_name,
                db_user,
                db_password_env,
            )
        }
        Commands::Doctor {
            config,
            db,
            verbose,
        } => {
            let code = doctor::run_doctor(config.as_deref(), db.as_deref(), verbose);
            std::process::exit(code);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_audit(
    config_path: Option<PathBuf>,
    project: Option<String>,
    full: bool,
    baseline: Option<String>,
    files: Vec<PathBuf>,
    dir: Vec<PathBuf>,
    manifest: Option<PathBuf>,
    output_format: String,
    output_path: Option<PathBuf>,
    no_db: bool,
    diff_aware: bool,
    db_host: Option<String>,
    db_name: Option<String>,
    db_user: Option<String>,
    db_password_env: Option<String>,
) {

    let config = load_config(config_path.as_deref());
    if let Err(e) = config.validate() {
        tracing::error!(error = %e, "配置校验失败");
        std::process::exit(2);
    }

    let format = cr_report::ReportFormat::parse(&output_format).unwrap_or(cr_report::ReportFormat::Markdown);

    if let Some(ref mpath) = manifest {
        return manifest::run_manifest(
            mpath,
            &config,
            format,
            output_path.as_deref(),
            no_db,
            db_host.as_deref(),
            db_name.as_deref(),
            db_user.as_deref(),
            db_password_env.as_deref(),
        );
    }

    match &project {
        Some(name) => run_single_project(
            name,
            &config,
            full,
            baseline.as_deref(),
            &files,
            &dir,
            format,
            output_path.as_deref(),
            no_db,
            diff_aware,
            db_host.as_deref(),
            db_name.as_deref(),
            db_user.as_deref(),
            db_password_env.as_deref(),
        ),
        None => {
            if baseline.is_some()
                || !files.is_empty()
                || !dir.is_empty()
                || db_host.is_some()
                || db_name.is_some()
                || db_user.is_some()
                || db_password_env.is_some()
            {
                tracing::warn!("项目级参数（--baseline/--files/--dir/--db-*）在未指定 --project 时将被忽略");
            }
            run_all_projects(&config, full, format, output_path.as_deref(), no_db, diff_aware);
        }
    }
}

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

fn load_config(config_path: Option<&std::path::Path>) -> cr_config::Config {
    let path = config_path.unwrap_or_else(|| std::path::Path::new(".roughcollie.toml"));
    if path.exists() {
        match cr_config::Config::load_from_file(path) {
            Ok(c) => {
                tracing::info!(path = %path.display(), "已加载配置文件");
                c
            }
            Err(e) => {
                tracing::error!(error = %e, path = %path.display(), "解析配置文件失败");
                std::process::exit(2);
            }
        }
    } else {
        tracing::error!(path = %path.display(), "未找到配置文件");
        std::process::exit(2);
    }
}

fn discover_audit_files(
    files: &[PathBuf],
    dirs: &[PathBuf],
    effective_baseline: Option<&str>,
    repo_path: &Path,
    project_type: Option<cr_config::ProjectType>,
) -> Vec<PathBuf> {
    let user_explicit_sources = !files.is_empty() || !dirs.is_empty();

    if user_explicit_sources {
        let mut audit_files: Vec<PathBuf> = Vec::new();
        audit_files.extend(files.iter().cloned());
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
        return audit_files;
    }

    let baseline = match effective_baseline {
        Some(b) => b,
        None => {
            tracing::error!("未指定 --files/--dir，且未提供 baseline，无法确定审核范围");
            std::process::exit(2);
        }
    };

    if let Err(e) = cr_git::validate_baseline(baseline, repo_path) {
        tracing::error!(error = %e, baseline = %baseline, "baseline 分支验证失败");
        std::process::exit(2);
    }

    match cr_git::changed_files(baseline, repo_path) {
        Ok(f) => {
            if f.is_empty() {
                tracing::warn!(baseline = %baseline, "相对于 baseline 未发现变更文件");
            }
            f.into_iter().filter(|cf| file_matches_project_type(cf, project_type)).map(|cf| cf.path).collect()
        }
        Err(e) => {
            tracing::error!(error = %e, "获取变更文件失败");
            std::process::exit(2);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_single_project(
    name: &str,
    config: &cr_config::Config,
    full: bool,
    cli_baseline: Option<&str>,
    files: &[PathBuf],
    dirs: &[PathBuf],
    format: cr_report::ReportFormat,
    output_path: Option<&std::path::Path>,
    no_db: bool,
    diff_aware: bool,
    db_host: Option<&str>,
    db_name: Option<&str>,
    db_user: Option<&str>,
    db_password_env: Option<&str>,
) {
    let project = match config.projects.get(name) {
        Some(p) => p,
        None => {
            let available: Vec<&str> = config.projects.keys().map(String::as_str).collect();
            tracing::error!(project = name, available = ?available, "项目不存在");
            std::process::exit(2);
        }
    };

    tracing::info!(project = name, "审核项目");

    let effective_baseline = cli_baseline.or(project.baseline.as_deref());
    let repo_path = Path::new(project.git_repo.as_deref().unwrap_or("."));
    let db_config = project.database.as_ref().and_then(|n| config.databases.get(n));

    if (!files.is_empty() || !dirs.is_empty() || full) && cli_baseline.is_some() {
        tracing::warn!("--baseline 在指定 --files/--dir/--full 时不会用于文件发现，仅用于报告展示");
    }

    let full_dirs: Vec<PathBuf> =
        if full && files.is_empty() && dirs.is_empty() { vec![repo_path.to_path_buf()] } else { Vec::new() };
    let effective_dirs: Vec<PathBuf> = if dirs.is_empty() { full_dirs } else { dirs.to_vec() };
    let audit_files = discover_audit_files(files, &effective_dirs, effective_baseline, repo_path, project.project_type);

    tracing::info!(project = name, file_count = audit_files.len(), "开始审核");

    let rt = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");
    let (all_findings, degraded, skipped) = rt.block_on(audit_files_async(
        &audit_files,
        db_config,
        &config.rules,
        no_db,
        db_host,
        db_name,
        db_user,
        db_password_env,
    ));

    let all_findings =
        if diff_aware { apply_diff_aware_filter(all_findings, effective_baseline, repo_path) } else { all_findings };

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
        project = name,
        critical = ctx.severity_counts.critical,
        warning = ctx.severity_counts.warning,
        info = ctx.severity_counts.info,
        health_score = ctx.health_score,
        "审核完成"
    );

    if ctx.severity_counts.has_critical() {
        std::process::exit(1);
    }
}

fn run_all_projects(
    config: &cr_config::Config,
    full: bool,
    format: cr_report::ReportFormat,
    output_path: Option<&std::path::Path>,
    no_db: bool,
    diff_aware: bool,
) {
    let mut sections = Vec::new();

    let rt = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");

    for (name, project) in &config.projects {
        tracing::info!(project = name, "审核项目");

        let baseline = project.baseline.as_deref();
        if baseline.is_none() && !full {
            tracing::warn!(project = name, "项目未配置 baseline 且未指定 --full，跳过");
            continue;
        }

        let repo_path = Path::new(project.git_repo.as_deref().unwrap_or("."));
        let db_config = project.database.as_ref().and_then(|n| config.databases.get(n));

        let audit_dirs: Vec<PathBuf> = if full { vec![repo_path.to_path_buf()] } else { Vec::new() };
        let audit_files = discover_audit_files(&[], &audit_dirs, baseline, repo_path, project.project_type);

        tracing::info!(project = name, file_count = audit_files.len(), "开始审核");

        let (findings, degraded, skipped) =
            rt.block_on(audit_files_async(&audit_files, db_config, &config.rules, no_db, None, None, None, None));

        let findings = if diff_aware { apply_diff_aware_filter(findings, baseline, repo_path) } else { findings };

        let severity_counts = cr_core::scoring::count_by_severity(&findings);
        let hs = cr_core::scoring::health_score(&findings);
        let hg = cr_core::scoring::HealthGrade::from_score(hs);

        let ctx = cr_report::RenderContext::new(
            findings,
            severity_counts,
            hs,
            hg,
            baseline.unwrap_or("unknown").to_string(),
            degraded,
        )
        .with_skipped_files(skipped);

        sections.push(cr_report::ProjectSection { name: name.clone(), ctx });
    }

    let multi_ctx = cr_report::MultiProjectContext { sections };
    let report = cr_report::render_multi(&multi_ctx, format);

    match output_path {
        Some(path) => {
            std::fs::write(path, &report).expect("写入报告文件失败");
            tracing::info!(path = %path.display(), "报告已写入");
        }
        None => println!("{report}"),
    }

    let total_critical: usize = multi_ctx.sections.iter().map(|s| s.ctx.severity_counts.critical).sum();
    if total_critical > 0 {
        std::process::exit(1);
    }
}

fn apply_diff_aware_filter(
    findings: Vec<cr_core::Finding>,
    baseline: Option<&str>,
    _repo_path: &Path,
) -> Vec<cr_core::Finding> {
    use cr_audit_static::diff_aware::{filter_findings_to_diff, parse_hunks};

    let Some(baseline) = baseline else {
        tracing::warn!("--diff-aware 缺少 baseline（--baseline 或 project.baseline），跳过行级过滤");
        return findings;
    };

    let mut file_hunks: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
    let unique_files: std::collections::HashSet<&String> = findings.iter().map(|f| &f.file_path).collect();
    for file in unique_files {
        match cr_git::file_diff(baseline, file) {
            Ok(diff_text) => {
                let hunks = parse_hunks(&diff_text);
                if hunks.is_empty() {
                    tracing::debug!(file = %file, "diff 无 hunk（路径不在 diff 范围，如子模块或未跟踪文件），保留全部 findings");
                } else {
                    file_hunks.insert(file.clone(), hunks);
                }
            }
            Err(e) => {
                tracing::debug!(file = %file, error = %e, "无法获取 diff，按保守策略保留全部 findings");
            }
        }
    }

    let before = findings.len();
    let filtered = filter_findings_to_diff(findings, &file_hunks);
    let dropped = before - filtered.len();
    tracing::info!(before, after = filtered.len(), dropped, "diff-aware 行级过滤完成");
    filtered
}

pub(crate) async fn audit_files_async(
    files: &[PathBuf],
    db_config: Option<&cr_config::DatabaseConfig>,
    rules: &cr_config::RulesConfig,
    no_db: bool,
    db_host: Option<&str>,
    db_name: Option<&str>,
    db_user: Option<&str>,
    db_password_env: Option<&str>,
) -> (Vec<cr_core::Finding>, bool, Vec<String>) {
    let mut all_findings = Vec::new();
    let mut degraded = false;
    let mut skipped: Vec<String> = Vec::new();

    let db_conn = if let Some(db) = db_config {
        if db.enabled && !no_db {
            let host = db_host.unwrap_or(&db.host);
            let database = db_name.unwrap_or(&db.database);
            let user = db_user.unwrap_or(&db.username);
            let password = db_password_env
                .and_then(|env_var| std::env::var(env_var).ok())
                .or_else(|| db.password_env.as_ref().and_then(|env_var| std::env::var(env_var).ok()))
                .or_else(|| db.password.clone())
                .unwrap_or_default();

            tracing::info!(host = host, db = database, "连接 GaussDB 中");
            match cr_db::GaussDbConnection::connect(host, db.port, database, user, &password).await {
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
        }
    } else {
        None
    };

    let java_files_for_astgrep: Vec<PathBuf> = files
        .iter()
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("java")).unwrap_or(false))
        .cloned()
        .collect();

    let astgrep_results: std::collections::HashMap<String, Vec<cr_core::Finding>> =
        if !java_files_for_astgrep.is_empty() && !rules.astgrep.preset.is_empty() {
            let options = cr_audit_static::AstgrepOptions {
                language: "java".to_string(),
                severity_threshold: rules.astgrep.severity_threshold.clone(),
                ..Default::default()
            };
            tracing::info!(
                files = java_files_for_astgrep.len(),
                presets = ?rules.astgrep.preset,
                "astgrep 批量扫描启动"
            );
            match cr_audit_static::audit_files_with_astgrep(&java_files_for_astgrep, &rules.astgrep.preset, &options) {
                Ok(findings) => {
                    tracing::info!(total = findings.len(), "astgrep 扫描完成");
                    let mut grouped: std::collections::HashMap<String, Vec<cr_core::Finding>> =
                        java_files_for_astgrep.iter().map(|p| (p.to_string_lossy().to_string(), Vec::new())).collect();
                    for f in findings {
                        grouped.entry(f.file_path.clone()).or_default().push(f);
                    }
                    grouped
                }
                Err(e) => {
                    tracing::warn!(error = %e, "astgrep 扫描失败，Java 文件降级到 regex");
                    std::collections::HashMap::new()
                }
            }
        } else {
            std::collections::HashMap::new()
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
                audit_sql_file(&content, &file_path_str, &db_conn, db_config, rules).await
            }
            cr_audit_static::FileKind::Java => {
                tracing::debug!(path = %file_path.display(), "Java 安全扫描中");
                if let Some(astgrep_findings) = astgrep_results.get(&file_path_str) {
                    astgrep_findings.clone()
                } else {
                    cr_audit_static::java_security::audit_java_source(&content, &file_path_str)
                }
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
    db_config: Option<&cr_config::DatabaseConfig>,
    rules: &cr_config::RulesConfig,
) -> Vec<cr_core::Finding> {
    let mut findings = Vec::new();

    tracing::debug!("静态审核中");
    findings.extend(cr_audit_static::audit_sql(sql, file_path));

    tracing::debug!("复杂度审核中");
    findings.extend(cr_audit_complexity::audit_complexity(
        sql,
        file_path,
        None,
        rules.complexity.warning_delta,
        rules.complexity.critical_delta,
    ));

    if let Some(conn) = db_conn {
        if !is_explainable_dml(sql) {
            tracing::debug!("跳过 EXPLAIN（非 DML 语句：CREATE/ALTER/DROP/SET/TRIGGER/...）");
        } else {
            tracing::debug!("EXPLAIN 审核中");
            let timeout = db_config.map(|d| d.explain.timeout_seconds).unwrap_or(30);
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
    }

    findings
}

fn is_explainable_dml(sql: &str) -> bool {
    for raw_line in sql.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("--") {
            continue;
        }
        if trimmed.starts_with("/*") {
            continue;
        }
        let first_word = trimmed
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_start_matches(|c: char| !c.is_alphabetic())
            .to_uppercase();
        return matches!(
            first_word.as_str(),
            "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "WITH"
        );
    }
    false
}
