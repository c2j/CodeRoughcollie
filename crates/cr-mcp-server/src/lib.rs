//! MCP Server for CodeRoughcollie audit tools.
//!
//! Exposes `audit_sql`, `list_rules`, and `audit_files` tools via stdio transport
//! for AI assistant integration (Claude Desktop, Cursor, etc.).

pub mod server;
pub mod types;

pub use server::run_stdio;
