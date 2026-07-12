//! 核心类型定义。
//!
//! 当前为独立定义；Week 3-4 集成 ogexplain-core 后将切换为 re-export。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 诊断严重度。
///
/// 与 ogexplain-core 的 `Severity` 保持一致：仅三级，不引入 `Error`。
/// 退出码策略通过配置将 `Critical` 映射为阻断（exit 1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Severity {
    /// 需立即处理：大表全扫、Nested Loop 高代价、未下推等。
    Critical,
    /// 可自愈或需关注：隐式类型转换、行数低估等。
    Warning,
    /// 信息性：分区剪枝、统计信息等。
    Info,
}

impl Severity {
    /// 返回字符串表示（用于 JSON 序列化和报告输出）。
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }

    /// 返回对应的 emoji 图标（用于 Markdown 报告）。
    #[must_use]
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Critical => "🔴",
            Self::Warning => "🟡",
            Self::Info => "🔵",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 诊断分类，对应规则的归属域。
///
/// 与 ogexplain-core 的 `DiagnosticCategory` 保持一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DiagnosticCategory {
    /// SCAN-* 规则：扫描效率。
    ScanEfficiency,
    /// JOIN-* 规则：连接策略。
    JoinStrategy,
    /// MEM-* 规则：内存使用。
    MemoryUsage,
    /// SORT-* 规则：排序效率。
    SortEfficiency,
    /// NET-* 规则：网络开销。
    NetworkOverhead,
    /// EST-* 规则：代价估算偏差。
    CostMisestimation,
    /// PUSH-* 规则：下推失败。
    PushdownFailure,
    /// TYPE-* 规则：类型不匹配。
    TypeMismatch,
    /// VEC-* 规则：向量化。
    Vectorization,
    /// SUBQ-* / REW-* 规则：子查询结构。
    SubqueryStructure,
    /// DIST-* / SKEW-* 规则：分布式问题。
    DistributionIssue,
    /// PARSE-* / VAL-SYNTAX-* 规则：SQL 词法/语法错误与警告。
    ParseError,
    /// VAL-PKG-* / VAL-MERGE-* / VAL-PL-* 规则：语义校验（package 一致性、MERGE 语义、PL 变量）。
    ValidationSemantic,
    /// GEN-* / ANTI-* / AGG-* / STATS-* / PART-* 规则：通用。
    General,
}

impl DiagnosticCategory {
    /// 返回 kebab-case 字符串表示（用于过滤表达式匹配）。
    #[must_use]
    pub fn as_kebab_str(&self) -> &'static str {
        match self {
            Self::ScanEfficiency => "scan-efficiency",
            Self::JoinStrategy => "join-strategy",
            Self::MemoryUsage => "memory-usage",
            Self::SortEfficiency => "sort-efficiency",
            Self::NetworkOverhead => "network-overhead",
            Self::CostMisestimation => "cost-misestimation",
            Self::PushdownFailure => "pushdown-failure",
            Self::TypeMismatch => "type-mismatch",
            Self::Vectorization => "vectorization",
            Self::SubqueryStructure => "subquery-structure",
            Self::DistributionIssue => "distribution-issue",
            Self::ParseError => "parse-error",
            Self::ValidationSemantic => "validation-semantic",
            Self::General => "general",
        }
    }
}

/// 单条审核发现。
///
/// 所有审核维度（静态、EXPLAIN、复杂度）统一产出此类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Finding {
    /// 规则 ID，如 `"SCAN-001"`、`"TYPE-001"`。
    pub rule_id: String,
    /// 严重度。
    pub severity: Severity,
    /// 诊断分类。
    pub category: DiagnosticCategory,
    /// 标题（一句话摘要）。
    pub title: String,
    /// 详细描述。
    pub detail: String,
    /// 来源文件路径。
    pub file_path: String,
    /// 触发规则的代码片段（可选）。
    pub code_snippet: Option<String>,
    /// 执行计划中的节点行号（可选）。
    pub node_line: Option<usize>,
    /// 执行计划中的节点类型（可选），如 `"Seq Scan"`、`"Hash Join"`。
    pub node_type: Option<String>,
    /// 优化建议（可选）。
    pub suggestion: Option<String>,
}

impl Finding {
    /// 创建一个新的审核发现（唯一外部构造入口）。
    ///
    /// `#[non_exhaustive]` 禁止外部 crate 使用结构体字面量构造，
    /// 此方法作为受控的构造点，允许后续添加字段而不破坏下游。
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        rule_id: impl Into<String>,
        severity: Severity,
        category: DiagnosticCategory,
        title: impl Into<String>,
        detail: impl Into<String>,
        file_path: impl Into<String>,
        code_snippet: Option<String>,
        node_line: Option<usize>,
        node_type: Option<String>,
        suggestion: Option<String>,
    ) -> Self {
        Self {
            rule_id: rule_id.into(),
            severity,
            category,
            title: title.into(),
            detail: detail.into(),
            file_path: file_path.into(),
            code_snippet,
            node_line,
            node_type,
            suggestion,
        }
    }
}

/// 一次审核任务的上下文（零 IO 依赖）。
///
/// 所有审核维度共享此上下文。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuditContext {
    /// 关联的 Git commit SHA。
    pub commit_sha: String,
    /// 分支名。
    pub branch: String,
    /// 是否启用了数据库连接（EXPLAIN 模式）。
    pub db_enabled: bool,
    /// 数据库主机（仅用于日志标识，不含密码）。
    pub db_host: Option<String>,
    /// 链路追踪 ID（M-LOG-05）。
    pub trace_id: String,
}

impl AuditContext {
    /// 创建一个新的审核上下文。
    #[must_use]
    pub fn new(commit_sha: impl Into<String>, branch: impl Into<String>) -> Self {
        Self {
            commit_sha: commit_sha.into(),
            branch: branch.into(),
            db_enabled: false,
            db_host: None,
            trace_id: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_as_str() {
        assert_eq!(Severity::Critical.as_str(), "critical");
        assert_eq!(Severity::Warning.as_str(), "warning");
        assert_eq!(Severity::Info.as_str(), "info");
    }

    #[test]
    fn test_severity_icon() {
        assert_eq!(Severity::Critical.icon(), "🔴");
        assert_eq!(Severity::Warning.icon(), "🟡");
        assert_eq!(Severity::Info.icon(), "🔵");
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Critical), "critical");
        assert_eq!(format!("{}", Severity::Warning), "warning");
        assert_eq!(format!("{}", Severity::Info), "info");
    }

    #[test]
    fn test_finding_new() {
        let finding = Finding::new(
            "TEST-001",
            Severity::Critical,
            DiagnosticCategory::General,
            "Test finding",
            "A detailed description",
            "src/test.sql",
            Some("SELECT * FROM users".into()),
            Some(42),
            Some("Seq Scan".into()),
            Some("Add an index".into()),
        );
        assert_eq!(finding.rule_id, "TEST-001");
        assert_eq!(finding.severity, Severity::Critical);
        assert_eq!(finding.category, DiagnosticCategory::General);
        assert_eq!(finding.title, "Test finding");
        assert_eq!(finding.detail, "A detailed description");
        assert_eq!(finding.file_path, "src/test.sql");
        assert_eq!(finding.code_snippet, Some("SELECT * FROM users".into()));
        assert_eq!(finding.node_line, Some(42));
        assert_eq!(finding.node_type, Some("Seq Scan".into()));
        assert_eq!(finding.suggestion, Some("Add an index".into()));
    }

    #[test]
    fn test_finding_new_with_defaults() {
        let finding = Finding::new(
            "TEST-002",
            Severity::Info,
            DiagnosticCategory::ScanEfficiency,
            "Info only",
            "Just info",
            "src/test.sql",
            None,
            None,
            None,
            None,
        );
        assert_eq!(finding.rule_id, "TEST-002");
        assert_eq!(finding.severity, Severity::Info);
        assert_eq!(finding.file_path, "src/test.sql");
        assert_eq!(finding.code_snippet, None);
        assert_eq!(finding.node_line, None);
        assert_eq!(finding.node_type, None);
        assert_eq!(finding.suggestion, None);
    }

    #[test]
    fn test_audit_context_new() {
        let ctx = AuditContext::new("abc123", "main");
        assert_eq!(ctx.commit_sha, "abc123");
        assert_eq!(ctx.branch, "main");
        assert!(!ctx.db_enabled);
        assert_eq!(ctx.db_host, None);
        assert!(ctx.trace_id.is_empty());
    }

    #[test]
    fn test_audit_context_new_with_owned_strings() {
        let sha = "deadbeef".to_string();
        let branch = "feature".to_string();
        let ctx = AuditContext::new(sha, branch);
        assert_eq!(ctx.commit_sha, "deadbeef");
        assert_eq!(ctx.branch, "feature");
    }
}
