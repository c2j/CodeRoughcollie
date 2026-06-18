//! Git diff 解析、baseline 对比。

use std::path::PathBuf;
use std::process::Command;

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

    /// 是否为 XML 文件（MyBatis Mapper）。
    #[must_use]
    pub fn is_mybatis_xml(&self) -> bool {
        self.extension() == Some("xml")
    }
}

/// 获取相对于 baseline 分支的变更文件列表。
///
/// # Errors
///
/// 当 git 命令执行失败时返回错误。
pub fn changed_files(baseline: &str) -> Result<Vec<ChangedFile>, std::io::Error> {
    let output = Command::new("git").args(["diff", "--name-status", baseline]).output()?;

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
        assert!(!file.is_mybatis_xml());
    }

    #[test]
    fn test_is_mybatis_xml() {
        let file = ChangedFile::from_diff_line("M\tUserMapper.xml").unwrap();
        assert!(file.is_mybatis_xml());
        assert!(!file.is_sql());
    }

    #[test]
    fn test_is_neither_sql_nor_xml() {
        let file = ChangedFile::from_diff_line("A\tREADME.md").unwrap();
        assert!(!file.is_sql());
        assert!(!file.is_mybatis_xml());
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
}
