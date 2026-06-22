//! File type detection with extension + content dual verification.
//!
//! Detects whether a file is an SQL script, Java source, MyBatis XML mapper,
//! or unsupported, by checking both the file extension (whitelist) and the
//! file content (case-insensitive keyword matching). This dual-check policy
//! prevents misclassification of files like `pom.xml` or `web.xml` as MyBatis
//! mappers based solely on their `.xml` extension.
//!
//! # Dual-check rationale
//!
//! A naive dispatch that only looks at the file extension can route a
//! `pom.xml` through the MyBatis mapper audit path, producing false
//! positives. By verifying that the content actually contains
//! language-specific keywords, we trade a small amount of CPU time for
//! significantly higher classification accuracy.

use std::path::Path;

/// Kinds of files supported by the static audit system.
///
/// Detection uses a two-step policy:
/// 1. **Extension whitelist** — only a predefined set of extensions is
///    considered.
/// 2. **Content keywords** — the file content is searched for
///    language-specific keywords (case-insensitive). If the content is empty,
///    some kinds (e.g. [`Sql`](FileKind::Sql)) are still accepted because the
///    downstream audit can handle an empty file gracefully.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// SQL script — recognised extensions: `.sql`, `.prc`, `.pck`, `.pkb`,
    /// `.fnc`. Content must contain at least one SQL keyword, or empty
    /// content is accepted.
    Sql,
    /// Java source file — extension `.java`. Content must contain at least
    /// one Java keyword. Empty content yields [`Unsupported`](FileKind::Unsupported).
    Java,
    /// MyBatis (or iBATIS) XML mapper — extension `.xml`. Content must
    /// contain a MyBatis/iBATIS-specific keyword such as `<mapper>` or
    /// `<!DOCTYPE mapper>`.
    MyBatisXml,
    /// File type that is not recognised or whose content does not match the
    /// expected keywords for its extension.
    Unsupported,
}

// ── Keyword lists ──────────────────────────────────────────────────
//
// Each list defines the case-insensitive substrings that must appear in
// the file content for the corresponding FileKind.

/// SQL keywords (matched case-insensitively).
const SQL_KEYWORDS: &[&str] = &[
    "select",
    "insert",
    "update",
    "delete",
    "create",
    "drop",
    "alter",
    "begin",
    "declare",
    "procedure",
    "function",
    "trigger",
    "package",
];

/// Java keywords (matched case-insensitively).
///
/// Most entries include a trailing space to avoid false matches with
/// identifiers that happen to start with these words.
const JAVA_KEYWORDS: &[&str] = &["package ", "import ", "class ", "interface ", "enum ", "@interface", "record "];

/// MyBatis / iBATIS XML keywords (matched case-insensitively).
const MYBATIS_KEYWORDS: &[&str] =
    &["<mapper", "<!doctype mapper", "<sqlmap", "-//ibatis.apache.org//dtd sql map", "-//mybatis.org//dtd mapper"];

/// Recognised SQL-family extensions (checked case-insensitively).
const SQL_EXTENSIONS: &[&str] = &["sql", "prc", "pck", "pkb", "fnc"];

/// Detects the [`FileKind`] of a file based on its path and content.
///
/// # Dual-check policy
///
/// 1. The file extension must match one of the recognised extensions.
/// 2. For non-empty content, at least one language-specific keyword must be
///    found (case-insensitive). Empty content is accepted for SQL-family
///    files (`.sql`, `.prc`, `.pck`, `.pkb`, `.fnc`) because the downstream
///    audit handles empty input gracefully.
///
/// # Examples
///
/// ```rust
/// use std::path::Path;
/// use cr_audit_static::file_type::{detect, FileKind};
///
/// assert_eq!(detect(Path::new("query.sql"), "SELECT * FROM t"), FileKind::Sql);
/// assert_eq!(detect(Path::new("pom.xml"), "<project/>"), FileKind::Unsupported);
/// ```
pub fn detect(path: &Path, content: &str) -> FileKind {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e,
        None => return FileKind::Unsupported,
    };
    let ext_lower = ext.to_ascii_lowercase();

    // ── SQL-family extensions ────────────────────────────────────────
    if SQL_EXTENSIONS.contains(&ext_lower.as_str()) {
        // Empty content is accepted; the downstream audit handles it.
        if content.is_empty() {
            return FileKind::Sql;
        }
        let lower = content.to_ascii_lowercase();
        if contains_any(&lower, SQL_KEYWORDS) {
            return FileKind::Sql;
        }
        return FileKind::Unsupported;
    }

    // ── Java source ──────────────────────────────────────────────────
    if ext_lower == "java" {
        if content.is_empty() {
            return FileKind::Unsupported;
        }
        let lower = content.to_ascii_lowercase();
        if contains_any(&lower, JAVA_KEYWORDS) {
            return FileKind::Java;
        }
        return FileKind::Unsupported;
    }

    // ── XML (potential MyBatis mapper) ───────────────────────────────
    if ext_lower == "xml" {
        let lower = content.to_ascii_lowercase();
        if contains_any(&lower, MYBATIS_KEYWORDS) {
            return FileKind::MyBatisXml;
        }
        return FileKind::Unsupported;
    }

    FileKind::Unsupported
}

/// Returns `true` if `text` contains any of the given `keywords`.
fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|kw| text.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── Positive cases ──────────────────────────────────────────

    #[test]
    fn sql_extension_with_select() {
        assert_eq!(detect(Path::new("query.sql"), "SELECT * FROM users"), FileKind::Sql,);
    }

    #[test]
    fn prc_extension_with_create_procedure() {
        assert_eq!(detect(Path::new("sp_report.prc"), "CREATE OR REPLACE PROCEDURE sp_report",), FileKind::Sql,);
    }

    #[test]
    fn pck_extension_with_package() {
        assert_eq!(detect(Path::new("util.pck"), "PACKAGE util AS"), FileKind::Sql,);
    }

    #[test]
    fn java_extension_with_package() {
        assert_eq!(detect(Path::new("Foo.java"), "package com.example;\nclass Foo {}",), FileKind::Java,);
    }

    #[test]
    fn mapper_xml_with_mapper_namespace() {
        assert_eq!(
            detect(Path::new("UserMapper.xml"), "<mapper namespace=\"com.example.UserMapper\">",),
            FileKind::MyBatisXml,
        );
    }

    #[test]
    fn ibatis_xml_with_sqlmap() {
        assert_eq!(detect(Path::new("SqlMap.xml"), "<sqlMap namespace=\"User\">"), FileKind::MyBatisXml,);
    }

    #[test]
    fn mybatis_doctype() {
        assert_eq!(
            detect(
                Path::new("mapper.xml"),
                "<!DOCTYPE mapper PUBLIC \"-//mybatis.org//DTD Mapper 3.0//EN\" \
                 \"http://mybatis.org/dtd/mybatis-3-mapper.dtd\">",
            ),
            FileKind::MyBatisXml,
        );
    }

    // ── Negative cases ──────────────────────────────────────────

    #[test]
    fn pom_xml_is_unsupported() {
        assert_eq!(detect(Path::new("pom.xml"), "<project>...</project>"), FileKind::Unsupported,);
    }

    #[test]
    fn web_xml_is_unsupported() {
        assert_eq!(detect(Path::new("web.xml"), "<web-app>...</web-app>"), FileKind::Unsupported,);
    }

    #[test]
    fn readme_md_is_unsupported() {
        assert_eq!(detect(Path::new("README.md"), "# Project"), FileKind::Unsupported,);
    }

    #[test]
    fn java_empty_content_is_unsupported() {
        assert_eq!(detect(Path::new("Empty.java"), ""), FileKind::Unsupported,);
    }

    #[test]
    fn java_with_non_java_text_is_unsupported() {
        assert_eq!(
            detect(Path::new("data.java"), "just some random text without java keywords",),
            FileKind::Unsupported,
        );
    }

    // ── Edge cases ──────────────────────────────────────────────

    #[test]
    fn sql_empty_content_still_sql() {
        assert_eq!(detect(Path::new("empty.sql"), ""), FileKind::Sql,);
    }

    #[test]
    fn prc_empty_content_still_sql() {
        assert_eq!(detect(Path::new("empty.prc"), ""), FileKind::Sql,);
    }

    #[test]
    fn pkb_empty_content_still_sql() {
        assert_eq!(detect(Path::new("empty.pkb"), ""), FileKind::Sql,);
    }

    #[test]
    fn xml_with_ibatis_doctype() {
        assert_eq!(
            detect(
                Path::new("mapper.xml"),
                "<!DOCTYPE mapper PUBLIC \"-//ibatis.apache.org//DTD SQL Map 2.0//EN\" \
                 \"http://ibatis.apache.org/dtd/sql-map-2.dtd\">",
            ),
            FileKind::MyBatisXml,
        );
    }

    #[test]
    fn path_with_no_extension_returns_unsupported() {
        assert_eq!(detect(Path::new("Makefile"), "all: build"), FileKind::Unsupported,);
    }

    #[test]
    fn uppercase_extension_is_recognised() {
        assert_eq!(detect(Path::new("query.SQL"), "SELECT 1"), FileKind::Sql,);
    }

    #[test]
    fn fnc_extension_with_function() {
        assert_eq!(detect(Path::new("calc.fnc"), "FUNCTION calc RETURN NUMBER"), FileKind::Sql,);
    }

    #[test]
    fn sql_keyword_with_mixed_case() {
        assert_eq!(detect(Path::new("q.sql"), "TrIgGeR before_insert"), FileKind::Sql,);
    }

    #[test]
    fn java_interface_keyword() {
        assert_eq!(detect(Path::new("Service.java"), "public interface UserService {"), FileKind::Java,);
    }

    #[test]
    fn java_enum_keyword() {
        assert_eq!(detect(Path::new("Status.java"), "enum Status { ACTIVE, INACTIVE }"), FileKind::Java,);
    }

    #[test]
    fn xml_with_non_mybatis_content_is_unsupported() {
        assert_eq!(
            detect(Path::new("config.xml"), "<configuration><property name=\"foo\">bar</property></configuration>",),
            FileKind::Unsupported,
        );
    }
}
