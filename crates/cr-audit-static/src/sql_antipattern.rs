//! SQL 反模式静态检测。
//!
//! 通过解析 SQL 并遍历 AST，检测常见的 SQL 反模式：
//!
//! - `STATIC-SELECT-STAR`: 使用 `SELECT *`（Warning）
//! - `STATIC-DELETE-NO-WHERE`: `DELETE` 无 `WHERE` 子句（Critical）
//! - `STATIC-UPDATE-NO-WHERE`: `UPDATE` 无 `WHERE` 子句（Critical）

use cr_core::{DiagnosticCategory, Finding, Severity};
use ogsql_parser::ast::visitor::{walk_statement, Visitor, VisitorResult};
use ogsql_parser::ast::SelectTarget;
use ogsql_parser::{Parser, Statement, Tokenizer};

/// 对单条或多条 SQL 进行静态反模式检测。
///
/// 解析输入的 SQL 文本，对每一条语句进行 AST 遍历，返回所有检测到的反模式发现。
/// 解析失败时返回空列表。
///
/// # 示例
///
/// ```
/// use cr_audit_static::audit_sql;
///
/// let findings = audit_sql("SELECT * FROM users");
/// assert_eq!(findings.len(), 1);
/// assert_eq!(findings[0].rule_id, "STATIC-SELECT-STAR");
/// ```
#[must_use]
pub fn audit_sql(sql: &str) -> Vec<Finding> {
    let tokens = match Tokenizer::new(sql).tokenize() {
        Ok(tokens) => tokens,
        Err(_) => return Vec::new(),
    };

    let mut parser = Parser::new(tokens);
    let statements = parser.parse();

    let mut visitor = SqlAntiPatternVisitor::default();
    for stmt in &statements {
        walk_statement(&mut visitor, stmt);
    }

    visitor.findings
}

/// SQL 反模式检测访问者。
///
/// 通过标准 `walk_statement` 遍历，在各回调中检测反模式。
#[derive(Default)]
struct SqlAntiPatternVisitor {
    findings: Vec<Finding>,
    /// 当前语句的源码位置（从 `visit_statement` 中提取，供无 span 参数的回调使用）。
    current_span: Option<ogsql_parser::ast::SourceSpan>,
}

impl SqlAntiPatternVisitor {
    fn check_select_star(&mut self, select: &ogsql_parser::SelectStatement) {
        for target in &select.targets {
            let has_star = match target {
                SelectTarget::Star(_) => true,
                SelectTarget::Expr(expr, _) => {
                    matches!(expr, ogsql_parser::Expr::QualifiedStar(_))
                }
            };
            if has_star {
                let line = self.current_span.as_ref().map(|s| s.start.line);
                self.findings.push(Finding::new(
                    "STATIC-SELECT-STAR",
                    Severity::Warning,
                    DiagnosticCategory::ScanEfficiency,
                    "使用 SELECT * 的查询",
                    "SELECT * 会检索所有列，可能导致不必要的数据传输和内存开销。\
                     当表结构变更时，查询结果集也会随之变化，可能导致隐式兼容性问题。",
                    line,
                    Some("SelectStatement".into()),
                    Some("明确列出需要的列名，避免使用 SELECT *。例如：SELECT id, name, email FROM ...".into()),
                ));
            }
        }
    }
}

impl Visitor for SqlAntiPatternVisitor {
    fn visit_statement(&mut self, stmt: &Statement) -> VisitorResult {
        match stmt {
            Statement::Select(spanned) => {
                self.current_span = spanned.span.clone();
            }
            Statement::Delete(spanned) => {
                self.current_span = spanned.span.clone();
            }
            Statement::Update(spanned) => {
                self.current_span = spanned.span.clone();
            }
            _ => {}
        }
        VisitorResult::Continue
    }

    fn visit_select(&mut self, select: &ogsql_parser::SelectStatement) -> VisitorResult {
        self.check_select_star(select);
        VisitorResult::Continue
    }

    fn visit_delete(&mut self, delete: &ogsql_parser::DeleteStatement) -> VisitorResult {
        if delete.where_clause.is_none() {
            let line = self.current_span.as_ref().map(|s| s.start.line);
            self.findings.push(Finding::new(
                "STATIC-DELETE-NO-WHERE",
                Severity::Critical,
                DiagnosticCategory::General,
                "DELETE 语句缺少 WHERE 子句",
                "DELETE 语句没有 WHERE 条件将删除表中所有行，这很可能是非预期的操作。\
                 即使确实需要清空表，也应使用 TRUNCATE 以获得更好的性能。",
                line,
                Some("DeleteStatement".into()),
                Some("添加 WHERE 子句限定删除范围。如需删除所有行，请改用 TRUNCATE TABLE。".into()),
            ));
        }
        VisitorResult::Continue
    }

    fn visit_update(&mut self, update: &ogsql_parser::UpdateStatement) -> VisitorResult {
        if update.where_clause.is_none() {
            let line = self.current_span.as_ref().map(|s| s.start.line);
            self.findings.push(Finding::new(
                "STATIC-UPDATE-NO-WHERE",
                Severity::Critical,
                DiagnosticCategory::General,
                "UPDATE 语句缺少 WHERE 子句",
                "UPDATE 语句没有 WHERE 条件将更新表中所有行，这很可能是非预期的操作。",
                line,
                Some("UpdateStatement".into()),
                Some("添加 WHERE 子句限定更新范围，避免全表更新。".into()),
            ));
        }
        VisitorResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_star_detected() {
        let findings = audit_sql("SELECT * FROM users");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "STATIC-SELECT-STAR");
        assert_eq!(findings[0].severity, Severity::Warning);
        assert_eq!(findings[0].category, DiagnosticCategory::ScanEfficiency);
    }

    #[test]
    fn test_select_star_with_table_prefix() {
        let findings = audit_sql("SELECT users.* FROM users");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "STATIC-SELECT-STAR");
    }

    #[test]
    fn test_select_specific_columns_no_finding() {
        let findings = audit_sql("SELECT id, name FROM users");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn test_delete_no_where() {
        let findings = audit_sql("DELETE FROM users");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "STATIC-DELETE-NO-WHERE");
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn test_delete_with_where_no_finding() {
        let findings = audit_sql("DELETE FROM users WHERE id = 1");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn test_update_no_where() {
        let findings = audit_sql("UPDATE users SET name = 'test'");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "STATIC-UPDATE-NO-WHERE");
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[test]
    fn test_update_with_where_no_finding() {
        let findings = audit_sql("UPDATE users SET name = 'test' WHERE id = 1");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn test_multiple_statements() {
        let findings =
            audit_sql("SELECT * FROM users;\nDELETE FROM orders;\nUPDATE products SET price = 10 WHERE id = 1;");
        assert_eq!(findings.len(), 2);
        let rule_ids: Vec<&str> = findings.iter().map(|f| f.rule_id.as_str()).collect();
        assert!(rule_ids.contains(&"STATIC-SELECT-STAR"));
        assert!(rule_ids.contains(&"STATIC-DELETE-NO-WHERE"));
    }

    #[test]
    fn test_invalid_sql_does_not_panic() {
        let findings = audit_sql("INVALID SQL &&&");
        assert!(findings.is_empty());
    }

    #[test]
    fn test_empty_sql() {
        let findings = audit_sql("");
        assert_eq!(findings.len(), 0);
    }

    #[test]
    fn test_select_star_in_subquery() {
        let findings = audit_sql("SELECT id FROM (SELECT * FROM users) AS u");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "STATIC-SELECT-STAR");
    }

    #[test]
    fn test_select_star_in_cte() {
        let findings = audit_sql("WITH cte AS (SELECT * FROM users) SELECT id FROM cte");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "STATIC-SELECT-STAR");
    }
}
