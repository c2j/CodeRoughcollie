//! 复杂度评估：集成 ogsql-complexity 评分。
//!
//! 调用 ogsql-complexity 对 SQL 进行复杂度评分（标准 0-100 评分或 GaussDB
//! 多维度评分），当复杂度超出阈值或与基线偏差超过告警线时返回审核发现。
//!
//! # 使用示例
//!
//! ```rust
//! use cr_audit_complexity::{audit_complexity, get_complexity_score};
//!
//! let sql = "SELECT * FROM t1 JOIN t2 ON t1.id = t2.id";
//! let score = get_complexity_score(sql).unwrap();
//! let findings = audit_complexity(sql, "test.sql", None, 10.0, 20.0);
//! ```

/// 复杂度等级。
///
/// 与 ogsql-complexity 的 `ComplexityLevel` 不同（后者为 `Trivial` /
/// `Simple` / `Moderate` / `Complex` / `VeryComplex`），本类型面向审核
/// 报告输出，使用三级分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComplexityLevel {
    /// 低复杂度（0-33）。
    Low,
    /// 中复杂度（34-66）。
    Medium,
    /// 高复杂度（67-100）。
    High,
}

// Re-export 非冲突的 ogsql-complexity 类型。
pub use ogsql_complexity::{
    ComplexityConfig, ComplexityMetrics, ComplexityReport, ComplexityTag, GaussDbComplexityReport,
    GaussDbScoreBreakdown, InputKind, ScoreDimensions, SqlCategory,
};

/// 审核 SQL 复杂度，返回审核发现列表。
///
/// 先尝试标准分析（`ogsql_complexity::analyze`），若解析失败则回退到
/// GaussDB 分析（`ogsql_complexity::gauss_analyze`）。
///
/// # 参数
///
/// * `sql`            - SQL 文本。
/// * `file_path`      - 来源文件路径。
/// * `baseline_score` - 可选的历史基线分数。为 `Some(score)` 时计算增量，
///   否则仅检查绝对分数。
/// * `warning_delta`  - 增量警告阈值。增量超过此值但未达 `critical_delta`
///   时产生 Warning 发现。
/// * `critical_delta` - 增量严重阈值。增量超过此值时产生 Critical 发现。
///
/// # 返回
///
/// 零条或一条审核发现。当基线存在时使用 `COMPLEX-001`，无基线时使用
/// `COMPLEX-002`。
///
/// # 规则
///
/// | 规则 ID | 触发条件 | 严重度 |
/// |---------|---------|--------|
/// | COMPLEX-001 | `score - baseline > critical_delta` | Critical |
/// | COMPLEX-001 | `score - baseline > warning_delta` | Warning |
/// | COMPLEX-002 | `score > 66`（无基线） | Warning |
#[must_use]
pub fn audit_complexity(
    sql: &str,
    file_path: &str,
    baseline_score: Option<f64>,
    warning_delta: f64,
    critical_delta: f64,
) -> Vec<cr_core::Finding> {
    // 先尝试标准分析，失败时回退到 GaussDB 分析。
    let score = match ogsql_complexity::analyze(sql) {
        Ok(report) => report.overall_score,
        Err(_) => match ogsql_complexity::gauss_analyze(sql, &ComplexityConfig::default()) {
            Ok(report) => report.overall_score as f64,
            Err(_) => return Vec::new(),
        },
    };

    let mut findings = Vec::new();

    if let Some(baseline) = baseline_score {
        let delta = score - baseline;
        if delta > critical_delta {
            findings.push(cr_core::Finding::new(
                "COMPLEX-001",
                cr_core::Severity::Critical,
                cr_core::DiagnosticCategory::General,
                "SQL 复杂度显著上升",
                format!(
                    "复杂度从基线 {baseline:.1} 上升至 {score:.1}（Δ={delta:.1}），超过严重阈值 {critical_delta:.1}"
                ),
                file_path,
                None,
                Some(1),
                None,
                None,
            ));
        } else if delta > warning_delta {
            findings.push(cr_core::Finding::new(
                "COMPLEX-001",
                cr_core::Severity::Warning,
                cr_core::DiagnosticCategory::General,
                "SQL 复杂度上升",
                format!(
                    "复杂度从基线 {baseline:.1} 上升至 {score:.1}（Δ={delta:.1}），超过警告阈值 {warning_delta:.1}"
                ),
                file_path,
                None,
                Some(1),
                None,
                None,
            ));
        }
    } else if score > 66.0 {
        findings.push(cr_core::Finding::new(
            "COMPLEX-002",
            cr_core::Severity::Warning,
            cr_core::DiagnosticCategory::General,
            "高复杂度",
            format!("SQL 复杂度评分为 {score:.1}，超过高复杂度阈值 66，建议优化"),
            file_path,
            None,
            Some(1),
            None,
            None,
        ));
    }

    findings
}

/// 获取 SQL 的复杂度评分。
///
/// 先尝试标准分析，失败时回退到 GaussDB 分析。
///
/// # 返回值
///
/// * `Ok(score)` - 复杂度分数（0-100，标准分析为 `f64`，GaussDB 分析转换为 `f64`）。
/// * `Err(RoughcollieError::RuleEngine)` - 两种分析均失败。
///
/// # 错误处理
///
/// 遵循 M-ERR-02，不调用 `unwrap()` 或 `expect()`。当 ogsql-complexity
/// 返回错误时（如 SQL 语法解析失败），统一转换为 `RoughcollieError::RuleEngine`。
pub fn get_complexity_score(sql: &str) -> Result<f64, cr_core::RoughcollieError> {
    let score = ogsql_complexity::analyze(sql)
        .map(|r| r.overall_score)
        .or_else(|_| ogsql_complexity::gauss_analyze(sql, &ComplexityConfig::default()).map(|r| r.overall_score as f64))
        .map_err(|e| cr_core::RoughcollieError::RuleEngine(e.to_string()))?;
    Ok(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_select_score() {
        let sql = "SELECT * FROM t1";
        let score = get_complexity_score(sql).expect("should parse");
        assert!(score > 0.0, "simple select should have positive score");
    }

    #[test]
    fn empty_input_returns_error() {
        let result = get_complexity_score("");
        assert!(result.is_err(), "empty input should error");
    }

    #[test]
    fn audit_no_baseline_low_score() {
        let sql = "SELECT * FROM t1";
        let findings = audit_complexity(sql, "test.sql", None, 10.0, 20.0);
        assert!(findings.is_empty(), "low complexity should have no findings");
    }

    #[test]
    fn audit_baseline_within_delta() {
        let sql = "SELECT * FROM t1";
        let findings = audit_complexity(sql, "test.sql", Some(5.0), 10.0, 20.0);
        assert!(findings.is_empty(), "small delta should have no findings");
    }

    #[test]
    fn complexity_finding_has_source_line() {
        let sql = "SELECT * FROM t1 JOIN t2 USING(id) JOIN t3 USING(id) ORDER BY t1.a";
        let findings = audit_complexity(sql, "test.sql", None, 10.0, 20.0);
        let f = findings.iter().find(|f| f.rule_id == "COMPLEX-002");
        if let Some(f) = f {
            assert_eq!(f.node_line, Some(1), "complexity finding should point to SQL start line");
        }
    }

    #[test]
    fn complexity_baseline_finding_has_source_line() {
        let sql = "SELECT * FROM t1 JOIN t2 USING(id) JOIN t3 USING(id) ORDER BY t1.a";
        let findings = audit_complexity(sql, "test.sql", Some(0.0), 10.0, 20.0);
        if let Some(f) =
            findings.iter().find(|f| f.rule_id == "COMPLEX-001" && f.severity == cr_core::Severity::Critical)
        {
            assert_eq!(f.node_line, Some(1));
        }
    }
}
