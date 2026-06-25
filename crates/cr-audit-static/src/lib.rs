//! 静态审核：无需数据库连接的规则匹配。
//!
//! Week 3-4 集成 ogexplain-core 的纯静态规则（如 TYPE-001、SUBQ-001）。

#[cfg(feature = "embed-astgrep")]
mod astgrep_embed;
pub mod astgrep_runner;
pub mod diff_aware;
pub mod file_type;
pub mod java_security;
pub mod rewrite;
pub mod rule_metadata;
pub mod sql_antipattern;
pub mod validation;

pub use astgrep_runner::{audit_files as audit_files_with_astgrep, AstgrepError, AstgrepOptions};
pub use file_type::{detect, FileKind};
pub use java_security::{audit_java_source, audit_mybatis_xml, audit_security};
pub use sql_antipattern::audit_sql;
pub use validation::{parser_errors_to_findings, sql_warnings_to_findings, validate_statements};
