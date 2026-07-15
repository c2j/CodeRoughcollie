//! 审核结果过滤：按 rule_id / severity / category 保留或筛除 Finding。

use crate::types::Finding;

/// 解析 `"includes:xxx,yyy"` 或 `"excludes:xxx,yyy"` 格式的过滤表达式。
///
/// 返回 `(mode, values)`，其中 mode 为 `"include"` 或 `"exclude"`。
fn parse_filter_expr(expr: &str) -> Option<(&str, Vec<&str>)> {
    let expr = expr.trim();
    if expr.is_empty() {
        return None;
    }
    let (mode, values_str) = expr.split_once(':')?;
    let mode = match mode.trim() {
        "includes" => "include",
        "excludes" => "exclude",
        _ => return None,
    };
    let values: Vec<&str> = values_str.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if values.is_empty() {
        return None;
    }
    Some((mode, values))
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == value;
    }
    if !value.starts_with(parts[0]) {
        return false;
    }
    if !value.ends_with(parts[parts.len() - 1]) {
        return false;
    }
    let mut pos = parts[0].len();
    for part in &parts[1..parts.len() - 1] {
        if let Some(found) = value[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }
    true
}

/// 根据过滤表达式过滤 findings。
///
/// - `rule_id_filter`：如 `Some("excludes:SCAN-001,TYPE-*")` 或 `None`（不过滤）
/// - `severity_filter`：如 `Some("includes:critical,warning")` 或 `None`
/// - `category_filter`：如 `Some("excludes:parse-error")` 或 `None`
///
/// 未配置的维度不过滤。
pub fn filter_findings(
    findings: Vec<Finding>,
    rule_id_filter: Option<&str>,
    severity_filter: Option<&str>,
    category_filter: Option<&str>,
) -> Vec<Finding> {
    findings.into_iter().filter(|f| passes_filter(f, rule_id_filter, severity_filter, category_filter)).collect()
}

fn warn_invalid_expr(expr: &str, field: &str) {
    tracing::warn!(field = field, expr = %expr, "filter 表达式格式无效，该维度不过滤。期望格式: includes:val1,val2 or excludes:val1,val2");
}

fn passes_filter(
    finding: &Finding,
    rule_id_filter: Option<&str>,
    severity_filter: Option<&str>,
    category_filter: Option<&str>,
) -> bool {
    if let Some(expr) = rule_id_filter {
        if let Some((mode, values)) = parse_filter_expr(expr) {
            let matched = values.iter().any(|p| wildcard_match(p, &finding.rule_id));
            if mode == "exclude" && matched {
                return false;
            }
            if mode == "include" && !matched {
                return false;
            }
        } else {
            warn_invalid_expr(expr, "rule_id");
        }
    }
    if let Some(expr) = severity_filter {
        if let Some((mode, values)) = parse_filter_expr(expr) {
            let matched = values.iter().any(|v| *v == finding.severity.as_str());
            if mode == "exclude" && matched {
                return false;
            }
            if mode == "include" && !matched {
                return false;
            }
        } else {
            warn_invalid_expr(expr, "severity");
        }
    }
    if let Some(expr) = category_filter {
        if let Some((mode, values)) = parse_filter_expr(expr) {
            let cat_str = finding.category.as_kebab_str();
            let matched = values.contains(&cat_str);
            if mode == "exclude" && matched {
                return false;
            }
            if mode == "include" && !matched {
                return false;
            }
        } else {
            warn_invalid_expr(expr, "category");
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DiagnosticCategory, Finding, Severity};

    fn make_finding(rule_id: &str, severity: Severity, category: DiagnosticCategory) -> Finding {
        Finding::new(rule_id, severity, category, "", "", "", None, None, None, None)
    }

    #[test]
    fn test_parse_filter_expr_include() {
        let (mode, values) = parse_filter_expr("includes:critical,warning").unwrap();
        assert_eq!(mode, "include");
        assert_eq!(values, vec!["critical", "warning"]);
    }

    #[test]
    fn test_parse_filter_expr_exclude() {
        let (mode, values) = parse_filter_expr("excludes:SCAN-001,TYPE-*").unwrap();
        assert_eq!(mode, "exclude");
        assert_eq!(values, vec!["SCAN-001", "TYPE-*"]);
    }

    #[test]
    fn test_parse_filter_expr_empty() {
        assert!(parse_filter_expr("").is_none());
        assert!(parse_filter_expr("  ").is_none());
    }

    #[test]
    fn test_parse_filter_expr_invalid() {
        assert!(parse_filter_expr("foo:bar").is_none());
        assert!(parse_filter_expr(":").is_none());
    }

    #[test]
    fn test_wildcard_match_exact() {
        assert!(wildcard_match("SCAN-001", "SCAN-001"));
        assert!(!wildcard_match("SCAN-001", "SCAN-002"));
    }

    #[test]
    fn test_wildcard_match_prefix() {
        assert!(wildcard_match("TYPE-*", "TYPE-001"));
        assert!(wildcard_match("TYPE-*", "TYPE-999"));
        assert!(!wildcard_match("TYPE-*", "SCAN-001"));
    }

    #[test]
    fn test_wildcard_match_suffix() {
        assert!(wildcard_match("*-001", "SCAN-001"));
        assert!(wildcard_match("*-001", "TYPE-001"));
        assert!(!wildcard_match("*-001", "SCAN-002"));
    }

    #[test]
    fn test_wildcard_match_wildcard_only() {
        assert!(wildcard_match("*", "ANYTHING"));
    }

    #[test]
    fn test_filter_by_rule_id_exclude() {
        let f = make_finding("SCAN-001", Severity::Critical, DiagnosticCategory::ScanEfficiency);
        let result = filter_findings(vec![f], Some("excludes:SCAN-001"), None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_by_rule_id_include() {
        let f = make_finding("SCAN-001", Severity::Critical, DiagnosticCategory::ScanEfficiency);
        let result = filter_findings(vec![f], Some("includes:SCAN-001"), None, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_filter_by_rule_id_include_no_match() {
        let f = make_finding("SCAN-001", Severity::Critical, DiagnosticCategory::ScanEfficiency);
        let result = filter_findings(vec![f], Some("includes:TYPE-*"), None, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_by_rule_id_wildcard() {
        let f1 = make_finding("TYPE-001", Severity::Warning, DiagnosticCategory::TypeMismatch);
        let f2 = make_finding("SCAN-001", Severity::Critical, DiagnosticCategory::ScanEfficiency);
        let result = filter_findings(vec![f1, f2], Some("excludes:TYPE-*"), None, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rule_id, "SCAN-001");
    }

    #[test]
    fn test_filter_by_severity_include() {
        let f1 = make_finding("R1", Severity::Critical, DiagnosticCategory::General);
        let f2 = make_finding("R2", Severity::Info, DiagnosticCategory::General);
        let result = filter_findings(vec![f1, f2], None, Some("includes:critical,warning"), None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rule_id, "R1");
    }

    #[test]
    fn test_filter_by_category_exclude() {
        let f1 = make_finding("R1", Severity::Critical, DiagnosticCategory::ScanEfficiency);
        let f2 = make_finding("R2", Severity::Warning, DiagnosticCategory::JoinStrategy);
        let result = filter_findings(vec![f1, f2], None, None, Some("excludes:scan-efficiency"));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rule_id, "R2");
    }

    #[test]
    fn test_filter_no_filter_returns_all() {
        let f1 = make_finding("R1", Severity::Critical, DiagnosticCategory::General);
        let f2 = make_finding("R2", Severity::Info, DiagnosticCategory::ParseError);
        let result = filter_findings(vec![f1, f2], None, None, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_multi_dimension_and() {
        let f1 = make_finding("SCAN-001", Severity::Critical, DiagnosticCategory::ScanEfficiency);
        let f2 = make_finding("TYPE-001", Severity::Info, DiagnosticCategory::TypeMismatch);
        let result = filter_findings(vec![f1, f2], Some("excludes:SCAN-001"), Some("excludes:info"), None);
        assert!(result.is_empty());
    }
}
