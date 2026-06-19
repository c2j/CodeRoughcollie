//! 评分聚合算法。
//!
//! 将多个 Finding 聚合为整体评分（0-100 审核健康度分数）。

use crate::types::{Finding, Severity};

/// 统计审核结果中的各严重度数量。
#[must_use]
pub fn count_by_severity(findings: &[Finding]) -> SeverityCounts {
    let mut counts = SeverityCounts::default();
    for f in findings {
        match f.severity {
            Severity::Critical => counts.critical += 1,
            Severity::Warning => counts.warning += 1,
            Severity::Info => counts.info += 1,
        }
    }
    counts
}

/// 按严重度统计的 Finding 数量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SeverityCounts {
    /// Critical 数量。
    pub critical: usize,
    /// Warning 数量。
    pub warning: usize,
    /// Info 数量。
    pub info: usize,
}

impl SeverityCounts {
    /// 是否存在 Critical 级别的 Finding。
    #[must_use]
    pub const fn has_critical(&self) -> bool {
        self.critical > 0
    }

    /// 总 Finding 数量。
    #[must_use]
    pub const fn total(&self) -> usize {
        self.critical + self.warning + self.info
    }
}

/// 审核健康度评分（0-100，越高越好）。
///
/// 算法：从 100 开始，每个 Critical 扣 20 分，每个 Warning 扣 5 分，每个 Info 扣 1 分。
/// 最低 0 分。
#[must_use]
pub fn health_score(findings: &[Finding]) -> f64 {
    let counts = count_by_severity(findings);
    let penalty = (counts.critical as f64 * 20.0) + (counts.warning as f64 * 5.0) + (counts.info as f64 * 1.0);
    (100.0 - penalty).max(0.0)
}

/// 评分等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthGrade {
    /// 优秀（90-100）。
    Excellent,
    /// 良好（70-89）。
    Good,
    /// 一般（50-69）。
    Fair,
    /// 差（0-49）。
    Poor,
}

impl HealthGrade {
    /// 根据分数返回对应的评分等级。
    #[must_use]
    pub fn from_score(score: f64) -> Self {
        match score {
            s if s >= 90.0 => Self::Excellent,
            s if s >= 70.0 => Self::Good,
            s if s >= 50.0 => Self::Fair,
            _ => Self::Poor,
        }
    }

    /// 返回中文标签。
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Excellent => "优秀",
            Self::Good => "良好",
            Self::Fair => "一般",
            Self::Poor => "差",
        }
    }

    /// 返回对应的 emoji 图标。
    #[must_use]
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Excellent => "🟢",
            Self::Good => "🟡",
            Self::Fair => "🟠",
            Self::Poor => "🔴",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DiagnosticCategory;

    fn make_finding(severity: Severity) -> Finding {
        Finding::new(
            "TEST",
            severity,
            DiagnosticCategory::General,
            "test",
            "test detail",
            "test.sql",
            None,
            None,
            None,
            None,
        )
    }

    #[test]
    fn test_count_by_severity_empty() {
        let counts = count_by_severity(&[]);
        assert_eq!(counts.critical, 0);
        assert_eq!(counts.warning, 0);
        assert_eq!(counts.info, 0);
        assert_eq!(counts.total(), 0);
        assert!(!counts.has_critical());
    }

    #[test]
    fn test_count_by_severity_mixed() {
        let findings = vec![
            make_finding(Severity::Critical),
            make_finding(Severity::Critical),
            make_finding(Severity::Warning),
            make_finding(Severity::Info),
            make_finding(Severity::Info),
            make_finding(Severity::Info),
        ];
        let counts = count_by_severity(&findings);
        assert_eq!(counts.critical, 2);
        assert_eq!(counts.warning, 1);
        assert_eq!(counts.info, 3);
        assert_eq!(counts.total(), 6);
        assert!(counts.has_critical());
    }

    #[test]
    fn test_health_score_perfect() {
        let score = health_score(&[]);
        assert_eq!(score, 100.0);
    }

    #[test]
    fn test_health_score_with_penalties() {
        let findings = vec![
            make_finding(Severity::Critical), // -20
            make_finding(Severity::Warning),  // -5
            make_finding(Severity::Info),     // -1
        ];
        let score = health_score(&findings);
        assert!((score - 74.0).abs() < 1e-9);
    }

    #[test]
    fn test_health_score_no_below_zero() {
        // 6 Criticals = -120, should floor at 0
        let findings = vec![make_finding(Severity::Critical); 6];
        let score = health_score(&findings);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_health_score_warnings_only() {
        let findings = vec![make_finding(Severity::Warning); 3]; // -15
        let score = health_score(&findings);
        assert!((score - 85.0).abs() < 1e-9);
    }

    #[test]
    fn test_health_grade_excellent() {
        assert_eq!(HealthGrade::from_score(100.0), HealthGrade::Excellent);
        assert_eq!(HealthGrade::from_score(90.0), HealthGrade::Excellent);
        assert_eq!(HealthGrade::from_score(95.5), HealthGrade::Excellent);
    }

    #[test]
    fn test_health_grade_good() {
        assert_eq!(HealthGrade::from_score(89.9), HealthGrade::Good);
        assert_eq!(HealthGrade::from_score(70.0), HealthGrade::Good);
        assert_eq!(HealthGrade::from_score(75.0), HealthGrade::Good);
    }

    #[test]
    fn test_health_grade_fair() {
        assert_eq!(HealthGrade::from_score(69.9), HealthGrade::Fair);
        assert_eq!(HealthGrade::from_score(50.0), HealthGrade::Fair);
        assert_eq!(HealthGrade::from_score(55.0), HealthGrade::Fair);
    }

    #[test]
    fn test_health_grade_poor() {
        assert_eq!(HealthGrade::from_score(49.9), HealthGrade::Poor);
        assert_eq!(HealthGrade::from_score(0.0), HealthGrade::Poor);
        assert_eq!(HealthGrade::from_score(-10.0), HealthGrade::Poor);
    }
}
