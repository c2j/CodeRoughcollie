use std::collections::HashMap;
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
    // 跟踪每个仓库当前 checkout 的分支：仅在分支变化时才同步，保证交叉分支序（X/Y/X）下正确切换。
    let mut repo_branch: HashMap<String, String> = HashMap::new();
    let mut deduped_count: usize = 0;
    for entry in &entries {
        let Some(project) = config.projects.get(&entry.project) else {
            let available: Vec<&str> = config.projects.keys().map(String::as_str).collect();
            tracing::error!(project = %entry.project, available = ?available, "清单引用的项目不存在，跳过");
            continue;
        };
        let repo_path = Path::new(project.git_repo.as_deref().unwrap_or("."));
        let repo_key = repo_path.display().to_string();
        let need_sync = repo_branch.get(&repo_key) != Some(&entry.branch);
        if need_sync {
            if let Err(e) = cr_git::sync_branch(&entry.branch, repo_path) {
                tracing::error!(project = %entry.project, branch = %entry.branch, error = %e, "git 同步失败，跳过该清单条目");
                continue;
            }
            repo_branch.insert(repo_key, entry.branch.clone());
            tracing::info!(project = %entry.project, branch = %entry.branch, "已同步分支");
        } else {
            deduped_count += 1;
            tracing::debug!(project = %entry.project, branch = %entry.branch, "分支已同步，跳过重复同步");
        }
        let audit_files: Vec<PathBuf> =
            entry.files.iter().map(|f| if f.is_absolute() { f.clone() } else { repo_path.join(f) }).collect();
        // If none of the resolved files exist, check for common misconfiguration:
        // manifest paths that already include the git_repo prefix, causing double-joining.
        let all_missing = audit_files.iter().all(|f| !f.exists());
        if all_missing && !audit_files.is_empty() {
            let example_resolved = audit_files[0].display().to_string();
            let repo_str = repo_path.display().to_string();
            if example_resolved.contains(&format!("{repo_str}/{repo_str}")) {
                tracing::warn!(
                    project = %entry.project,
                    example = %example_resolved,
                    "清单文件路径解析异常：路径中出现了重复的项目目录前缀。\
                     清单中的 files 字段应为相对于项目 git_repo（{}）的路径，\
                     不要包含 git_repo 前缀。",
                    repo_str,
                );
            }
        }
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
    if deduped_count > 0 {
        tracing::info!(deduped = deduped_count, "跳过了重复的分支同步");
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
