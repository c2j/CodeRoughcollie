# 多项目 + 共享数据库配置

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 `.roughcollie.toml` 从单项目扁平结构重构为多项目命名 map 结构，支持一个配置文件定义多个项目 + 多个数据库，项目按名称引用数据库。CLI 新增 `--project` 选择单项目审核或全项目审核。

**Architecture:** `cr-config` 根结构改 `HashMap<String, ProjectConfig>` / `HashMap<String, DatabaseConfig>`；`cr-cli` 拆分单项目/全项目审核流程；`cr-report` 新增 `MultiProjectContext` 支持合并报告。

**Tech Stack:** Rust 2021 / serde / clap / thiserror / tracing / toml

---

## 设计决策

| 决策点 | 选择 | 理由 |
|---|---|---|
| 项目选择 | `--project <name>` 选单项目，缺省审核全部 | 显式可控，缺省全审 |
| 配置结构 | 命名 map `[projects.*]` + `[databases.*]` | TOML 原生 map，引用自然 |
| CLI 参数作用域 | 项目级参数仅 `--project` 模式生效，全局参数两模式都生效 | 不同项目 baseline/git_repo 不同，CLI 不应全局覆盖 |
| 报告输出 | 全项目模式合并一份报告，按项目分段 | 一站式总览 |
| 向后兼容 | 不兼容，直接改格式 | 项目尚未推广，无迁移成本 |

---

## 配置数据模型

### 根 Config

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub databases: HashMap<String, DatabaseConfig>,
    pub projects: HashMap<String, ProjectConfig>,
    #[serde(default)]
    pub rules: RulesConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}
```

### ProjectConfig

```rust
pub struct ProjectConfig {
    pub root: Option<String>,           // reserved
    pub git_repo: Option<String>,       // git 操作目录
    pub project_type: Option<ProjectType>,
    pub baseline: Option<String>,       // --baseline 默认值
    pub database: Option<String>,       // 引用 databases map 的 key
}
```

- `name` 字段移除 — map key 就是项目名
- `database = None` → 纯静态审核
- `database = Some("foo")` 但 `databases["foo"]` 不存在 → 加载时校验报错

### TOML 示例

```toml
# ── 数据库定义（命名 map）──
[databases.gaussdb-prod]
enabled = true
host = "10.0.1.100"
port = 5432
database = "audit_db"
username = "roughcollie"
password_env = "GAUSSDB_PASSWORD"

[databases.gaussdb-prod.explain]
timeout_seconds = 30
max_cost_warning = 10000

[databases.gaussdb-prod.security]
enforce_readonly = true

# ── 项目定义（引用数据库）──
[projects.order-service]
project_type = "gaussdb-sql"
baseline = "main"
git_repo = "."
database = "gaussdb-prod"

[projects.payment-service]
project_type = "java"
baseline = "dev"
git_repo = "."
database = "gaussdb-prod"

# ── 全局配置 ──
[rules.ogexplain]
preset = "all"
[output]
format = "markdown"
```

---

## CLI 与审核流程

### CLI 参数分类

```rust
Commands::Audit {
    // 项目选择
    #[arg(long)]
    project: Option<String>,

    // ── 项目级参数：仅 --project 模式生效 ──
    baseline: Option<String>,
    files: Vec<PathBuf>,
    dir: Vec<PathBuf>,
    db_host: Option<String>,
    db_name: Option<String>,
    db_user: Option<String>,
    db_password_env: Option<String>,

    // ── 全局参数：两种模式都生效 ──
    output_format: String,
    output_path: Option<PathBuf>,
    no_db: bool,
}
```

### 单项目流程（`--project X`）

1. 查找 `config.projects["X"]` → 不存在则报错退出
2. 解析数据库引用：`project.database` → `config.databases[name]`
3. CLI 覆盖：`--baseline` > `project.baseline`，`--db-host` > `database.host`
4. `--no_db`：强制 `enabled = false`
5. 执行审核（与现有流程一致）
6. 输出单项目报告

### 全项目流程（无 `--project`）

1. 若传入项目级参数 → `tracing::warn!` 提示已忽略
2. 遍历 `config.projects` 所有项目
3. 每个项目用各自的 baseline / git_repo / project_type / database
4. `--no_db` 全局生效
5. 汇总所有项目的 findings
6. 渲染合并报告（按项目分段）

### 数据库引用校验

配置加载后校验：`project.database = Some("foo")` 但 `databases["foo"]` 不存在 → 立即报错退出。

---

## 报告结构

### 新增类型

```rust
pub struct ProjectSection {
    pub name: String,
    pub ctx: RenderContext,
}

pub struct MultiProjectContext {
    pub sections: Vec<ProjectSection>,
}
```

### 各格式多项目渲染

| 格式 | 渲染方式 |
|---|---|
| Markdown | `## Project: order-service` 标题分段，末尾汇总 |
| JSON | `{"projects": [{"name": "...", "findings": [...]}], "summary": {...}}` |
| CSV | 新增 `project` 列 |
| SARIF | 每个 project 一个 `run` |

单项目模式走现有 `RenderContext` 路径。

---

## 实现任务

### Task 1: `cr-config` — 结构重构

**Files:**
- Modify: `crates/cr-config/src/lib.rs`

**Steps:**
1. `Config` 根结构：`database: DatabaseConfig` → `databases: HashMap<String, DatabaseConfig>`，`project: ProjectConfig` → `projects: HashMap<String, ProjectConfig>`
2. `ProjectConfig`：移除 `name`，新增 `database: Option<String>`
3. 加 `#[serde(deny_unknown_fields)]` 到 `Config`
4. `DatabaseConfig` 和 `ProjectConfig` 加 `Default` impl（HashMap 需要）
5. 新增 `Config::validate()` 校验数据库引用完整性
6. 更新全部测试（新结构）
7. `cargo test -p cr-config` 通过

**Commit:** `refactor(config): restructure to multi-project named maps with database references`

---

### Task 2: `cr-report` — 多项目报告支持

**Files:**
- Modify: `crates/cr-report/src/lib.rs`

**Steps:**
1. 新增 `ProjectSection` 和 `MultiProjectContext` 结构
2. 新增 `render_multi(ctx: &MultiProjectContext, format: ReportFormat) -> String`
3. Markdown 多项目渲染：按项目分段 + 汇总
4. JSON 多项目渲染：`{"projects": [...], "summary": {...}}`
5. CSV 多项目渲染：新增 `project` 列
6. SARIF 多项目渲染：多 `run`
7. 更新测试
8. `cargo test -p cr-report` 通过

**Commit:** `feat(report): add multi-project report rendering`

---

### Task 3: `cr-cli` — 单/全项目审核流程

**Files:**
- Modify: `crates/cr-cli/src/main.rs`

**Steps:**
1. 新增 `--project` CLI flag
2. 配置加载后调用 `Config::validate()`
3. 单项目分支：查项目 → 解析 DB → CLI 覆盖 → 审核现有流程
4. 全项目分支：遍历项目 → 各自配置审核 → 汇总 findings
5. 全项目模式下项目级参数 → warn 忽略
6. 输出：单项目走 `RenderContext`，全项目走 `MultiProjectContext`
7. `cargo build -p cr-cli` 通过

**Commit:** `feat(cli): add --project flag and multi-project audit flow`

---

### Task 4: 示例配置 + 设计文档

**Files:**
- Modify: `.roughcollie.toml.example`
- Create: `docs/plans/2026-06-22-multi-project-config-design.md`（本文档）

**Steps:**
1. `.roughcollie.toml.example` 重写为新格式
2. **Commit:** `docs: update example config and add multi-project design`

---

## 验证标准

- [ ] `cargo test -p cr-config` — 新结构测试 + 引用校验测试
- [ ] `cargo test -p cr-report` — 多项目渲染测试
- [ ] `cargo test --workspace` 全量通过（除 pre-existing 失败）
- [ ] `cargo clippy --workspace -- -D warnings` 零警告
- [ ] `cargo fmt --all -- --check` 通过
- [ ] `cargo build --workspace` 成功
- [ ] 手动验证：多项目配置 → `coderc audit` 审核全部 → 合并报告

---

## 不做的事（YAGNI 边界）

- ❌ per-project `rules` / `output` / `notifications`（保持全局）
- ❌ `--project` 通配符（如 `--project "order-*"`）
- ❌ 项目间依赖关系
- ❌ 并行审核（顺序执行）
- ❌ 向后兼容旧 `[project]` / `[database]` 格式

---

## 风险点

1. **Config 结构 breaking change** — `deny_unknown_fields` 拒绝旧配置，用户确认无需兼容
2. **cr-report 接口扩展** — 新增 `MultiProjectContext` 不影响单项目路径
3. **全项目模式性能** — 顺序执行，DB 连接不复用（未来优化）
