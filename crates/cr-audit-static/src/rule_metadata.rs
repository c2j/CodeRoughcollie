//! ogexplain-core 28 条规则元数据。
//!
//! 提供规则 ID、名称、分类和严重度的静态注册表，
//! 供 cr-audit-static 在静态模式下引用和呈现。

use cr_core::{DiagnosticCategory, Severity};

/// ogexplain-core 规则元数据。
#[derive(Debug, Clone)]
pub struct RuleMetadata {
    pub id: &'static str,
    pub name: &'static str,
    pub category: DiagnosticCategory,
    pub severity: Severity,
    pub static_detectable: bool,
}

/// 全部 28 条 ogexplain-core 规则。
pub const OGEXPLAIN_RULES: &[RuleMetadata] = &[
    RuleMetadata {
        id: "SCAN-001",
        name: "Large table full scan",
        category: DiagnosticCategory::ScanEfficiency,
        severity: Severity::Critical,
        static_detectable: false,
    },
    RuleMetadata {
        id: "SCAN-004",
        name: "Filter without index",
        category: DiagnosticCategory::ScanEfficiency,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "JOIN-001",
        name: "Nested loop on large tables",
        category: DiagnosticCategory::JoinStrategy,
        severity: Severity::Critical,
        static_detectable: false,
    },
    RuleMetadata {
        id: "JOIN-002",
        name: "Hash join spill to disk",
        category: DiagnosticCategory::JoinStrategy,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "MEM-001",
        name: "Sort spill to disk",
        category: DiagnosticCategory::MemoryUsage,
        severity: Severity::Critical,
        static_detectable: false,
    },
    RuleMetadata {
        id: "MEM-004",
        name: "High peak memory",
        category: DiagnosticCategory::MemoryUsage,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "SORT-003",
        name: "Duplicate sort",
        category: DiagnosticCategory::SortEfficiency,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "NET-001",
        name: "Broadcast large data",
        category: DiagnosticCategory::NetworkOverhead,
        severity: Severity::Critical,
        static_detectable: false,
    },
    RuleMetadata {
        id: "EST-001",
        name: "Severe row estimation error",
        category: DiagnosticCategory::CostMisestimation,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "EST-004",
        name: "Nested loop from underestimation",
        category: DiagnosticCategory::CostMisestimation,
        severity: Severity::Critical,
        static_detectable: false,
    },
    RuleMetadata {
        id: "PUSH-001",
        name: "Query not pushed down",
        category: DiagnosticCategory::PushdownFailure,
        severity: Severity::Critical,
        static_detectable: false,
    },
    RuleMetadata {
        id: "PUSH-002",
        name: "Multi-layer streaming",
        category: DiagnosticCategory::PushdownFailure,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "TYPE-001",
        name: "Implicit type coercion",
        category: DiagnosticCategory::TypeMismatch,
        severity: Severity::Warning,
        static_detectable: true,
    },
    RuleMetadata {
        id: "TYPE-004",
        name: "LIKE with leading wildcard",
        category: DiagnosticCategory::TypeMismatch,
        severity: Severity::Warning,
        static_detectable: true,
    },
    RuleMetadata {
        id: "VEC-001",
        name: "Mixed row/vector engines",
        category: DiagnosticCategory::Vectorization,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "GEN-001",
        name: "Plan too deep",
        category: DiagnosticCategory::General,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "SUBQ-001",
        name: "Subquery not pulled up",
        category: DiagnosticCategory::SubqueryStructure,
        severity: Severity::Warning,
        static_detectable: true,
    },
    RuleMetadata {
        id: "REW-001",
        name: "Large IN list not rewritten",
        category: DiagnosticCategory::SubqueryStructure,
        severity: Severity::Warning,
        static_detectable: true,
    },
    RuleMetadata {
        id: "SUBQ-006",
        name: "Correlated subquery self-update",
        category: DiagnosticCategory::SubqueryStructure,
        severity: Severity::Critical,
        static_detectable: true,
    },
    RuleMetadata {
        id: "AGG-001",
        name: "Group aggregate should be hash",
        category: DiagnosticCategory::General,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "AGG-002",
        name: "Hash aggregate spill to disk",
        category: DiagnosticCategory::General,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "SKEW-001",
        name: "Data skew detected",
        category: DiagnosticCategory::DistributionIssue,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "DIST-001",
        name: "Distribution column mismatch",
        category: DiagnosticCategory::DistributionIssue,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "STATS-001",
        name: "Stats not collected",
        category: DiagnosticCategory::General,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "PART-001",
        name: "Partition pruning failure",
        category: DiagnosticCategory::General,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "ANTI-003",
        name: "Index Scan Amplification",
        category: DiagnosticCategory::General,
        severity: Severity::Critical,
        static_detectable: false,
    },
    RuleMetadata {
        id: "ANTI-005",
        name: "Multi-layer Materialization",
        category: DiagnosticCategory::General,
        severity: Severity::Warning,
        static_detectable: false,
    },
    RuleMetadata {
        id: "ANTI-007",
        name: "CN-side Large Sort",
        category: DiagnosticCategory::General,
        severity: Severity::Critical,
        static_detectable: false,
    },
];

/// 获取所有静态可检测的 ogexplain 规则。
#[must_use]
pub fn static_rules() -> Vec<&'static RuleMetadata> {
    OGEXPLAIN_RULES.iter().filter(|r| r.static_detectable).collect()
}

/// 根据 ID 查找规则。
#[must_use]
pub fn find_rule(id: &str) -> Option<&'static RuleMetadata> {
    OGEXPLAIN_RULES.iter().find(|r| r.id == id)
}
