//! Java/MyBatis 安全扫描。
//!
//! ## astgrep 集成状态
//! 目标：替换 regex 为 astgrep AST 模式匹配。
//! 阻塞：astgrep-parser 的 ogsql-parser.workspace=true 版本冲突（5 处类型不匹配）。
//! Issue: https://github.com/c2j/astgrep/issues/21
//! 当前实现：regex 过渡方案，消费 astgrep-core 类型以保证兼容性。

use std::path::Path;

use cr_core::{DiagnosticCategory, Finding, Severity};
use regex::Regex;

fn line_number(content: &str, byte_offset: usize) -> Option<usize> {
    if byte_offset > content.len() {
        return None;
    }
    Some(content[..byte_offset].matches('\n').count() + 1)
}

pub fn audit_mybatis_xml(xml_content: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let dollar_re = match Regex::new(r#"\$\{[^}]*\}"#) {
        Ok(r) => r,
        Err(_) => return findings,
    };
    for m in dollar_re.find_iter(xml_content) {
        let matched = m.as_str();
        let line = line_number(xml_content, m.start());
        findings.push(Finding::new(
            "SECURITY-MYBATIS-DOLLAR-PARAM",
            Severity::Critical,
            DiagnosticCategory::General,
            "检测到 ${} 参数替换，存在 SQL 注入风险",
            format!("MyBatis 中使用 '{matched}' 进行字符串替换，应使用 #{{param}} 参数化绑定"),
            file_path,
            Some(matched.to_string()),
            line,
            None,
            Some(format!("将 '{matched}' 替换为 #{{param}} 形式")),
        ));
    }
    findings
}

pub fn audit_java_source(java_content: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let exec_re = match Regex::new(r#"(?s)\.(?:execute|executeQuery|executeUpdate)\s*\([^)]*"[^"]*"\s*\+"#) {
        Ok(r) => r,
        Err(_) => return findings,
    };
    for m in exec_re.find_iter(java_content) {
        let line = line_number(java_content, m.start());
        findings.push(Finding::new(
            "SECURITY-JAVA-STATEMENT-EXEC",
            Severity::Critical,
            DiagnosticCategory::General,
            "Statement.execute() 中使用字符串拼接构建 SQL",
            "Statement.execute() 调用中包含字符串拼接，应使用 PreparedStatement",
            file_path,
            None,
            line,
            None,
            Some("使用 PreparedStatement 替换 Statement".into()),
        ));
    }
    findings
}

pub fn audit_security(filename: &str, content: &str) -> Vec<Finding> {
    let path = Path::new(filename);
    match crate::file_type::detect(path, content) {
        crate::FileKind::MyBatisXml => audit_mybatis_xml(content, filename),
        crate::FileKind::Java => audit_java_source(content, filename),
        _ => Vec::new(),
    }
}
