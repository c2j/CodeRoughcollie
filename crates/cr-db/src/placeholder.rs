//! SQL placeholder inference for EXPLAIN execution.
//!
//! When executing `EXPLAIN` on SQL containing parameter placeholders, the
//! database cannot execute them directly. This module replaces placeholders
//! with type-appropriate default values so that a valid SQL statement is
//! produced for planning.
//!
//! # Supported placeholder styles
//!
//! | Style | Example | Replacement |
//! |-------|---------|-------------|
//! | MyBatis named | `#{userId}` | Type-inferred default |
//! | MyBatis raw substitution | `${tableName}` | `'placeholder'` |
//! | JDBC positional | `?` | `'placeholder'` |
//! | PostgreSQL numbered | `$1`, `$2` | `'placeholder'` |
//!
//! Stored-procedure variables (`v_variable`) are left as-is — they cannot
//! be reliably distinguished from regular identifiers without full PL/pgSQL
//! parsing.

use regex::Regex;

/// Replace SQL parameter placeholders with type-inferred default values.
///
/// Handles `#{param}`, `${param}`, `?`, and `$N` placeholders in a single pass
/// (applied in that order to avoid cross-contamination).
///
/// # Examples
///
/// ```
/// use cr_db::placeholder::fill_placeholders;
///
/// // MyBatis named — integer-like name
/// let sql = fill_placeholders("SELECT * FROM users WHERE id = #{userId}");
/// assert_eq!(sql, "SELECT * FROM users WHERE id = 1");
///
/// // MyBatis named — date-like name
/// let sql = fill_placeholders("SELECT * FROM logs WHERE ts = #{createTime}");
/// assert!(sql.contains("2024-01-01"));
///
/// // JDBC positional
/// let sql = fill_placeholders("SELECT * FROM t WHERE a = ? AND b = ?");
/// assert_eq!(sql, "SELECT * FROM t WHERE a = 'placeholder' AND b = 'placeholder'");
/// ```
#[must_use]
pub fn fill_placeholders(sql: &str) -> String {
    // Step 1: MyBatis named params  #{paramName}
    let re_named = Regex::new(r"#\{(\w+)\}").expect("Invalid regex: named param");
    let sql = re_named
        .replace_all(sql, |caps: &regex::Captures<'_>| {
            default_for_param(caps.get(1).expect("capture group 1").as_str())
        })
        .into_owned();

    // Step 2: MyBatis raw substitution  ${paramName}
    let re_raw = Regex::new(r"\$\{(\w+)\}").expect("Invalid regex: raw substitution");
    let sql = re_raw.replace_all(&sql, |_caps: &regex::Captures<'_>| "'placeholder'").into_owned();

    // Step 3: PostgreSQL numbered  $1, $2, ...
    let re_pg = Regex::new(r"\$(\d+)").expect("Invalid regex: pg numbered");
    let sql = re_pg.replace_all(&sql, |_caps: &regex::Captures<'_>| "'placeholder'").into_owned();

    // Step 4: JDBC positional  ?
    let re_q = Regex::new(r"\?").expect("Invalid regex: positional");
    re_q.replace_all(&sql, "'placeholder'").into_owned()
}

/// Detect whether SQL contains any placeholders.
///
/// Returns `true` if the string contains `#{`, `${`, `?`, or `$` followed
/// by a digit (PostgreSQL positional).
///
/// # Examples
///
/// ```
/// use cr_db::placeholder::has_placeholders;
///
/// assert!(has_placeholders("SELECT * FROM t WHERE id = #{id}"));
/// assert!(has_placeholders("SELECT * FROM t WHERE id = ?"));
/// assert!(!has_placeholders("SELECT * FROM t WHERE id = 1"));
/// ```
#[must_use]
pub fn has_placeholders(sql: &str) -> bool {
    count_placeholders(sql) > 0
}

/// Count the number of placeholders in SQL.
///
/// Counts all `#{…}`, `${…}`, `?`, and `$N` occurrences.
///
/// # Examples
///
/// ```
/// use cr_db::placeholder::count_placeholders;
///
/// assert_eq!(count_placeholders("SELECT * FROM t WHERE a = #{a} AND b = ?"), 2);
/// assert_eq!(count_placeholders("SELECT * FROM t WHERE id = 1"), 0);
/// ```
#[must_use]
pub fn count_placeholders(sql: &str) -> usize {
    let re_named = Regex::new(r"#\{\w+\}").expect("Invalid regex: named param");
    let re_raw = Regex::new(r"\$\{\w+\}").expect("Invalid regex: raw subst");
    let re_pg = Regex::new(r"\$\d+").expect("Invalid regex: pg numbered");
    let re_q = Regex::new(r"\?").expect("Invalid regex: positional");

    re_named.find_iter(sql).count()
        + re_raw.find_iter(sql).count()
        + re_pg.find_iter(sql).count()
        + re_q.find_iter(sql).count()
}

/// Infer a default SQL literal value from a parameter name.
///
/// Uses heuristics based on common naming conventions — no schema information
/// is available at EXPLAIN time.
///
/// Both `snake_case` and `camelCase` names are recognised by normalising
/// camelCase boundaries into underscores before matching.
fn default_for_param(name: &str) -> String {
    let normalised = normalise_param_name(name);

    // Integer-like names
    if matches!(
        normalised.as_str(),
        "id" | "ids"
            | "count"
            | "num"
            | "size"
            | "page"
            | "limit"
            | "offset"
            | "number"
            | "amount"
            | "total"
            | "index"
            | "priority"
            | "level"
            | "version"
            | "parent_id"
            | "user_id"
            | "role_id"
            | "type_id"
    ) {
        return "1".to_string();
    }

    // Date-like names
    if matches!(
        normalised.as_str(),
        "date"
            | "dates"
            | "time"
            | "since"
            | "create_time"
            | "update_time"
            | "created_at"
            | "updated_at"
            | "create_date"
            | "update_date"
            | "timestamp"
            | "start_date"
            | "end_date"
            | "start_time"
            | "end_time"
            | "from_date"
            | "to_date"
    ) {
        return "'2024-01-01'".to_string();
    }

    // Boolean-like names
    if matches!(
        normalised.as_str(),
        "flag"
            | "flags"
            | "status"
            | "enabled"
            | "active"
            | "is_active"
            | "is_enabled"
            | "visible"
            | "deleted"
            | "is_deleted"
            | "locked"
            | "completed"
    ) {
        return "true".to_string();
    }

    // Fallback: safe string literal
    "'placeholder'".to_string()
}

/// Convert camelCase identifiers to snake_case for lookup.
///
/// Examples:
/// - `userId` → `user_id`
/// - `createTime` → `create_time`
/// - `isActive` → `is_active`
/// - `id` → `id`
fn normalise_param_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    for (i, ch) in name.char_indices() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            for lower_ch in ch.to_lowercase() {
                result.push(lower_ch);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── MyBatis named ────────────────────────────────────────────────────

    #[test]
    fn mybatis_integer_like() {
        let cases = [
            ("id", "SELECT * FROM users WHERE id = #{id}"),
            ("count", "SELECT count FROM t WHERE cnt = #{count}"),
            ("page", "SELECT * FROM t LIMIT #{page}"),
            ("offset", "SELECT * FROM t OFFSET #{offset}"),
            ("userId", "SELECT * FROM users WHERE id = #{userId}"),
        ];
        for (_name, sql) in &cases {
            let result = fill_placeholders(sql);
            assert!(result.contains(" 1"), "Expected integer-like default for {sql}: got {result}");
        }
    }

    #[test]
    fn mybatis_date_like() {
        let cases = [
            ("createTime", "SELECT * FROM logs WHERE ts = #{createTime}"),
            ("updateTime", "SELECT * FROM logs WHERE ts = #{updateTime}"),
            ("date", "SELECT * FROM t WHERE d = #{date}"),
        ];
        for (_name, sql) in &cases {
            let result = fill_placeholders(sql);
            assert!(result.contains("2024-01-01"), "Expected date-like default for {sql}: got {result}");
        }
    }

    #[test]
    fn mybatis_boolean_like() {
        let cases = [
            ("flag", "SELECT * FROM t WHERE f = #{flag}"),
            ("status", "SELECT * FROM t WHERE s = #{status}"),
            ("enabled", "SELECT * FROM t WHERE e = #{enabled}"),
            ("active", "SELECT * FROM t WHERE a = #{active}"),
        ];
        for (_name, sql) in &cases {
            let result = fill_placeholders(sql);
            assert!(result.contains(" true"), "Expected boolean-like default for {sql}: got {result}");
        }
    }

    #[test]
    fn mybatis_unknown_name_defaults_to_placeholder() {
        let sql = "SELECT * FROM t WHERE msg = #{message}";
        let result = fill_placeholders(sql);
        assert!(result.contains("'placeholder'"));
    }

    // ── MyBatis raw substitution ─────────────────────────────────────────

    #[test]
    fn mybatis_raw_substitution() {
        let sql = "SELECT * FROM ${tableName} WHERE id = 1";
        let result = fill_placeholders(sql);
        assert_eq!(result, "SELECT * FROM 'placeholder' WHERE id = 1");
    }

    // ── JDBC positional ──────────────────────────────────────────────────

    #[test]
    fn jdbc_positional_single() {
        let sql = "SELECT * FROM users WHERE id = ?";
        let result = fill_placeholders(sql);
        assert_eq!(result, "SELECT * FROM users WHERE id = 'placeholder'");
    }

    #[test]
    fn jdbc_positional_multiple() {
        let sql = "SELECT * FROM users WHERE id = ? AND name = ?";
        let result = fill_placeholders(sql);
        assert_eq!(result, "SELECT * FROM users WHERE id = 'placeholder' AND name = 'placeholder'");
    }

    // ── PostgreSQL numbered ──────────────────────────────────────────────

    #[test]
    fn pg_numbered_single() {
        let sql = "SELECT * FROM t WHERE created > $1";
        let result = fill_placeholders(sql);
        assert_eq!(result, "SELECT * FROM t WHERE created > 'placeholder'");
    }

    #[test]
    fn pg_numbered_multiple() {
        let sql = "SELECT * FROM t WHERE a = $1 AND b = $2";
        let result = fill_placeholders(sql);
        assert_eq!(result, "SELECT * FROM t WHERE a = 'placeholder' AND b = 'placeholder'");
    }

    // ── Mixed placeholders ───────────────────────────────────────────────

    #[test]
    fn mixed_placeholders() {
        let sql = "SELECT #{col} FROM t WHERE id = ? AND name = ${table} AND age > $1";
        let result = fill_placeholders(sql);
        assert_eq!(
            result,
            "SELECT 'placeholder' FROM t WHERE id = 'placeholder' AND name = 'placeholder' AND age > 'placeholder'"
        );
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    #[test]
    fn no_placeholders() {
        let sql = "SELECT * FROM users WHERE id = 1";
        assert_eq!(fill_placeholders(sql), sql);
    }

    #[test]
    fn empty_string() {
        assert_eq!(fill_placeholders(""), "");
    }

    // ── has_placeholders / count_placeholders ────────────────────────────

    #[test]
    fn detect_and_count() {
        assert!(!has_placeholders("SELECT 1"));
        assert!(has_placeholders("WHERE id = #{id}"));
        assert!(has_placeholders("WHERE id = ?"));
        assert!(has_placeholders("WHERE id = $1"));
        assert!(has_placeholders("WHERE name = ${x}"));

        assert_eq!(count_placeholders("SELECT 1"), 0);
        assert_eq!(count_placeholders("WHERE a = #{a} AND b = ?"), 2);
        assert_eq!(count_placeholders("WHERE a = $1 AND b = $2 AND c = ?"), 3);
    }

    // ── Realistic SQL fragments ──────────────────────────────────────────

    #[test]
    fn realistic_mybatis_select() {
        let sql = "\
SELECT u.id, u.name, u.email
FROM users u
WHERE u.id = #{userId}
  AND u.status = #{status}
  AND u.created_at > #{since}
ORDER BY u.id
LIMIT #{limit} OFFSET #{offset}";
        let result = fill_placeholders(sql);
        assert!(result.contains("u.id = 1"), "Expected integer for userId");
        assert!(result.contains("u.status = true"), "Expected boolean for status");
        assert!(result.contains("u.created_at > '2024-01-01'"), "Expected date for since");
        assert!(result.contains("LIMIT 1"), "Expected integer for limit");
        assert!(result.contains("OFFSET 1"), "Expected integer for offset");
    }
}
