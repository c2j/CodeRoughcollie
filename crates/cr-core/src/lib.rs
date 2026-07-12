//! CodeRoughcollie 审核引擎核心。
//!
//! 提供审核流程所需的全部基础类型、trait 和错误定义。
//! 本 crate 零外部 IO 依赖（M-ARCH-02），所有数据库/文件系统交互通过 trait 抽象注入。

pub mod baseline;
pub mod compliance;
pub mod dedup;
pub mod engine;
pub mod error;
pub mod filter;
pub mod scoring;
pub mod traits;
pub mod types;

pub use compliance::{check_compliance, get_compliance_rules, CompliancePack, ComplianceRule};
pub use engine::{AuditEngine, AuditMetrics, AuditResult};
pub use error::{DbError, RoughcollieError};
pub use traits::DbConnection;
pub use types::{AuditContext, DiagnosticCategory, Finding, Severity};
