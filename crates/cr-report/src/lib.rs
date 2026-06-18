//! 审核报告生成。
//!
//! 支持 Markdown / JSON / SARIF 三种输出格式。

use cr_core::Finding;

/// 报告输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportFormat {
    /// Markdown 格式（适合 PR 评论）。
    Markdown,
    /// JSON 格式（适合程序消费）。
    Json,
    /// SARIF 格式（兼容 GitHub Advanced Security）。
    Sarif,
}

impl ReportFormat {
    /// 从字符串解析格式。
    ///
    /// # Errors
    ///
    /// 当格式名称不被识别时返回错误。
    pub fn parse(s: &str) -> Result<Self, UnknownFormatError> {
        match s.to_lowercase().as_str() {
            "markdown" | "md" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            "sarif" => Ok(Self::Sarif),
            other => Err(UnknownFormatError(other.to_string())),
        }
    }
}

/// 未知格式错误。
#[derive(Debug, thiserror::Error)]
#[error("未知报告格式: {0}（支持: markdown, json, sarif）")]
pub struct UnknownFormatError(pub String);

/// 根据格式渲染报告。
#[must_use]
pub fn render(findings: &[Finding], format: ReportFormat) -> String {
    match format {
        ReportFormat::Markdown => render_markdown(findings),
        ReportFormat::Json => render_json(findings),
        ReportFormat::Sarif => render_sarif(findings),
    }
}

/// 将 Finding 列表渲染为 Markdown 报告。
#[must_use]
pub fn render_markdown(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "✅ 审核通过，未发现问题。".to_string();
    }

    let mut out = String::with_capacity(findings.len() * 256);
    out.push_str("# CodeRoughcollie 审核报告\n\n");

    for f in findings {
        out.push_str(&format!(
            "### {} [{}] {}\n\n**严重度**: {}\n\n**详情**: {}\n\n",
            f.severity.icon(),
            f.rule_id,
            f.title,
            f.severity.as_str(),
            f.detail,
        ));
        if let Some(ref suggestion) = f.suggestion {
            out.push_str(&format!("**建议**: {suggestion}\n\n"));
        }
        out.push_str("---\n\n");
    }

    out
}

/// 将 Finding 列表渲染为 JSON 报告。
#[must_use]
pub fn render_json(findings: &[Finding]) -> String {
    serde_json::to_string_pretty(findings).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

/// 将 Finding 列表渲染为 SARIF 报告（GitHub Advanced Security 兼容）。
#[must_use]
pub fn render_sarif(findings: &[Finding]) -> String {
    let results: Vec<serde_json::Value> = findings
        .iter()
        .map(|f| {
            serde_json::json!({
                "ruleId": f.rule_id,
                "level": match f.severity {
                    cr_core::Severity::Critical => "error",
                    cr_core::Severity::Warning => "warning",
                    _ => "note",
                },
                "message": { "text": &f.detail }
            })
        })
        .collect();

    let sarif = serde_json::json!({
        "version": "2.1.0",
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "CodeRoughcollie",
                    "version": env!("CARGO_PKG_VERSION")
                }
            },
            "results": results
        }]
    });

    serde_json::to_string_pretty(&sarif).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::{DiagnosticCategory, Severity};

    fn sample_findings() -> Vec<Finding> {
        vec![
            Finding::new(
                "SCAN-001",
                Severity::Critical,
                DiagnosticCategory::ScanEfficiency,
                "大表全表扫描",
                "表 users 未使用索引，预计扫描 1M 行",
                Some(3),
                Some("Seq Scan".into()),
                Some("添加索引 idx_users_status".into()),
            ),
            Finding::new(
                "TYPE-001",
                Severity::Warning,
                DiagnosticCategory::TypeMismatch,
                "隐式类型转换",
                "WHERE id = '123' 导致类型转换",
                None,
                None,
                None,
            ),
        ]
    }

    #[test]
    fn test_render_markdown_empty() {
        let output = render_markdown(&[]);
        assert!(output.contains("审核通过"));
    }

    #[test]
    fn test_render_markdown_with_findings() {
        let output = render_markdown(&sample_findings());
        assert!(output.contains("SCAN-001"));
        assert!(output.contains("TYPE-001"));
        assert!(output.contains("🔴"));
        assert!(output.contains("🟡"));
        assert!(output.contains("CodeRoughcollie 审核报告"));
        assert!(output.contains("添加索引"));
    }

    #[test]
    fn test_render_json_valid() {
        let output = render_json(&sample_findings());
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_render_json_empty() {
        let output = render_json(&[]);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
        assert!(parsed.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_render_sarif_version() {
        let output = render_sarif(&sample_findings());
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["version"], "2.1.0");
        assert!(parsed["$schema"].as_str().unwrap().contains("sarif"));
        assert_eq!(parsed["runs"][0]["tool"]["driver"]["name"], "CodeRoughcollie");
    }

    #[test]
    fn test_render_sarif_empty() {
        let output = render_sarif(&[]);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed["runs"][0]["results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_render_dispatches_correctly() {
        let findings = sample_findings();
        let md = render(&findings, ReportFormat::Markdown);
        assert!(md.contains("SCAN-001"));

        let json = render(&findings, ReportFormat::Json);
        assert!(json.contains("SCAN-001"));

        let sarif = render(&findings, ReportFormat::Sarif);
        assert!(sarif.contains("SCAN-001"));
    }

    #[test]
    fn test_report_format_parse_markdown() {
        assert_eq!(ReportFormat::parse("markdown").unwrap(), ReportFormat::Markdown);
        assert_eq!(ReportFormat::parse("md").unwrap(), ReportFormat::Markdown);
        assert_eq!(ReportFormat::parse("MARKDOWN").unwrap(), ReportFormat::Markdown);
    }

    #[test]
    fn test_report_format_parse_json() {
        assert_eq!(ReportFormat::parse("json").unwrap(), ReportFormat::Json);
        assert_eq!(ReportFormat::parse("JSON").unwrap(), ReportFormat::Json);
    }

    #[test]
    fn test_report_format_parse_sarif() {
        assert_eq!(ReportFormat::parse("sarif").unwrap(), ReportFormat::Sarif);
        assert_eq!(ReportFormat::parse("SARIF").unwrap(), ReportFormat::Sarif);
    }

    #[test]
    fn test_report_format_parse_unknown() {
        assert!(ReportFormat::parse("html").is_err());
        assert!(ReportFormat::parse("").is_err());
        assert!(ReportFormat::parse("pdf").is_err());
    }

    #[test]
    fn test_render_markdown_empty() {
        assert!(render_markdown(&[]).contains("审核通过"));
    }

    #[test]
    fn test_render_json_valid() {
        let f = vec![Finding::new(
            "TEST-001",
            cr_core::Severity::Warning,
            cr_core::DiagnosticCategory::General,
            "test",
            "detail".into(),
            None,
            None,
            None,
        )];
        let json = render_json(&f);
        assert!(json.contains("TEST-001"));
        assert!(json.contains("test"));
        serde_json::from_str::<Vec<Finding>>(&json).unwrap();
    }

    #[test]
    fn test_render_sarif_has_schema() {
        let f = vec![Finding::new(
            "R001",
            cr_core::Severity::Critical,
            cr_core::DiagnosticCategory::ScanEfficiency,
            "title",
            "body".into(),
            Some(1),
            None,
            None,
        )];
        let sarif = render_sarif(&f);
        assert!(sarif.contains("2.1.0"));
        assert!(sarif.contains("CodeRoughcollie"));
        assert!(sarif.contains("error"));
    }

    #[test]
    fn test_render_dispatches_by_format() {
        let f = vec![Finding::new(
            "X",
            cr_core::Severity::Info,
            cr_core::DiagnosticCategory::General,
            "t",
            "d".into(),
            None,
            None,
            None,
        )];
        assert!(render(&f, ReportFormat::Markdown).contains("审核报告"));
        assert!(render(&f, ReportFormat::Json).contains('['));
        assert!(render(&f, ReportFormat::Sarif).contains("runs"));
    }
}
