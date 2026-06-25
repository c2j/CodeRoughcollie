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

    let parsed = ogsql_parser::ibatis::parse_mapper_bytes_with_path(xml_content.as_bytes(), Some(file_path));
    for stmt in &parsed.statements {
        if let Some((stmt_infos, parse_errors)) = &stmt.parse_result {
            findings.extend(crate::validation::parser_errors_to_findings(parse_errors, file_path));
            findings.extend(crate::validation::validate_statements(stmt_infos, file_path));
        }
    }

    let structured = ogsql_parser::ibatis::parse_mapper_bytes_structured(xml_content.as_bytes());
    let lint_config = ogsql_parser::linter::LintConfig::default();
    let structured_warnings = ogsql_parser::linter::structured::lint_structured_mapper(&structured, &lint_config);
    findings.extend(crate::validation::sql_warnings_to_findings(&structured_warnings, file_path));

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

    let config = ogsql_parser::java::JavaExtractConfig::default();
    let result = ogsql_parser::java::extract_sql_from_java(java_content, file_path, &config);
    for ext in &result.extractions {
        if let Some(pr) = &ext.parse_result {
            findings.extend(crate::validation::parser_errors_to_findings(&pr.errors, file_path));
            findings.extend(crate::validation::validate_statements(&pr.statements, file_path));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mybatis_xml_dollar_param_security() {
        let xml = r#"<mapper namespace="t"><select id="x">SELECT * FROM t WHERE n = ${name}</select></mapper>"#;
        let findings = audit_mybatis_xml(xml, "mapper.xml");
        assert!(findings.iter().any(|f| f.rule_id == "SECURITY-MYBATIS-DOLLAR-PARAM"));
    }

    #[test]
    fn test_mybatis_xml_select_star_lint() {
        let xml = r#"<mapper namespace="t"><select id="x">SELECT * FROM t WHERE id = #{id}</select></mapper>"#;
        let findings = audit_mybatis_xml(xml, "mapper.xml");
        assert!(!findings.is_empty(), "SELECT * in XML should produce lint findings, got: {findings:?}");
    }

    #[test]
    fn test_mybatis_xml_syntax_error() {
        let xml = r#"<mapper namespace="t"><select id="x">SELECT FROM WHERE</select></mapper>"#;
        let findings = audit_mybatis_xml(xml, "mapper.xml");
        assert!(
            findings.iter().any(|f| f.rule_id.starts_with("PARSE-")),
            "syntax error in XML SQL should produce PARSE-* findings, got: {findings:?}"
        );
    }

    #[test]
    fn test_mybatis_xml_c018_foreach_insert() {
        // 70 params per row × 1000 estimated rows = 70000 > 65535 default threshold
        let params: String = (0..70).map(|i| format!("#{{r.c{i}}}")).collect::<Vec<_>>().join(", ");
        let xml = format!(
            r#"<mapper namespace="t">
            <insert id="batch">
                INSERT INTO t (c0) VALUES
                <foreach collection="rows" item="r" separator=",">({params})</foreach>
            </insert>
        </mapper>"#
        );
        let findings = audit_mybatis_xml(&xml, "mapper.xml");
        assert!(
            findings.iter().any(|f| f.rule_id == "LINT-C018"),
            "foreach with 70 params/row should exceed threshold and fire C018, got: {}",
            findings.iter().map(|f| f.rule_id.as_str()).collect::<Vec<_>>().join(", ")
        );
    }

    #[test]
    fn test_java_select_annotation_lint() {
        let java = r#"
            package com.example;
            public interface UserMapper {
                @Select("SELECT * FROM users WHERE id = #{id}")
                User findById(int id);
            }
        "#;
        let findings = audit_java_source(java, "UserMapper.java");
        assert!(!findings.is_empty(), "SELECT * in Java annotation should produce findings, got: {findings:?}");
    }

    #[test]
    fn test_java_syntax_error_in_annotation() {
        let java = r#"
            package com.example;
            public interface UserMapper {
                @Select("SELECT FROM WHERE")
                User bad();
            }
        "#;
        let findings = audit_java_source(java, "UserMapper.java");
        assert!(
            findings.iter().any(|f| f.rule_id.starts_with("PARSE-")),
            "syntax error in Java annotation SQL should produce PARSE-* findings, got: {findings:?}"
        );
    }
}
