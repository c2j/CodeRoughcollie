//! Axum REST API 路由和服务启动入口。

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tracing;

use crate::db::{AuditStore, StoreError};
use crate::types::{AuditRecord, AuditRequest, AuditResponse, TrendPoint};
use cr_audit_static::audit_sql;
use cr_core::scoring::health_score;

/// 服务器错误。
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// 存储层错误。
    #[error("Store error: {0}")]
    Store(#[from] StoreError),

    /// IO 错误（如绑定端口失败）。
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> axum::response::Response {
        let (status, body) = match &self {
            Self::Store(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Store error: {e}")),
            Self::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("IO error: {e}")),
        };
        (status, Json(serde_json::json!({"error": body}))).into_response()
    }
}

/// 生成简单的审核记录 ID。
fn generate_audit_id(commit_sha: &str) -> String {
    let now = chrono::Utc::now();
    format!("{}-{}", commit_sha, now.timestamp_millis())
}

// ---------------------------------------------------------------------------
// Query param types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RecentQuery {
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct TrendQuery {
    days: Option<usize>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/health` — 健康检查。
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

/// `POST /api/audit` — 对传入的 SQL 执行审核并持久化结果。
async fn run_audit_handler(
    State(store): State<Arc<AuditStore>>,
    Json(req): Json<AuditRequest>,
) -> Result<Json<AuditResponse>, ServerError> {
    let findings = audit_sql(&req.sql, "<inline>");
    let score = health_score(&findings);

    let audit_id = generate_audit_id(&req.commit_sha);
    let timestamp = chrono::Utc::now().to_rfc3339();
    let degraded = false; // 静态审核不会触发降级

    let record = AuditRecord {
        audit_id: audit_id.clone(),
        commit_sha: req.commit_sha,
        branch: req.branch,
        timestamp,
        findings: findings.clone(),
        health_score: score,
        degraded,
    };

    store.save(&record)?;

    tracing::info!(audit_id = %record.audit_id, health_score = %score, "Audit completed");

    Ok(Json(AuditResponse { audit_id, findings, health_score: score, degraded }))
}

/// `GET /api/audit/:commit_sha` — 根据 commit SHA 查询审核记录。
async fn get_audit_by_commit_handler(
    State(store): State<Arc<AuditStore>>,
    Path(commit_sha): Path<String>,
) -> Result<Json<Vec<AuditRecord>>, ServerError> {
    let records = store.get_by_commit(&commit_sha)?;
    Ok(Json(records))
}

/// `GET /api/audits/recent` — 获取最近 N 条审核记录。
async fn get_recent_audits_handler(
    State(store): State<Arc<AuditStore>>,
    Query(query): Query<RecentQuery>,
) -> Result<Json<Vec<AuditRecord>>, ServerError> {
    let limit = query.limit.unwrap_or(10);
    let records = store.get_recent(limit)?;
    Ok(Json(records))
}

/// `GET /api/trend` — 获取健康度趋势。
async fn get_trend_handler(
    State(store): State<Arc<AuditStore>>,
    Query(query): Query<TrendQuery>,
) -> Result<Json<Vec<TrendPoint>>, ServerError> {
    let days = query.days.unwrap_or(30);
    let trend = store.get_trend(days)?;
    Ok(Json(trend))
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

/// 启动 REST API 服务器。
///
/// 绑定到指定地址，等待 Ctrl-C 信号触发优雅关闭。
///
/// # Errors
///
/// 当端口绑定失败或 axum 服务器运行出错时返回 [`ServerError`]。
pub async fn run_server(addr: &str, store: AuditStore) -> Result<(), ServerError> {
    let state = Arc::new(store);

    let app = Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/audit", post(run_audit_handler))
        .route("/api/audit/{commit_sha}", get(get_audit_by_commit_handler))
        .route("/api/audits/recent", get(get_recent_audits_handler))
        .route("/api/trend", get(get_trend_handler))
        .route("/dashboard", get(dashboard_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(addr, "cr-server starting");

    axum::serve(listener, app).await?;

    Ok(())
}

/// `GET /dashboard` — 审核趋势 Dashboard。
async fn dashboard_handler(State(store): State<Arc<AuditStore>>) -> Result<axum::response::Html<String>, ServerError> {
    let trend = store.get_trend(30)?;
    let trend_json = serde_json::to_string(&trend).unwrap_or_else(|_| "[]".into());
    let html = DASHBOARD_HTML.replace("__TREND_DATA__", &trend_json);
    Ok(axum::response::Html(html))
}

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>CodeRoughcollie Dashboard</title>
<style>body{font-family:sans-serif;margin:2rem}h1{color:#333}canvas{max-width:800px}</style>
</head><body>
<h1>CodeRoughcollie 审核趋势</h1>
<p>最近 30 天的健康度评分趋势</p>
<canvas id="chart" width="800" height="300"></canvas>
<script>
const data = __TREND_DATA__;
const ctx = document.getElementById('chart').getContext('2d');
if (data.length > 0) {
  ctx.beginPath();
  data.forEach((d,i) => {
    const x = i * (800/data.length);
    const y = 300 - (d.avg_health_score * 3);
    i === 0 ? ctx.moveTo(x,y) : ctx.lineTo(x,y);
  });
  ctx.strokeStyle = '#4CAF50'; ctx.lineWidth = 2; ctx.stroke();
} else {
  ctx.fillText('暂无数据', 350, 150);
}
</script>
</body></html>"#;
