//! 去重：跨系统语义重叠发现（同一问题可能被多个审计子系统以不同 `rule_id` 检出）。
//!
//! 按 `(file_path, 等价组)` 分组，每组仅保留优先级最高的一条 Finding。
//! 优先级由 `DedupRule` 中 `canonical`（保留）> `also_fires`（抑制）定义。
//!
//! # 使用场景
//!
//! 例如 `SELECT *` 可能同时触发：
//! - `STATIC-SELECT-STAR`（cr-audit-static 自检，优先级最高）
//! - `JAVA-SQL-001`（cr-rules astgrep 规则）
//! - `LINT-R001`（ogsql-parser 泛化 linter）
//!
//! 去重后仅保留 `STATIC-SELECT-STAR`，其余被静默抑制。

use std::collections::HashMap;

use crate::types::Finding;

/// 一条去重规则：`canonical` 是等价组的保留 ID，`also_fires` 是该组被抑制的 ID。
///
/// 当同一 `file_path` 下有多个 Finding 的 `rule_id` 落入同一组时：
/// - 若 `canonical` 存在，其余丢弃
/// - 若 `canonical` 不存在，按 `also_fires` 的顺序保留第一个匹配的
#[derive(Debug, Clone)]
pub struct DedupRule {
    /// 该组语义问题的 canonical rule_id（优先级最高，保留它）
    pub canonical: &'static str,
    /// 该组中其他 rule_id（被抑制），按优先级降序排列
    pub also_fires: &'static [&'static str],
}

/// 返回内置的等效组表。
///
/// 优先级原则：`STATIC-*` > `JAVA-SQL-*` > `LINT-*`
/// - `STATIC-*`：cr-audit-static 自检规则，message 最精良
/// - `JAVA-SQL-*`：cr-rules astgrep 规则，有 Java 上下文
/// - `LINT-*`：ogsql-parser 泛化 linter，最通用、噪声最大
#[must_use]
pub fn builtin_groups() -> Vec<DedupRule> {
    vec![
        // G1: SELECT *
        DedupRule { canonical: "STATIC-SELECT-STAR", also_fires: &["JAVA-SQL-001", "LINT-R001"] },
        // G2: UPDATE 无 WHERE
        // JAVA-SQL-002 是 UPDATE 和 DELETE 的统一规则，在找不到 STATIC-* 时作为 fallback
        DedupRule { canonical: "STATIC-UPDATE-NO-WHERE", also_fires: &["JAVA-SQL-002", "LINT-C007"] },
        // G3: DELETE 无 WHERE
        DedupRule { canonical: "STATIC-DELETE-NO-WHERE", also_fires: &["JAVA-SQL-002", "LINT-C008"] },
        // G4: 隐式类型转换（EXPLAIN 实际执行 > 静态 lint 推断）
        DedupRule { canonical: "TYPE-001", also_fires: &["LINT-R005"] },
        // G5: 全表扫描 / 无索引查询（EXPLAIN 实际执行 > 静态 lint 推断）
        DedupRule { canonical: "SCAN-001", also_fires: &["LINT-R009"] },
    ]
}

/// 对 Findings 执行跨系统语义去重。
///
/// 算法：
/// 1. 构建 `rule_id → Vec<(group_idx, priority)>` 查找索引（一个 rule_id 可属多个组）
/// 2. 遍历，按 `(file_path, group_index)` 分组
/// 3. 每组内按优先级（canonical > also_fires 的顺序）保留第一条
/// 4. 未命中任何组的 finding 原样保留
#[must_use]
pub fn dedup_findings(findings: Vec<Finding>, groups: &[DedupRule]) -> Vec<Finding> {
    if groups.is_empty() {
        return findings;
    }

    // 构建 rule_id → Vec<(group_idx, priority)> 索引
    // priority: 0 = canonical, 1..N = also_fires 中的位置
    let mut rule_index: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for (g_idx, group) in groups.iter().enumerate() {
        rule_index.entry(group.canonical).or_default().push((g_idx, 0));
        for (a_idx, also) in group.also_fires.iter().enumerate() {
            rule_index.entry(also).or_default().push((g_idx, a_idx + 1));
        }
    }

    // 按 (file_path, group_index) 分组
    struct Candidate {
        finding: Finding,
        priority: usize,
    }

    let mut groups_map: HashMap<(String, usize), Vec<Candidate>> = HashMap::new();
    let mut ungrouped: Vec<Finding> = Vec::new();

    for f in findings {
        if let Some(entries) = rule_index.get(f.rule_id.as_str()) {
            // 一个 rule_id 可能属于多个组（如 JAVA-SQL-002 同时属于 G2 和 G3）
            // 优先加入 canonical 存在的组；若都不存在 canonical，加入优先级最高的组
            let mut best: Option<(usize, usize)> = None;
            for &(g_idx, priority) in entries {
                let has_canonical = groups_map
                    .get(&(f.file_path.clone(), g_idx))
                    .is_some_and(|candidates| candidates.iter().any(|c| c.priority == 0));
                if has_canonical {
                    // canonical 已存在，加入此组会被抑制，跳过
                    continue;
                }
                // 选优先级最高的组（priority 越小优先级越高）
                best = match best {
                    None => Some((g_idx, priority)),
                    Some((_, best_p)) if priority < best_p => Some((g_idx, priority)),
                    Some((best_g, best_p)) => Some((best_g, best_p)),
                };
            }
            if let Some((g_idx, priority)) = best {
                groups_map.entry((f.file_path.clone(), g_idx)).or_default().push(Candidate { finding: f, priority });
            } else {
                // 所有组都有 canonical，这条 finding 被完全抑制
            }
        } else {
            ungrouped.push(f);
        }
    }

    // 每组内按优先级排序，保留第一条
    let mut deduped: Vec<Finding> = Vec::with_capacity(ungrouped.len() + groups_map.len());
    for (_, mut candidates) in groups_map {
        candidates.sort_by_key(|c| c.priority);
        if let Some(best) = candidates.into_iter().next() {
            deduped.push(best.finding);
        }
    }

    deduped.extend(ungrouped);
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DiagnosticCategory, Severity};

    fn make_finding(rule_id: &str, file_path: &str) -> Finding {
        Finding::new(
            rule_id,
            Severity::Warning,
            DiagnosticCategory::General,
            "test",
            "test detail",
            file_path,
            None,
            None,
            None,
            None,
        )
    }

    /// 验证基本去重：canonical 存在时抑制 also_fires
    #[test]
    fn test_dedup_removes_also_fires() {
        let groups = vec![DedupRule { canonical: "STATIC-SELECT-STAR", also_fires: &["JAVA-SQL-001", "LINT-R001"] }];

        let findings = vec![
            make_finding("LINT-R001", "test.sql"),
            make_finding("STATIC-SELECT-STAR", "test.sql"),
            make_finding("JAVA-SQL-001", "test.sql"),
            make_finding("LINT-C007", "test.sql"), // 不在组中
        ];

        let result = dedup_findings(findings, &groups);
        assert_eq!(result.len(), 2, "should keep static + LINT-C007");

        let ids: Vec<&str> = result.iter().map(|f| f.rule_id.as_str()).collect();
        assert!(ids.contains(&"STATIC-SELECT-STAR"));
        assert!(ids.contains(&"LINT-C007"));
    }

    /// canonical 不存在时，保留 also_fires 中的第一条
    #[test]
    fn test_dedup_fallback_to_also_fires() {
        let groups = vec![DedupRule { canonical: "STATIC-SELECT-STAR", also_fires: &["JAVA-SQL-001", "LINT-R001"] }];

        let findings = vec![make_finding("LINT-R001", "test.sql"), make_finding("JAVA-SQL-001", "test.sql")];

        let result = dedup_findings(findings, &groups);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rule_id, "JAVA-SQL-001");
    }

    /// 不同 file_path 各自独立去重
    #[test]
    fn test_dedup_respects_file_path() {
        let groups = vec![DedupRule { canonical: "STATIC-SELECT-STAR", also_fires: &["LINT-R001"] }];

        let findings = vec![
            make_finding("LINT-R001", "a.sql"),
            make_finding("LINT-R001", "b.sql"),
            make_finding("STATIC-SELECT-STAR", "a.sql"),
        ];

        let result = dedup_findings(findings, &groups);
        assert_eq!(result.len(), 2);

        // a.sql 只保留 STATIC-SELECT-STAR
        // b.sql 只保留 LINT-R001（因为 b.sql 没有 canonical）
        for f in &result {
            if f.file_path == "a.sql" {
                assert_eq!(f.rule_id, "STATIC-SELECT-STAR");
            } else if f.file_path == "b.sql" {
                assert_eq!(f.rule_id, "LINT-R001");
            } else {
                panic!("unexpected file_path");
            }
        }
    }

    /// 不在任意组中的 finding 原样保留
    #[test]
    fn test_dedup_preserves_ungrouped() {
        let groups = vec![DedupRule { canonical: "STATIC-SELECT-STAR", also_fires: &["LINT-R001"] }];

        let findings = vec![make_finding("TYPE-001", "test.sql"), make_finding("COMPLEX-001", "test.sql")];

        let result = dedup_findings(findings, &groups);
        assert_eq!(result.len(), 2);
    }

    /// 空 groups 应原样返回
    #[test]
    fn test_dedup_empty_groups() {
        let findings = vec![make_finding("R001", "test.sql")];
        let result = dedup_findings(findings, &[]);
        assert_eq!(result.len(), 1);
    }

    /// 空 findings 应返回空
    #[test]
    fn test_dedup_empty_findings() {
        let groups = builtin_groups();
        let result = dedup_findings(vec![], &groups);
        assert!(result.is_empty());
    }

    /// 使用内置等价组表验证完整数据流
    #[test]
    fn test_dedup_with_builtin_groups() {
        let groups = builtin_groups();

        let findings = vec![
            // G1: SELECT * — 出三条，只保留 STATIC-SELECT-STAR
            make_finding("LINT-R001", "query.sql"),
            make_finding("STATIC-SELECT-STAR", "query.sql"),
            make_finding("JAVA-SQL-001", "query.sql"),
            // G4: 隐式类型转换 — 出两条，只保留 TYPE-001
            make_finding("LINT-R005", "query.sql"),
            make_finding("TYPE-001", "query.sql"),
            // 不在组中的正常保留
            make_finding("PARSE-SYNTAX", "query.sql"),
        ];

        let result = dedup_findings(findings, &groups);

        let ids: Vec<&str> = {
            let mut v: Vec<&str> = result.iter().map(|f| f.rule_id.as_str()).collect();
            v.sort_unstable();
            v
        };

        assert_eq!(ids, vec!["PARSE-SYNTAX", "STATIC-SELECT-STAR", "TYPE-001"]);
    }

    /// 验证 G2/G3：STATIC-UPDATE-NO-WHERE 抑制 LINT-C007，STATIC-DELETE-NO-WHERE 抑制 LINT-C008
    #[test]
    fn test_dedup_update_delete_no_where() {
        let groups = builtin_groups();

        let findings = vec![
            make_finding("STATIC-UPDATE-NO-WHERE", "query.sql"),
            make_finding("LINT-C007", "query.sql"),
            make_finding("STATIC-DELETE-NO-WHERE", "query.sql"),
            make_finding("LINT-C008", "query.sql"),
        ];

        let result = dedup_findings(findings, &groups);
        let ids: Vec<&str> = {
            let mut v: Vec<&str> = result.iter().map(|f| f.rule_id.as_str()).collect();
            v.sort_unstable();
            v
        };

        assert_eq!(
            ids,
            vec!["STATIC-DELETE-NO-WHERE", "STATIC-UPDATE-NO-WHERE"],
            "STATIC-* should suppress LINT-C007/C008"
        );
    }

    /// 回归：STATIC-UPDATE-NO-WHERE 不存在时，JAVA-SQL-002 作为 fallback 抑制 LINT-C007
    #[test]
    fn test_dedup_update_fallback_to_java_sql() {
        let groups = builtin_groups();

        let findings = vec![make_finding("JAVA-SQL-002", "query.sql"), make_finding("LINT-C007", "query.sql")];

        let result = dedup_findings(findings, &groups);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].rule_id, "JAVA-SQL-002");
    }
}
