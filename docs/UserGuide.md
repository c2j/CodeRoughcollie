# CodeRoughcollie 用户指南

## 概述

CodeRoughcollie 是一个 GaussDB/openGauss SQL 代码审核工具，支持静态 AST 反模式检测与真实 EXPLAIN 动态分析。可在本地命令行、CI 流水线中运行，或作为 MCP Server 供 AI 编程助手（Claude/Cursor）调用。

## 安装

### 从 GitHub Release 下载（推荐）

```bash
# Linux x86_64（glibc ≥ 2.17）
curl -L https://github.com/c2j/CodeRoughcollie/releases/latest/download/coderc-linux-x86_64.tar.gz | tar xz
sudo mv coderc /usr/local/bin/

# Linux ARM64
curl -L https://github.com/c2j/CodeRoughcollie/releases/latest/download/coderc-linux-arm64.tar.gz | tar xz
sudo mv coderc /usr/local/bin/

# Windows
# 下载 coderc-{version}-windows-x86_64.zip，解压到 PATH 目录
```

### 从源码构建

```bash
git clone --recurse-submodules https://github.com/c2j/CodeRoughcollie.git
cd CodeRoughcollie
cargo build --release
# 二进制文件位于 target/release/coderc
```

## 快速开始

```bash
# 审核单个 SQL 文件
coderc audit --files query.sql --no-db

# 审核多个文件
coderc audit --files query.sql,proc.sql,order.sql --no-db

# 对比分支变更，自动检测 .sql 文件
coderc audit --baseline origin/main --no-db

# JSON 格式输出
coderc audit --files query.sql --no-db --output-format json

# 保存报告到文件
coderc audit --files query.sql --no-db --output-path audit-report.md

# SARIF 格式（兼容 GitHub Advanced Security）
coderc audit --files query.sql --no-db --output-format sarif
```

## 配置

项目根目录创建 `.roughcollie.toml`（可参考 [示例配置](https://github.com/c2j/CodeRoughcollie/blob/main/.roughcollie.toml.example)），CLI 启动时自动加载。

### 完整配置参考

```toml
[project]
name = "my-gauss-app"          # 项目名称
root = "."                     # 项目根目录

[database]
enabled = false                # 是否启用 EXPLAIN 分析（需 GaussDB 连接）
host = "10.0.1.100"
port = 5432
database = "roughcollie_audit"
username = "roughcollie"
password_env = "GAUSSDB_PASSWORD"   # 密码从环境变量读取，不推荐明文
ssl_mode = "verify-full"            # verify-full / disable / prefer
auth_method = "sha256"              # sha256 / sm3 / scram-sha-256

[database.explain]
timeout_seconds = 30                # EXPLAIN 超时（秒）
max_cost_warning = 10000            # 代价 > 此值 报 Warning
max_cost_critical = 50000           # 代价 > 此值 报 Critical，阻断 CI
buffers_threshold = 100000          # 磁盘读 > 此值 标记
enable_analyze = true               # 是否执行 EXPLAIN ANALYZE
enable_buffers = true               # 是否显示缓冲区信息

[database.security]
enforce_readonly = true             # 启动时校验只读权限
allowed_commands = ["EXPLAIN", "SET", "SHOW"]

[rules.ogexplain]
preset = "all"                      # 全部 28 条诊断规则
# 自定义规则子集: preset = ["SCAN-001", "TYPE-001"]
severity_override = { SCAN-001 = "critical", TYPE-001 = "warning" }

[rules.astgrep]
preset = ["gaussdb-security", "java-sql-injection"]
severity_threshold = "warning"

[rules.complexity]
enabled = true
baseline_compare = true             # 与 baseline 对比增量
warning_delta = 10                  # 复杂度上升 10 分 → Warning
critical_delta = 25                 # 复杂度上升 25 分 → Critical

[rules.compliance]
enabled = false
packages = ["PCI-DSS", "SOC2"]      # 合规规则包
severity = "critical"

[plugins]
paths = ["./cr-plugins/"]          # 自定义规则插件目录
enabled = ["my-company-rules"]

[notifications]
enabled = false

[notifications.slack]
webhook_url_env = "SLACK_WEBHOOK_URL"
channel = "#db-audit-alerts"
min_severity = "critical"

[notifications.webhook]
url = "https://internal.company.com/hooks/audit"
include_full_report = true

[output]
format = "markdown"                 # markdown / json / sarif
path = "./audit-report.md"
exit_code_on_critical = 1           # Critical 时退出码非 0
exit_code_on_warning = 0            # Warning 时退出码 0（仅报告）

[output.filter]
# 可选：按 rule_id / severity / category 保留或筛除审核结果（详见下方说明）
severity = "includes:critical,warning"
rule_id = "excludes:COMPLEX-*"
category = "excludes:parse-error"
```

### 文件排除与结果过滤

CodeRoughcollie 提供两个独立层次的过滤机制，作用阶段和效果不同：

#### 项目级文件排除 `[projects.*].exclude`

在文件进入审核管道**之前**按 glob 模式排除。模式相对于项目的 `git_repo` 目录。

```toml
[projects.my-sql-project]
git_repo = "."
project_type = "gaussdb-sql"
exclude = [
    "**/test/**",           # 排除测试目录
    "**/*_test.sql",        # 排除测试 SQL
    "**/migrations/**",     # 排除数据库迁移脚本
]
```

不同 CLI 模式下被排除文件的处理方式：

| 模式 | 被排除文件的处理 |
|------|-----------------|
| `--baseline`（默认） | 静默丢弃，不出现在报告中 |
| `--dir` | 静默丢弃，不出现在报告中 |
| `--files` | 标记为 ⏭️ Ignored，在报告中列出但不审核 |
| `--manifest` | 标记为 ⏭️ Ignored，在报告中列出但不审核 |

> `--files` 和 `--manifest` 模式下保留可见性是因为用户显式指定了这些文件，需要告知"你指定了它，但配置排除了它"。

#### 审核结果过滤 `[output.filter]`

在审核完成**之后**、报告输出之前，对 Finding 做保留或筛除。规则照跑、文件照扫，只决定哪些结果出现在最终报告里。

三个维度，彼此是 AND 关系：

| 字段 | 说明 | 通配符 | 合法值示例 |
|------|------|:---:|---------|
| `rule_id` | 按规则 ID 过滤 | `*` | `excludes:SCAN-001,TYPE-*` |
| `severity` | 按严重度过滤 | — | `includes:critical,warning` |
| `category` | 按诊断分类过滤 | — | `excludes:parse-error` |

每个字段格式为 `{mode}:{value1},{value2},...`：

- `includes` — 白名单：仅保留匹配项
- `excludes` — 黑名单：排除匹配项

`severity` 取值：`critical` / `warning` / `info`

`category` 取值（kebab-case）：

| 分类 | 说明 | 对应规则前缀 |
|------|------|-------------|
| `scan-efficiency` | 扫描效率 | SCAN-* |
| `join-strategy` | 连接策略 | JOIN-* |
| `memory-usage` | 内存使用 | MEM-* |
| `sort-efficiency` | 排序效率 | SORT-* |
| `type-mismatch` | 类型不匹配 | TYPE-* |
| `subquery-structure` | 子查询结构 | SUBQ-* / REW-* |
| `parse-error` | 词法/语法错误 | PARSE-* / VAL-SYNTAX-* |
| `validation-semantic` | 语义校验 | VAL-PKG-* / VAL-PL-* |
| `general` | 通用 | GEN-* / ANTI-* / AGG-* |

#### 过滤层次总结

```
文件发现                     审核管道                     报告输出
    │                           │                           │
    ▼                           ▼                           ▼
[projects.*].exclude  →  规则匹配  →  [output.filter]  →  最终报告
  排除整个文件              产出 Finding     筛除部分 Finding
```

## 命令行接口

```
coderc audit [选项]
```

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--baseline <分支>` | — | Baseline 分支名，自动检测变更的 `.sql` 文件 |
| `--files <文件1,文件2>` | — | 待审核文件列表（逗号分隔） |
| `--output-format <格式>` | `markdown` | 输出格式：`markdown` / `json` / `sarif` |
| `--output-path <路径>` | — | 报告输出文件路径（未指定时输出到 stdout） |
| `--no-db` | — | 强制禁用 EXPLAIN 分析，仅静态规则 |
| `--db-host <主机>` | 配置文件 | 数据库主机（覆盖配置文件） |
| `--db-name <库名>` | 配置文件 | 数据库名称（覆盖配置文件） |
| `--db-user <用户>` | 配置文件 | 数据库用户（覆盖配置文件） |
| `--db-password-env <变量>` | 配置文件 | 密码环境变量（覆盖配置文件） |

### 退出码

| 退出码 | 含义 |
|--------|------|
| 0 | 审核通过，无 Critical 问题 |
| 1 | 存在 Critical 级别问题，阻断 CI |
| 2 | 工具自身错误（配置解析失败等） |
| 3 | 审核完成但发生降级（EXPLAIN 不可用时回退静态分析） |

## 检测规则

### SQL 反模式（静态检测，无需数据库）

| 规则 ID | 描述 | 严重度 |
|---------|------|--------|
| `STATIC-SELECT-STAR` | 使用 `SELECT *` | Warning |
| `STATIC-DELETE-NO-WHERE` | `DELETE` 无 WHERE 条件 | Critical |
| `STATIC-UPDATE-NO-WHERE` | `UPDATE` 无 WHERE 条件 | Critical |

### Java/MyBatis 安全扫描

| 规则 ID | 描述 | 严重度 |
|---------|------|--------|
| `SECURITY-MYBATIS-DOLLAR-PARAM` | MyBatis `${param}` 直接替换 | Critical |
| `SECURITY-JAVA-STATEMENT-EXEC` | `Statement.execute()` 字符串拼接 | Critical |
| `SECURITY-JAVA-SQL-CONCAT` | JPA `createQuery()` 字符串拼接 | Critical |

### EXPLAIN 执行计划分析（需 GaussDB 连接）

> **注意**: `cr-audit-explain` 因 ogsql-parser 版本兼容性问题暂时禁用（[ogexplain-analyzer#12](https://github.com/c2j/ogexplain-analyzer/issues/12)）。修复后将恢复以下 28 条诊断规则：

| 规则 ID | 名称 | 分类 |
|---------|------|------|
| SCAN-001 | 大表全表扫描 | 扫描效率 |
| JOIN-001 | Nested Loop 处理大数据集 | 连接策略 |
| MEM-001 | 排序溢出磁盘 | 内存使用 |
| TYPE-001 | 隐式类型转换 | 类型不匹配 |
| SUBQ-001 | 关联子查询未提升 | 子查询结构 |
| ... | 等 28 条规则 | 完整列表见 Design.md |

### 复杂度评估

通过 `ogsql-complexity` 计算 SQL 的复杂度分数（0-100），支持 GaussDB 存储过程 11 维度评分。当复杂度增量为正（较 baseline 上升）超过阈值时产生 Finding。

### 合规检查

可选的合规规则包：PCI-DSS / SOC 2 / GDPR / HIPAA。在 `rules.compliance.packages` 中配置。

## 输出格式

### Markdown（默认）

适合 PR 评论、邮件、文档。

```markdown
### 🟡 [STATIC-SELECT-STAR] 使用 SELECT * 的查询
**严重度**: warning
**建议**: 明确列出需要的列名，避免使用 SELECT *。
```

### JSON

适合程序消费。

```json
[{
  "rule_id": "STATIC-SELECT-STAR",
  "severity": "Warning",
  "title": "使用 SELECT * 的查询",
  "detail": "SELECT * 会检索所有列...",
  "suggestion": "明确列出需要的列名..."
}]
```

### SARIF

兼容 GitHub Advanced Security、GitLab SAST 等平台。

## 审核文件类型

| 文件类型 | 审核内容 |
|---------|---------|
| `.sql` | SQL 反模式 + 复杂度 + 合规检查 |
| `.xml`（MyBatis Mapper） | MyBatis `${param}` SQL 注入检测 |
| `.java` | SQL 注入检测（Statement.execute / createQuery） |

## CI 集成

### GitHub Actions

```yaml
name: CodeRoughcollie Audit
on: [pull_request]

jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Install CodeRoughcollie
        run: |
          curl -L https://github.com/c2j/CodeRoughcollie/releases/latest/download/coderc-linux-x86_64.tar.gz | tar xz
          sudo mv coderc /usr/local/bin/

      - name: Run Audit
        run: |
          coderc audit \
            --baseline origin/${{ github.base_ref }} \
            --output-format markdown \
            --output-path audit-report.md

      - name: Comment PR
        uses: actions/github-script@v7
        with:
          script: |
            const fs = require('fs');
            const report = fs.readFileSync('audit-report.md', 'utf8');
            github.rest.issues.createComment({
              issue_number: context.issue.number,
              owner: context.repo.owner,
              repo: context.repo.repo,
              body: report
            });
```

### GitLab CI

```yaml
audit:
  image: rust:latest
  script:
    - curl -L https://github.com/c2j/CodeRoughcollie/releases/latest/download/coderc-linux-x86_64.tar.gz | tar xz
    - ./coderc audit --baseline origin/main --output-format json --output-path audit-report.json
  artifacts:
    paths:
      - audit-report.json
```

## 已知限制

| 功能 | 状态 | 说明 |
|------|------|------|
| EXPLAIN 执行计划分析 | ⚠️ 暂不可用 | ogsql-parser v0.8.1 类型不兼容（[ogexplain-analyzer#12](https://github.com/c2j/ogexplain-analyzer/issues/12)） |
| astgrep AST 模式匹配 | ⚠️ 暂用 regex | astgrep-parser 跨工作区依赖问题（[astgrep#21](https://github.com/c2j/astgrep/issues/21)） |
| metamorphosis 完整重写 | ⚠️ 部分支持 | DELETE/UPDATE no WHERE 规则未实现（[metamorphosis#19](https://github.com/c2j/metamorphosis/issues/19)） |
| MySQL 支持 | ❌ 暂不支持 | 预留扩展点 |
| gRPC API | ❌ 暂不支持 | 仅 REST API |

## 项目结构

```
CodeRoughcollie/
├── crates/
│   ├── cr-core/               # 核心类型、trait、错误定义
│   ├── cr-audit-static/       # 静态审核（SQL 反模式 + Java 安全）
│   ├── cr-audit-explain/      # EXPLAIN 分析（ogexplain-core，暂禁用）
│   ├── cr-audit-complexity/   # 复杂度评估
│   ├── cr-audit-impact/       # 影响分析（子进程调用 codeweb）
│   ├── cr-db/                 # GaussDB 连接管理
│   ├── cr-git/                # Git diff 解析
│   ├── cr-config/             # TOML 配置解析
│   ├── cr-report/             # 报告渲染
│   ├── cr-plugin/             # 插件系统
│   ├── cr-mcp-server/         # MCP Server
│   ├── cr-server/             # REST API + 持久化 + 通知
│   └── cr-cli/                # CLI 入口
├── docs/                      # 文档
├── lib/                       # Git 子模块（仅阅读，非构建依赖）
└── .github/workflows/         # CI/CD
```

## 文档索引

| 文档 | 用途 |
|------|------|
| [Design.md](./Design.md) | 设计规划与架构 |
| [CONTRIBUTING.md](./CONTRIBUTING.md) | 贡献指南与编码规范 |
| [BEST-PRATICE.md](./BEST-PRATICE.md) | Rust 最佳实践 |
| [UserGuide.md](./UserGuide.md) | 本用户指南 |
