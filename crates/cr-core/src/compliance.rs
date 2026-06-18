//! 合规规则包：PCI-DSS、SOC2、GDPR 规则定义与基于关键词的 SQL 合规检查。
//!
//! 每条合规规则定义代码 ID、严重度和描述；`check_compliance` 对输入的 SQL 文本
//! 执行正则/关键词匹配，返回所有命中的 `Finding`。

use std::collections::HashSet;

use regex::Regex;

use crate::types::{DiagnosticCategory, Finding, Severity};

// ---------------------------------------------------------------------------
// 规则包枚举
// ---------------------------------------------------------------------------

/// 合规规则包标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CompliancePack {
    /// PCI-DSS（支付卡行业数据安全标准）。
    PciDss,
    /// SOC2（服务组织控制 2 型）。
    Soc2,
    /// GDPR（通用数据保护条例）。
    Gdpr,
    /// HIPAA（健康保险可移植性和责任法案，预留）。
    Hipaa,
    /// 内部安全基线。
    InternalBaseline,
}

impl CompliancePack {
    /// 返回该合规包包含的所有规则。
    #[must_use]
    pub fn rules(&self) -> &'static [ComplianceRule] {
        match self {
            Self::PciDss => &PCI_DSS_RULES,
            Self::Soc2 => &SOC2_RULES,
            Self::Gdpr => &GDPR_RULES,
            Self::Hipaa => &HIPAA_RULES,
            Self::InternalBaseline => &INTERNAL_BASELINE_RULES,
        }
    }
}

// ---------------------------------------------------------------------------
// 规则定义
// ---------------------------------------------------------------------------

/// 单条合规规则。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ComplianceRule {
    /// 规则 ID，如 `"COMPLIANCE-PCI-001"`。
    pub id: &'static str,
    /// 规则描述。
    pub description: &'static str,
    /// 严重度。
    pub severity: Severity,
}

impl ComplianceRule {
    /// 创建一个合规规则（const 构造器，可在静态数组中内联使用）。
    #[must_use]
    pub const fn new(id: &'static str, description: &'static str, severity: Severity) -> Self {
        Self { id, description, severity }
    }
}

// ---------------------------------------------------------------------------
// 各包规则表
// ---------------------------------------------------------------------------

const PCI_DSS_RULES: [ComplianceRule; 2] = [
    ComplianceRule::new(
        "COMPLIANCE-PCI-001",
        "SQL 中包含疑似信用卡号（16 位数字或 4 开头的 15 位数字）—— 严禁明文存储 PAN",
        Severity::Critical,
    ),
    ComplianceRule::new(
        "COMPLIANCE-PCI-002",
        "SQL 中引用了未加密的敏感列（ssn、credit_card、cvv）—— 需加密存储",
        Severity::Critical,
    ),
];

const SOC2_RULES: [ComplianceRule; 2] = [
    ComplianceRule::new("COMPLIANCE-SOC2-001", "SQL 中包含 GRANT ALL —— 应遵循最小权限原则", Severity::Warning),
    ComplianceRule::new(
        "COMPLIANCE-SOC2-002",
        "DROP TABLE 未包含备份注释 —— 生产环境删除前需确认备份",
        Severity::Warning,
    ),
];

const GDPR_RULES: [ComplianceRule; 2] = [
    ComplianceRule::new(
        "COMPLIANCE-GDPR-001",
        "对 PII 表（user、customer、patient）执行 SELECT * —— 需按需取列",
        Severity::Critical,
    ),
    ComplianceRule::new(
        "COMPLIANCE-GDPR-002",
        "数据导出语句缺少 LIMIT 子句 —— 应限制导出量以防止数据泄露",
        Severity::Warning,
    ),
];

/// HIPAA 规则（预留，当前仅占位）。
const HIPAA_RULES: [ComplianceRule; 0] = [];

const INTERNAL_BASELINE_RULES: [ComplianceRule; 2] = [
    ComplianceRule::new(
        "COMPLIANCE-INT-001",
        "SQL 中包含硬编码凭据（password = '...'）—— 应使用密钥管理服务",
        Severity::Critical,
    ),
    ComplianceRule::new(
        "COMPLIANCE-INT-002",
        "TRUNCATE 出现在非测试上下文中 —— 生产环境需变更审批",
        Severity::Critical,
    ),
];

// ---------------------------------------------------------------------------
// 公共 API
// ---------------------------------------------------------------------------

/// 根据合规包列表获取所有去重的规则。
///
/// 同一规则 ID 只会出现一次（后出现的包中同名规则被跳过）。
#[must_use]
pub fn get_compliance_rules(packs: &[CompliancePack]) -> Vec<&'static ComplianceRule> {
    let mut seen: HashSet<&'static str> = HashSet::new();
    let mut rules = Vec::new();
    for pack in packs {
        for rule in pack.rules() {
            if seen.insert(rule.id) {
                rules.push(rule);
            }
        }
    }
    rules
}

/// 对 SQL 文本执行所选合规包的关键词/正则检查。
///
/// 返回所有命中的 `Finding`（规则 ID、严重度、描述、优化建议等均已填充）。
#[must_use]
pub fn check_compliance(sql: &str, packs: &[CompliancePack]) -> Vec<Finding> {
    let rules = get_compliance_rules(packs);
    let mut findings = Vec::new();

    for rule in &rules {
        if let Some(finding) = check_rule(sql, rule) {
            findings.push(finding);
        }
    }

    findings
}

// ---------------------------------------------------------------------------
// 内部检查函数
// ---------------------------------------------------------------------------

/// 对单条规则执行检查，若命中则返回 `Finding`，否则返回 `None`。
fn check_rule(sql: &str, rule: &ComplianceRule) -> Option<Finding> {
    match rule.id {
        "COMPLIANCE-PCI-001" => check_pci_001(sql),
        "COMPLIANCE-PCI-002" => check_pci_002(sql),
        "COMPLIANCE-SOC2-001" => check_soc2_001(sql),
        "COMPLIANCE-SOC2-002" => check_soc2_002(sql),
        "COMPLIANCE-GDPR-001" => check_gdpr_001(sql),
        "COMPLIANCE-GDPR-002" => check_gdpr_002(sql),
        "COMPLIANCE-INT-001" => check_int_001(sql),
        "COMPLIANCE-INT-002" => check_int_002(sql),
        _ => None,
    }
    .map(|detail| Finding {
        rule_id: rule.id.to_owned(),
        severity: rule.severity,
        category: DiagnosticCategory::General,
        title: rule.description.to_owned(),
        detail,
        node_line: None,
        node_type: None,
        suggestion: None,
    })
}

/// PCI-001：信用卡号模式。
fn check_pci_001(sql: &str) -> Option<String> {
    // 匹配 16 位纯数字 或 4 开头 + 15 位数字
    let re1 = Regex::new(r"[0-9]{16}").ok()?;
    let re2 = Regex::new(r"4[0-9]{15}").ok()?;
    if re1.is_match(sql) || re2.is_match(sql) {
        Some("检测到疑似信用卡号（16 位数字序列），严禁在 SQL 中明文存储 PAN".to_owned())
    } else {
        None
    }
}

/// PCI-002：未加密的敏感列访问。
fn check_pci_002(sql: &str) -> Option<String> {
    let re = Regex::new(r"(?i)\b(ssn|credit_card|cvv|pan|card_number)\b").ok()?;
    if re.is_match(sql) {
        Some("SQL 中引用了需加密的敏感列（ssn / credit_card / cvv），请确认列级加密已启用".to_owned())
    } else {
        None
    }
}

/// SOC2-001：GRANT ALL。
fn check_soc2_001(sql: &str) -> Option<String> {
    let re = Regex::new(r"(?i)GRANT\s+ALL").ok()?;
    if re.is_match(sql) {
        Some("GRANT ALL 授予了过高权限，请按最小权限原则改为 GRANT SELECT / INSERT / ...".to_owned())
    } else {
        None
    }
}

/// SOC2-002：DROP TABLE 未包含备份注释。
fn check_soc2_002(sql: &str) -> Option<String> {
    let drop_re = Regex::new(r"(?i)\bDROP\s+TABLE\b").ok()?;
    if !drop_re.is_match(sql) {
        return None;
    }
    // 检查是否包含备份相关的注释
    let backup_re = Regex::new(r"(?i)backup|备份").ok()?;
    if !backup_re.is_match(sql) {
        Some("DROP TABLE 未包含备份确认注释（/* backup */），生产环境禁止直接删除".to_owned())
    } else {
        None
    }
}

/// GDPR-001：对 PII 表 SELECT *。
fn check_gdpr_001(sql: &str) -> Option<String> {
    let re = Regex::new(r"(?i)SELECT\s+\*\s+FROM\s+.*\b(user|customer|patient)\b").ok()?;
    if re.is_match(sql) {
        Some("对 PII 表执行 SELECT * 可能泄露个人数据，请显式指定所需列".to_owned())
    } else {
        None
    }
}

/// GDPR-002：数据导出未加 LIMIT。
fn check_gdpr_002(sql: &str) -> Option<String> {
    // 检测导出模式：INSERT ... SELECT、COPY ... TO、INTO OUTFILE
    let export_re = Regex::new(r"(?i)(INSERT\s+INTO.*SELECT|COPY\s+.*\s+TO|INTO\s+OUTFILE)").ok()?;
    if !export_re.is_match(sql) {
        return None;
    }
    let limit_re = Regex::new(r"(?i)\bLIMIT\b").ok()?;
    if !limit_re.is_match(sql) {
        Some("数据导出语句缺少 LIMIT 子句，导出量不受控可能导致大规模数据泄露".to_owned())
    } else {
        None
    }
}

/// INT-001：硬编码凭据。
fn check_int_001(sql: &str) -> Option<String> {
    // 匹配 password = 'xxx' / password = "xxx" / password 'xxx'
    let re = Regex::new(r"(?i)password\s*[:=]?\s*['\u{2018}\u{2019}][^'\u{2018}\u{2019}]+['\u{2018}\u{2019}]").ok()?;
    if re.is_match(sql) {
        Some("SQL 中包含疑似硬编码的密码字面量，请使用参数绑定或密钥管理服务".to_owned())
    } else {
        None
    }
}

/// INT-002：非测试环境 TRUNCATE。
fn check_int_002(sql: &str) -> Option<String> {
    let re = Regex::new(r"(?i)\bTRUNCATE\s+TABLE\b").ok()?;
    if re.is_match(sql) {
        // 检查是否包含 test / 测试 上下文标识
        let test_re = Regex::new(r"(?i)\btest\b|测试").ok()?;
        if test_re.is_match(sql) {
            None // 在测试上下文中，允许
        } else {
            Some("TRUNCATE TABLE 出现在非测试上下文中，生产环境需审批后方可执行".to_owned())
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_001_detected() {
        let findings =
            check_compliance("SELECT * FROM payments WHERE card = 4111111111111111", &[CompliancePack::PciDss]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-PCI-001"));
    }

    #[test]
    fn test_pci_002_detected() {
        let findings = check_compliance("SELECT ssn, credit_card FROM users", &[CompliancePack::PciDss]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-PCI-002"));
    }

    #[test]
    fn test_soc2_001_detected() {
        let findings = check_compliance("GRANT ALL ON users TO app_user", &[CompliancePack::Soc2]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-SOC2-001"));
    }

    #[test]
    fn test_soc2_002_detected() {
        let findings = check_compliance("DROP TABLE orders", &[CompliancePack::Soc2]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-SOC2-002"));
    }

    #[test]
    fn test_soc2_002_with_backup_comment_passes() {
        let findings = check_compliance("DROP TABLE orders /* backup taken */", &[CompliancePack::Soc2]);
        assert!(!findings.iter().any(|f| f.rule_id == "COMPLIANCE-SOC2-002"));
    }

    #[test]
    fn test_gdpr_001_detected() {
        let findings = check_compliance("SELECT * FROM user", &[CompliancePack::Gdpr]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-GDPR-001"));
    }

    #[test]
    fn test_gdpr_002_detected() {
        let findings = check_compliance("INSERT INTO export_table SELECT * FROM source", &[CompliancePack::Gdpr]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-GDPR-002"));
    }

    #[test]
    fn test_gdpr_002_with_limit_passes() {
        let findings =
            check_compliance("INSERT INTO export_table SELECT * FROM source LIMIT 100", &[CompliancePack::Gdpr]);
        assert!(!findings.iter().any(|f| f.rule_id == "COMPLIANCE-GDPR-002"));
    }

    #[test]
    fn test_int_001_detected() {
        let findings =
            check_compliance("CREATE USER app WITH PASSWORD 'secret123'", &[CompliancePack::InternalBaseline]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-INT-001"));
    }

    #[test]
    fn test_int_002_detected() {
        let findings = check_compliance("TRUNCATE TABLE orders", &[CompliancePack::InternalBaseline]);
        assert!(findings.iter().any(|f| f.rule_id == "COMPLIANCE-INT-002"));
    }

    #[test]
    fn test_int_002_in_test_context_passes() {
        let findings = check_compliance("TRUNCATE TABLE orders -- test cleanup", &[CompliancePack::InternalBaseline]);
        assert!(!findings.iter().any(|f| f.rule_id == "COMPLIANCE-INT-002"));
    }

    #[test]
    fn test_clean_sql_no_findings() {
        let findings = check_compliance(
            "SELECT id, name FROM users WHERE status = 'active'",
            &[CompliancePack::PciDss, CompliancePack::Soc2, CompliancePack::Gdpr, CompliancePack::InternalBaseline],
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn test_empty_packs_no_findings() {
        let findings = check_compliance("SELECT * FROM user", &[]);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_multiple_packs() {
        let findings = check_compliance(
            "SELECT * FROM user; GRANT ALL ON payments TO public",
            &[CompliancePack::Gdpr, CompliancePack::Soc2],
        );
        let ids: Vec<&str> = findings.iter().map(|f| f.rule_id.as_str()).collect();
        assert!(ids.contains(&"COMPLIANCE-GDPR-001"));
        assert!(ids.contains(&"COMPLIANCE-SOC2-001"));
    }
}
