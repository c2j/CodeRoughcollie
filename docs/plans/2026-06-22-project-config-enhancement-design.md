# 项目配置增强：`[project]` 段扩展

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 扩展 `.roughcollie.toml` 的 `[project]` 段，新增 `git_repo`、`project_type`、`baseline` 三个配置项，激活 `name` 日志层，使项目元数据集中可配，减少 CLI 重复参数。

**Architecture:** `cr-config` 新增枚举类型；`cr-git` git 操作接受 `repo_path` 参数；`cr-cli` 实现 CLI > config > default 的解析优先级。

**Tech Stack:** Rust 2021 / serde / clap / thiserror / tracing

---

## 设计决策

| 决策点 | 选择 | 理由 |
|---|---|---|
| 配置载体 | 扩展现有 `.roughcollie.toml` | 单一配置源，无迁移成本 |
| `project_type` 类型 | 枚举（`gaussdb-sql` / `java` / `mixed`） | 驱动审核策略，三值覆盖现有全部审核能力 |
| `project_type` 取值数 | 三值 | 精准覆盖现有能力，不预留未实现枚举（YAGNI） |
| `baseline` 语义 | `--baseline` CLI 参数的默认值 | 减少重复传参，CLI 可覆盖 |
| `git_repo` vs `root` | 独立字段 | 支持未来 monorepo 场景（root = 文件基准，git_repo = 仓库根） |
| 字段总数 | 五个（name + root + git_repo + project_type + baseline） | 闭环，不过度设计 |

---

## 数据模型

### `crates/cr-config/src/lib.rs`

```rust
#[derive(Debug, Clone, Deserialize, Default)]
#[non_exhaustive]
pub struct ProjectConfig {
    pub name: Option<String>,           // 已有
    pub root: Option<String>,           // 已有（reserved，本次不激活行为）
    pub git_repo: Option<String>,       // 🆕 git 操作工作目录
    pub project_type: Option<ProjectType>, // 🆕 项目类型枚举
    pub baseline: Option<String>,       // 🆕 默认 baseline 分支名
}

/// 项目类型，驱动审核策略。
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectType {
    /// 纯 GaussDB SQL 项目：仅审核 .sql
    GaussdbSql,
    /// 纯 Java 项目：仅审核 .java / MyBatis XML
    Java,
    /// 混合项目：审核 SQL + Java + XML（当前默认行为）
    Mixed,
}
```

**要点：**
- `ProjectType` 用 `serde(rename_all = "kebab-case")` → TOML 写 `gaussdb-sql` / `java` / `mixed`
- 枚举 `Copy` + 无 `Default` → 未配置时为 `None`，CLI 按 `Mixed` 兼容现有行为
- `git_repo` / `baseline` 均 `Option` → 向后兼容，缺省时回落现有逻辑

### TOML 示例

```toml
[project]
name = "my-gauss-app"
root = "."
git_repo = "."            # git 操作目录，缺省 = CWD
project_type = "mixed"    # gaussdb-sql | java | mixed
baseline = "main"         # --baseline 缺省值
```

---

## 行为变更

### `baseline`（默认值）

**当前：** `--baseline` 为 `Option<String>`，未提供且无 `--files/--dir` 时报错退出。

**变更：** 解析优先级 `CLI --baseline` > `config.project.baseline` > 报错。

```rust
let effective_baseline = baseline.or(config.project.baseline.as_deref());
```

仅在进入 git diff 分支时才需要 `effective_baseline`，`--files/--dir` 路径不受影响（保持现有告警逻辑）。

### `git_repo`（git 工作目录）

**当前：** `cr_git::changed_files(baseline)` 和 `validate_baseline(baseline)` 在 CWD 隐式执行 git。

**变更：** 两个函数签名增加 `repo_path: &Path` 参数：

```rust
pub fn validate_baseline(baseline: &str, repo_path: &Path) -> Result<(), std::io::Error>
pub fn changed_files(baseline: &str, repo_path: &Path) -> Result<Vec<ChangedFile>, std::io::Error>
```

`Command::new("git").current_dir(repo_path)`。`git_repo` 为 `None` 时传 CWD（`.`）。

### `project_type`（文件过滤）

**当前：** git diff 发现的文件过滤为 `cf.is_sql() || cf.is_xml() || cf.is_java()`。

**变更：** 按 `project_type` 收窄：

| 类型 | 过滤条件 |
|---|---|
| `GaussdbSql` | `cf.is_sql()` |
| `Java` | `cf.is_java() \|\| cf.is_xml()` |
| `Mixed` / `None` | `cf.is_sql() \|\| cf.is_xml() \|\| cf.is_java()`（现状） |

被过滤的文件打 `tracing::debug!` 记录，便于排查。

### `name`（报告标识）

**当前：** 无使用。

**变更：** `name` 若存在则记入 `tracing::info!(project = name, ...)` 日志。报告展示不改（避免动 `cr-report` 接口）。

### `root`

**本次不激活行为**，保持 `Option<String>` 元数据字段。未来作为文件发现基准路径。

---

## 实现任务

### Task 1: `cr-config` — 枚举与字段

**Files:**
- Modify: `crates/cr-config/src/lib.rs`

**Steps:**
1. 新增 `ProjectType` 枚举（`GaussdbSql` / `Java` / `Mixed`，`kebab-case`）
2. `ProjectConfig` 增加 `git_repo`、`project_type`、`baseline` 三个 `Option` 字段
3. 新增测试：三值反序列化、缺省为 `None`、非法值报错
4. `cargo test -p cr-config` 通过

**Commit:** `feat(config): add project_type enum, git_repo and baseline fields`

---

### Task 2: `cr-git` — `repo_path` 参数

**Files:**
- Modify: `crates/cr-git/src/lib.rs`

**Steps:**
1. `validate_baseline(baseline: &str)` → `validate_baseline(baseline: &str, repo_path: &Path)`
2. `changed_files(baseline: &str)` → `changed_files(baseline: &str, repo_path: &Path)`
3. 两个函数内 `Command::new("git").current_dir(repo_path)`
4. 更新现有测试，传入临时仓库目录
5. `cargo test -p cr-git` 通过

**Commit:** `feat(git): accept repo_path for baseline validation and changed_files`

---

### Task 3: `cr-cli` — 接线配置

**Files:**
- Modify: `crates/cr-cli/src/main.rs`

**Steps:**
1. `effective_baseline = cli_baseline.or(config.project.baseline.as_deref())`
2. `repo_path = config.project.git_repo.as_deref().unwrap_or(".")`
3. 调用 `cr_git::changed_files(effective_baseline, repo_path)` 和 `validate_baseline(effective_baseline, repo_path)`
4. git diff 文件过滤按 `config.project.project_type` 收窄（抽 helper 函数 `filter_by_project_type`）
5. `name` 若存在打 `tracing::info!` 日志
6. `cargo build -p cr-cli` 通过

**Commit:** `feat(cli): wire project config for baseline default, git_repo and project_type filtering`

---

### Task 4: 示例配置更新

**Files:**
- Modify: `.roughcollie.toml.example`

**Steps:**
1. `[project]` 段补全 `git_repo`、`project_type`、`baseline` 示例及注释
2. **Commit:** `docs: update example config with new project fields`

---

## 验证标准

- [ ] `cargo test -p cr-config` — 枚举反序列化测试（三值 + 缺省 + 非法值）
- [ ] `cargo test -p cr-git` — `repo_path` 参数测试
- [ ] `cargo test --workspace` 全量通过
- [ ] `cargo clippy --workspace -- -D warnings` 零警告
- [ ] `cargo fmt --all -- --check` 通过
- [ ] `cargo build --workspace` 成功
- [ ] 手动验证：`.roughcollie.toml` 设 `baseline = "main"`，跑 `coderc audit`（不传 `--baseline`），应使用配置值

---

## 不做的事（YAGNI 边界）

- ❌ 不加 `--project-type` CLI flag（配置驱动即可）
- ❌ 不改 `cr-report` 接口（`name` 仅日志层）
- ❌ 不激活 `root` 行为（reserved）
- ❌ 不加 `exclude` / `include` 路径配置
- ❌ 不做多 repo 支持（`git_repo` 单值）

---

## 风险点

1. **`cr-git` 签名变更**是 breaking change — 但该 crate 仅 `cr-cli` 消费，workspace 内可控
2. **`project_type` 过滤** 可能意外屏蔽用户期望审核的文件 → 过滤时打 `tracing::debug!` 记录
