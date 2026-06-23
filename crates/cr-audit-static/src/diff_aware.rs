//! 行级 diff-aware 过滤：把 astgrep findings 与 git diff hunks 求交。
//!
//! 仅保留落在新增行区间内的 findings，PR 增量审核场景的关键降噪手段。
//! 无 diff 信息的文件按保守策略保留全部 findings（避免漏报）。

use std::collections::HashMap;

use cr_core::Finding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hunk {
    pub new_start: usize,
    pub new_end: usize,
}

/// 从 `git diff -U0` 输出解析 hunks（仅取新增侧行范围）。
/// 形如 `@@ -10,5 +12,8 @@` → `Hunk { new_start: 12, new_end: 19 }`。
pub fn parse_hunks(diff_text: &str) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    for line in diff_text.lines() {
        let Some(rest) = line.strip_prefix("@@ ") else {
            continue;
        };
        let Some(plus_pos) = rest.find('+') else {
            continue;
        };
        let after_plus = &rest[plus_pos + 1..];
        let end = after_plus.find(|c: char| c.is_whitespace() || c == '@').unwrap_or(after_plus.len());
        let spec = &after_plus[..end];
        let (start_str, len_str) = spec.split_once(',').unwrap_or((spec, "1"));
        let (Ok(start), Ok(len)) = (start_str.parse::<usize>(), len_str.parse::<usize>()) else {
            continue;
        };
        let new_end = if len == 0 { start } else { start + len - 1 };
        if start > 0 {
            hunks.push(Hunk { new_start: start, new_end });
        }
    }
    hunks
}

pub fn finding_in_hunks(node_line: Option<usize>, hunks: &[Hunk]) -> bool {
    let Some(line) = node_line else {
        return false;
    };
    hunks.iter().any(|h| line >= h.new_start && line <= h.new_end)
}

/// 按 file_path 查表过滤 findings：
/// - 文件无 diff 信息（不在 map）：保留（保守）
/// - 文件 diff 为空（hunks 空 vec）：全部丢弃（明确无变更）
/// - 否则按 hunk 区间过滤
pub fn filter_findings_to_diff(findings: Vec<Finding>, file_hunks: &HashMap<String, Vec<Hunk>>) -> Vec<Finding> {
    findings.into_iter().filter(|f| match file_hunks.get(&f.file_path) {
        None => true,
        Some(hunks) => finding_in_hunks(f.node_line, hunks),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::{DiagnosticCategory, Finding, Severity};

    fn make_finding(file: &str, line: usize) -> Finding {
        Finding::new(
            "TEST",
            Severity::Warning,
            DiagnosticCategory::General,
            "t",
            "d",
            file,
            None,
            Some(line),
            None,
            None,
        )
    }

    #[test]
    fn parses_typical_unified_diff() {
        let diff = "\
diff --git a/Foo.java b/Foo.java
index 123..456 100644
--- a/Foo.java
+++ b/Foo.java
@@ -10,5 +12,8 @@ class Foo
     line1
     line2
+new1
+new2
     line3
@@ -30,3 +45,2 @@ class Foo
-old1
-old2
     keep
";
        let hunks = parse_hunks(diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0], Hunk { new_start: 12, new_end: 19 });
        assert_eq!(hunks[1], Hunk { new_start: 45, new_end: 46 });
    }

    #[test]
    fn parses_hunk_with_implicit_len() {
        let hunks = parse_hunks("@@ -5 +7 @@ ctx");
        assert_eq!(hunks, vec![Hunk { new_start: 7, new_end: 7 }]);
    }

    #[test]
    fn ignores_malformed_hunk_header() {
        assert!(parse_hunks("@@ garbage").is_empty());
        assert!(parse_hunks("not a hunk").is_empty());
    }

    #[test]
    fn finding_in_hunks_basic() {
        let hunks = vec![Hunk { new_start: 10, new_end: 20 }];
        assert!(finding_in_hunks(Some(10), &hunks));
        assert!(finding_in_hunks(Some(15), &hunks));
        assert!(finding_in_hunks(Some(20), &hunks));
        assert!(!finding_in_hunks(Some(9), &hunks));
        assert!(!finding_in_hunks(Some(21), &hunks));
        assert!(!finding_in_hunks(None, &hunks));
    }

    #[test]
    fn filter_preserves_unknown_files_drops_known_clean() {
        let findings = vec![
            make_finding("A.java", 5),
            make_finding("B.java", 5),
            make_finding("C.java", 5),
            make_finding("D.java", 100),
        ];
        let mut map = HashMap::new();
        map.insert("B.java".to_string(), Vec::new());
        map.insert("C.java".to_string(), vec![Hunk { new_start: 1, new_end: 10 }]);
        map.insert("D.java".to_string(), vec![Hunk { new_start: 1, new_end: 10 }]);
        let filtered = filter_findings_to_diff(findings, &map);
        let files: Vec<_> = filtered.iter().map(|f| f.file_path.as_str()).collect();
        assert_eq!(files, vec!["A.java", "C.java"]);
    }

    #[test]
    fn parse_real_world_mybatis_diff() {
        let diff = "\
diff --git a/src/Mapper.xml b/src/Mapper.xml
@@ -1,4 +1,5 @@
 <mapper>
+  <select id=\"new\">${unsafe}</select>
   <select id=\"old\">#{safe}</select>
 </mapper>
";
        let hunks = parse_hunks(diff);
        assert_eq!(hunks, vec![Hunk { new_start: 1, new_end: 5 }]);
    }
}
