use std::path::{Path, PathBuf};

/// 按清单文件（CSV）逐条执行审核。清单解析失败时 `exit(2)`，Critical 发现时 `exit(1)`。
#[allow(clippy::too_many_arguments)]
pub fn run_manifest(
    manifest_path: &Path,
    config: &cr_config::Config,
    format: cr_report::ReportFormat,
    output_path: Option<&Path>,
    no_db: bool,
    db_host: Option<&str>,
    db_name: Option<&str>,
    db_user: Option<&str>,
    db_password_env: Option<&str>,
) {
    let entries = cr_config::parse_manifest(manifest_path).unwrap_or_else(|e| {
        tracing::error!(error = %e, "清单解析失败");
        std::process::exit(2);
    });
    if entries.is_empty() {
        tracing::warn!("清单为空，无条目待审核");
        return;
    }
    tracing::info!(entries = entries.len(), "清单已加载，开始审核");
    let rt = tokio::runtime::Runtime::new().expect("创建 tokio 运行时失败");
    let mut sections = Vec::new();
    for entry in &entries {
        let Some(project) = config.projects.get(&entry.project) else {
            let available: Vec<&str> = config.projects.keys().map(String::as_str).collect();
            tracing::error!(project = %entry.project, available = ?available, "清单引用的项目不存在，跳过");
            continue;
        };
        let repo_path = Path::new(project.git_repo.as_deref().unwrap_or("."));
        if let Err(e) = cr_git::sync_branch(&entry.branch, repo_path) {
            tracing::error!(project = %entry.project, branch = %entry.branch, error = %e, "git 同步失败，跳过该清单条目");
            continue;
        }
        tracing::info!(project = %entry.project, branch = %entry.branch, "已同步分支");
        let audit_files: Vec<PathBuf> =
            entry.files.iter().map(|f| if f.is_absolute() { f.clone() } else { repo_path.join(f) }).collect();
        let db_config = project.database.as_ref().and_then(|n| config.databases.get(n));
        let (findings, degraded, skipped) = rt.block_on(crate::audit_files_async(
            &audit_files,
            db_config,
            &config.rules,
            no_db,
            db_host,
            db_name,
            db_user,
            db_password_env,
        ));
        let severity_counts = cr_core::scoring::count_by_severity(&findings);
        let hs = cr_core::scoring::health_score(&findings);
        let ctx = cr_report::RenderContext::new(
            findings,
            severity_counts,
            hs,
            cr_core::scoring::HealthGrade::from_score(hs),
            entry.branch.clone(),
            degraded,
        )
        .with_skipped_files(skipped);
        sections.push(cr_report::ProjectSection { name: format!("{}@{}", entry.project, entry.branch), ctx });
    }
    let multi_ctx = cr_report::MultiProjectContext { sections };
    let report = cr_report::render_multi(&multi_ctx, format);
    if let Some(path) = output_path {
        std::fs::write(path, &report).expect("写入报告文件失败");
        tracing::info!(path = %path.display(), "报告已写入");
    } else {
        println!("{report}");
    }
    if multi_ctx.sections.iter().map(|s| s.ctx.severity_counts.critical).sum::<usize>() > 0 {
        std::process::exit(1);
    }
}
