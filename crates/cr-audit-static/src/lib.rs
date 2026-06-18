//! 静态审核：无需数据库连接的规则匹配。
//!
//! Week 3-4 集成 ogexplain-core 的纯静态规则（如 TYPE-001、SUBQ-001）。

pub mod java_security;
pub mod rewrite;
pub mod rule_metadata;
pub mod sql_antipattern;

pub use java_security::{audit_java_source, audit_mybatis_xml, audit_security};
pub use sql_antipattern::audit_sql;
