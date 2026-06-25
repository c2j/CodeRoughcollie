//! ogsql-parser 诊断 → cr_core::Finding 映射 + 校验编排。
//!
//! 纯消费 ogsql-parser 公共 API（validate_statements + SqlLinter）。

use cr_core::{DiagnosticCategory, Finding, Severity};
use ogsql_parser::linter::{Confidence, LintConfig, SqlLinter, SqlWarning, WarningLevel};
use ogsql_parser::{
    validate_statements as upstream_validate, MergeSemanticError, PackageConsistencyError, ParserError, SourceLocation,
    StatementInfo, UndefinedRefKind, UndefinedVariableError,
};

fn loc_line(loc: &SourceLocation) -> Option<usize> {
    (loc.line >= 1).then_some(loc.line)
}

fn parser_error_to_finding(err: &ParserError, file_path: &str) -> Finding {
    match err {
        ParserError::UnexpectedToken { location, expected, got } => Finding::new(
            "PARSE-SYNTAX",
            Severity::Critical,
            DiagnosticCategory::ParseError,
            format!("语法错误：期望 {expected}，实际 {got}"),
            format!("期望 token: {expected}\n实际 token: {got}"),
            file_path,
            None,
            loc_line(location),
            None,
            Some("修正 SQL 语法，使其符合期望的 token".into()),
        ),
        ParserError::UnexpectedEof { expected, location } => Finding::new(
            "PARSE-EOF",
            Severity::Critical,
            DiagnosticCategory::ParseError,
            "SQL 不完整：意外到达输入末尾".to_string(),
            format!("期望: {expected}"),
            file_path,
            None,
            loc_line(location),
            None,
            Some("补全 SQL 语句，确保结构完整".into()),
        ),
        ParserError::Warning { message, location } => Finding::new(
            "PARSE-WARN",
            Severity::Warning,
            DiagnosticCategory::ParseError,
            "SQL 解析警告".to_string(),
            message.clone(),
            file_path,
            None,
            loc_line(location),
            None,
            None,
        ),
        ParserError::ReservedKeywordAsIdentifier { keyword, location } => Finding::new(
            "PARSE-RESERVED-KW",
            Severity::Warning,
            DiagnosticCategory::ParseError,
            format!("保留字 \"{keyword}\" 被用作标识符"),
            format!("保留关键字 \"{keyword}\" 不能作为标识符使用，可能导致歧义"),
            file_path,
            None,
            loc_line(location),
            None,
            Some(format!("更换标识符名，避免使用保留字 \"{keyword}\"")),
        ),
        ParserError::TokenizerError(te) => Finding::new(
            "PARSE-TOKENIZER",
            Severity::Critical,
            DiagnosticCategory::ParseError,
            "SQL 词法错误".to_string(),
            te.to_string(),
            file_path,
            None,
            None,
            None,
            Some("修正词法错误（未闭合的字符串/注释/引号等）".into()),
        ),
        ParserError::UnsupportedSyntax { location, syntax, hint } => Finding::new(
            "PARSE-UNSUPPORTED",
            Severity::Warning,
            DiagnosticCategory::ParseError,
            format!("不支持的语法: {syntax}"),
            format!("语法: {syntax}\n提示: {hint}"),
            file_path,
            None,
            loc_line(location),
            None,
            Some(hint.clone()),
        ),
    }
}

fn package_error_to_finding(err: &PackageConsistencyError, file_path: &str) -> Finding {
    let detail = err.detail.clone().unwrap_or_else(|| format!("{:?}", err.kind));
    Finding::new(
        "VAL-PKG",
        Severity::Warning,
        DiagnosticCategory::ValidationSemantic,
        format!("Package 一致性: {}.{}", err.package_name, err.subprogram_name),
        detail,
        file_path,
        None,
        None,
        None,
        Some("对齐 PACKAGE spec 与 body 的子程序签名（参数个数/类型/默认值）".into()),
    )
}

fn merge_error_to_finding(err: &MergeSemanticError, file_path: &str) -> Finding {
    Finding::new(
        "VAL-MERGE",
        Severity::Critical,
        DiagnosticCategory::ValidationSemantic,
        format!("MERGE 语义问题: {:?}", err.kind),
        format!("MERGE 语句存在语义问题: {:?}", err.kind),
        file_path,
        None,
        Some(err.location.line),
        None,
        Some("检查 MERGE 的 WHEN MATCHED / NOT MATCHED 分支是否语义正确".into()),
    )
}

fn undefined_variable_error_to_finding(err: &UndefinedVariableError, file_path: &str) -> Finding {
    let (rule_id, title) = match err.kind {
        UndefinedRefKind::Variable => ("VAL-PL-VAR", format!("PL 未定义变量: {}", err.variable_name)),
        UndefinedRefKind::Function => ("VAL-PL-FUNC", format!("PL 未定义函数: {}", err.variable_name)),
    };
    Finding::new(
        rule_id,
        Severity::Warning,
        DiagnosticCategory::ValidationSemantic,
        title,
        format!("上下文: {}\n变量/函数: {}", err.context, err.variable_name),
        file_path,
        None,
        err.location.as_ref().map(|s| s.start.line),
        None,
        Some("确认变量/函数已声明，或检查拼写与作用域".into()),
    )
}

fn warning_level_to_severity(level: WarningLevel) -> Severity {
    match level {
        WarningLevel::Prohibition => Severity::Critical,
        WarningLevel::Performance | WarningLevel::Caution => Severity::Warning,
        WarningLevel::Suggestion => Severity::Info,
    }
}

fn sql_warning_to_finding(w: &SqlWarning, file_path: &str) -> Finding {
    Finding::new(
        format!("LINT-{}", w.rule_id),
        warning_level_to_severity(w.level),
        DiagnosticCategory::General,
        w.rule_name.clone(),
        w.message.clone(),
        file_path,
        None,
        Some(w.location.line),
        None,
        w.suggestion.clone(),
    )
}

/// 对已解析的 StatementInfo 列表执行语义校验 + lint，返回所有诊断 Finding。
///
/// 包含 Package spec/body 一致性、MERGE 语义、PL 变量/函数校验、53 条 lint 反模式规则。
pub fn validate_statements(stmts: &[StatementInfo], file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    let report = upstream_validate(stmts, &[], false);

    for err in &report.package_errors {
        findings.push(package_error_to_finding(err, file_path));
    }
    for err in &report.merge_errors {
        findings.push(merge_error_to_finding(err, file_path));
    }
    for err in &report.undefined_variable_errors {
        findings.push(undefined_variable_error_to_finding(err, file_path));
    }

    let linter = SqlLinter::with_default_rules(LintConfig::default());
    for w in linter.lint(stmts, None, Confidence::Full) {
        findings.push(sql_warning_to_finding(&w, file_path));
    }

    findings
}

pub fn parser_errors_to_findings(errors: &[ParserError], file_path: &str) -> Vec<Finding> {
    errors.iter().map(|e| parser_error_to_finding(e, file_path)).collect()
}

pub fn sql_warnings_to_findings(warnings: &[SqlWarning], file_path: &str) -> Vec<Finding> {
    warnings.iter().map(|w| sql_warning_to_finding(w, file_path)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogsql_parser::Parser;

    #[test]
    fn test_parser_error_to_finding_unexpected_token() {
        let err = ParserError::UnexpectedToken {
            location: SourceLocation { line: 3, column: 5, offset: 20 },
            expected: "FROM".into(),
            got: "WHERE".into(),
        };
        let f = parser_error_to_finding(&err, "test.sql");
        assert_eq!(f.rule_id, "PARSE-SYNTAX");
        assert_eq!(f.severity, Severity::Critical);
        assert_eq!(f.category, DiagnosticCategory::ParseError);
        assert_eq!(f.node_line, Some(3));
    }

    #[test]
    fn test_parser_error_to_finding_warning() {
        let err = ParserError::Warning { message: "suspicious construct".into(), location: SourceLocation::default() };
        let f = parser_error_to_finding(&err, "test.sql");
        assert_eq!(f.rule_id, "PARSE-WARN");
        assert_eq!(f.severity, Severity::Warning);
    }

    #[test]
    fn test_validate_statements_clean_sql() {
        let (stmts, errors) = Parser::parse_sql("SELECT id FROM users");
        assert!(errors.is_empty());
        let findings = validate_statements(&stmts, "test.sql");
        assert!(findings.is_empty(), "clean SQL should produce no findings, got: {findings:?}");
    }

    #[test]
    fn test_validate_statements_select_star_lint() {
        let (stmts, _errors) = Parser::parse_sql("SELECT * FROM users");
        let findings = validate_statements(&stmts, "test.sql");
        assert!(!findings.is_empty(), "SELECT * should trigger at least one lint rule");
        assert!(findings.iter().all(|f| f.rule_id.starts_with("LINT-")));
    }

    #[test]
    fn test_validate_statements_undefined_pl_variable() {
        let sql = "DO $$ BEGIN undeclared_var := 1; END $$";
        let (stmts, _errors) = Parser::parse_sql(sql);
        let findings = validate_statements(&stmts, "test.sql");
        assert!(
            findings.iter().any(|f| f.rule_id.starts_with("VAL-PL")),
            "undefined PL variable should produce VAL-PL-* finding, got: {findings:?}"
        );
    }

    #[test]
    fn test_parser_errors_to_findings_from_bad_sql() {
        let (_stmts, errors) = Parser::parse_sql("SELECT FROM WHERE");
        let findings = parser_errors_to_findings(&errors, "bad.sql");
        assert!(!findings.is_empty(), "bad SQL should produce parse findings");
        assert!(findings.iter().all(|f| f.category == DiagnosticCategory::ParseError));
    }
}
