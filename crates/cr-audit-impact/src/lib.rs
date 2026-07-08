//! 语义影响分析：集成 codeweb 查询调用链（三期）。
//!
//! 本 crate 通过子进程调用 codeweb 二进制来查询 Java/Mapper 文件的
//! 上下游调用链，并将分析结果转化为审核发现。
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use cr_audit_impact::CodewebRunner;
//! use std::path::Path;
//! use std::time::Duration;
//!
//! # fn example() -> Result<(), cr_audit_impact::CodewebError> {
//! let runner = CodewebRunner {
//!     binary: None,
//!     timeout: Duration::from_secs(30),
//! };
//!
//! // 先探测二进制是否可用
//! runner.check_available()?;
//!
//! // 查询影响分析
//! let result = runner.query_impact(
//!     "src/main/java/com/example/Mapper.java",
//!     Path::new("/path/to/project"),
//! )?;
//!
//! let findings = cr_audit_impact::impact_to_findings(&result, "src/main/java/com/example/Mapper.java");
//! # Ok(())
//! # }
//! ```

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// codeweb 子进程错误。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodewebError {
    /// codeweb 二进制未找到。
    #[error("codeweb 二进制未找到: {0}（请在 [codeweb].binary 配置或加入 PATH）")]
    BinaryNotFound(String),

    /// 子进程执行超时。
    #[error("codeweb 子进程超时（{secs}s）")]
    Timeout { secs: u64 },

    /// 子进程退出码非零。
    #[error("codeweb 退出码 {code}: {stderr}")]
    NonZeroExit { code: i32, stderr: String },

    /// JSON 输出解析失败。
    #[error("codeweb 输出解析失败: {0}")]
    Parse(#[from] serde_json::Error),

    /// 子进程成功退出但 stdout 为空（项目未建图）。
    #[error(
        "codeweb 输出了空的 impact 结果——项目可能未建图。\n\
         建议: 在 coderc 命令中添加 --codeweb-analyze 参数，\
         或先手动执行 `codeweb analyze --project <project_path>`"
    )]
    EmptyOutput,

    /// IO 错误（子进程创建、通信等）。
    #[error("codeweb IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 子进程运行配置。
#[derive(Debug, Clone)]
pub struct CodewebRunner {
    /// 二进制路径；`None` 则从 PATH 查找 `codeweb`。
    pub binary: Option<PathBuf>,

    /// 子进程超时。
    pub timeout: Duration,
}

impl CodewebRunner {
    /// 解析最终二进制路径。
    ///
    /// - `self.binary` 为 `Some(path)` 时检查文件是否存在；
    /// - `None` 时从 `PATH` 环境变量查找 `codeweb`。
    fn bin_path(&self) -> Result<PathBuf, CodewebError> {
        if let Some(ref bin) = self.binary {
            if bin.exists() {
                Ok(bin.clone())
            } else {
                Err(CodewebError::BinaryNotFound(bin.to_string_lossy().to_string()))
            }
        } else {
            // 在 PATH 中查找 "codeweb"
            if let Some(paths) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&paths) {
                    let candidate = dir.join("codeweb");
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                    #[cfg(windows)]
                    {
                        let candidate_exe = dir.join("codeweb.exe");
                        if candidate_exe.is_file() {
                            return Ok(candidate_exe);
                        }
                    }
                }
            }
            Err(CodewebError::BinaryNotFound("codeweb".to_string()))
        }
    }

    /// 运行 codeweb 命令并捕获输出。
    fn run_codeweb(&self, args: &[&str]) -> Result<(String, String), CodewebError> {
        use wait_timeout::ChildExt;
        let bin = self.bin_path()?;
        // 重定向到临时文件而非管道：避免输出超过 ~64KB 管道缓冲时子进程阻塞写、
        // 父进程阻塞 wait 的死锁（表现为假的超时挂起）。
        let mut out_file = tempfile::tempfile()?;
        let mut err_file = tempfile::tempfile()?;
        let mut child = Command::new(&bin)
            .args(args)
            .stdout(Stdio::from(out_file.try_clone()?))
            .stderr(Stdio::from(err_file.try_clone()?))
            .spawn()?;

        let status = match child.wait_timeout(self.timeout)? {
            Some(status) => status,
            None => {
                // 超时：终止子进程并回收
                child.kill()?;
                child.wait()?;
                return Err(CodewebError::Timeout { secs: self.timeout.as_secs() });
            }
        };

        // try_clone 在 Unix 上 dup 出的 fd 共享偏移（子进程写后偏移在末尾），
        // seek(0) 回到起始再读，跨平台都能拿到完整输出。
        out_file.seek(SeekFrom::Start(0))?;
        err_file.seek(SeekFrom::Start(0))?;
        // lossy 而非 read_to_string：stderr 可能含非 UTF-8 字节（GBK 路径/原始栈迹），
        // read_to_string 会 InvalidData 失败并丢失 NonZeroExit 的真实错误信息。
        let mut stdout_bytes = Vec::new();
        out_file.read_to_end(&mut stdout_bytes)?;
        let stdout = String::from_utf8_lossy(&stdout_bytes).into_owned();
        let mut stderr_bytes = Vec::new();
        err_file.read_to_end(&mut stderr_bytes)?;
        let stderr = String::from_utf8_lossy(&stderr_bytes).into_owned();

        if status.success() {
            Ok((stdout, stderr))
        } else {
            Err(CodewebError::NonZeroExit { code: status.code().unwrap_or(-1), stderr })
        }
    }

    /// 探测二进制是否可用（`codeweb --version` 退出 0）。
    pub fn check_available(&self) -> Result<(), CodewebError> {
        self.run_codeweb(&["--version"])?;
        Ok(())
    }

    /// `codeweb analyze --project <path>`：建图 / 增量刷新。
    pub fn analyze(&self, project_path: &Path) -> Result<(), CodewebError> {
        let proj = project_path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "project path is not valid UTF-8"))?;
        self.run_codeweb(&["analyze", "--project", proj])?;
        Ok(())
    }

    /// `codeweb impact --file <file> --project <proj> --format json`：查询影响。
    pub fn query_impact(&self, file: &str, project_path: &Path) -> Result<ImpactResult, CodewebError> {
        let proj = project_path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "project path is not valid UTF-8"))?;
        let (stdout, _stderr) = self.run_codeweb(&["impact", "--file", file, "--project", proj, "--format", "json"])?;
        // codeweb 在项目未建图时 exit 0 但 stdout 为空（输出仅写入 stderr），
        // 此时 serde_json::from_str("") 会产生晦涩的 "EOF while parsing" 错误。
        // 提前识别这个场景，返回更明确的错误信息。
        if stdout.trim().is_empty() {
            return Err(CodewebError::EmptyOutput);
        }
        let result: ImpactResult = serde_json::from_str(&stdout)?;
        Ok(result)
    }
}

/// 影响分析结果。
///
/// 包含变更文件的上游调用者和下游被调用者列表。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImpactResult {
    /// 上游调用者（哪些文件调用了变更文件）。
    pub upstream: Vec<ImpactNode>,
    /// 下游被调用者（变更文件调用了哪些文件）。
    pub downstream: Vec<ImpactNode>,
}

/// 调用链节点。
///
/// 描述单条调用关系中的文件、符号及行号。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImpactNode {
    /// 文件路径（无源文件时为 `None`，如未解析节点、CGEF 合并节点）。
    pub file_path: Option<String>,
    /// 符号名称（方法名、类名等）。
    pub symbol: String,
    /// 符号所在行号（可选）。
    pub line: Option<usize>,
}

/// 将影响分析结果转为 Finding 列表。
///
/// 当变更文件有大量上游调用者时，标记为高影响变更。
///
/// # 规则
///
/// | 规则 ID | 触发条件 | 严重度 |
/// |---------|---------|--------|
/// | IMPACT-001 | 上游调用者 > 10 | Warning |
///
/// # 参数
///
/// * `result` — 影响分析结果。
/// * `changed_file` — 变更文件路径（用于发现描述）。
#[must_use]
pub fn impact_to_findings(result: &ImpactResult, changed_file: &str) -> Vec<cr_core::Finding> {
    let upstream_count = result.upstream.len();

    if upstream_count > 10 {
        let callers: Vec<&str> =
            result.upstream.iter().map(|n| n.file_path.as_deref().unwrap_or("<unknown>")).collect();

        vec![cr_core::Finding::new(
            "IMPACT-001",
            cr_core::Severity::Warning,
            cr_core::DiagnosticCategory::General,
            "高影响变更",
            format!("文件 `{changed_file}` 有 {upstream_count} 个上游调用者，变更影响范围较大，建议仔细审查"),
            changed_file,
            None,
            None,
            None,
            Some(format!("上游调用者列表：{}", callers.join(", "))),
        )]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_not_found() {
        let runner =
            CodewebRunner { binary: Some(PathBuf::from("/nonexistent/codeweb")), timeout: Duration::from_secs(5) };
        assert!(matches!(runner.check_available(), Err(CodewebError::BinaryNotFound(_))));
    }

    #[test]
    fn test_impact_result_parses_schema() {
        let json = r#"{"schema_version":1,"file":"a.java","upstream":[{"file_path":"b.java","symbol":"m","line":1}],"downstream":[]}"#;
        let r: ImpactResult = serde_json::from_str(json).unwrap();
        assert_eq!(r.upstream.len(), 1);
        assert_eq!(r.upstream[0].symbol, "m");
    }

    #[test]
    fn test_impact_result_handles_null_file_path() {
        let json = r#"{"schema_version":1,"file":"a.java","upstream":[{"file_path":null,"symbol":"m","line":1}],"downstream":[]}"#;
        let r: ImpactResult = serde_json::from_str(json).unwrap();
        assert_eq!(r.upstream.len(), 1);
        assert!(r.upstream[0].file_path.is_none());
    }

    #[test]
    fn test_impact_empty() {
        let r = ImpactResult { upstream: vec![], downstream: vec![] };
        assert!(impact_to_findings(&r, "test.java").is_empty());
    }

    #[test]
    fn test_impact_many_upstream() {
        let upstream: Vec<ImpactNode> = (0..15)
            .map(|i| ImpactNode { file_path: Some(format!("c{i}.java")), symbol: format!("m{i}"), line: Some(i) })
            .collect();
        let r = ImpactResult { upstream, downstream: vec![] };
        let f = impact_to_findings(&r, "changed.java");
        assert!(!f.is_empty());
        assert!(f.iter().any(|f| f.rule_id.contains("IMPACT")));
    }

    #[test]
    fn test_impact_many_upstream_null_file_path() {
        let upstream: Vec<ImpactNode> =
            (0..15).map(|i| ImpactNode { file_path: None, symbol: format!("m{i}"), line: Some(i) }).collect();
        let r = ImpactResult { upstream, downstream: vec![] };
        let f = impact_to_findings(&r, "changed.java");
        assert!(!f.is_empty(), "null file_path 不应导致 impact_to_findings 失败");
    }
}
