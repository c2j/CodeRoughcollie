//! MCP server handler and stdio transport.
//!
//! Exposes `audit_sql`, `list_rules`, and `audit_files` tools to MCP clients.

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{ServerHandler, ServiceExt};

use crate::types::*;

/// Stateless MCP server — all audit operations performed per-request.
pub struct CodeRoughcollieServer {
    /// Tool routing table populated by `#[rmcp::tool_router]`.
    pub(crate) tool_router: ToolRouter<Self>,
}

impl Default for CodeRoughcollieServer {
    fn default() -> Self {
        Self { tool_router: Self::tool_router() }
    }
}

#[rmcp::tool_router(vis = "pub(crate)")]
impl CodeRoughcollieServer {
    /// Audit SQL text for anti-patterns and return findings as JSON.
    #[rmcp::tool(name = "audit_sql", description = "Audit SQL for anti-patterns. Returns findings as JSON array.")]
    async fn audit_sql(&self, Parameters(params): Parameters<SqlParams>) -> String {
        let findings = cr_audit_static::audit_sql(&params.sql, "<inline>");
        serde_json::to_string_pretty(&findings)
            .unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#))
    }

    /// Explain SQL: run static + complexity analysis and return diagnostics.
    #[rmcp::tool(
        name = "explain_sql",
        description = "Explain SQL: run static anti-pattern detection + complexity scoring. Returns findings with rule IDs, severity, and suggestions."
    )]
    async fn explain_sql(&self, Parameters(params): Parameters<SqlParams>) -> String {
        let mut findings = cr_audit_static::audit_sql(&params.sql, "<inline>");
        findings.extend(cr_audit_complexity::audit_complexity(&params.sql, "<inline>", None, 10.0, 25.0));
        serde_json::to_string_pretty(&serde_json::json!({
            "sql": &params.sql,
            "finding_count": findings.len(),
            "findings": findings,
        }))
        .unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#))
    }

    /// List all available audit rules with metadata.
    #[rmcp::tool(
        name = "list_rules",
        description = "List all available audit rules with metadata: id, description, severity, and category."
    )]
    async fn list_rules(&self) -> String {
        let rules = get_rules_list();
        serde_json::to_string_pretty(&rules).unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#))
    }

    /// Read SQL files from disk and audit each one for anti-patterns.
    #[rmcp::tool(
        name = "audit_files",
        description = "Audit SQL files for anti-patterns. Reads each file, audits SQL content, returns findings JSON."
    )]
    async fn audit_files(&self, Parameters(params): Parameters<AuditFilesParams>) -> String {
        let mut all_findings: Vec<cr_core::Finding> = Vec::new();
        for path in &params.files {
            match tokio::fs::read_to_string(path).await {
                Ok(content) => {
                    let findings = cr_audit_static::audit_sql(&content, path);
                    all_findings.extend(findings);
                }
                Err(e) => {
                    return serde_json::to_string_pretty(&ErrorResponse {
                        error: format!("failed to read {}: {}", path, e),
                    })
                    .unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#));
                }
            }
        }
        serde_json::to_string_pretty(&all_findings)
            .unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#))
    }

    /// Compare current SQL complexity against a baseline score.
    #[rmcp::tool(
        name = "compare_baseline",
        description = "Compare SQL complexity score against a baseline. Returns delta and findings if thresholds exceeded."
    )]
    async fn compare_baseline(&self, Parameters(params): Parameters<CompareBaselineParams>) -> String {
        let findings = cr_audit_complexity::audit_complexity(&params.sql, "<inline>", Some(params.baseline_score), 10.0, 25.0);
        serde_json::to_string_pretty(&findings)
            .unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#))
    }

    /// Generate fix suggestions for a specific finding.
    #[rmcp::tool(name = "suggest_fix", description = "Generate fix suggestions for a given rule ID and SQL text.")]
    async fn suggest_fix(&self, Parameters(params): Parameters<SuggestFixParams>) -> String {
        let suggestion = suggest_fix_for_rule(&params.rule_id, &params.sql);
        serde_json::to_string_pretty(&suggestion)
            .unwrap_or_else(|e| format!(r#"{{"error": "serialization failed: {e}"}}"#))
    }
}

#[rmcp::tool_handler]
impl ServerHandler for CodeRoughcollieServer {}

/// Blocks until the transport is closed (e.g., stdin EOF).
pub async fn run_stdio() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = CodeRoughcollieServer::default();
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}

fn suggest_fix_for_rule(rule_id: &str, sql: &str) -> FixSuggestion {
    match cr_audit_static::rewrite::rewrite_sql(sql, rule_id) {
        Ok(rewritten) => FixSuggestion {
            rule_id: rule_id.into(),
            description: format!("metamorphosis 重写建议（规则 {rule_id}）"),
            suggested_sql: Some(rewritten),
        },
        Err(e) => FixSuggestion {
            rule_id: rule_id.into(),
            description: format!("metamorphosis 重写失败: {e}"),
            suggested_sql: None,
        },
    }
}

/// Returns the hardcoded list of all supported audit rules.
///
/// Includes 28 ogexplain diagnostic rule IDs and 3 static SQL anti-pattern rule IDs.
pub(crate) fn get_rules_list() -> Vec<RuleInfo> {
    vec![
        RuleInfo {
            id: "SCAN-001".into(),
            description: "Sequential scan on large table without index".into(),
            severity: "Critical".into(),
            category: "ScanEfficiency".into(),
        },
        RuleInfo {
            id: "SCAN-004".into(),
            description: "Bitmap scan with high recheck cost".into(),
            severity: "Warning".into(),
            category: "ScanEfficiency".into(),
        },
        RuleInfo {
            id: "JOIN-001".into(),
            description: "Nested Loop with high inner cost".into(),
            severity: "Critical".into(),
            category: "JoinStrategy".into(),
        },
        RuleInfo {
            id: "JOIN-002".into(),
            description: "Hash Join with large skew".into(),
            severity: "Warning".into(),
            category: "JoinStrategy".into(),
        },
        RuleInfo {
            id: "MEM-001".into(),
            description: "WorkMem too small for Hash Aggregation".into(),
            severity: "Warning".into(),
            category: "MemoryUsage".into(),
        },
        RuleInfo {
            id: "MEM-004".into(),
            description: "Sort memory exceeded work_mem threshold".into(),
            severity: "Warning".into(),
            category: "MemoryUsage".into(),
        },
        RuleInfo {
            id: "SORT-003".into(),
            description: "Multiple sort operations on same key".into(),
            severity: "Info".into(),
            category: "SortEfficiency".into(),
        },
        RuleInfo {
            id: "NET-001".into(),
            description: "Large data redistribution across nodes".into(),
            severity: "Warning".into(),
            category: "NetworkOverhead".into(),
        },
        RuleInfo {
            id: "EST-001".into(),
            description: "Row count estimate off by more than 10x".into(),
            severity: "Warning".into(),
            category: "CostMisestimation".into(),
        },
        RuleInfo {
            id: "EST-004".into(),
            description: "Widely varying row estimates across nodes".into(),
            severity: "Warning".into(),
            category: "CostMisestimation".into(),
        },
        RuleInfo {
            id: "PUSH-001".into(),
            description: "Predicate not pushed down to scan".into(),
            severity: "Warning".into(),
            category: "PushdownFailure".into(),
        },
        RuleInfo {
            id: "PUSH-002".into(),
            description: "Join filter not pushed down".into(),
            severity: "Warning".into(),
            category: "PushdownFailure".into(),
        },
        RuleInfo {
            id: "TYPE-001".into(),
            description: "Implicit type conversion on indexed column".into(),
            severity: "Warning".into(),
            category: "TypeMismatch".into(),
        },
        RuleInfo {
            id: "TYPE-004".into(),
            description: "Implicit type conversion in predicate".into(),
            severity: "Info".into(),
            category: "TypeMismatch".into(),
        },
        RuleInfo {
            id: "VEC-001".into(),
            description: "Sequential scan on column-oriented table".into(),
            severity: "Warning".into(),
            category: "Vectorization".into(),
        },
        RuleInfo {
            id: "SUBQ-001".into(),
            description: "Correlated subquery not pulled up".into(),
            severity: "Warning".into(),
            category: "SubqueryStructure".into(),
        },
        RuleInfo {
            id: "SUBQ-006".into(),
            description: "Unnecessary subquery can be flattened".into(),
            severity: "Info".into(),
            category: "SubqueryStructure".into(),
        },
        RuleInfo {
            id: "REW-001".into(),
            description: "Large IN list not converted to JOIN".into(),
            severity: "Warning".into(),
            category: "SubqueryStructure".into(),
        },
        RuleInfo {
            id: "DIST-001".into(),
            description: "Distribution column mismatch causing redistribution".into(),
            severity: "Warning".into(),
            category: "DistributionIssue".into(),
        },
        RuleInfo {
            id: "SKEW-001".into(),
            description: "Data skew detected via redistribute nodes".into(),
            severity: "Warning".into(),
            category: "DistributionIssue".into(),
        },
        RuleInfo {
            id: "PART-001".into(),
            description: "Partition pruning not effective".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "GEN-001".into(),
            description: "General performance issue detected".into(),
            severity: "Info".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "ANTI-003".into(),
            description: "Index scan amplification in Nested Loop".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "ANTI-005".into(),
            description: "Materialize cascade in nested loop".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "ANTI-007".into(),
            description: "Gather-then-sort with large row count".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "AGG-001".into(),
            description: "GroupAggregate on large dataset; prefer HashAggregate".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "AGG-002".into(),
            description: "HashAggregate spilling to disk".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "STATS-001".into(),
            description: "Default plan rows (10) indicate missing statistics".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "STATIC-SELECT-STAR".into(),
            description: "SELECT * retrieves all columns; specify needed columns explicitly".into(),
            severity: "Warning".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "STATIC-DELETE-NO-WHERE".into(),
            description: "DELETE without WHERE clause removes all rows".into(),
            severity: "Critical".into(),
            category: "General".into(),
        },
        RuleInfo {
            id: "STATIC-UPDATE-NO-WHERE".into(),
            description: "UPDATE without WHERE clause modifies all rows".into(),
            severity: "Critical".into(),
            category: "General".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_suggest_select_star() {
        let r = suggest_fix_for_rule("STATIC-SELECT-STAR", "SELECT * FROM users");
        assert_eq!(r.rule_id, "STATIC-SELECT-STAR");
    }

    #[test]
    fn test_suggest_delete_no_where() {
        let r = suggest_fix_for_rule("STATIC-DELETE-NO-WHERE", "DELETE FROM users");
        assert!(r.suggested_sql.is_none());
    }

    #[test]
    fn test_suggest_mybatis_dollar() {
        let r = suggest_fix_for_rule("SECURITY-MYBATIS-DOLLAR-PARAM", "WHERE n = '${name}'");
        assert_eq!(r.rule_id, "SECURITY-MYBATIS-DOLLAR-PARAM");
    }

    #[test]
    fn test_suggest_unknown() {
        let r = suggest_fix_for_rule("UNKNOWN", "SELECT 1");
        assert!(r.suggested_sql.is_none());
    }

    #[test]
    fn test_replace_star() {
        let r = suggest_fix_for_rule("STATIC-SELECT-STAR", "SELECT * FROM t");
        assert_eq!(r.rule_id, "STATIC-SELECT-STAR");
    }
}
