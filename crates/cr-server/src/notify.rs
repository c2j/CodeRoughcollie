//! 通知发送器：Slack 兼容 webhook 与通用 JSON POST。
//!
//! 将审核发现通过 webhook 推送至协作平台（Slack / 自建通知桥）。

use cr_core::{Finding, Severity};

/// Email 通知配置。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmailConfig {
    /// SMTP 主机。
    pub smtp_host: String,
    /// SMTP 端口。
    pub smtp_port: u16,
    /// 收件人列表。
    pub recipients: Vec<String>,
    /// 发件人地址。
    pub from: String,
}

/// 发送 Email 通知（通过 webhook 桥接或 SMTP）。
///
/// 当前实现将邮件内容格式化为 JSON，通过 webhook 转发到邮件服务。
/// 生产环境可对接 SES / SendGrid / 自建 SMTP。
///
/// # Errors
pub async fn send_email_notification(
    webhook_url: &str,
    config: &EmailConfig,
    findings: &[Finding],
) -> Result<(), NotifyError> {
    let critical_count = findings.iter().filter(|f| f.severity == Severity::Critical).count();
    let subject = if critical_count > 0 {
        format!("[CRITICAL] CodeRoughcollie 审核发现 {critical_count} 个严重问题")
    } else {
        format!("CodeRoughcollie 审核报告（{} 个发现）", findings.len())
    };

    let body = findings
        .iter()
        .map(|f| format!("[{}] {} — {}", f.severity.as_str(), f.rule_id, f.title))
        .collect::<Vec<_>>()
        .join("\n");

    let payload = serde_json::json!({
        "to": config.recipients,
        "from": config.from,
        "subject": subject,
        "body": body,
    });

    send_webhook(webhook_url, &payload).await
}

// ---------------------------------------------------------------------------
// 配置与错误
// ---------------------------------------------------------------------------

/// Webhook 通知配置。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NotifyConfig {
    /// Webhook URL（Slack Incoming Webhook 或兼容端点）。
    pub webhook_url: String,
    /// 最低严重度：仅发送 >= 此级别的发现。  
    /// 排序：Critical > Warning > Info。
    pub min_severity: Severity,
    /// 可选的目标频道（Slack channel 覆盖）。
    pub channel: Option<String>,
}

impl NotifyConfig {
    /// 创建通知配置。
    #[must_use]
    pub fn new(webhook_url: impl Into<String>, min_severity: Severity) -> Self {
        Self { webhook_url: webhook_url.into(), min_severity, channel: None }
    }
}

/// 通知发送失败类型。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NotifyError {
    /// HTTP 请求失败或非 2xx 响应。
    #[error("通知发送失败: {0}")]
    SendFailed(String),
    /// JSON 序列化失败。
    #[error("序列化失败: {0}")]
    Serialize(String),
}

// ---------------------------------------------------------------------------
// 公共 API
// ---------------------------------------------------------------------------

/// 发送 Slack 兼容的 webhook 通知。
///
/// 按 `config.min_severity` 过滤发现，仅当有符合条件的发现时才实际发送；
/// 无匹配发现时直接返回 `Ok(())`。
///
/// # Errors
///
/// 返回 `NotifyError::SendFailed` 当请求失败或服务端返回非 2xx；
/// 返回 `NotifyError::Serialize` 当 JSON 构造失败。
pub async fn send_slack_notification(config: &NotifyConfig, findings: &[Finding]) -> Result<(), NotifyError> {
    let filtered: Vec<&Finding> =
        findings.iter().filter(|f| meets_min_severity(f.severity, config.min_severity)).collect();

    if filtered.is_empty() {
        return Ok(());
    }

    let payload = build_slack_payload(&filtered, config.channel.as_deref())?;
    send_webhook(&config.webhook_url, &payload).await
}

/// 发送通用 JSON POST webhook。
///
/// # Errors
///
/// 返回 `NotifyError::SendFailed` 当请求失败或服务端返回非 2xx。
pub async fn send_webhook(url: &str, payload: &serde_json::Value) -> Result<(), NotifyError> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| NotifyError::SendFailed(format!("创建 HTTP 客户端失败: {e}")))?;

    let resp =
        client.post(url).json(payload).send().await.map_err(|e| NotifyError::SendFailed(format!("请求失败: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(NotifyError::SendFailed(format!("服务端返回 {status}: {body}")));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 内部辅助
// ---------------------------------------------------------------------------

/// 判断 `severity` 是否达到 `min` 阈值。
fn meets_min_severity(severity: Severity, min: Severity) -> bool {
    fn level(s: Severity) -> u8 {
        match s {
            Severity::Critical => 3,
            Severity::Warning => 2,
            _ => 1,
        }
    }
    level(severity) >= level(min)
}

/// 构造 Slack Incoming Webhook 消息体。
fn build_slack_payload(findings: &[&Finding], channel: Option<&str>) -> Result<serde_json::Value, NotifyError> {
    let critical_count = findings.iter().filter(|f| f.severity == Severity::Critical).count();
    let warning_count = findings.iter().filter(|f| f.severity == Severity::Warning).count();
    let info_count = findings.iter().filter(|f| f.severity == Severity::Info).count();

    let total = findings.len();
    let summary_parts: Vec<String> = {
        let mut parts = Vec::new();
        if critical_count > 0 {
            parts.push(format!("{} 个 Critical", critical_count));
        }
        if warning_count > 0 {
            parts.push(format!("{} 个 Warning", warning_count));
        }
        if info_count > 0 {
            parts.push(format!("{} 个 Info", info_count));
        }
        parts
    };

    let icon = if critical_count > 0 { "🔴" } else { "🟡" };
    let text = format!("{} CodeRoughcollie 审核发现 {} 问题（{}）", icon, total, summary_parts.join("、"),);

    let attachments: Vec<serde_json::Value> = findings
        .iter()
        .map(|f| {
            let color = match f.severity {
                Severity::Critical => "danger",
                Severity::Warning => "warning",
                _ => "good",
            };
            serde_json::json!({
                "color": color,
                "title": format!("{}: {}", f.rule_id, f.title),
                "text": f.detail,
                "fields": [
                    {
                        "title": "严重度",
                        "value": f.severity.as_str(),
                        "short": true
                    }
                ]
            })
        })
        .collect();

    let mut payload = serde_json::json!({
        "text": text,
        "attachments": attachments,
    });

    if let Some(ch) = channel {
        payload["channel"] = serde_json::json!(ch);
    }

    Ok(payload)
}
