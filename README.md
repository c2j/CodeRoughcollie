# CodeRoughcollie

GaussDB/openGauss 代码审核工具 — 静态 + 动态混合审核。

## 核心能力

- **SQL 反模式检测**：消费 ogexplain-analyzer 的 28 条诊断规则，基于 AST 精确匹配
- **真实 EXPLAIN 审核**：连接 GaussDB 执行 EXPLAIN ANALYZE，获取实际代价与算子
- **复杂度评估**：复用 ogsql-complexity 评分，与生态口径统一
- **降级策略**：无数据库连接时自动回退静态分析

## 快速开始

```bash
# 构建
cargo build --workspace

# 静态审核（无需数据库）
cargo run -p cr-cli -- audit \
  --baseline main \
  --files src/sql/query.sql \
  --output-format markdown

# 查看 CLI 帮助
cargo run -p cr-cli -- --help
```

### 清单批量审核（manifest）

传入 CSV 清单文件，工具自动 `git pull` 每个仓库到指定分支并审核：

```bash
# 清单文件（CSV，首行表头固定为 project,branch,files）
# project — .roughcollie.toml 中 [projects.*] 的项目名
# branch  — 待审核的分支名（工具自动 fetch + checkout + pull --ff-only）
# files   — 待审核文件列表，多个用分号 ; 分隔
cat > audit.csv <<'EOF'
project,branch,files
order-service,feat/order-opt,"src/sql/query.sql;src/sql/proc.sql"
payment-service,fix/pay-bug,src/mapper/PayMapper.xml
EOF

coderc audit --manifest audit.csv --output-format markdown
```

清单模式下 `--manifest` 与 `--project`/`--files`/`--dir`/`--baseline` 互斥；`--no-db`、`--output-*`、`--db-*` 等全局参数仍然生效。多个条目的审核结果合并为一份多项目报告。

## Workspace 结构

| Crate | 职责 |
|-------|------|
| `cr-core` | 审核引擎核心：类型、trait、错误（零 IO） |
| `cr-db` | GaussDB 连接层：连接池、认证、EXPLAIN 执行 |
| `cr-audit-static` | 静态审核：SQL 反模式 + Java 安全扫描 |
| `cr-audit-explain` | 真实执行计划审核 |
| `cr-audit-complexity` | 复杂度评估 |
| `cr-audit-impact` | 语义影响分析（三期） |
| `cr-plugin` | 插件加载层（三期） |
| `cr-git` | Git diff 解析、分支同步 |
| `cr-config` | TOML 配置解析、CSV 清单解析 |
| `cr-report` | Markdown / JSON / SARIF 报告 |
| `cr-mcp-server` | MCP Server（三期） |
| `cr-cli` | 命令行入口（`coderc`） |

## 开发

```bash
cargo test --workspace          # 全量测试
cargo clippy --workspace -- -D warnings  # lint
cargo fmt --all -- --check      # 格式检查
```

## 文档

- [设计文档](docs/Design.md)
- [编码规范（强制）](docs/CONTRIBUTING.md)
- [最佳实践（推荐）](docs/BEST-PRATICE.md)

## 许可证

MIT OR Apache-2.0
