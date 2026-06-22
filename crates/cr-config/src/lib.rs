//! `.roughcollie.toml` 配置解析。

use std::path::Path;

use serde::Deserialize;

/// 根配置，对应 `.roughcollie.toml` 文件。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct Config {
    /// `[project]` 段。
    #[serde(default)]
    pub project: ProjectConfig,
    /// `[database]` 段。
    #[serde(default)]
    pub database: DatabaseConfig,
    /// `[rules]` 段。
    #[serde(default)]
    pub rules: RulesConfig,
    /// `[output]` 段。
    #[serde(default)]
    pub output: OutputConfig,
    /// `[notifications]` 段（四期）。
    #[serde(default)]
    pub notifications: NotificationsConfig,
    /// `[plugins]` 段（三期）。
    #[serde(default)]
    pub plugins: PluginsConfig,
}

/// `[project]` 段。
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct ProjectConfig {
    /// 项目名称。
    pub name: Option<String>,
    /// 项目根目录（reserved，未来作为文件发现基准路径）。
    pub root: Option<String>,
    /// Git 版本库目录（git 操作工作目录），缺省为 CWD。
    pub git_repo: Option<String>,
    /// 项目类型，驱动审核策略。
    pub project_type: Option<ProjectType>,
    /// 默认 baseline 分支名，作为 `--baseline` CLI 参数的缺省值。
    pub baseline: Option<String>,
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
#[derive(Debug, Clone, Deserialize, Default)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.project.name.is_none());
        assert!(config.project.root.is_none());
        assert!(config.project.git_repo.is_none());
        assert!(config.project.project_type.is_none());
        assert!(config.project.baseline.is_none());
        assert!(!config.database.enabled);
        // serde(default = "default_port") only applies during deserialization, not Default::default()
        assert_eq!(config.database.port, 0);
        // serde(default = "default_ssl_mode") — Default gives ""
        assert_eq!(config.database.ssl_mode, "");
        // serde(default = "default_auth_method") — Default gives ""
        assert_eq!(config.database.auth_method, "");
        // ExplainConfig has a manual Default impl
        assert_eq!(config.database.explain.timeout_seconds, 30);
        assert_eq!(config.database.explain.max_cost_warning, 10_000.0);
        assert_eq!(config.database.explain.max_cost_critical, 50_000.0);

        assert_eq!(config.database.security.enforce_readonly, true);
        assert_eq!(
            config.database.security.allowed_commands,
            vec!["EXPLAIN".to_string(), "SET".to_string(), "SHOW".to_string()]
        );

        assert_eq!(config.output.format, "markdown");
        assert!(config.output.path.is_none());
        assert_eq!(config.output.exit_code_on_critical, 1);

        assert!(!config.notifications.enabled);
        assert!(config.plugins.paths.is_empty());
        assert!(config.plugins.enabled.is_empty());
    }

    #[test]
    fn test_config_load_from_file() {
        let toml_content = r##"
[project]
name = "test-project"
root = "/tmp/test"

[database]
enabled = true
host = "localhost"
port = 15432
database = "testdb"
username = "admin"
password_env = "DB_PASS"
ssl_mode = "disable"
auth_method = "md5"

[database.explain]
timeout_seconds = 60
max_cost_warning = 5000.0
max_cost_critical = 20000.0
buffers_threshold = 50000
enable_analyze = false
enable_buffers = false

[database.security]
enforce_readonly = false
allowed_commands = ["EXPLAIN"]

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
        assert_eq!(config.project.name.unwrap(), "test-project");
        assert_eq!(config.database.port, 15432);
        assert_eq!(config.database.explain.timeout_seconds, 60);
        assert!(!config.database.security.enforce_readonly);
        assert_eq!(config.output.format, "json");
        assert_eq!(config.notifications.slack.unwrap().channel, "#reviews");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn test_project_type_deserialize() {
        fn project_type_from(toml: &str) -> ProjectType {
            let wrapped = format!("[project]\nproject_type = {toml}\n");
            let config: Config = toml::from_str(&wrapped).unwrap();
            config.project.project_type.unwrap()
        }
        assert_eq!(project_type_from("\"gaussdb-sql\""), ProjectType::GaussdbSql);
        assert_eq!(project_type_from("\"java\""), ProjectType::Java);
        assert_eq!(project_type_from("\"mixed\""), ProjectType::Mixed);
    }

    #[test]
    fn test_project_type_invalid_value() {
        let toml_content = "[project]\nproject_type = \"python\"\n";
        let result: Result<Config, _> = toml::from_str(toml_content);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_project_fields_load() {
        let toml_content = r##"
[project]
name = "audit-demo"
root = "."
git_repo = "/srv/repos/audit-demo"
project_type = "gaussdb-sql"
baseline = "release-v2"
"##;
        let config: Config = toml::from_str(toml_content).unwrap();
        assert_eq!(config.project.name.as_deref(), Some("audit-demo"));
        assert_eq!(config.project.root.as_deref(), Some("."));
        assert_eq!(config.project.git_repo.as_deref(), Some("/srv/repos/audit-demo"));
        assert_eq!(config.project.project_type, Some(ProjectType::GaussdbSql));
        assert_eq!(config.project.baseline.as_deref(), Some("release-v2"));
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
}
