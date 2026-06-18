//! MCP tool parameter and response types.
//!
//! All parameter types derive `Deserialize` + `JsonSchema` for automatic
//! MCP tool input schema generation. Response types derive `Serialize`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the `audit_sql` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SqlParams {
    /// SQL text to audit (supports multiple statements).
    pub sql: String,
}

/// Parameters for the `audit_files` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AuditFilesParams {
    /// Absolute or relative paths to SQL files to audit.
    pub files: Vec<String>,
}

/// Parameters for the `compare_baseline` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompareBaselineParams {
    /// Current SQL text.
    pub sql: String,
    /// Baseline complexity score (0-100).
    pub baseline_score: f64,
}

/// Parameters for the `suggest_fix` tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SuggestFixParams {
    /// Rule ID to generate a fix for (e.g., "STATIC-SELECT-STAR").
    pub rule_id: String,
    /// SQL text that triggered the finding.
    pub sql: String,
}

/// Fix suggestion response.
#[derive(Debug, Serialize)]
pub struct FixSuggestion {
    /// Rule ID being addressed.
    pub rule_id: String,
    /// Suggested fix description.
    pub description: String,
    /// Suggested rewritten SQL (if applicable).
    pub suggested_sql: Option<String>,
}

/// Error response body.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Human-readable error description.
    pub error: String,
}

/// Metadata for a single audit rule.
#[derive(Debug, Serialize)]
pub struct RuleInfo {
    /// Rule identifier (e.g., `"SCAN-001"`, `"STATIC-SELECT-STAR"`).
    pub id: String,
    /// Short human-readable description of the rule.
    pub description: String,
    /// Severity level: `"Critical"`, `"Warning"`, or `"Info"`.
    pub severity: String,
    /// Diagnostic category (e.g., `"ScanEfficiency"`, `"JoinStrategy"`).
    pub category: String,
}
