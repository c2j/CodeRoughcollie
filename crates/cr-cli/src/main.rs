use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use encoding_rs::Encoding;
use globset::{Glob, GlobSet, GlobSetBuilder};
use indicatif::{ProgressBar, ProgressStyle};

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
            conflicts_with = "baseline",
            conflicts_with = "codeweb_analyze"
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

        /// 显式触发 codeweb 建图（`codeweb analyze`），不依赖 --full 模式。
        ///
        /// 仅当项目配置了 [projects.x.codeweb] enabled=true 时生效。
        /// 建图失败仅 warn 不中断审核。
        #[arg(long)]
        codeweb_analyze: bool,

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
            codeweb_analyze,
            db_host,
            db_name,
            db_user,
            db_password_env,
        } => run_audit(
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
            codeweb_analyze,
            db_host,
            db_name,
            db_user,
            db_password_env,
        ),
        Commands::Doctor { config, db, verbose } => {
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
    codeweb_analyze: bool,
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
            codeweb_analyze,
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
            run_all_projects(&config, full, format, output_path.as_deref(), no_db, diff_aware, codeweb_analyze);
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
    exclude_patterns: &[String],
) -> (Vec<PathBuf>, usize) {
    let user_explicit_sources = !files.is_empty() || !dirs.is_empty();

    if user_explicit_sources {
        let mut audit_files: Vec<PathBuf> = Vec::new();

        audit_files.extend(files.iter().cloned());

        if !dirs.is_empty() {
            match cr_git::walk_directory(dirs) {
                Ok(walked) => {
                    let (walked, excluded) = apply_exclude_filter(walked, exclude_patterns, repo_path);
                    if excluded > 0 {
                        tracing::info!(excluded, "文件因 exclude 配置被跳过");
                    }
                    audit_files.extend(walked);
                }
                Err(e) => {
                    tracing::error!(error = %e, "目录遍历失败");
                    std::process::exit(2);
                }
            }
        }
        audit_files.sort();
        audit_files.dedup();
        return (audit_files, 0);
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
            let total = f.len();
            let filtered: Vec<PathBuf> =
                f.into_iter().filter(|cf| file_matches_project_type(cf, project_type)).map(|cf| cf.path).collect();
            let type_skipped = total - filtered.len();

            let (filtered, exclude_skipped) = apply_exclude_filter(filtered, exclude_patterns, repo_path);
            if exclude_skipped > 0 {
                tracing::info!(excluded = exclude_skipped, "文件因 exclude 配置被跳过");
            }

            (filtered, type_skipped)
        }
        Err(e) => {
            tracing::error!(error = %e, "获取变更文件失败");
            std::process::exit(2);
        }
    }
}

/// 根据 `exclude` glob 模式过滤文件列表。
///
/// 将每个文件路径归一化为 `repo_path` 的相对路径后再匹配，确保 `git diff`
/// 产生的 repo-relative 路径和 `walk_directory` 返回的绝对路径行为一致。
///
/// 返回 (保留的文件, 被排除的文件数)。空 patterns 时直接返回原列表。
fn apply_exclude_filter(files: Vec<PathBuf>, patterns: &[String], repo_path: &Path) -> (Vec<PathBuf>, usize) {
    if patterns.is_empty() {
        return (files, 0);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(e) => {
                tracing::warn!(pattern = %pattern, error = %e, "exclude 模式无效，忽略");
            }
        }
    }

    let glob_set: GlobSet = match builder.build() {
        Ok(g) => g,
        Err(_) => return (files, 0),
    };

    let mut included = Vec::with_capacity(files.len());
    let mut excluded = 0usize;

    for file in files {
        // 归一化为 repo-relative 路径后匹配，屏蔽绝对/相对路径差异
        let match_path: &Path = file.strip_prefix(repo_path).unwrap_or(&file);
        if glob_set.is_match(match_path) {
            tracing::debug!(path = %file.display(), "文件被 exclude 配置匹配，跳过");
            excluded += 1;
        } else {
            included.push(file);
        }
    }

    (included, excluded)
}

/// 将文件列表按 `exclude` glob 模式分为「可审核」和「被忽略」两组。
///
/// 与 [`apply_exclude_filter`] 不同，此函数不静默丢弃被排除的文件，
/// 而是返回被排除的文件路径（字符串形式），用于在报告中展示。
/// 适用于 `--files` 和 `--manifest` 模式。
pub(crate) fn partition_by_exclude(
    files: Vec<PathBuf>,
    patterns: &[String],
    repo_path: &Path,
) -> (Vec<PathBuf>, Vec<String>) {
    if patterns.is_empty() {
        return (files, vec![]);
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
            }
            Err(e) => {
                tracing::warn!(pattern = %pattern, error = %e, "exclude 模式无效，忽略");
            }
        }
    }

    let glob_set: GlobSet = match builder.build() {
        Ok(g) => g,
        Err(_) => return (files, vec![]),
    };

    let mut auditable = Vec::with_capacity(files.len());
    let mut ignored = Vec::new();

    for file in files {
        let match_path: &Path = file.strip_prefix(repo_path).unwrap_or(&file);
        if glob_set.is_match(match_path) {
            tracing::debug!(path = %file.display(), "文件被 exclude 配置匹配，标记为 Ignored");
            ignored.push(file.to_string_lossy().to_string());
        } else {
            auditable.push(file);
        }
    }

    (auditable, ignored)
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
    codeweb_analyze: bool,
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
    let (audit_files, type_filtered) = discover_audit_files(
        files,
        &effective_dirs,
        effective_baseline,
        repo_path,
        project.project_type,
        &project.exclude,
    );

    if type_filtered > 0 {
        tracing::info!(project = name, type_filtered, "文件因 project_type 过滤被跳过");
    }

    // 将 --files 中匹配 exclude 配置的文件分离为"忽略"组（不审核，但在报告中展示）
    let (audit_files, ignored_files) = partition_by_exclude(audit_files, &project.exclude, repo_path);
    if !ignored_files.is_empty() {
        tracing::info!(project = name, ignored = ignored_files.len(), "文件因 exclude 配置被标记为 Ignored");
    }

    tracing::info!(project = name, file_count = audit_files.len(), "开始审核");

    maybe_codeweb_analyze(config, project, codeweb_analyze);

    let rt = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");

    let pb = if full && !audit_files.is_empty() {
        let pb = ProgressBar::new(audit_files.len() as u64);
        pb.set_style(
            ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .expect("valid progress bar template")
                .progress_chars("=>-"),
        );
        Some(pb)
    } else {
        None
    };

    let (all_findings, degraded, skipped) = rt.block_on(audit_files_async(
        &audit_files,
        db_config,
        &config.rules,
        no_db,
        db_host,
        db_name,
        db_user,
        db_password_env,
        pb.as_ref(),
    ));

    let all_findings = augment_with_impact(all_findings, &audit_files, &config.codeweb, project.codeweb.as_ref());

    let all_findings =
        if diff_aware { apply_diff_aware_filter(all_findings, effective_baseline, repo_path) } else { all_findings };

    let all_findings = cr_core::dedup::dedup_findings(all_findings, &cr_core::dedup::builtin_groups());

    let all_findings = cr_core::filter::filter_findings(
        all_findings,
        config.output.filter.as_ref().and_then(|f| f.rule_id.as_deref()),
        config.output.filter.as_ref().and_then(|f| f.severity.as_deref()),
        config.output.filter.as_ref().and_then(|f| f.category.as_deref()),
    );

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
    .with_skipped_files(skipped)
    .with_ignored_files(ignored_files);

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

/// 对已审核文件追加 codeweb 语义影响 findings（优雅降级）。
///
/// 仅当 `project_codeweb` 为 `Some` 且 `enabled`、且 codeweb 二进制可用时执行。
/// 任何失败仅 warn 并跳过，不影响 `findings`。
fn augment_with_impact(
    findings: Vec<cr_core::Finding>,
    audit_files: &[std::path::PathBuf],
    config_codeweb: &cr_config::CodewebConfig,
    project_codeweb: Option<&cr_config::CodewebProjectConfig>,
) -> Vec<cr_core::Finding> {
    let Some(cw) = project_codeweb.filter(|c| c.enabled) else {
        return findings;
    };
    let runner = cr_audit_impact::CodewebRunner {
        binary: config_codeweb.binary.as_ref().map(std::path::PathBuf::from),
        timeout: std::time::Duration::from_secs(config_codeweb.timeout_secs),
    };
    if let Err(e) = runner.check_available() {
        tracing::warn!(error = %e, "codeweb 不可用，跳过 impact 分析");
        return findings;
    }
    let proj_path = std::path::Path::new(&cw.project_path);
    let scope_count = audit_files
        .iter()
        .filter(|f| matches!(f.extension().and_then(|e| e.to_str()), Some("java") | Some("xml") | Some("sql")))
        .count();
    tracing::info!(files = scope_count, "codeweb impact 分析启动（逐文件子进程调用，文件多时较慢）");
    let mut combined = findings;
    let mut total = 0usize;
    let mut failed = 0usize;
    for f in audit_files {
        let is_codeweb_scope =
            matches!(f.extension().and_then(|e| e.to_str()), Some("java") | Some("xml") | Some("sql"));
        if !is_codeweb_scope {
            continue;
        }
        total += 1;
        let key = f.to_string_lossy();
        match runner.query_impact(&key, proj_path) {
            Ok(impact) => combined.extend(cr_audit_impact::impact_to_findings(&impact, &key)),
            Err(e) => {
                failed += 1;
                // 全部文件都失败时，只在最后输出汇总建议（避免逐文件刷屏）。
                // 部分成功部分失败时仍然输出单个文件的警告，方便定位异常文件。
                if failed < total {
                    tracing::warn!(error = %e, file = %key, "codeweb impact 查询失败，跳过该文件");
                }
            }
        }
    }
    if failed > 0 && failed == total {
        tracing::warn!(
            "codeweb impact 全部 {total} 个文件查询失败（项目可能未建图），\
             建议在下一次审核时添加 --codeweb-analyze 参数让 coderc 自动建图"
        );
    }
    combined
}

fn maybe_codeweb_analyze(config: &cr_config::Config, project: &cr_config::ProjectConfig, codeweb_analyze: bool) {
    if !codeweb_analyze {
        return;
    }
    let Some(cw) = project.codeweb.as_ref() else {
        tracing::warn!(
            "--codeweb-analyze 已传入，但项目未配置 [projects.<name>.codeweb] 段，跳过建图。\
             如需启用，请在配置文件中为该项目添加 [projects.<name>.codeweb] 并设置 project_path 与 enabled = true"
        );
        return;
    };
    if !cw.enabled {
        tracing::warn!(
            "--codeweb-analyze 已传入，但项目 codeweb.enabled = false，跳过建图。\
             如需启用，请将 [projects.<name>.codeweb] 的 enabled 设为 true"
        );
        return;
    }
    let runner = cr_audit_impact::CodewebRunner {
        binary: config.codeweb.binary.as_ref().map(PathBuf::from),
        timeout: std::time::Duration::from_secs(config.codeweb.timeout_secs),
    };
    if let Err(e) = runner.analyze(Path::new(&cw.project_path)) {
        tracing::warn!(error = %e, "codeweb analyze 建图失败，继续审核");
    }
}

fn run_all_projects(
    config: &cr_config::Config,
    full: bool,
    format: cr_report::ReportFormat,
    output_path: Option<&std::path::Path>,
    no_db: bool,
    diff_aware: bool,
    codeweb_analyze: bool,
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
        let (audit_files, type_filtered) =
            discover_audit_files(&[], &audit_dirs, baseline, repo_path, project.project_type, &project.exclude);

        if type_filtered > 0 {
            tracing::info!(project = name, type_filtered, "文件因 project_type 过滤被跳过");
        }

        tracing::info!(project = name, file_count = audit_files.len(), "开始审核");

        maybe_codeweb_analyze(config, project, codeweb_analyze);

        let pb = if full && !audit_files.is_empty() {
            let pb = ProgressBar::new(audit_files.len() as u64);
            pb.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg}",
                )
                .expect("valid progress bar template")
                .progress_chars("=>-"),
            );
            Some(pb)
        } else {
            None
        };

        let (findings, degraded, skipped) = rt.block_on(audit_files_async(
            &audit_files,
            db_config,
            &config.rules,
            no_db,
            None,
            None,
            None,
            None,
            pb.as_ref(),
        ));

        let findings = augment_with_impact(findings, &audit_files, &config.codeweb, project.codeweb.as_ref());

        let findings = if diff_aware { apply_diff_aware_filter(findings, baseline, repo_path) } else { findings };

        let findings = cr_core::filter::filter_findings(
            findings,
            config.output.filter.as_ref().and_then(|f| f.rule_id.as_deref()),
            config.output.filter.as_ref().and_then(|f| f.severity.as_deref()),
            config.output.filter.as_ref().and_then(|f| f.category.as_deref()),
        );

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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn audit_files_async(
    files: &[PathBuf],
    db_config: Option<&cr_config::DatabaseConfig>,
    rules: &cr_config::RulesConfig,
    no_db: bool,
    db_host: Option<&str>,
    db_name: Option<&str>,
    db_user: Option<&str>,
    db_password_env: Option<&str>,
    progress: Option<&ProgressBar>,
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
                if let Some(pb) = progress {
                    pb.inc(1);
                }
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
                if let Some(pb) = progress {
                    pb.inc(1);
                }
                continue;
            }
            _ => {
                tracing::warn!(path = %file_path.display(), "未知文件类型，已跳过");
                skipped.push(file_path_str.clone());
                if let Some(pb) = progress {
                    pb.inc(1);
                }
                continue;
            }
        };

        let file_finding_count = findings.len();
        all_findings.append(&mut findings);

        if let Some(pb) = progress {
            pb.inc(1);
            pb.set_message(format!("{} findings", all_findings.len()));
        } else {
            tracing::info!(
                path = %file_path.display(),
                findings = file_finding_count,
                "文件审核完成"
            );
        }
    }

    if let Some(pb) = progress {
        pb.finish_with_message(format!("{} files audited, {} findings", files.len(), all_findings.len()));
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
                Ok(explain_text) => match cr_audit_explain::analyze_explain_text(&explain_text, file_path) {
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
        if matches!(first_word.as_str(), "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "WITH") {
            return true;
        }
        // If the first statement is non-DML (SET, CREATE, ALTER, etc.), don't give up —
        // check whether any later statement in the file is DML.
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 从 TOML 构造全局 codeweb 配置。
    ///
    /// `CodewebConfig` 标记 `#[non_exhaustive]`，跨 crate 不能用结构体字面量构造，
    /// 故通过 TOML 反序列化（与真实配置加载路径一致）。
    fn codeweb_config(binary: Option<&str>, timeout_secs: u64) -> cr_config::CodewebConfig {
        let mut s = format!("[codeweb]\ntimeout_secs = {timeout_secs}\n");
        if let Some(b) = binary {
            s.push_str(&format!("binary = \"{b}\"\n"));
        }
        toml::from_str::<cr_config::Config>(&s).unwrap().codeweb
    }

    /// 从 TOML 构造每项目 codeweb 配置。
    fn project_codeweb(project_path: &str, enabled: bool) -> cr_config::CodewebProjectConfig {
        let s = format!(
            "[projects.test]\ngit_repo = \".\"\n[projects.test.codeweb]\nproject_path = \"{project_path}\"\nenabled = {enabled}\n"
        );
        toml::from_str::<cr_config::Config>(&s).unwrap().projects.get("test").unwrap().codeweb.clone().unwrap()
    }

    #[test]
    fn test_augment_with_impact_no_config() {
        let cfg = cr_config::CodewebConfig::default();
        let out = augment_with_impact(vec![], &[std::path::PathBuf::from("a.sql")], &cfg, None);
        assert!(out.is_empty(), "无 codeweb 配置时不应产生 finding");
    }

    #[test]
    fn test_augment_with_impact_disabled() {
        let cw = project_codeweb("/tmp", false);
        let cfg = cr_config::CodewebConfig::default();
        let out = augment_with_impact(vec![], &[std::path::PathBuf::from("a.java")], &cfg, Some(&cw));
        assert!(out.is_empty(), "disabled 时不应产生 finding");
    }

    #[test]
    fn test_augment_with_impact_binary_not_found() {
        let cw = project_codeweb("/tmp", true);
        let cfg = codeweb_config(Some("/nonexistent/codeweb"), 5);
        let out = augment_with_impact(vec![], &[std::path::PathBuf::from("a.java")], &cfg, Some(&cw));
        assert!(out.is_empty(), "codeweb 二进制不存在时应优雅返回原始 findings");
    }

    #[test]
    fn test_augment_with_impact_preserves_existing() {
        let existing = vec![cr_core::Finding::new(
            "TEST-001",
            cr_core::Severity::Info,
            cr_core::DiagnosticCategory::General,
            "test",
            "existing finding",
            "a.java",
            None,
            None,
            None,
            None,
        )];
        let cw = project_codeweb("/tmp", true);
        let cfg = codeweb_config(Some("/nonexistent/codeweb"), 5);
        let out = augment_with_impact(existing, &[std::path::PathBuf::from("a.java")], &cfg, Some(&cw));
        assert_eq!(out.len(), 1, "二进制不可用时应保留原始 findings");
        assert_eq!(out[0].rule_id, "TEST-001");
    }

    // ── apply_exclude_filter tests ────────────────────────────────

    #[test]
    fn test_apply_exclude_filter_empty_patterns() {
        let files = vec![PathBuf::from("a.sql")];
        let (kept, excluded) = apply_exclude_filter(files, &[], Path::new("."));
        assert_eq!(kept.len(), 1);
        assert_eq!(excluded, 0);
    }

    #[test]
    fn test_apply_exclude_filter_matches_relative_path() {
        // git diff 产生 repo-relative 路径
        let files = vec![PathBuf::from("src/test/foo.sql"), PathBuf::from("src/main/bar.sql")];
        let (kept, excluded) = apply_exclude_filter(files, &["**/test/**".to_string()], Path::new("."));
        assert_eq!(kept.len(), 1, "main/bar.sql 应保留");
        assert_eq!(kept[0], PathBuf::from("src/main/bar.sql"));
        assert_eq!(excluded, 1);
    }

    #[test]
    fn test_apply_exclude_filter_matches_absolute_path() {
        // walk_directory 在 full/--dir 模式下可能产生绝对路径
        let repo = Path::new("/srv/repos/myapp");
        let files = vec![repo.join("src/test/foo.sql"), repo.join("src/main/bar.sql")];
        let patterns = vec!["**/test/**".to_string(), "**/*_test.sql".to_string()];
        let (kept, excluded) = apply_exclude_filter(files, &patterns, repo);
        assert_eq!(kept.len(), 1, "绝对路径下 bar.sql 应保留");
        assert_eq!(kept[0], repo.join("src/main/bar.sql"));
        assert_eq!(excluded, 1);
    }

    #[test]
    fn test_apply_exclude_filter_no_match() {
        let files = vec![PathBuf::from("src/main/query.sql")];
        let (kept, excluded) = apply_exclude_filter(files, &["**/test/**".to_string()], Path::new("."));
        assert_eq!(kept.len(), 1);
        assert_eq!(excluded, 0);
    }

    #[test]
    fn test_apply_exclude_filter_multiple_patterns() {
        let files = vec![
            PathBuf::from("src/test/unit/test.sql"),
            PathBuf::from("src/mock/data.sql"),
            PathBuf::from("src/main/app.sql"),
        ];
        let patterns = vec!["**/test/**".to_string(), "**/mock/**".to_string()];
        let (kept, excluded) = apply_exclude_filter(files, &patterns, Path::new("."));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], PathBuf::from("src/main/app.sql"));
        assert_eq!(excluded, 2);
    }

    #[test]
    fn test_apply_exclude_filter_anchored_pattern_works_both_modes() {
        // 用户写 "test/**"（不带前导 **/），相对路径和绝对路径都应生效
        let repo = Path::new("/srv/repos/myapp");

        // 模式：从 repo 根开始的 test 目录
        let patterns = vec!["test/**".to_string()];

        // git diff 模式：repo-relative
        let rel_files = vec![PathBuf::from("test/foo.sql"), PathBuf::from("src/main/bar.sql")];
        let (kept, excluded) = apply_exclude_filter(rel_files, &patterns, repo);
        assert_eq!(kept.len(), 1);
        assert_eq!(excluded, 1, "repo-relative 路径应匹配 test/**");

        // full/--dir 模式：绝对路径
        let abs_files = vec![repo.join("test/foo.sql"), repo.join("src/main/bar.sql")];
        let (kept, excluded) = apply_exclude_filter(abs_files, &patterns, repo);
        assert_eq!(kept.len(), 1);
        assert_eq!(excluded, 1, "绝对路径 strip_prefix 后也应匹配 test/**");
    }

    // ── partition_by_exclude tests ─────────────────────────────────

    #[test]
    fn test_partition_by_exclude_empty_patterns() {
        let files = vec![PathBuf::from("a.sql")];
        let (auditable, ignored) = partition_by_exclude(files, &[], Path::new("."));
        assert_eq!(auditable.len(), 1);
        assert!(ignored.is_empty());
    }

    #[test]
    fn test_partition_by_exclude_returns_ignored_as_strings() {
        let files = vec![PathBuf::from("src/test/foo.sql"), PathBuf::from("src/main/bar.sql")];
        let (auditable, ignored) = partition_by_exclude(files, &["**/test/**".to_string()], Path::new("."));
        assert_eq!(auditable.len(), 1);
        assert_eq!(auditable[0], PathBuf::from("src/main/bar.sql"));
        assert_eq!(ignored, vec!["src/test/foo.sql"]);
    }

    #[test]
    fn test_partition_by_exclude_absolute_path() {
        let repo = Path::new("/srv/repos/myapp");
        let files = vec![repo.join("src/test/foo.sql"), repo.join("src/main/bar.sql")];
        let (auditable, ignored) = partition_by_exclude(files, &["**/test/**".to_string()], repo);
        assert_eq!(auditable.len(), 1);
        assert_eq!(auditable[0], repo.join("src/main/bar.sql"));
        assert_eq!(ignored, vec![repo.join("src/test/foo.sql").to_string_lossy().to_string()]);
    }

    #[test]
    fn test_partition_by_exclude_no_match() {
        let files = vec![PathBuf::from("src/main/query.sql")];
        let (auditable, ignored) = partition_by_exclude(files, &["**/test/**".to_string()], Path::new("."));
        assert_eq!(auditable.len(), 1);
        assert!(ignored.is_empty());
    }

    #[test]
    fn test_partition_by_exclude_multiple_patterns() {
        let files = vec![
            PathBuf::from("src/test/unit/test.sql"),
            PathBuf::from("src/mock/data.sql"),
            PathBuf::from("src/main/app.sql"),
        ];
        let patterns = vec!["**/test/**".to_string(), "**/mock/**".to_string()];
        let (auditable, ignored) = partition_by_exclude(files, &patterns, Path::new("."));
        assert_eq!(auditable.len(), 1);
        assert_eq!(auditable[0], PathBuf::from("src/main/app.sql"));
        assert_eq!(ignored.len(), 2);
        assert!(ignored.contains(&"src/test/unit/test.sql".to_string()));
        assert!(ignored.contains(&"src/mock/data.sql".to_string()));
    }

    #[test]
    fn test_partition_by_exclude_all_ignored() {
        let files = vec![PathBuf::from("src/test/a.sql"), PathBuf::from("src/test/b.sql")];
        let (auditable, ignored) = partition_by_exclude(files, &["**/test/**".to_string()], Path::new("."));
        assert!(auditable.is_empty());
        assert_eq!(ignored.len(), 2);
    }
}
