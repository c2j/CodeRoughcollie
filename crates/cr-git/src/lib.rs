//! Git diff 解析、baseline 对比。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Supported file extensions for the audit pipeline.
///
/// MUST stay in sync with the per-language extension lists in
/// `cr-audit-static/src/file_type.rs`. Used by [`walk_directory`] as a cheap
/// pre-filter so we don't read every file in `target/` or `node_modules/`.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["sql", "prc", "pck", "pkb", "fnc", "java", "xml"];

/// Recursively walk one or more directories, returning candidate files whose
/// extensions match [`SUPPORTED_EXTENSIONS`].
///
/// Uses the `ignore` crate which respects `.gitignore`, `.ignore`, and parent
/// `.git` directory boundaries by default. Hidden files (dotfiles) are skipped
/// by default. Symlinks are not followed.
///
/// # Errors
///
/// Returns `std::io::Error` if a provided path does not exist or is neither a
/// file nor a directory.
///
/// # Examples
///
/// ```no_run
/// use std::path::PathBuf;
/// let files = cr_git::walk_directory(&[PathBuf::from("src/")]).unwrap();
/// ```
pub fn walk_directory(dirs: &[PathBuf]) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = BTreeSet::new();

    for dir in dirs {
        if !dir.exists() {
            return Err(std::io::Error::other(format!("目录不存在: {}", dir.display())));
        }

        let walker = ignore::WalkBuilder::new(dir).standard_filters(true).build();

        for entry in walker {
            let entry = match entry {
                Err(err) => {
                    tracing::warn!(error = %err, "遍历文件时出错");
                    continue;
                }
                Ok(e) => e,
            };

            let Some(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }

            let ext =
                entry.path().extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();

            if SUPPORTED_EXTENSIONS.contains(&ext.as_str()) {
                files.insert(entry.path().to_path_buf());
            }
        }
    }

    Ok(files.into_iter().collect())
}

/// 变更文件信息。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ChangedFile {
    /// 文件路径。
    pub path: PathBuf,
    /// 变更状态。
    pub status: ChangeStatus,
}

/// 文件变更状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChangeStatus {
    /// 新增文件。
    Added,
    /// 修改文件。
    Modified,
    /// 删除文件。
    Deleted,
}

impl ChangedFile {
    /// 从 `git diff --name-status` 的一行输出解析。
    ///
    /// 格式：`A\tpath/to/file` 或 `M\tpath/to/file`
    #[must_use]
    pub fn from_diff_line(line: &str) -> Option<Self> {
        let (status_char, path) = line.split_once('\t')?;
        let status = match status_char.chars().next()? {
            'A' => ChangeStatus::Added,
            'M' => ChangeStatus::Modified,
            'D' => ChangeStatus::Deleted,
            _ => return None,
        };
        Some(Self { path: PathBuf::from(path.trim()), status })
    }

    /// 文件扩展名。
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.path.extension()?.to_str()
    }

    /// 是否为 SQL 文件。
    #[must_use]
    pub fn is_sql(&self) -> bool {
        self.extension() == Some("sql")
    }

    /// 是否为 XML 文件（仅按扩展名判断，内容由审核层校验）。
    #[must_use]
    pub fn is_xml(&self) -> bool {
        self.extension() == Some("xml")
    }

    /// 是否为 Java 源文件（仅按扩展名判断）。
    #[must_use]
    pub fn is_java(&self) -> bool {
        self.extension() == Some("java")
    }
}

/// 验证 baseline 是否为有效的 git commit ref。
///
/// 通过在 `repo_path` 目录下执行 `git rev-parse --verify <baseline>^{commit}` 检查。
///
/// # Errors
///
/// 当 baseline 不是有效 git ref，或 git 命令执行失败时返回错误。
pub fn validate_baseline(baseline: &str, repo_path: &Path) -> Result<(), std::io::Error> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &format!("{baseline}^{{commit}}")])
        .current_dir(repo_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!("无效的 baseline 分支: {baseline}: {stderr}")));
    }

    Ok(())
}

/// 获取相对于 baseline 分支的变更文件列表。
///
/// 在 `repo_path` 目录下执行 `git diff --name-status <baseline>`。
///
/// # Errors
///
/// 当 git 命令执行失败时返回错误。
pub fn changed_files(baseline: &str, repo_path: &Path) -> Result<Vec<ChangedFile>, std::io::Error> {
    let output = Command::new("git").args(["diff", "--name-status", baseline]).current_dir(repo_path).output()?;

    if !output.status.success() {
        return Err(std::io::Error::other(format!("git diff failed: {}", String::from_utf8_lossy(&output.stderr))));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files = stdout.lines().filter_map(ChangedFile::from_diff_line).collect();

    Ok(files)
}

/// 获取指定文件相对于 baseline 的 diff 内容。
///
/// # Errors
///
/// 当 git 命令执行失败时返回错误。
pub fn file_diff(baseline: &str, file_path: &str) -> Result<String, std::io::Error> {
    let output = Command::new("git").args(["diff", baseline, "--", file_path]).output()?;

    if !output.status.success() {
        return Err(std::io::Error::other(format!("git diff failed: {}", String::from_utf8_lossy(&output.stderr))));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// 同步仓库到指定分支：fetch origin → checkout → pull --ff-only。
///
/// 若本地分支不存在，自动从 `origin/<branch>` 创建跟踪分支。
/// 若 fast-forward 失败（本地有分叉提交），返回错误。
///
/// # Errors
///
/// 当仓库不存在、分支不存在于远端、或 git 命令执行失败时返回错误。
pub fn sync_branch(branch: &str, repo_path: &Path) -> Result<(), std::io::Error> {
    tracing::debug!(branch, repo_path = %repo_path.display(), step = "fetch", "sync_branch: 拉取 origin");
    let fetch_output = Command::new("git").args(["fetch", "origin"]).current_dir(repo_path).output()?;

    if !fetch_output.status.success() {
        let stderr = String::from_utf8_lossy(&fetch_output.stderr);
        return Err(std::io::Error::other(format!("git fetch origin 失败: {stderr}")));
    }

    tracing::debug!(branch, repo_path = %repo_path.display(), step = "checkout", "sync_branch: 切换分支");
    let checkout_output = Command::new("git").args(["checkout", branch]).current_dir(repo_path).output()?;

    if !checkout_output.status.success() {
        // 本地分支不存在，尝试创建跟踪分支
        tracing::debug!(branch, repo_path = %repo_path.display(), step = "checkout_B", "sync_branch: 创建跟踪分支");
        let checkout_b_output = Command::new("git")
            .args(["checkout", "-B", branch, &format!("origin/{branch}")])
            .current_dir(repo_path)
            .output()?;

        if !checkout_b_output.status.success() {
            let stderr = String::from_utf8_lossy(&checkout_b_output.stderr);
            return Err(std::io::Error::other(format!("git checkout 失败: {stderr}")));
        }
    }

    tracing::debug!(branch, repo_path = %repo_path.display(), step = "pull", "sync_branch: 快进拉取");
    let pull_output = Command::new("git").args(["pull", "--ff-only"]).current_dir(repo_path).output()?;

    if !pull_output.status.success() {
        let stderr = String::from_utf8_lossy(&pull_output.stderr);
        return Err(std::io::Error::other(format!("git pull --ff-only 失败: {stderr}")));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_diff_line_added() {
        let file = ChangedFile::from_diff_line("A\tsrc/main.rs").unwrap();
        assert_eq!(file.path, PathBuf::from("src/main.rs"));
        assert_eq!(file.status, ChangeStatus::Added);
    }

    #[test]
    fn test_from_diff_line_modified() {
        let file = ChangedFile::from_diff_line("M\tsrc/lib.rs").unwrap();
        assert_eq!(file.path, PathBuf::from("src/lib.rs"));
        assert_eq!(file.status, ChangeStatus::Modified);
    }

    #[test]
    fn test_from_diff_line_deleted() {
        let file = ChangedFile::from_diff_line("D\told_file.rs").unwrap();
        assert_eq!(file.path, PathBuf::from("old_file.rs"));
        assert_eq!(file.status, ChangeStatus::Deleted);
    }

    #[test]
    fn test_from_diff_line_invalid_status() {
        assert!(ChangedFile::from_diff_line("C\tsrc/file.rs").is_none());
        assert!(ChangedFile::from_diff_line("R\tsrc/file.rs").is_none());
        assert!(ChangedFile::from_diff_line("").is_none());
    }

    #[test]
    fn test_from_diff_line_no_tab() {
        assert!(ChangedFile::from_diff_line("A src/file.rs").is_none());
    }

    #[test]
    fn test_from_diff_line_whitespace_path() {
        let file = ChangedFile::from_diff_line("A\t  path/with/spaces  ").unwrap();
        assert_eq!(file.path, PathBuf::from("path/with/spaces"));
    }

    #[test]
    fn test_is_sql() {
        let file = ChangedFile::from_diff_line("A\tquery.sql").unwrap();
        assert!(file.is_sql());
        assert!(!file.is_xml());
    }

    #[test]
    fn test_is_xml() {
        let file = ChangedFile::from_diff_line("M\tUserMapper.xml").unwrap();
        assert!(file.is_xml());
        assert!(!file.is_sql());
    }

    #[test]
    fn test_is_neither_sql_nor_xml() {
        let file = ChangedFile::from_diff_line("A\tREADME.md").unwrap();
        assert!(!file.is_sql());
        assert!(!file.is_xml());
    }

    #[test]
    fn test_is_java() {
        let file = ChangedFile::from_diff_line("A\tFoo.java").unwrap();
        assert!(file.is_java());
        assert!(!file.is_sql());
        assert!(!file.is_xml());

        let non_java = ChangedFile::from_diff_line("A\tREADME.md").unwrap();
        assert!(!non_java.is_java());
    }

    #[test]
    fn test_extension() {
        let file = ChangedFile::from_diff_line("A\tfoo/bar/baz.rs").unwrap();
        assert_eq!(file.extension(), Some("rs"));
    }

    #[test]
    fn test_extension_no_extension() {
        let file = ChangedFile::from_diff_line("A\tDockerfile").unwrap();
        assert_eq!(file.extension(), None);
    }

    // ── validate_baseline tests ───────────────────────────────

    #[test]
    fn test_validate_baseline_invalid_ref() {
        let result = validate_baseline("__nonexistent_branch_xyz__", Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_baseline_empty_string() {
        let result = validate_baseline("", Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_baseline_null_char() {
        let result = validate_baseline("\0", Path::new("."));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_baseline_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_baseline("main", dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_changed_files_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = changed_files("main", dir.path());
        assert!(result.is_err());
    }

    // ── walk_directory tests ──────────────────────────────────

    #[test]
    fn test_walk_directory_finds_supported_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.sql"), "").unwrap();
        std::fs::write(dir.path().join("b.java"), "").unwrap();
        std::fs::write(dir.path().join("c.xml"), "").unwrap();
        std::fs::write(dir.path().join("d.txt"), "").unwrap();

        let files = walk_directory(&[dir.path().to_path_buf()]).unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&dir.path().join("a.sql")));
        assert!(files.contains(&dir.path().join("b.java")));
        assert!(files.contains(&dir.path().join("c.xml")));
        assert!(!files.contains(&dir.path().join("d.txt")));
    }

    #[test]
    fn test_walk_directory_recursive() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("x.sql"), "").unwrap();

        let files = walk_directory(&[dir.path().to_path_buf()]).unwrap();
        assert!(files.contains(&sub.join("x.sql")));
    }

    #[test]
    fn test_walk_directory_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        // Use .ignore (not .gitignore) for reliability — ignore crate reads both.
        std::fs::write(dir.path().join(".ignore"), "*.txt").unwrap();
        std::fs::write(dir.path().join("keep.sql"), "").unwrap();
        std::fs::write(dir.path().join("ignore.txt"), "").unwrap();

        let files = walk_directory(&[dir.path().to_path_buf()]).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files.contains(&dir.path().join("keep.sql")));
    }

    #[test]
    fn test_walk_directory_dedup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.sql"), "").unwrap();

        let files = walk_directory(&[dir.path().to_path_buf(), dir.path().to_path_buf()]).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_walk_directory_nonexistent_path() {
        let result = walk_directory(&[PathBuf::from("/tmp/__nonexistent_cr_test_dir__")]);
        assert!(result.is_err());
    }

    #[test]
    fn test_walk_directory_sql_family_extensions() {
        let dir = tempfile::tempdir().unwrap();
        for ext in &["sql", "prc", "pck", "pkb", "fnc", "java", "xml"] {
            std::fs::write(dir.path().join(format!("test.{ext}")), "").unwrap();
        }

        let files = walk_directory(&[dir.path().to_path_buf()]).unwrap();
        assert_eq!(files.len(), 7);
    }

    #[test]
    fn test_walk_directory_empty_dirs() {
        let files = walk_directory(&[]).unwrap();
        assert!(files.is_empty());
    }

    // ── sync_branch tests ─────────────────────────────────────

    /// Helper: initialize a bare repo + working clone with user config.
    fn init_temp_repo() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let bare_path = dir.path().join("repo.git");

        let status =
            Command::new("git").args(["init", "--bare"]).arg(&bare_path).status().expect("git init --bare failed");
        assert!(status.success());

        let work_path = dir.path().join("work");
        let status = Command::new("git")
            .args(["clone", &bare_path.to_string_lossy(), &work_path.to_string_lossy()])
            .status()
            .expect("git clone failed");
        assert!(status.success());

        for &(key, value) in &[("user.email", "test@test.com"), ("user.name", "Test User")] {
            let status = Command::new("git")
                .args(["config", key, value])
                .current_dir(&work_path)
                .status()
                .expect("git config failed");
            assert!(status.success());
        }

        (dir, work_path)
    }

    #[test]
    fn test_sync_branch_ok() {
        let (dir, work_path) = init_temp_repo();

        // Push initial commit to establish main on remote
        std::fs::write(work_path.join("initial.txt"), b"initial").expect("write failed");
        let status =
            Command::new("git").args(["add", "initial.txt"]).current_dir(&work_path).status().expect("git add failed");
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(&work_path)
            .status()
            .expect("git commit failed");
        assert!(status.success());
        let status = Command::new("git")
            .args(["push", "-u", "origin", "HEAD"])
            .current_dir(&work_path)
            .status()
            .expect("git push failed");
        assert!(status.success());

        // Simulate upstream: clone bare, create feature branch, push
        let upstream = tempfile::tempdir().expect("failed to create upstream temp dir");
        let upstream_path = upstream.path().join("upstream");
        let bare_path = dir.path().join("repo.git");
        let status = Command::new("git")
            .args(["clone", &bare_path.to_string_lossy(), &upstream_path.to_string_lossy()])
            .status()
            .expect("git clone upstream failed");
        assert!(status.success());

        for &(key, value) in &[("user.email", "upstream@test.com"), ("user.name", "Upstream User")] {
            let status = Command::new("git")
                .args(["config", key, value])
                .current_dir(&upstream_path)
                .status()
                .expect("git config failed");
            assert!(status.success());
        }

        let status = Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(&upstream_path)
            .status()
            .expect("git checkout -b failed");
        assert!(status.success());
        std::fs::write(upstream_path.join("feature.txt"), b"feature work").expect("write failed");
        let status = Command::new("git")
            .args(["add", "feature.txt"])
            .current_dir(&upstream_path)
            .status()
            .expect("git add failed");
        assert!(status.success());
        let status = Command::new("git")
            .args(["commit", "-m", "feature commit"])
            .current_dir(&upstream_path)
            .status()
            .expect("git commit failed");
        assert!(status.success());
        let status = Command::new("git")
            .args(["push", "-u", "origin", "feature"])
            .current_dir(&upstream_path)
            .status()
            .expect("git push failed");
        assert!(status.success());

        // Call sync_branch — should fetch, create tracking branch, pull
        let result = sync_branch("feature", &work_path);
        assert!(result.is_ok(), "sync_branch failed: {result:?}");

        // Verify we're on the feature branch with the right content
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&work_path)
            .output()
            .expect("git rev-parse failed");
        let branch_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(branch_name, "feature");

        assert!(work_path.join("feature.txt").exists());
    }

    #[test]
    fn test_sync_branch_non_git_dir() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let result = sync_branch("main", dir.path());
        assert!(result.is_err());
    }
}
