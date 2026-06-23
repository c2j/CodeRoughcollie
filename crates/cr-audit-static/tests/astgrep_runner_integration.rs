//! 端到端集成测试：调用真实 astgrep 二进制。
//!
//! 默认 `cargo test` 跳过；显式运行：
//! ```sh
//! ASTGREP_BIN=/path/to/astgrep \
//! CR_RULES_ROOT=/path/to/lib/cr-rules \
//! cargo test --test astgrep_runner_integration -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use cr_audit_static::{audit_files_with_astgrep, AstgrepOptions};

fn env_required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} 必须设置"))
}

fn fixture_secure_random(rules_root: &str) -> PathBuf {
    PathBuf::from(rules_root).join("java/security/cases/secure_random/JAVA-SEC-017_secure_random.java")
}

#[test]
#[ignore = "需要 ASTGREP_BIN 与 CR_RULES_ROOT 环境变量"]
fn end_to_end_java_security() {
    let bin = env_required("ASTGREP_BIN");
    let rules_root = env_required("CR_RULES_ROOT");
    let target = fixture_secure_random(&rules_root);
    assert!(target.is_file(), "fixture 缺失: {}", target.display());

    let options = AstgrepOptions {
        binary_path: Some(PathBuf::from(&bin)),
        rules_root: Some(PathBuf::from(&rules_root)),
        language: "java".into(),
        severity_threshold: "warning".into(),
        timeout_secs: 30,
    };
    let findings =
        audit_files_with_astgrep(&[target], &["java/security".to_string()], &options).expect("astgrep runner 应成功");

    assert!(!findings.is_empty(), "secure_random fixture 必触发规则");
    assert!(
        findings.iter().any(|f| f.rule_id.contains("secure-random")),
        "应包含 secure-random 规则，实际: {:?}",
        findings.iter().map(|f| &f.rule_id).collect::<Vec<_>>()
    );
    let sample = &findings[0];
    assert!(sample.severity == cr_core::Severity::Critical, "ERROR 应映射为 Critical");
    assert!(sample.node_line.is_some(), "start_line 应填到 node_line");
    assert!(matches!(sample.category, cr_core::DiagnosticCategory::General));
}

#[test]
#[ignore = "需要 ASTGREP_BIN 与 CR_RULES_ROOT 环境变量"]
fn batch_multiple_files() {
    let bin = env_required("ASTGREP_BIN");
    let rules_root = env_required("CR_RULES_ROOT");
    let cases_dir = PathBuf::from(&rules_root).join("java/security/cases");

    let files: Vec<PathBuf> = ["secure_random", "hardcoded_passwords"]
        .iter()
        .filter_map(|name| {
            let d = cases_dir.join(name);
            std::fs::read_dir(&d).ok()?.flatten().find(|e| {
                let p = e.path();
                p.extension().and_then(|x| x.to_str()) == Some("java") && !p.to_string_lossy().contains(".neg.")
            })
        })
        .map(|e| e.path())
        .collect();
    assert!(files.len() >= 2, "需要至少 2 个 fixture");

    let options = AstgrepOptions {
        binary_path: Some(PathBuf::from(&bin)),
        rules_root: Some(PathBuf::from(&rules_root)),
        language: "java".into(),
        severity_threshold: "info".into(),
        timeout_secs: 60,
    };
    let findings = audit_files_with_astgrep(&files, &["java/security".to_string()], &options).expect("批量扫描应成功");
    assert!(!findings.is_empty(), "至少一个 fixture 应触发规则");
    let rule_ids: std::collections::HashSet<&str> = findings.iter().map(|f| f.rule_id.as_str()).collect();
    eprintln!("batch 触发的规则: {:?}", rule_ids);
}
