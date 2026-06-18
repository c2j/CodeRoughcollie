use std::collections::BTreeMap;
use std::sync::Mutex;

use chrono::{Duration, Utc};
use rusqlite::params;
use serde_json;
use tracing;

use crate::types::{AuditRecord, TrendPoint};
use cr_core::{Finding, Severity};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Mutex poisoned: {0}")]
    Lock(String),
}

/// 审核结果存储器。
///
/// 内部使用 `Mutex` 包装 SQLite 连接以提供线程安全的访问。
pub struct AuditStore {
    conn: Mutex<rusqlite::Connection>,
}

/// 存储后端类型。
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StorageBackend {
    /// SQLite（默认，单文件）。
    Sqlite,
    /// PostgreSQL（企业级，支持远程连接）。
    /// 实现方式：将 AuditStore 的 rusqlite::Connection 替换为 tokio-postgres::Client，
    /// SQL 语法保持兼容（CREATE TABLE / INSERT / SELECT）。
    Postgres,
}

/// 根据后端类型创建存储。
///
/// # Errors
pub fn create_store(backend: &StorageBackend, path_or_connstr: &str) -> Result<AuditStore, StoreError> {
    match backend {
        StorageBackend::Sqlite => AuditStore::open(path_or_connstr),
        StorageBackend::Postgres => Err(StoreError::Lock(
            "PostgreSQL 后端尚未实现，请使用 SQLite。迁移路径：替换 rusqlite → tokio-postgres，SQL DDL 兼容".into(),
        )),
    }
}

impl AuditStore {
    /// 打开（或创建）指定路径的 SQLite 数据库。
    ///
    /// # Errors
    ///
    /// 当无法打开数据库或执行建表语句时返回 [`StoreError`]。
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let conn = rusqlite::Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_records (
                audit_id       TEXT PRIMARY KEY,
                commit_sha     TEXT NOT NULL,
                branch         TEXT NOT NULL,
                timestamp      TEXT NOT NULL,
                findings_json  TEXT NOT NULL,
                health_score   REAL NOT NULL,
                degraded       INTEGER NOT NULL DEFAULT 0
            );",
        )?;

        tracing::info!(path, "AuditStore opened");
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// 保存一条审核记录。
    ///
    /// # Errors
    ///
    /// 当 JSON 序列化失败或 SQLite 写入失败时返回 [`StoreError`]。
    pub fn save(&self, record: &AuditRecord) -> Result<(), StoreError> {
        let findings_json = serde_json::to_string(&record.findings)?;
        let degraded_int: i32 = if record.degraded { 1 } else { 0 };

        let conn = self.conn.lock().map_err(|e| StoreError::Lock(e.to_string()))?;
        conn.execute(
            "INSERT INTO audit_records (audit_id, commit_sha, branch, timestamp, findings_json, health_score, degraded)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.audit_id,
                record.commit_sha,
                record.branch,
                record.timestamp,
                findings_json,
                record.health_score,
                degraded_int,
            ],
        )?;

        tracing::debug!(audit_id = %record.audit_id, "Audit record saved");
        Ok(())
    }

    /// 根据 commit SHA 查询审核记录。
    ///
    /// # Errors
    ///
    /// 当 SQLite 查询失败或 JSON 反序列化失败时返回 [`StoreError`]。
    pub fn get_by_commit(&self, commit_sha: &str) -> Result<Vec<AuditRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|e| StoreError::Lock(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT audit_id, commit_sha, branch, timestamp, findings_json, health_score, degraded
             FROM audit_records
             WHERE commit_sha = ?1
             ORDER BY timestamp DESC",
        )?;

        let records = stmt.query_map(params![commit_sha], Self::map_row)?;
        records.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
    }

    /// 获取最近的 N 条审核记录。
    ///
    /// # Errors
    ///
    /// 当 SQLite 查询失败或 JSON 反序列化失败时返回 [`StoreError`]。
    pub fn get_recent(&self, limit: usize) -> Result<Vec<AuditRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|e| StoreError::Lock(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT audit_id, commit_sha, branch, timestamp, findings_json, health_score, degraded
             FROM audit_records
             ORDER BY timestamp DESC
             LIMIT ?1",
        )?;

        let limit_i64: i64 = limit.try_into().unwrap_or(i64::MAX);
        let records = stmt.query_map(params![limit_i64], Self::map_row)?;
        records.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
    }

    /// 获取指定天数内的健康度趋势。
    ///
    /// 按日期聚合，返回每日平均健康度、审核总数和 Critical 发现总数。
    ///
    /// # Errors
    ///
    /// 当 SQLite 查询失败或 JSON 反序列化失败时返回 [`StoreError`]。
    pub fn get_trend(&self, days: usize) -> Result<Vec<TrendPoint>, StoreError> {
        let threshold = (Utc::now() - Duration::days(days as i64)).format("%Y-%m-%dT%H:%M:%S").to_string();

        let conn = self.conn.lock().map_err(|e| StoreError::Lock(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT findings_json, health_score, timestamp
             FROM audit_records
             WHERE timestamp >= ?1
             ORDER BY timestamp ASC",
        )?;

        let rows = stmt.query_map(params![threshold], |row| {
            let findings_json: String = row.get(0)?;
            let health_score: f64 = row.get(1)?;
            let timestamp: String = row.get(2)?;
            Ok((findings_json, health_score, timestamp))
        })?;

        let mut map: BTreeMap<String, Aggregation> = BTreeMap::new();

        for row in rows {
            let (findings_json, health_score, timestamp) = row?;
            let date = timestamp.chars().take(10).collect::<String>();

            let findings: Vec<Finding> = serde_json::from_str(&findings_json)?;
            let critical_count = findings.iter().filter(|f| f.severity == Severity::Critical).count();

            let entry = map.entry(date).or_default();
            entry.health_score_sum += health_score;
            entry.audit_count += 1;
            entry.critical_count += critical_count;
        }

        Ok(map
            .into_iter()
            .map(|(date, agg)| TrendPoint {
                date,
                avg_health_score: if agg.audit_count > 0 { agg.health_score_sum / agg.audit_count as f64 } else { 0.0 },
                total_audits: agg.audit_count,
                total_critical: agg.critical_count,
            })
            .collect())
    }

    fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditRecord> {
        let audit_id: String = row.get(0)?;
        let commit_sha: String = row.get(1)?;
        let branch: String = row.get(2)?;
        let timestamp: String = row.get(3)?;
        let findings_json: String = row.get(4)?;
        let health_score: f64 = row.get(5)?;
        let degraded_int: i32 = row.get(6)?;

        let findings: Vec<Finding> = serde_json::from_str(&findings_json).unwrap_or_else(|e| {
            tracing::warn!(%audit_id, error = %e, "Failed to deserialize findings_json");
            Vec::new()
        });

        Ok(AuditRecord { audit_id, commit_sha, branch, timestamp, findings, health_score, degraded: degraded_int != 0 })
    }
}

#[derive(Default)]
struct Aggregation {
    health_score_sum: f64,
    audit_count: usize,
    critical_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_open_save_get() {
        let path = format!("/tmp/cr_test_{}.db", std::process::id());
        let store = AuditStore::open(&path).expect("open");

        store
            .save(&AuditRecord {
                audit_id: "t1".into(),
                commit_sha: "abc".into(),
                branch: "main".into(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                findings: vec![],
                health_score: 95.0,
                degraded: false,
            })
            .expect("save");

        let records = store.get_by_commit("abc").expect("get");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].audit_id, "t1");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_store_recent_empty() {
        let path = format!("/tmp/cr_test_r_{}.db", std::process::id());
        let store = AuditStore::open(&path).expect("open");
        assert!(store.get_recent(5).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_store_trend_empty() {
        let path = format!("/tmp/cr_test_t_{}.db", std::process::id());
        let store = AuditStore::open(&path).expect("open");
        assert!(store.get_trend(30).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
