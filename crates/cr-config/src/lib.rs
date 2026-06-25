//! `.roughcollie.toml` 配置解析。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

pub mod manifest;

pub use manifest::{parse_manifest, ManifestEntry, ManifestError};

/// 根配置，对应 `.roughcollie.toml` 文件。
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Config {
    /// `[databases.*]` 段，命名 map。
    #[serde(default)]
    pub databases: BTreeMap<String, DatabaseConfig>,
    /// `[projects.*]` 段，命名 map。
    #[serde(default)]
    pub projects: BTreeMap<String, ProjectConfig>,
    /// `[rules]` 段（全局）。
    #[serde(default)]
    pub rules: RulesConfig,
    /// `[output]` 段（全局）。
    #[serde(default)]
    pub output: OutputConfig,
    /// `[notifications]` 段（四期，全局）。
    #[serde(default)]
    pub notifications: NotificationsConfig,
    /// `[plugins]` 段（三期，全局）。
    #[serde(default)]
    pub plugins: PluginsConfig,
    /// `[codeweb]` 段（三期，全局）。
    #[serde(default)]
    pub codeweb: CodewebConfig,
}

/// `[projects.*]` 段。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct ProjectConfig {
    /// 项目根目录（reserved，未来作为文件发现基准路径）。
    pub root: Option<String>,
    /// Git 版本库目录（git 操作工作目录），缺省为 CWD。
    pub git_repo: Option<String>,
    /// 项目类型，驱动审核策略。
    pub project_type: Option<ProjectType>,
    /// 默认 baseline 分支名，作为 `--baseline` CLI 参数的缺省值。
    pub baseline: Option<String>,
    /// 引用的数据库名称（`[databases.*]` 的 key），`None` 为纯静态审核。
    pub database: Option<String>,
    /// codeweb 影响分析配置（三期）。`None` 表示该项目不启用。
    pub codeweb: Option<CodewebProjectConfig>,
}

/// 项目类型，驱动审核策略。
///
/// 未配置（`None`）时等价于 [`ProjectType::Mixed`]，保持向后兼容。
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ProjectType {
    /// 纯 GaussDB SQL 项目：仅审核 `.sql` 文件。
    GaussdbSql,
    /// 纯 Java 项目：仅审核 `.java` / MyBatis XML 文件。
    Java,
    /// 混合项目：审核 SQL + Java + XML（默认行为）。
    Mixed,
}

/// `[database]` 段。
#[derive(Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct DatabaseConfig {
    /// 是否启用真实 EXPLAIN。
    #[serde(default)]
    pub enabled: bool,
    /// 数据库主机。
    #[serde(default)]
    pub host: String,
    /// 数据库端口。
    #[serde(default = "default_port")]
    pub port: u16,
    /// 数据库名称。
    #[serde(default)]
    pub database: String,
    /// 连接用户名。
    #[serde(default)]
    pub username: String,
    /// 密码环境变量名。
    pub password_env: Option<String>,
    /// 直接明文密码（优先级高于 `password_env`）。
    #[serde(default)]
    pub password: Option<String>,
    /// SSL 模式。
    #[serde(default = "default_ssl_mode")]
    pub ssl_mode: String,
    /// 认证方式。
    #[serde(default = "default_auth_method")]
    pub auth_method: String,
    /// EXPLAIN 配置。
    #[serde(default)]
    pub explain: ExplainConfig,
    /// 安全配置。
    #[serde(default)]
    pub security: SecurityConfig,
}

impl std::fmt::Debug for DatabaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseConfig")
            .field("enabled", &self.enabled)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("username", &self.username)
            .field("password_env", &self.password_env)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("ssl_mode", &self.ssl_mode)
            .field("auth_method", &self.auth_method)
            .field("explain", &self.explain)
            .field("security", &self.security)
            .finish_non_exhaustive()
    }
}

/// `[database.explain]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ExplainConfig {
    /// EXPLAIN 超时（秒）。
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    /// 代价超过此值报 Warning。
    #[serde(default = "default_cost_warning")]
    pub max_cost_warning: f64,
    /// 代价超过此值报 Critical。
    #[serde(default = "default_cost_critical")]
    pub max_cost_critical: f64,
    /// 磁盘读块数阈值。
    #[serde(default = "default_buffers_threshold")]
    pub buffers_threshold: i64,
    /// 是否执行 EXPLAIN ANALYZE。
    #[serde(default = "default_true")]
    pub enable_analyze: bool,
    /// 是否显示 Buffers。
    #[serde(default = "default_true")]
    pub enable_buffers: bool,
}

/// `[database.security]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct SecurityConfig {
    /// 启动时强制检查只读权限。
    #[serde(default = "default_true")]
    pub enforce_readonly: bool,
    /// 允许的命令列表。
    #[serde(default = "default_allowed_commands")]
    pub allowed_commands: Vec<String>,
}

/// `[rules]` 段。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct RulesConfig {
    /// ogexplain 规则配置。
    #[serde(default)]
    pub ogexplain: OgexplainRulesConfig,
    /// astgrep 规则配置。
    #[serde(default)]
    pub astgrep: AstgrepRulesConfig,
    /// 复杂度规则配置。
    #[serde(default)]
    pub complexity: ComplexityRulesConfig,
    /// 合规规则配置（四期）。
    #[serde(default)]
    pub compliance: ComplianceRulesConfig,
}

/// `[rules.ogexplain]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct OgexplainRulesConfig {
    /// 规则预设："all" 或规则 ID 列表。
    #[serde(default = "default_preset_all")]
    pub preset: String,
    /// 严重度覆盖。
    #[serde(default)]
    pub severity_override: std::collections::HashMap<String, String>,
}

/// `[rules.astgrep]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct AstgrepRulesConfig {
    /// 规则预设列表。
    #[serde(default)]
    pub preset: Vec<String>,
    /// 严重度阈值。
    #[serde(default = "default_severity_threshold")]
    pub severity_threshold: String,
}

/// `[rules.complexity]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct ComplexityRulesConfig {
    /// 是否启用。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 是否与基线对比。
    #[serde(default = "default_true")]
    pub baseline_compare: bool,
    /// 复杂度上升此分报 Warning。
    #[serde(default = "default_warning_delta")]
    pub warning_delta: f64,
    /// 复杂度上升此分报 Critical。
    #[serde(default = "default_critical_delta")]
    pub critical_delta: f64,
}

/// `[rules.compliance]` 段（四期）。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct ComplianceRulesConfig {
    /// 是否启用。
    #[serde(default)]
    pub enabled: bool,
    /// 合规规则包列表。
    #[serde(default)]
    pub packages: Vec<String>,
    /// 合规违规严重度。
    #[serde(default = "default_compliance_severity")]
    pub severity: String,
}

/// `[output]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct OutputConfig {
    /// 输出格式：markdown / json / sarif。
    #[serde(default = "default_output_format")]
    pub format: String,
    /// 输出文件路径。
    pub path: Option<String>,
    /// Critical 时退出码。
    #[serde(default = "default_exit_code")]
    pub exit_code_on_critical: i32,
    /// Warning 时退出码。
    #[serde(default)]
    pub exit_code_on_warning: i32,
}

/// `[notifications]` 段（四期）。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct NotificationsConfig {
    /// 是否启用通知。
    #[serde(default)]
    pub enabled: bool,
    /// Slack 通知配置。
    pub slack: Option<SlackConfig>,
    /// Webhook 通知配置。
    pub webhook: Option<WebhookConfig>,
}

/// `[notifications.slack]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct SlackConfig {
    /// Webhook URL 环境变量名。
    pub webhook_url_env: String,
    /// 频道。
    pub channel: String,
    /// 最低通知严重度。
    #[serde(default = "default_compliance_severity")]
    pub min_severity: String,
}

/// `[notifications.webhook]` 段。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct WebhookConfig {
    /// Webhook URL。
    pub url: String,
    /// 是否发送完整报告。
    #[serde(default = "default_true")]
    pub include_full_report: bool,
}

/// `[plugins]` 段（三期）。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct PluginsConfig {
    /// 插件搜索路径。
    #[serde(default)]
    pub paths: Vec<String>,
    /// 启用的插件名称。
    #[serde(default)]
    pub enabled: Vec<String>,
    /// 禁用的插件名称（支持通配符）。
    #[serde(default)]
    pub disabled: Vec<String>,
}

/// `[codeweb]` 段（全局）—— 子进程调用 codeweb 做语义影响分析。
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct CodewebConfig {
    /// codeweb 可执行文件路径；`None` 时从 PATH 查找 `codeweb`。
    pub binary: Option<String>,
    /// 子进程超时（秒）。
    #[serde(default = "default_codeweb_timeout")]
    pub timeout_secs: u64,
}

/// `[projects.x.codeweb]` 段（每项目）。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct CodewebProjectConfig {
    /// codeweb 项目目录（含 codeweb.toml / .codeweb/store.bincode）。
    pub project_path: String,
    /// 是否启用 impact 分析（opt-in，默认 false）。
    #[serde(default)]
    pub enabled: bool,
}

impl Config {
    /// 从文件加载配置。
    ///
    /// # Errors
    ///
    /// 当文件不存在或 TOML 解析失败时返回错误。
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::ReadFailed(path.display().to_string(), e.to_string()))?;
        toml::from_str(&content).map_err(ConfigError::Parse)
    }

    /// 校验配置完整性：检查项目引用的数据库是否存在、astgrep preset 路径格式合法。
    ///
    /// # Errors
    ///
    /// 当项目引用了不存在的数据库，或 astgrep preset 含 `..`/绝对路径/空字符串时返回错误。
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (name, project) in &self.projects {
            if let Some(db_ref) = &project.database {
                if !self.databases.contains_key(db_ref) {
                    let available: Vec<&str> = self.databases.keys().map(String::as_str).collect();
                    return Err(ConfigError::InvalidDatabaseRef(name.clone(), db_ref.clone(), available.join(", ")));
                }
            }
        }
        for preset in &self.rules.astgrep.preset {
            validate_astgrep_preset(preset)?;
        }
        Ok(())
    }
}

fn validate_astgrep_preset(preset: &str) -> Result<(), ConfigError> {
    if preset.is_empty() {
        return Err(ConfigError::InvalidAstgrepPreset { preset: preset.into(), reason: "空 preset".into() });
    }
    let path = Path::new(preset);
    if path.is_absolute() {
        return Err(ConfigError::InvalidAstgrepPreset { preset: preset.into(), reason: "禁止绝对路径".into() });
    }
    use std::path::Component;
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ConfigError::InvalidAstgrepPreset { preset: preset.into(), reason: "禁止 `..` 越权".into() });
    }
    Ok(())
}

/// 配置错误。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// 文件读取失败。
    #[error("读取配置文件失败 ({0}): {1}")]
    ReadFailed(String, String),
    /// TOML 解析失败。
    #[error("配置解析失败: {0}")]
    Parse(#[from] toml::de::Error),
    /// 项目引用了不存在的数据库。
    #[error("项目 '{0}' 引用了不存在的数据库 '{1}'。可用数据库: {2}")]
    InvalidDatabaseRef(String, String, String),
    /// astgrep preset 路径格式非法。
    #[error("无效的 astgrep preset `{preset}`：{reason}")]
    InvalidAstgrepPreset { preset: String, reason: String },
}

// --- 默认值函数 ---

const fn default_port() -> u16 {
    5432
}

fn default_ssl_mode() -> String {
    "verify-full".to_string()
}

fn default_auth_method() -> String {
    "sha256".to_string()
}

const fn default_timeout() -> u64 {
    30
}

const fn default_cost_warning() -> f64 {
    10_000.0
}

const fn default_cost_critical() -> f64 {
    50_000.0
}

const fn default_buffers_threshold() -> i64 {
    100_000
}

const fn default_true() -> bool {
    true
}

fn default_preset_all() -> String {
    "all".to_string()
}

fn default_severity_threshold() -> String {
    "warning".to_string()
}

const fn default_warning_delta() -> f64 {
    10.0
}

const fn default_critical_delta() -> f64 {
    25.0
}

fn default_compliance_severity() -> String {
    "critical".to_string()
}

fn default_output_format() -> String {
    "markdown".to_string()
}

const fn default_exit_code() -> i32 {
    1
}

fn default_allowed_commands() -> Vec<String> {
    vec!["EXPLAIN".to_string(), "SET".to_string(), "SHOW".to_string()]
}

const fn default_codeweb_timeout() -> u64 {
    120
}

impl Default for ExplainConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: default_timeout(),
            max_cost_warning: default_cost_warning(),
            max_cost_critical: default_cost_critical(),
            buffers_threshold: default_buffers_threshold(),
            enable_analyze: default_true(),
            enable_buffers: default_true(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self { enforce_readonly: default_true(), allowed_commands: default_allowed_commands() }
    }
}

impl Default for OgexplainRulesConfig {
    fn default() -> Self {
        Self { preset: default_preset_all(), severity_override: std::collections::HashMap::new() }
    }
}

impl Default for AstgrepRulesConfig {
    fn default() -> Self {
        Self { preset: Vec::new(), severity_threshold: default_severity_threshold() }
    }
}

impl Default for ComplexityRulesConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            baseline_compare: default_true(),
            warning_delta: default_warning_delta(),
            critical_delta: default_critical_delta(),
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: default_output_format(),
            path: None,
            exit_code_on_critical: default_exit_code(),
            exit_code_on_warning: 0,
        }
    }
}

impl Default for CodewebConfig {
    fn default() -> Self {
        Self { binary: None, timeout_secs: default_codeweb_timeout() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.projects.is_empty());
        assert!(config.databases.is_empty());
        assert_eq!(config.output.format, "markdown");
        assert!(config.output.path.is_none());
        assert_eq!(config.output.exit_code_on_critical, 1);
        assert!(!config.notifications.enabled);
        assert!(config.plugins.paths.is_empty());
    }

    #[test]
    fn test_config_load_from_file() {
        let toml_content = r##"
[databases.testdb]
enabled = true
host = "localhost"
port = 15432
database = "testdb"
username = "admin"
password_env = "DB_PASS"
ssl_mode = "disable"
auth_method = "md5"

[databases.testdb.explain]
timeout_seconds = 60
max_cost_warning = 5000.0
max_cost_critical = 20000.0
buffers_threshold = 50000
enable_analyze = false
enable_buffers = false

[databases.testdb.security]
enforce_readonly = false
allowed_commands = ["EXPLAIN"]

[projects.test-project]
git_repo = "/tmp/test"
project_type = "mixed"
baseline = "main"
database = "testdb"

[rules.ogexplain]
preset = "strict"
[rules.complexity]
enabled = false
baseline_compare = false
[rules.compliance]
enabled = true
packages = ["PCI-DSS", "GDPR"]

[output]
format = "json"
path = "/tmp/report.json"
exit_code_on_critical = 2
exit_code_on_warning = 1

[notifications.slack]
webhook_url_env = "SLACK_WEBHOOK"
channel = "#reviews"
min_severity = "warning"

[plugins]
paths = ["/usr/local/coderc/plugins"]
enabled = ["git-hook"]
disabled = ["old-plugin"]
"##;

        let path = std::env::temp_dir().join("test_roughcollie_config.toml");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, toml_content).unwrap();

        let config = Config::load_from_file(&path).unwrap();
        let db = config.databases.get("testdb").unwrap();
        assert_eq!(db.port, 15432);
        assert_eq!(db.explain.timeout_seconds, 60);
        assert!(!db.security.enforce_readonly);
        assert_eq!(config.output.format, "json");
        assert_eq!(config.notifications.slack.unwrap().channel, "#reviews");

        let project = config.projects.get("test-project").unwrap();
        assert_eq!(project.git_repo.as_deref(), Some("/tmp/test"));
        assert_eq!(project.database.as_deref(), Some("testdb"));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_project_type_deserialize() {
        fn project_type_from(toml_val: &str) -> ProjectType {
            let wrapped = format!("[projects.test]\nproject_type = {toml_val}\n");
            let config: Config = toml::from_str(&wrapped).unwrap();
            config.projects.get("test").unwrap().project_type.unwrap()
        }
        assert_eq!(project_type_from("\"gaussdb-sql\""), ProjectType::GaussdbSql);
        assert_eq!(project_type_from("\"java\""), ProjectType::Java);
        assert_eq!(project_type_from("\"mixed\""), ProjectType::Mixed);
    }

    #[test]
    fn test_project_type_invalid_value() {
        let toml_content = "[projects.test]\nproject_type = \"python\"\n";
        let result: Result<Config, _> = toml::from_str(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_project_fields_load() {
        let toml_content = r##"
[projects.audit-demo]
root = "."
git_repo = "/srv/repos/audit-demo"
project_type = "gaussdb-sql"
baseline = "release-v2"
"##;
        let config: Config = toml::from_str(toml_content).unwrap();
        let project = config.projects.get("audit-demo").unwrap();
        assert_eq!(project.root.as_deref(), Some("."));
        assert_eq!(project.git_repo.as_deref(), Some("/srv/repos/audit-demo"));
        assert_eq!(project.project_type, Some(ProjectType::GaussdbSql));
        assert_eq!(project.baseline.as_deref(), Some("release-v2"));
        assert!(project.database.is_none());
    }

    #[test]
    fn test_config_validate_valid_ref() {
        let toml_content = r##"
[databases.prod]
host = "10.0.1.100"

[projects.a]
database = "prod"

[projects.b]
database = "prod"
"##;
        let config: Config = toml::from_str(toml_content).unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_invalid_ref() {
        let toml_content = r##"
[databases.prod]
host = "10.0.1.100"

[projects.a]
database = "nonexistent"
"##;
        let config: Config = toml::from_str(toml_content).unwrap();
        let result = config.validate();
        assert!(result.is_err());
        match result {
            Err(ConfigError::InvalidDatabaseRef(project, db, _)) => {
                assert_eq!(project, "a");
                assert_eq!(db, "nonexistent");
            }
            _ => panic!("Expected InvalidDatabaseRef error"),
        }
    }

    #[test]
    fn test_config_validate_no_database_ref() {
        let toml_content = r##"
[projects.static-only]
project_type = "gaussdb-sql"
"##;
        let config: Config = toml::from_str(toml_content).unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_reject_old_format() {
        let toml_content = r##"
[project]
name = "old-style"
"##;
        let result: Result<Config, _> = toml::from_str(toml_content);
        assert!(result.is_err(), "old [project] format should be rejected");
    }

    #[test]
    fn test_config_reject_old_database_format() {
        let toml_content = r##"
[database]
host = "localhost"
"##;
        let result: Result<Config, _> = toml::from_str(toml_content);
        assert!(result.is_err(), "old [database] format should be rejected");
    }

    #[test]
    fn test_project_name_with_slash() {
        let toml_content = r#"
[projects."c2j/ogagila"]
project_type = "gaussdb-sql"
baseline = "main"
"#;
        let config: Config = toml::from_str(toml_content).unwrap();
        let project = config.projects.get("c2j/ogagila").unwrap();
        assert_eq!(project.project_type, Some(ProjectType::GaussdbSql));
        assert_eq!(project.baseline.as_deref(), Some("main"));
    }

    #[test]
    fn test_config_load_nonexistent_file() {
        let path = std::env::temp_dir().join("nonexistent_config.toml");
        let _ = std::fs::remove_file(&path);
        let result = Config::load_from_file(&path);
        assert!(result.is_err());
        match result {
            Err(ConfigError::ReadFailed(_, _)) => {}
            _ => panic!("Expected ReadFailed error"),
        }
    }

    #[test]
    fn test_config_load_invalid_toml() {
        let path = std::env::temp_dir().join("invalid_config.toml");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, "[[[invalid toml").unwrap();
        let result = Config::load_from_file(&path);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_database_config_debug_redacts_password() {
        let config = DatabaseConfig { password: Some("secret".into()), ..Default::default() };
        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("<redacted>"), "should contain redacted marker: {debug_str}");
        assert!(!debug_str.contains("secret"), "should NOT contain plaintext: {debug_str}");
    }

    #[test]
    fn test_database_config_debug_none_password() {
        let config = DatabaseConfig { password: None, ..Default::default() };
        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("None"), "should show None for absent password: {debug_str}");
    }

    #[test]
    fn test_codeweb_global_config() {
        let toml_content = r#"
[codeweb]
binary = "/usr/local/bin/codeweb"
timeout_secs = 180
"#;
        let config: Config = toml::from_str(toml_content).unwrap();
        assert_eq!(config.codeweb.binary.as_deref(), Some("/usr/local/bin/codeweb"));
        assert_eq!(config.codeweb.timeout_secs, 180);
    }

    #[test]
    fn test_codeweb_per_project_config() {
        let toml_content = r#"
[projects.demo]
git_repo = "/srv/demo"

[projects.demo.codeweb]
project_path = "/srv/demo"
enabled = true
"#;
        let config: Config = toml::from_str(toml_content).unwrap();
        let p = config.projects.get("demo").unwrap();
        let cw = p.codeweb.as_ref().unwrap();
        assert_eq!(cw.project_path, "/srv/demo");
        assert!(cw.enabled);
    }

    #[test]
    fn test_codeweb_absent_defaults() {
        let config: Config = toml::from_str("[projects.x]\ngit_repo=\".\"\n").unwrap();
        assert!(config.codeweb.binary.is_none());
        assert_eq!(config.codeweb.timeout_secs, 120);
        assert!(config.projects.get("x").unwrap().codeweb.is_none());
    }
}
