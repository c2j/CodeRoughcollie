//! 待审核清单（CSV manifest）解析。

use std::path::{Path, PathBuf};

use thiserror::Error;

/// 一条清单项：一个 (项目, 分支) 组合及其待审核文件列表。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ManifestEntry {
    /// 项目名称（对应 `.roughcollie.toml` 中 `[projects.*]` 的 key）。
    pub project: String,
    /// 待审核的分支名。
    pub branch: String,
    /// 待审核文件路径列表。
    pub files: Vec<PathBuf>,
}

/// 清单解析错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// 读取清单文件失败。
    #[error("读取清单文件失败 ({0}): {1}")]
    ReadFailed(String, String),
    /// 清单解析失败（CSV 格式错误）。
    #[error("清单解析失败: {0}")]
    Parse(String),
    /// 清单表头无效或缺失。
    #[error("清单表头无效: {0}")]
    InvalidHeader(String),
    /// 清单条目字段无效。
    #[error("清单条目无效 (行 {line}): {reason}")]
    InvalidEntry { line: usize, reason: String },
}

/// 解析 CSV 清单文件，返回清单条目列表。
///
/// 清单文件为 CSV 格式，首行必须为 `project,branch,files` 表头。
/// `files` 字段使用分号（`;`）分隔多个文件路径。
///
/// # Errors
///
/// 返回 [`ManifestError`] 的几种情况：
/// - 文件无法读取时返回 [`ManifestError::ReadFailed`]。
/// - CSV 格式错误时返回 [`ManifestError::Parse`]。
/// - 表头缺失或列不匹配时返回 [`ManifestError::InvalidHeader`]。
/// - 条目中 project/branch 为空或 files 为空时返回 [`ManifestError::InvalidEntry`]。
pub fn parse_manifest(path: &Path) -> Result<Vec<ManifestEntry>, ManifestError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ManifestError::ReadFailed(path.display().to_string(), e.to_string()))?;

    let mut reader = csv::ReaderBuilder::new().has_headers(true).flexible(false).from_reader(content.as_bytes());

    // 校验表头
    let headers = reader.headers().map_err(|e| ManifestError::Parse(e.to_string()))?;
    let raw_headers: Vec<&str> = headers.iter().collect();
    if raw_headers.len() != 3
        || raw_headers[0].trim().to_lowercase() != "project"
        || raw_headers[1].trim().to_lowercase() != "branch"
        || raw_headers[2].trim().to_lowercase() != "files"
    {
        let joined = raw_headers.join(",");
        return Err(ManifestError::InvalidHeader(joined));
    }

    let mut entries: Vec<ManifestEntry> = Vec::new();

    for result in reader.records() {
        let record = result.map_err(|e| ManifestError::Parse(e.to_string()))?;

        // position().line() 返回 1-based 行号（表头为第 1 行，首条数据为第 2 行）
        let line = record.position().map_or(0, |p| p.line()) as usize;

        let project = record.get(0).unwrap_or("").trim().to_string();
        if project.is_empty() {
            return Err(ManifestError::InvalidEntry { line, reason: "project 不能为空".to_string() });
        }

        let branch = record.get(1).unwrap_or("").trim().to_string();
        if branch.is_empty() {
            return Err(ManifestError::InvalidEntry { line, reason: "branch 不能为空".to_string() });
        }

        let files_raw = record.get(2).unwrap_or("");
        let files: Vec<PathBuf> =
            files_raw.split(';').map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).map(PathBuf::from).collect();
        if files.is_empty() {
            return Err(ManifestError::InvalidEntry { line, reason: "files 不能为空".to_string() });
        }

        entries.push(ManifestEntry { project, branch, files });
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_csv(content: &str) -> (PathBuf, tempfile::NamedTempFile) {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "{content}").unwrap();
        let path = file.path().to_path_buf();
        (path, file)
    }

    #[test]
    fn test_parse_valid() {
        let csv_content = "project,branch,files\n\
                           order-service,feat/order-opt,\"src/sql/query.sql;src/sql/proc.sql\"\n\
                           payment-service,fix/pay-bug,src/mapper/PayMapper.xml\n";
        let (path, _file) = write_temp_csv(csv_content);
        let entries = parse_manifest(&path).unwrap();

        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].project, "order-service");
        assert_eq!(entries[0].branch, "feat/order-opt");
        assert_eq!(entries[0].files.len(), 2);
        assert_eq!(entries[0].files[0], PathBuf::from("src/sql/query.sql"));
        assert_eq!(entries[0].files[1], PathBuf::from("src/sql/proc.sql"));

        assert_eq!(entries[1].project, "payment-service");
        assert_eq!(entries[1].branch, "fix/pay-bug");
        assert_eq!(entries[1].files.len(), 1);
        assert_eq!(entries[1].files[0], PathBuf::from("src/mapper/PayMapper.xml"));
    }

    #[test]
    fn test_parse_missing_header() {
        let csv_content = "name,branch,files\norder,main,file.sql\n";
        let (path, _file) = write_temp_csv(csv_content);
        let result = parse_manifest(&path);
        assert!(result.is_err());
        match result {
            Err(ManifestError::InvalidHeader(_)) => {}
            _ => panic!("Expected InvalidHeader error"),
        }
    }

    #[test]
    fn test_parse_empty_project() {
        let csv_content = "project,branch,files\n\
                           ,feat/x,file.sql\n";
        let (path, _file) = write_temp_csv(csv_content);
        let result = parse_manifest(&path);
        assert!(result.is_err());
        match result {
            Err(ManifestError::InvalidEntry { line, reason }) => {
                assert_eq!(line, 2);
                assert!(reason.contains("project"));
            }
            _ => panic!("Expected InvalidEntry error"),
        }
    }

    #[test]
    fn test_parse_empty_files() {
        let csv_content = "project,branch,files\n\
                           svc,feat/x,\n";
        let (path, _file) = write_temp_csv(csv_content);
        let result = parse_manifest(&path);
        assert!(result.is_err());
        match result {
            Err(ManifestError::InvalidEntry { line, reason }) => {
                assert_eq!(line, 2);
                assert!(reason.contains("files"));
            }
            _ => panic!("Expected InvalidEntry error"),
        }
    }

    #[test]
    fn test_parse_skips_empty_lines() {
        // csv crate 默认跳过空行
        let csv_content = "project,branch,files\n\
                           svc,feat/a,file1.sql\n\
                           \n\
                           svc2,feat/b,file2.sql\n";
        let (path, _file) = write_temp_csv(csv_content);
        let entries = parse_manifest(&path).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_parse_nonexistent_file() {
        let path = PathBuf::from("/tmp/__nonexistent_manifest_12345.csv");
        let result = parse_manifest(&path);
        assert!(result.is_err());
        match result {
            Err(ManifestError::ReadFailed(_, _)) => {}
            _ => panic!("Expected ReadFailed error"),
        }
    }

    #[test]
    fn test_parse_files_trimming() {
        let csv_content = "project,branch,files\n\
                           svc,feat/x,\" a.sql ; b.sql \"\n";
        let (path, _file) = write_temp_csv(csv_content);
        let entries = parse_manifest(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].files.len(), 2);
        assert_eq!(entries[0].files[0], PathBuf::from("a.sql"));
        assert_eq!(entries[0].files[1], PathBuf::from("b.sql"));
    }

    #[test]
    fn test_parse_invalid_csv() {
        let csv_content = "project,branch,files\n\"unclosed\n";
        let (path, _file) = write_temp_csv(csv_content);
        let result = parse_manifest(&path);
        assert!(result.is_err());
        match result {
            Err(ManifestError::Parse(_)) => {}
            _ => panic!("Expected Parse error"),
        }
    }
}
