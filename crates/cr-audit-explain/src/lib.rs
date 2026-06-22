//! 真实执行计划审核。
//!
//! 消费 ogexplain-core 对 EXPLAIN TEXT 进行解析和规则诊断，
//! 将 ogexplain-core 的发现类型转换为 cr-core 统一类型，
//! 供审核引擎（`AuditEngine`）下游消费。

use ogexplain_core::{analyze, analyze_with_config, parse};

use cr_core::Finding;

/// 含执行计划元数据的审核发现包装。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ExplainFinding {
    /// 基础发现。
    pub finding: Finding,
    /// 原始 EXPLAIN TEXT 输出。
    pub plan_text: String,
    /// 总代价。
    pub total_cost: f64,
    /// ANALYZE 模式下的实际执行时间（毫秒）。
    pub actual_time_ms: Option<f64>,
    /// 优化器估计行数。
    pub rows_estimated: i64,
    /// ANALYZE 模式下的实际行数。
    pub rows_actual: Option<i64>,
    /// 共享缓冲区命中块数。
    pub shared_hit_blocks: Option<i64>,
    /// 磁盘读取块数。
    pub shared_read_blocks: Option<i64>,
}

/// 解析 EXPLAIN TEXT 并运行全部诊断规则，返回审核发现。
///
/// 内部依次调用：
/// 1. `ogexplain_core::parse()` — 将原始文本解析为结构化执行计划
/// 2. `ogexplain_core::analyze()` — 运行 28 条诊断规则
/// 3. 类型转换 — 将 ogexplain-core 发现映射为 cr-core 统一类型
///
/// # 错误
///
/// 若 EXPLAIN 文本为空、不包含计划节点或行解析失败，返回
/// [`RoughcollieError::Parse`]。
///
/// # 示例
///
/// ```ignore
/// use cr_audit_explain::analyze_explain_text;
///
/// let text = "Seq Scan on t1  (cost=0.00..35.00 rows=2500 width=4)";
/// let findings = analyze_explain_text(text)?;
/// ```
pub fn analyze_explain_text(
    explain_text: &str,
    file_path: &str,
) -> Result<Vec<cr_core::Finding>, cr_core::RoughcollieError> {
    let plan = parse(explain_text).map_err(map_parse_error)?;
    let report = analyze(&plan);
    Ok(report.findings.into_iter().map(|f| convert_finding(f, file_path)).collect())
}

/// 解析 EXPLAIN TEXT 并运行指定规则的诊断，返回审核发现。
///
/// 相较于 [`analyze_explain_text`]，此函数允许调用方通过 `disabled_rules`
/// 屏蔽无需执行的规则（如 `["TYPE-001", "SCAN-004"]`）。
///
/// # 错误
///
/// 若 EXPLAIN 文本为空、不包含计划节点或行解析失败，返回
/// [`RoughcollieError::Parse`]。
///
/// # 示例
///
/// ```ignore
/// use cr_audit_explain::analyze_explain_with_config;
///
/// let text = "Seq Scan on t1  (cost=0.00..35.00 rows=2500 width=4)";
/// let disabled = vec!["TYPE-001".to_string()];
/// let findings = analyze_explain_with_config(text, &disabled)?;
/// ```
pub fn analyze_explain_with_config(
    explain_text: &str,
    file_path: &str,
    disabled_rules: &[String],
) -> Result<Vec<cr_core::Finding>, cr_core::RoughcollieError> {
    let plan = parse(explain_text).map_err(map_parse_error)?;
    let config =
        ogexplain_core::analyzer::DiagnosticConfig { disabled_rules: disabled_rules.to_vec(), ..Default::default() };
    let report = analyze_with_config(&plan, &config);
    Ok(report.findings.into_iter().map(|f| convert_finding(f, file_path)).collect())
}

// ---------------------------------------------------------------------------
// Private helpers — type conversion
// ---------------------------------------------------------------------------

/// 将 ogexplain-core 的发现转换为 cr-core 统一发现。
///
/// 映射规则：
/// - `rule_id`、`title`、`detail`、`node_line`、`node_type`、`suggestion` 直接复制
/// - `severity` / `category` 通过枚举变体一一映射
/// - `sql_rewrite`、`evidence` 暂不纳入 cr-core 发现（保留扩展空间）
fn convert_finding(f: ogexplain_core::analyzer::Finding, file_path: &str) -> cr_core::Finding {
    cr_core::Finding::new(
        f.rule_id,
        convert_severity(f.severity),
        convert_category(f.category),
        f.title,
        f.detail,
        file_path,
        None,
        f.node_line,
        f.node_type,
        f.suggestion,
    )
}

fn convert_severity(s: ogexplain_core::analyzer::Severity) -> cr_core::Severity {
    match s {
        ogexplain_core::analyzer::Severity::Critical => cr_core::Severity::Critical,
        ogexplain_core::analyzer::Severity::Warning => cr_core::Severity::Warning,
        ogexplain_core::analyzer::Severity::Info => cr_core::Severity::Info,
    }
}

fn convert_category(c: ogexplain_core::analyzer::DiagnosticCategory) -> cr_core::DiagnosticCategory {
    match c {
        ogexplain_core::analyzer::DiagnosticCategory::ScanEfficiency => cr_core::DiagnosticCategory::ScanEfficiency,
        ogexplain_core::analyzer::DiagnosticCategory::JoinStrategy => cr_core::DiagnosticCategory::JoinStrategy,
        ogexplain_core::analyzer::DiagnosticCategory::MemoryUsage => cr_core::DiagnosticCategory::MemoryUsage,
        ogexplain_core::analyzer::DiagnosticCategory::SortEfficiency => cr_core::DiagnosticCategory::SortEfficiency,
        ogexplain_core::analyzer::DiagnosticCategory::NetworkOverhead => cr_core::DiagnosticCategory::NetworkOverhead,
        ogexplain_core::analyzer::DiagnosticCategory::CostMisestimation => {
            cr_core::DiagnosticCategory::CostMisestimation
        }
        ogexplain_core::analyzer::DiagnosticCategory::PushdownFailure => cr_core::DiagnosticCategory::PushdownFailure,
        ogexplain_core::analyzer::DiagnosticCategory::TypeMismatch => cr_core::DiagnosticCategory::TypeMismatch,
        ogexplain_core::analyzer::DiagnosticCategory::Vectorization => cr_core::DiagnosticCategory::Vectorization,
        ogexplain_core::analyzer::DiagnosticCategory::SubqueryStructure => {
            cr_core::DiagnosticCategory::SubqueryStructure
        }
        ogexplain_core::analyzer::DiagnosticCategory::DistributionIssue => {
            cr_core::DiagnosticCategory::DistributionIssue
        }
        ogexplain_core::analyzer::DiagnosticCategory::General => cr_core::DiagnosticCategory::General,
    }
}

/// 将 ogexplain-core 的解析错误映射为 cr-core 统一错误。
fn map_parse_error(e: ogexplain_core::parser::ParseError) -> cr_core::RoughcollieError {
    match e {
        ogexplain_core::parser::ParseError::LineParse { line, message } => {
            cr_core::RoughcollieError::Parse { line, col: 0, message }
        }
        ogexplain_core::parser::ParseError::EmptyInput => {
            cr_core::RoughcollieError::Parse { line: 0, col: 0, message: "Empty input".to_string() }
        }
        ogexplain_core::parser::ParseError::NoPlanNodes => {
            cr_core::RoughcollieError::Parse { line: 0, col: 0, message: "No plan nodes found".to_string() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyze_empty_input() {
        assert!(analyze_explain_text("", "test").is_err());
    }

    #[test]
    fn test_analyze_invalid_text_doesnt_panic() {
        let _ = analyze_explain_text("not explain output", "test");
    }

    #[test]
    fn test_analyze_simple_seq_scan_doesnt_panic() {
        let explain = "Seq Scan on public.users  (cost=0.00..154.00 rows=5400 width=4)";
        let _ = analyze_explain_text(explain, "test");
    }

    #[test]
    fn test_analyze_with_config_no_disabled() {
        let explain = "Seq Scan on public.users  (cost=0.00..154.00 rows=5400 width=4)";
        let result = analyze_explain_with_config(explain, "test", &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_analyze_scan_plan_doesnt_panic() {
        let explain = "\
Seq Scan on public.orders  (cost=0.00..48231.50 rows=5000000 width=244)
  Filter: (create_time > '2024-01-01 00:00:00'::timestamp without time zone)
  Rows Removed by Filter: 4990000";
        let result = analyze_explain_text(explain, "test");
        assert!(result.is_ok());
    }

    #[test]
    fn test_disabled_rules_empty_result() {
        let explain = "Seq Scan on public.users  (cost=0.00..154.00 rows=5400 width=4)";
        let result = analyze_explain_with_config(explain, "test", &["SCAN-001".into(), "SCAN-004".into()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_error_returns_error() {
        let result = analyze_explain_text("", "test");
        assert!(result.is_err());
    }
}
