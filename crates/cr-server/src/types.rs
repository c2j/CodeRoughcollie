//! API 请求/响应类型定义。

use serde::{Deserialize, Serialize};

use cr_core::Finding;

/// 发起审核的请求体。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct AuditRequest {
    /// 待审核的 SQL 文本。
    pub sql: String,
    /// 关联的 Git commit SHA。
    pub commit_sha: String,
    /// 分支名。
    pub branch: String,
}

/// 审核结果响应体。
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct AuditResponse {
    /// 审核记录 ID。
    pub audit_id: String,
    /// 审核发现列表。
    pub findings: Vec<Finding>,
    /// 健康度评分（0-100）。
    pub health_score: f64,
    /// 是否发生了降级（如 EXPLAIN 不可用）。
    pub degraded: bool,
}

/// 持久化的审核记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct AuditRecord {
    /// 审核记录 ID（主键）。
    pub audit_id: String,
    /// 关联的 Git commit SHA。
    pub commit_sha: String,
    /// 分支名。
    pub branch: String,
    /// ISO 8601 时间戳。
    pub timestamp: String,
    /// 审核发现列表。
    pub findings: Vec<Finding>,
    /// 健康度评分（0-100）。
    pub health_score: f64,
    /// 是否发生了降级（如 EXPLAIN 不可用）。
    pub degraded: bool,
}

/// 健康度趋势数据点。
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct TrendPoint {
    /// 日期（YYYY-MM-DD）。
    pub date: String,
    /// 当日平均健康度评分。
    pub avg_health_score: f64,
    /// 当日审核总数。
    pub total_audits: usize,
    /// 当日 Critical 级别发现总数。
    pub total_critical: usize,
}
