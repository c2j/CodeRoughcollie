//! metamorphosis SQL 重写集成。
//!
//! 将 CodeRoughcollie Finding 映射到 metamorphosis RewriteRule，
//! 调用重写引擎生成实际的 SQL 修正。
//!
//! ## 规则映射
//!
//! | CodeRoughcollie Rule | metamorphosis Rule | 类型 |
//! |----------------------|--------------------|------|
//! | STATIC-SELECT-STAR | eliminate-select-star | Safe (自动重写) |
//! | STATIC-DELETE-NO-WHERE | (metamorphosis不支持) | 需提 issue |
//! | STATIC-UPDATE-NO-WHERE | (metamorphosis不支持) | 需提 issue |

use metamorphosis_core::{RewriteAction, RewriteConfig, RewriteContext, RewriteEngine, RuleRegistry};
use ogsql_parser::formatter::SqlFormatter;
use ogsql_parser::Parser;
use std::collections::HashSet;

/// CodeRoughcollie → metamorphosis 规则 ID 映射。
const RULE_MAP: &[(&str, &str)] = &[("STATIC-SELECT-STAR", "eliminate-select-star")];

/// 使用 metamorphosis 重写 SQL。
///
/// # Errors
///
/// 当 SQL 解析失败或 metamorphosis 规则未找到时返回错误。
pub fn rewrite_sql(sql: &str, cr_rule_id: &str) -> Result<String, String> {
    let m_rule_id = RULE_MAP.iter().find(|(cr, _)| *cr == cr_rule_id).map(|(_, m)| *m).ok_or_else(|| {
        format!("规则 '{cr_rule_id}' 在 metamorphosis 中无对应规则。metamorphosis 仅支持: eliminate-select-star 等")
    })?;

    let (stmt_infos, errors) = Parser::parse_sql(sql);
    if stmt_infos.is_empty() {
        let err_msg = errors.first().map_or("unknown".into(), |e| e.to_string());
        return Err(format!("SQL 解析失败: {err_msg}"));
    }

    let all_rules = metamorphosis_rules::builtin_rules();
    let filtered: Vec<_> = all_rules.into_iter().filter(|r| r.id() == m_rule_id).collect();
    if filtered.is_empty() {
        return Err(format!("metamorphosis 规则 '{m_rule_id}' 未找到"));
    }

    let registry = RuleRegistry::new(filtered);
    let engine = RewriteEngine::new(registry);

    let config = RewriteConfig { enabled_rules: HashSet::from([m_rule_id.to_string()]), ..Default::default() };

    let ctx = RewriteContext {
        version: None,
        schema: None,
        config: &config,
        source_file: None,
        known_variables: None,
        diagnostic_hints: None,
    };

    let stmts: Vec<_> = stmt_infos.into_iter().map(|si| si.statement).collect();
    let result = engine.rewrite(&ctx, stmts);

    if result.changed {
        let formatter = SqlFormatter::new().pretty_print(true);
        let rewritten: Vec<String> = result.statements.iter().map(|stmt| formatter.format_statement(stmt)).collect();
        Ok(rewritten.join(";\n"))
    } else {
        let reason = result
            .match_failures
            .iter()
            .find(|m| m.rule_id == m_rule_id)
            .map(|m| m.reason.clone())
            .unwrap_or_else(|| "规则未匹配".into());
        Err(format!("重写未执行: {reason}"))
    }
}

/// 获取 metamorphosis 重写建议列表（不修改原始 SQL）。
pub fn get_suggestions(sql: &str) -> Vec<(String, String)> {
    let mut suggestions = Vec::new();

    let (stmt_infos, _) = Parser::parse_sql(sql);
    if stmt_infos.is_empty() {
        return suggestions;
    }

    let all_rules = metamorphosis_rules::builtin_rules();
    let registry = RuleRegistry::new(all_rules);
    let engine = RewriteEngine::new(registry);

    let config = RewriteConfig::default();
    let ctx = RewriteContext {
        version: None,
        schema: None,
        config: &config,
        source_file: None,
        known_variables: None,
        diagnostic_hints: None,
    };

    let stmts: Vec<_> = stmt_infos.into_iter().map(|si| si.statement).collect();
    let result = engine.rewrite(&ctx, stmts);

    let formatter = SqlFormatter::new().pretty_print(true);
    for suggestion in &result.suggestions {
        let sql_text = match &suggestion.action {
            RewriteAction::Replace(stmt) | RewriteAction::Generate { stmt, .. } => formatter.format_statement(stmt),
            RewriteAction::Suggest { message, .. } => message.clone(),
            _ => continue,
        };
        suggestions.push((suggestion.rule_id.clone(), sql_text));
    }

    suggestions
}
