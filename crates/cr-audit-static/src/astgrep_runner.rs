//! astgrep runner：批量扫描磁盘文件，把结果归一化为 [`cr_core::Finding`]。
//!
//! 与 `java_security::audit_java_source` 等 content-based API 不同，runner 需要
//! 文件位于磁盘（astgrep 通过路径扫描），并接受批量路径以分摊进程启动开销。

use std::path::{Path, PathBuf};
use std::process::Command;

use cr_core::{DiagnosticCategory, Finding, Severity};
use serde::Deserialize;
use tracing::warn;

#[cfg(feature = "embed-astgrep")]
use crate::astgrep_embed::{materialize_astgrep_binary, materialize_rules_root};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AstgrepError {
    #[error("astgrep 二进制不可用：{0}。请设置 ASTGREP_BIN 环境变量，或启用 embed-astgrep feature")]
    BinaryUnavailable(String),
    #[error("无效的 preset 路径 `{preset}`：{reason}")]
    InvalidPreset { preset: String, reason: String },
    #[error("规则根目录不可用：{0}")]
    RulesRootUnavailable(String),
    #[error("astgrep 进程失败（exit={code}）：{stderr}")]
    ProcessFailed { code: i32, stderr: String },
    #[error("astgrep 输出反序列化失败：{0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct AstgrepOptions {
    pub binary_path: Option<PathBuf>,
    pub rules_root: Option<PathBuf>,
    pub language: String,
    pub severity_threshold: String,
    pub timeout_secs: u64,
}

impl Default for AstgrepOptions {
    fn default() -> Self {
        Self {
            binary_path: None,
            rules_root: None,
            language: "java".to_string(),
            severity_threshold: "warning".to_string(),
            timeout_secs: 120,
        }
    }
}

#[derive(Debug, Deserialize)]
struct AstgrepOutput {
    findings: Vec<AstgrepFinding>,
}

#[derive(Debug, Deserialize)]
struct AstgrepFinding {
    rule_id: String,
    severity: String,
    message: String,
    #[serde(default)]
    fix: Option<String>,
    location: AstgrepLocation,
}

#[derive(Debug, Deserialize)]
struct AstgrepLocation {
    file: String,
    start_line: usize,
    #[serde(default)]
    #[allow(dead_code)]
    end_line: Option<usize>,
}

/// 解析 `config.rules.astgrep.preset` 列表为规则目录路径。
/// preset 直接是相对路径（如 `java/security`），禁止 `..` 与绝对路径。
pub fn resolve_rule_dirs(presets: &[String], rules_root: &Path) -> Result<Vec<PathBuf>, AstgrepError> {
    let mut dirs = Vec::with_capacity(presets.len());
    for preset in presets {
        validate_preset_path(preset)?;
        let candidate = rules_root.join(preset);
        if !candidate.is_dir() {
            return Err(AstgrepError::InvalidPreset {
                preset: preset.clone(),
                reason: format!("规则目录不存在：{}", candidate.display()),
            });
        }
        dirs.push(candidate);
    }
    Ok(dirs)
}

fn validate_preset_path(preset: &str) -> Result<(), AstgrepError> {
    if preset.is_empty() {
        return Err(AstgrepError::InvalidPreset { preset: preset.into(), reason: "空路径".into() });
    }
    let path = Path::new(preset);
    if path.is_absolute() {
        return Err(AstgrepError::InvalidPreset {
            preset: preset.into(),
            reason: "禁止绝对路径".into(),
        });
    }
    for comp in path.components() {
        use std::path::Component;
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(AstgrepError::InvalidPreset {
                    preset: preset.into(),
                    reason: "禁止 `..` 越权".into(),
                });
            }
            _ => {
                return Err(AstgrepError::InvalidPreset {
                    preset: preset.into(),
                    reason: "包含非法路径组件".into(),
                });
            }
        }
    }
    Ok(())
}

pub fn resolve_binary(options: &AstgrepOptions) -> Result<PathBuf, AstgrepError> {
    if let Some(p) = &options.binary_path {
        if p.is_file() {
            return Ok(p.clone());
        }
    }
    if let Ok(p) = std::env::var("ASTGREP_BIN") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
    }
    #[cfg(feature = "embed-astgrep")]
    {
        if let Ok(p) = materialize_astgrep_binary() {
            return Ok(p);
        }
    }
    for candidate in ["astgrep", "ast-grep"].into_iter().map(PathBuf::from) {
        if which_on_path(&candidate).is_some() {
            return Ok(candidate);
        }
    }
    Err(AstgrepError::BinaryUnavailable(
        "未在 ASTGREP_BIN、内嵌资源或 PATH 中找到 astgrep 可执行文件".into(),
    ))
}

pub fn resolve_rules_root(options: &AstgrepOptions) -> Result<PathBuf, AstgrepError> {
    if let Some(p) = &options.rules_root {
        if p.is_dir() {
            return Ok(p.clone());
        }
    }
    if let Ok(p) = std::env::var("CR_RULES_ROOT") {
        let path = PathBuf::from(p);
        if path.is_dir() {
            return Ok(path);
        }
    }
    #[cfg(feature = "embed-astgrep")]
    {
        if let Ok(p) = materialize_rules_root() {
            return Ok(p);
        }
    }
    Err(AstgrepError::RulesRootUnavailable(
        "未在 AstgrepOptions、CR_RULES_ROOT 环境变量或内嵌资源中找到 cr-rules 根目录".into(),
    ))
}

fn which_on_path(name: &Path) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        #[cfg(unix)]
        if candidate.is_file() {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = candidate.metadata() {
                if meta.permissions().mode() & 0o111 != 0 {
                    return Some(candidate);
                }
            }
        }
        #[cfg(not(unix))]
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 批量扫描入口。
pub fn audit_files(
    files: &[PathBuf],
    presets: &[String],
    options: &AstgrepOptions,
) -> Result<Vec<Finding>, AstgrepError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let binary = resolve_binary(options)?;
    let rules_root = resolve_rules_root(options)?;
    let rule_dirs = resolve_rule_dirs(presets, &rules_root)?;
    if rule_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let mut cmd = Command::new(&binary);
    cmd.arg("analyze");
    for dir in &rule_dirs {
        cmd.arg("--rules").arg(dir);
    }
    cmd.arg("--language").arg(&options.language);
    cmd.arg("--format").arg("json");
    cmd.arg("--severity").arg(&options.severity_threshold);
    for f in files {
        cmd.arg(f);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let code = output.status.code().unwrap_or(-1);
        if output.stdout.is_empty() {
            return Err(AstgrepError::ProcessFailed { code, stderr });
        }
        warn!(code, stderr = %stderr, "astgrep exited non-zero but produced output; attempting to parse");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_start = stdout
        .find('{')
        .ok_or_else(|| AstgrepError::ProcessFailed { code: -1, stderr: String::from("no JSON object in stdout") })?;
    let parsed: AstgrepOutput = serde_json::from_str(&stdout[json_start..])?;
    Ok(parsed.findings.into_iter().map(convert_finding).collect())
}

fn convert_finding(f: AstgrepFinding) -> Finding {
    Finding::new(
        f.rule_id,
        map_severity(&f.severity),
        DiagnosticCategory::General,
        truncate_title(&f.message),
        f.message,
        f.location.file,
        None,
        Some(f.location.start_line),
        None,
        f.fix,
    )
}

fn map_severity(raw: &str) -> Severity {
    match raw.to_ascii_uppercase().as_str() {
        "ERROR" | "CRITICAL" => Severity::Critical,
        "WARNING" => Severity::Warning,
        _ => Severity::Info,
    }
}

fn truncate_title(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or(message);
    if first_line.chars().count() > 120 {
        let mut end = 0;
        for (i, _) in first_line.char_indices().take(120) {
            end = i;
        }
        format!("{}…", &first_line[..end])
    } else {
        first_line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_astgrep_json() {
        let raw = r#"{
            "findings": [
                {
                    "rule_id": "java-sec-017-secure-random",
                    "severity": "ERROR",
                    "message": "应使用安全随机数java.security.SecureRandom",
                    "fix": "使用 SecureRandom",
                    "location": {"file": "A.java", "start_line": 17, "end_line": 18}
                },
                {
                    "rule_id": "java-sec-031-input-sanitization",
                    "severity": "WARNING",
                    "message": "输入未清理",
                    "location": {"file": "B.java", "start_line": 5}
                }
            ],
            "summary": {"total_findings": 2}
        }"#;
        let parsed: AstgrepOutput = serde_json::from_str(raw).expect("deserialize");
        assert_eq!(parsed.findings.len(), 2);
        assert_eq!(parsed.findings[0].location.start_line, 17);
        assert!(parsed.findings[0].fix.is_some());
        assert!(parsed.findings[1].fix.is_none());
    }

    #[test]
    fn severity_mapping() {
        assert_eq!(map_severity("ERROR"), Severity::Critical);
        assert_eq!(map_severity("error"), Severity::Critical);
        assert_eq!(map_severity("WARNING"), Severity::Warning);
        assert_eq!(map_severity("INFO"), Severity::Info);
        assert_eq!(map_severity("weird"), Severity::Info);
    }

    #[test]
    fn preset_accepts_simple_relative() {
        validate_preset_path("java/security").expect("simple relative");
        validate_preset_path("shell/security").expect("shell");
    }

    #[test]
    fn preset_rejects_traversal() {
        assert!(validate_preset_path("../etc/passwd").is_err());
        assert!(validate_preset_path("java/../../etc").is_err());
        assert!(validate_preset_path("/etc/passwd").is_err());
        assert!(validate_preset_path("").is_err());
    }

    #[test]
    fn title_truncation() {
        let short = "短标题";
        assert_eq!(truncate_title(short), short);

        let long = "a".repeat(200);
        let t = truncate_title(&long);
        assert!(t.chars().count() <= 121);
        assert!(t.ends_with('…'));

        let multiline = "first line\nsecond line";
        assert_eq!(truncate_title(multiline), "first line");
    }
}
