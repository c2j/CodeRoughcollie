//! 审核调度引擎。
//!
//! 接收 SQL 文本，协调多个审核维度（静态、EXPLAIN、复杂度），聚合所有 Finding。

use crate::types::Finding;

/// 审核指标（使用 metrics crate，R-OBS-03）。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct AuditMetrics {
    /// 审核任务总数。
    pub audits_total: u64,
    /// 审核耗时（毫秒）。
    pub audit_duration_ms: u64,
    /// Critical Finding 数量。
    pub critical_count: usize,
    /// Warning Finding 数量。
    pub warning_count: usize,
    /// EXPLAIN 成功次数。
    pub explain_success: u64,
    /// EXPLAIN 失败/降级次数。
    pub explain_degraded: u64,
}

impl AuditMetrics {
    /// 记录一次审核完成。
    pub fn record_audit(&mut self, duration_ms: u64, critical: usize, warning: usize) {
        self.audits_total += 1;
        self.audit_duration_ms += duration_ms;
        self.critical_count += critical;
        self.warning_count += warning;
    }

    /// 记录一次 EXPLAIN 成功。
    pub fn record_explain_success(&mut self) {
        self.explain_success += 1;
    }

    /// 记录一次 EXPLAIN 降级。
    pub fn record_explain_degraded(&mut self) {
        self.explain_degraded += 1;
    }
}

/// 单次审核的结果聚合。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct AuditResult {
    /// 全部 Finding。
    pub findings: Vec<Finding>,
    /// 是否发生了降级（如 EXPLAIN 不可用）。
    pub degraded: bool,
}

impl AuditResult {
    /// 合并另一个审核结果。
    pub fn merge(&mut self, other: AuditResult) {
        self.findings.extend(other.findings);
        self.degraded = self.degraded || other.degraded;
    }
}

/// 审核引擎配置。
///
/// 控制启用哪些审核维度。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuditEngine {
    /// 是否启用静态审核。
    pub enable_static: bool,
    /// 是否启用 EXPLAIN 审核（需要数据库连接）。
    pub enable_explain: bool,
    /// 是否启用复杂度审核。
    pub enable_complexity: bool,
}

impl Default for AuditEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditEngine {
    /// 创建新的审核引擎，默认启用静态 + 复杂度，禁用 EXPLAIN。
    #[must_use]
    pub fn new() -> Self {
        Self { enable_static: true, enable_explain: false, enable_complexity: true }
    }

    /// 仅静态审核模式（无数据库连接）。
    #[must_use]
    pub fn static_only() -> Self {
        Self { enable_static: true, enable_explain: false, enable_complexity: false }
    }

    /// 启用全部审核维度（含 EXPLAIN）。
    #[must_use]
    pub fn full() -> Self {
        Self { enable_static: true, enable_explain: true, enable_complexity: true }
    }
}
