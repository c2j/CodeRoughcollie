//! 语义影响分析：集成 codeweb 查询调用链（三期）。
//!
//! 本 crate 提供 codeweb HTTP 客户端，用于查询 Java/Mapper 文件的
//! 上下游调用链，并将分析结果转化为审核发现。
//!
//! # 使用示例
//!
//! ```rust,no_run
//! use cr_audit_impact::{CodewebClient, CodewebConfig};
//!
//! # async fn example() -> Result<(), cr_audit_impact::CodewebError> {
//! let config = CodewebConfig::new("http://localhost:8080");
//! let client = CodewebClient::new(config)?;
//! let result = client.query_impact("src/main/java/com/example/Mapper.java").await?;
//! let findings = cr_audit_impact::impact_to_findings(&result, "src/main/java/com/example/Mapper.java");
//! # Ok(())
//! # }
//! ```

use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};

/// codeweb 集成错误。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodewebError {
    /// 网络连接失败或 HTTP 客户端初始化失败。
    #[error("codeweb 连接失败: {0}")]
    ConnectionFailed(String),

    /// 请求超时。
    #[error("codeweb 请求超时")]
    Timeout,

    /// API 返回错误（非 2xx 状态码）或响应解析失败。
    #[error("codeweb 返回错误: {0}")]
    ApiError(String),
}

/// codeweb HTTP 客户端配置。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CodewebConfig {
    /// codeweb 服务地址（如 "http://localhost:8080"）
    pub endpoint: String,

    /// 请求超时（秒）
    pub timeout_secs: u64,
}

impl CodewebConfig {
    /// 创建默认配置。
    #[must_use]
    pub fn new(endpoint: &str) -> Self {
        Self { endpoint: endpoint.into(), timeout_secs: 30 }
    }
}

/// codeweb 客户端。
///
/// 通过 HTTP 与 codeweb 服务通信，查询文件的上下游调用链。
pub struct CodewebClient {
    config: CodewebConfig,
    http: reqwest::Client,
}

impl CodewebClient {
    /// 创建一个新的 codeweb 客户端。
    ///
    /// # 参数
    ///
    /// * `config` — 客户端配置。
    ///
    /// # 错误
    ///
    /// 返回 [`CodewebError::ConnectionFailed`] 当 HTTP 客户端初始化失败时
    ///（例如 TLS 后端缺失）。
    pub fn new(config: CodewebConfig) -> Result<Self, CodewebError> {
        let timeout = Duration::from_secs(config.timeout_secs);
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| CodewebError::ConnectionFailed(e.to_string()))?;
        Ok(Self { config, http })
    }

    /// 查询指定文件的上下游调用链。
    ///
    /// 发送 `GET {endpoint}/api/impact?file={file_path}` 请求到 codeweb 服务。
    ///
    /// # 参数
    ///
    /// * `file_path` — 文件路径（相对于项目根目录）。
    ///
    /// # 返回
    ///
    /// * `Ok(ImpactResult)` — 包含上游调用者和下游被调用者的影响分析结果。
    ///
    /// # 错误
    ///
    /// * [`CodewebError::ConnectionFailed`] — 网络连接失败。
    /// * [`CodewebError::Timeout`] — 请求超时。
    /// * [`CodewebError::ApiError`] — API 返回错误或响应解析失败。
    pub async fn query_impact(&self, file_path: &str) -> Result<ImpactResult, CodewebError> {
        let base = format!("{}/api/impact", self.config.endpoint.trim_end_matches('/'));
        let mut url = Url::parse(&base).map_err(|e| CodewebError::ConnectionFailed(format!("URL 解析失败: {e}")))?;
        url.query_pairs_mut().append_pair("file", file_path);

        let response = self.http.get(url).send().await.map_err(|e| {
            if e.is_timeout() {
                CodewebError::Timeout
            } else {
                CodewebError::ConnectionFailed(e.to_string())
            }
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(CodewebError::ApiError(format!("HTTP {}: {}", status.as_u16(), body)));
        }

        let result: ImpactResult = response.json().await.map_err(|e| CodewebError::ApiError(e.to_string()))?;

        Ok(result)
    }
}

/// 影响分析结果。
///
/// 包含变更文件的上游调用者和下游被调用者列表。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImpactResult {
    /// 上游调用者（哪些文件调用了变更文件）。
    pub upstream: Vec<ImpactNode>,
    /// 下游被调用者（变更文件调用了哪些文件）。
    pub downstream: Vec<ImpactNode>,
}

/// 调用链节点。
///
/// 描述单条调用关系中的文件、符号及行号。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ImpactNode {
    /// 文件路径。
    pub file_path: String,
    /// 符号名称（方法名、类名等）。
    pub symbol: String,
    /// 符号所在行号（可选）。
    pub line: Option<usize>,
}

/// 将影响分析结果转为 Finding 列表。
///
/// 当变更文件有大量上游调用者时，标记为高影响变更。
///
/// # 规则
///
/// | 规则 ID | 触发条件 | 严重度 |
/// |---------|---------|--------|
/// | IMPACT-001 | 上游调用者 > 10 | Warning |
///
/// # 参数
///
/// * `result` — 影响分析结果。
/// * `changed_file` — 变更文件路径（用于发现描述）。
#[must_use]
pub fn impact_to_findings(result: &ImpactResult, changed_file: &str) -> Vec<cr_core::Finding> {
    let upstream_count = result.upstream.len();

    if upstream_count > 10 {
        let callers: Vec<&str> = result.upstream.iter().map(|n| n.file_path.as_str()).collect();

        vec![cr_core::Finding::new(
            "IMPACT-001",
            cr_core::Severity::Warning,
            cr_core::DiagnosticCategory::General,
            "高影响变更",
            format!("文件 `{changed_file}` 有 {upstream_count} 个上游调用者，变更影响范围较大，建议仔细审查"),
            None,
            None,
            Some(format!("上游调用者列表：{}", callers.join(", "))),
        )]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_new() {
        let c = CodewebConfig::new("http://localhost:8080");
        assert_eq!(c.endpoint, "http://localhost:8080");
        assert_eq!(c.timeout_secs, 30);
    }

    #[test]
    fn test_impact_empty() {
        let r = ImpactResult { upstream: vec![], downstream: vec![] };
        assert!(impact_to_findings(&r, "test.java").is_empty());
    }

    #[test]
    fn test_impact_many_upstream() {
        let upstream: Vec<ImpactNode> = (0..15)
            .map(|i| ImpactNode { file_path: format!("c{i}.java"), symbol: format!("m{i}"), line: Some(i) })
            .collect();
        let r = ImpactResult { upstream, downstream: vec![] };
        let f = impact_to_findings(&r, "changed.java");
        assert!(!f.is_empty());
        assert!(f.iter().any(|f| f.rule_id.contains("IMPACT")));
    }

    #[test]
    fn test_error_display() {
        assert!(CodewebError::ConnectionFailed("timeout".into()).to_string().contains("timeout"));
    }
}
