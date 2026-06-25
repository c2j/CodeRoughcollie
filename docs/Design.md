# CodeRoughcollie — 设计规划与实施路线

> **文档状态**: 草稿
> **目标读者**: 架构师、核心开发者
> **关联文档**: [CONTRIBUTING.md](./CONTRIBUTING.md)（强制编码规则）、[BEST-PRATICE.md](./BEST-PRATICE.md)（可选最佳实践）

---

## 一、设计哲学与质量承诺

### 1.1 核心定位：静态 + 动态混合审核

| 维度 | 传统静态审核（如 SonarQube） | CodeRoughcollie 设计 |
|------|------------------------------|----------------------|
| **SQL 反模式** | 正则/字符串匹配，误报高 | 消费 `ogexplain-analyzer` 的 28 条诊断规则（25 条经典 + 3 条反模式），基于 AST 精确匹配 |
| **执行计划风险** | 仅能做"表名 + 条件"的文本猜测 | **真实连接 GaussDB 执行 EXPLAIN ANALYZE**，获取实际代价与算子 |
| **复杂度评估** | 自行实现，与生态割裂 | 直接复用 `ogexplain-analyzer` 的 `ogsql-complexity` 模块，口径统一 |

**关键优势**：在 CI 服务器上预置一个 GaussDB 测试库连接，CodeRoughcollie 对变更 SQL 执行真实 EXPLAIN，这是任何纯静态 SAST 工具无法做到的。

### 1.2 编码规范承诺

CodeRoughcollie 自身代码遵循项目规定的双重规范体系：

| 层级 | 依据文档 | 执行方式 |
|------|---------|---------|
| **强制规则（Mandatory）** | `docs/CONTRIBUTING.md` 文档一 | CI 门禁阻断（`clippy -D warnings`、`rustfmt --check`、`cargo-audit`） |
| **推荐规则（Recommended）** | `docs/BEST-PRATICE.md` 文档二 | Code Review 中鼓励，不强制阻断 |

关键承诺：

- **M-ARCH-01**: Cargo Workspace 分层，禁止反向依赖
- **M-ARCH-02**: `cr-core` 零外部 IO 依赖（不含网络、文件系统、数据库连接）
- **M-ARCH-03**: 单文件 ≤ 600 行，理想 ≤ 400 行
- **M-ERR-01**: 库代码（lib）禁止返回 `anyhow`，使用 `thiserror` 定义具体错误类型
- **M-ERR-02**: 库代码禁止 `unwrap()`；bin 代码极度克制
- **M-LOG-01**: 统一使用 `tracing`，禁止 `log` crate
- **M-LOG-02**: 生产日志输出结构化 JSON
- **M-LOG-04**: 严禁在日志中记录密码、Token、PII
- **M-DEP-01**: 禁止依赖通配符 `*`
- **M-DEP-02**: `Cargo.lock` 提交到版本控制
- **M-DEP-03**: 声明 MSRV 并在 CI 中验证
- **M-TYP-01**: 禁止裸 `as` 类型转换，使用 `try_from` / `into`
- **M-TYP-03**: 公开 Struct/Enum 添加 `#[non_exhaustive]`
- **M-UNS-02**: `unsafe` 块前必须有 `SAFETY` 注释
- **M-DOC-01**: 所有 `pub` API 必须有文档注释，`cargo doc` 零警告

### 1.3 命名约定

| 对象 | 命名 | 说明 |
|------|------|------|
| CLI 命令 | `coderc` | 用户终端调用入口 |
| Crate 前缀 | `cr-*` | 如 `cr-core`、`cr-db`、`cr-audit-static` |
| 配置文件 | `.roughcollie.toml` | 保留全名以提升可读性；项目名 CodeRoughcollie 为一个词，不拆分 |
| 系统级路径 | `coderc/plugins/` | 系统安装路径使用 CLI 命令名 `coderc` |

---

## 二、架构设计

### 2.1 Workspace 布局

```
CodeRoughcollie/
├── crates/
│   ├── cr-core/                       # 审核引擎核心：路由、调度、聚合、基线对比（零 IO）
│   │   ├── src/
│   │   │   ├── lib.rs                 # ≤ 200 行，仅模块聚合 + re-export
│   │   │   ├── types.rs               # Finding、Severity、AuditContext 等核心类型
│   │   │   ├── traits.rs              # AuditRule、DbConnection、Reporter trait 体系
│   │   │   ├── engine.rs              # 审核调度引擎（≤ 400 行）
│   │   │   ├── baseline.rs            # 基线对比逻辑
│   │   │   ├── scoring.rs             # 评分聚合算法
│   │   │   └── error.rs               # RoughcollieError（thiserror）
│   │   └── Cargo.toml                 # 零外部 IO 依赖
│   │
│   ├── cr-db/                         # 数据库连接管理层
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── connection.rs          # 封装 rust-opengauss，连接池 + 认证
│   │   │   ├── explain_executor.rs    # EXPLAIN (ANALYZE, COSTS, BUFFERS) 执行器
│   │   │   └── security.rs            # 权限校验：仅允许 EXPLAIN，阻断 DML/DDL
│   │   └── Cargo.toml
│   │
│   ├── cr-audit-static/               # 静态审核（无数据库连接时降级）
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── sql_antipattern.rs     # 消费 ogexplain-analyzer 规则库做 AST 匹配
│   │   │   └── java_security.rs       # 封装 astgrep 规则子集
│   │   └── Cargo.toml
│   │
│   ├── cr-audit-explain/              # 真实执行计划审核
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── plan_fetcher.rs        # 调用 cr-db 获取执行计划
│   │   │   └── plan_analyzer.rs       # 调用 ogexplain-analyzer 解析与诊断
│   │   └── Cargo.toml
│   │
│   ├── cr-audit-complexity/           # 复杂度评估
│   │   ├── src/
│   │   │   └── lib.rs                 # 调用 ogsql-complexity::analyze / gauss_analyze
│   │   └── Cargo.toml
│   │
│   ├── cr-audit-impact/               # 语义影响分析（子进程调用 codeweb，三期）
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   │
│   ├── cr-plugin/                     # 插件加载层（三期）
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── loader.rs              # 动态库加载（libloading）+ C ABI 封装
│   │   │   ├── abi.rs                 # C ABI 安全边界：版本协商 + ABI 兼容层
│   │   │   ├── registry.rs            # 规则注册表
│   │   │   └── sandbox.rs             # 插件沙箱（限制文件系统/网络访问）
│   │   └── Cargo.toml
│   │
│   ├── cr-git/                        # Git diff、baseline 对比、blame
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   │
│   ├── cr-report/                     # Markdown / JSON / SARIF 报告
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   │
│   ├── cr-config/                     # TOML 配置解析
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   │
│   ├── cr-mcp-server/                 # MCP Server（三期，基于 rmcp）
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── server.rs              # rmcp::tool_router 工具定义
│   │   │   ├── tools.rs               # 工具实现
│   │   │   └── types.rs               # 工具参数/响应类型
│   │   └── Cargo.toml
│   │
│   └── cr-cli/                        # 命令行入口（clap）
│       ├── src/
│       │   └── main.rs                # ≤ 200 行
│       └── Cargo.toml
│
├── docs/                              # 设计文档、贡献指南、最佳实践
├── tests/                             # 集成测试
├── benches/                           # 性能基准测试（criterion）
├── lib/                               # Git 子模块：ogexplain-analyzer 等
├── Cargo.toml                         # Workspace 根配置
├── Cargo.lock                         # 提交到版本控制（M-DEP-02）
├── rustfmt.toml                       # 格式化配置
├── clippy.toml                        # Clippy lint 配置
├── .roughcollie.toml                  # 示例配置
└── .github/
    └── workflows/
        ├── ci.yml                     # CI：fmt → clippy → test → audit
        ├── release.yml                # 跨平台构建发布（cargo-zigbuild）
        └── dogfood.yml                # Dogfood：CodeRoughcollie 审核自身代码
```

### 2.2 依赖层次（强制单向依赖）

```
cr-cli ──► cr-report ──► cr-core
  ├──► cr-audit-static ──► cr-core
  ├──► cr-audit-explain ──► cr-db ──► cr-core
  ├──► cr-audit-complexity ──► cr-core
  ├──► cr-audit-impact ──► cr-core
  ├──► cr-git ──► cr-core
  ├──► cr-config ──► cr-core
  └──► cr-plugin ──► cr-core           （三期）

cr-mcp-server ──► cr-audit-* ──► cr-core   （三期）
```

**约束**（遵循 M-ARCH-01）：

- `cr-core` 零外部 IO 依赖（M-ARCH-02），不依赖 `tokio`、`reqwest`、文件系统、数据库驱动
- `cr-db` 仅 `cr-core` + `rust-opengauss`，不依赖任何 audit crate
- `cr-plugin` 仅 `cr-core` + `libloading`，不依赖任何 audit crate
- `cr-mcp-server` 依赖 audit crates + `rmcp`，是三期的装配点之一
- `cr-cli` 是所有模块的装配点，不包含业务逻辑
- **禁止任何反向依赖**

### 2.3 生态依赖关系

```
CodeRoughcollie
    │
    ├─► ogsql-parser ──────────┐
    ├─► ogexplain-core ────────┼── DiagnosticRule trait / Severity / Finding / analyze()
    ├─► ogsql-complexity ──────┤── analyze() / gauss_analyze() 复杂度评分
    ├─► rust-opengauss ────────┤── 预置连接，真实 EXPLAIN
    ├─► astgrep ───────────────┤── 安全规则（Java + SQL）
    ├─► codeweb ───────────────┤── 语义影响分析（子进程调用，三期）
    └─► rmcp ──────────────────┘── MCP Server SDK（三期）
```

> **注意**：`ogexplain-analyzer` 是一个 Workspace，包含 `ogexplain-core`、`ogexplain-cli`、`ogexplain-tui`、`ogsql-complexity`、`ogexplain-mcp` 五个 crate。CodeRoughcollie 消费其中 `ogexplain-core`（规则引擎）和 `ogsql-complexity`（复杂度评分），不依赖其 CLI/TUI/MCP。

### 2.4 核心类型定义（消费 ogexplain-analyzer）

CodeRoughcollie 的核心类型直接复用 `ogexplain-core` 的定义，确保诊断口径一致。

#### Severity

```rust
// 来源：ogexplain-core/src/analyzer/report.rs
// CodeRoughcollie 直接 re-export，不另造 Severity 枚举。

/// 诊断严重度。ogexplain-analyzer 仅定义三级，CodeRoughcollie 全局沿用。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum Severity {
    Critical,
    Warning,
    Info,
}
```

> **设计决策**：不引入 `Error` 级别。ogexplain-analyzer 的 `Severity` 是生态唯一口径。退出码策略通过配置将 `Critical` 映射为阻断（exit 1），`Warning` 可配置，`Info` 永远通过。详见 [§ 8.2 退出码](#82-退出码约定)。

#### Finding

```rust
// 来源：ogexplain-core/src/analyzer/report.rs

/// 单条审核发现。所有审核维度（静态、EXPLAIN、复杂度）统一产出此类型。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub rule_id: String,          // 如 "SCAN-001"、"TYPE-001"
    pub severity: Severity,
    pub category: DiagnosticCategory,
    pub title: String,
    pub detail: String,
    pub node_line: Option<usize>,
    pub node_type: Option<String>,
    pub suggestion: Option<String>,
    pub sql_rewrite: Option<RewriteResult>,
    pub evidence: Option<Evidence>,
}
```

#### DiagnosticCategory

```rust
// 来源：ogexplain-core/src/analyzer/report.rs

/// 诊断分类，对应 28 条规则的归属域。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[non_exhaustive]
pub enum DiagnosticCategory {
    ScanEfficiency,     // SCAN-* 规则
    JoinStrategy,       // JOIN-* 规则
    MemoryUsage,        // MEM-* 规则
    SortEfficiency,     // SORT-* 规则
    NetworkOverhead,    // NET-* 规则
    CostMisestimation,  // EST-* 规则
    PushdownFailure,    // PUSH-* 规则
    TypeMismatch,       // TYPE-* 规则
    Vectorization,      // VEC-* 规则
    SubqueryStructure,  // SUBQ-* / REW-* 规则
    DistributionIssue,  // DIST-* / SKEW-* 规则
    General,            // GEN-* / ANTI-* / AGG-* / STATS-* / PART-* 规则
}
```

#### AuditContext

```rust
// cr-core/src/types.rs — CodeRoughcollie 自定义

/// 一次审核任务的上下文（零 IO 依赖）。
///
/// 所有审核维度共享此上下文，包含数据库连接状态、规则配置、trace 信息。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuditContext {
    /// 关联的 Git commit SHA
    pub commit_sha: String,
    /// 分支名
    pub branch: String,
    /// 是否启用了数据库连接（EXPLAIN 模式）
    pub db_enabled: bool,
    /// 数据库主机（仅用于日志，不含密码）
    pub db_host: Option<String>,
    /// 链路追踪 ID（M-LOG-05）
    pub trace_id: String,
    /// 规则配置快照
    pub rules_config: RulesConfig,
}
```

---

## 三、审核维度详细设计

### 3.1 SQL 规范审核 — 反模式检测（复用 ogexplain-analyzer）

**不再自行实现规则**，而是将 `ogexplain-core` 的 28 条诊断规则作为"静态规则库"消费。

| 规则来源 | 处理方式 | 输出 |
|----------|----------|------|
| `ogexplain-core` 的 `DiagnosticRule` trait | 对变更 SQL 的 AST 做静态遍历，无需连接数据库 | `Finding`（规则 ID、位置、优化建议） |

#### DiagnosticRule Trait（真实签名）

```rust
// 来源：ogexplain-core/src/analyzer/rules/mod.rs

pub trait DiagnosticRule: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn severity(&self) -> Severity;
    fn category(&self) -> DiagnosticCategory;
    fn check(&self, node: &PlanNode, ctx: &PlanContext) -> Option<Finding>;
    fn check_global(&self, _plan: &ExplainPlan, _stats: &GlobalStats) -> Vec<Finding> {
        Vec::new()
    }
}
```

#### 全部 28 条规则一览

| 规则 ID | 名称 | 分类 | 严重度 |
|---------|------|------|--------|
| `SCAN-001` | 大表全表扫描 | ScanEfficiency | Critical |
| `SCAN-004` | 过滤条件无索引 | ScanEfficiency | Warning |
| `JOIN-001` | Nested Loop 处理大数据集 | JoinStrategy | Critical |
| `JOIN-002` | Hash Join 溢出磁盘 | JoinStrategy | Warning |
| `MEM-001` | 排序溢出磁盘 | MemoryUsage | Critical |
| `MEM-004` | 峰值内存过高 | MemoryUsage | Warning |
| `SORT-003` | 重复排序 | SortEfficiency | Warning |
| `NET-001` | 广播大表数据 | NetworkOverhead | Critical |
| `EST-001` | 行数严重低估 | CostMisestimation | Warning |
| `EST-004` | 低估导致 Nested Loop | CostMisestimation | Critical |
| `PUSH-001` | 查询未下推 | PushdownFailure | Critical |
| `PUSH-002` | 多层流式传输 | PushdownFailure | Warning |
| `TYPE-001` | 疑似隐式类型转换 | TypeMismatch | Warning |
| `TYPE-004` | LIKE 前导通配符 | TypeMismatch | Warning |
| `VEC-001` | 混合向量化/行存引擎 | Vectorization | Warning |
| `GEN-001` | 执行计划层级过深 | General | Warning |
| `SUBQ-001` | 关联子查询未提升 | SubqueryStructure | Warning |
| `REW-001` | 大 IN 列表未转换 | SubqueryStructure | Warning |
| `SUBQ-006` | 关联子查询自引用 UPDATE | SubqueryStructure | Critical |
| `AGG-001` | 聚合策略不当（应使用 HashAggregate） | General | Warning |
| `AGG-002` | HashAggregate 磁盘溢出 | General | Warning |
| `SKEW-001` | 数据倾斜 | DistributionIssue | Warning |
| `DIST-001` | 分布列不当导致重分布 | DistributionIssue | Warning |
| `STATS-001` | 统计信息未收集 | General | Warning |
| `PART-001` | 分区剪枝失效 | General | Warning |
| `ANTI-003` | Index Scan Amplification | General | Critical |
| `ANTI-005` | Multi-layer Materialization | General | Warning |
| `ANTI-007` | CN-side Large Sort | General | Critical |

**关键设计**：`ogexplain-core` 的规则分为两类：
- **纯静态可检测**：如 `TYPE-001`（隐式类型转换）、`TYPE-004`（LIKE 前导通配符）、`SUBQ-001`（关联子查询）。这些在 CodeRoughcollie 中直接走 AST 匹配。
- **需执行计划验证**：如 `SCAN-001`（大表全扫）、`JOIN-001`（Nested Loop 代价）。这些在 3.2 中通过真实 EXPLAIN 确认。

### 3.2 执行计划风险 — 真实 EXPLAIN ANALYZE（核心竞争力）

**这是 CodeRoughcollie 区别于所有静态审核工具的关键能力**。

#### 运行条件

- CI 服务器上预置 GaussDB/openGauss 测试库连接（建议与生产库同构，数据量可小但统计信息需更新）
- 连接用户权限严格限制：**仅 `EXPLAIN` 权限**，无 `INSERT/UPDATE/DELETE/CREATE`（`cr-db/security.rs` 启动时校验）

#### 审核流程

```
提取变更 SQL 中的 DML 语句
    │
    ▼
参数占位符处理（#{param} / ${param} → 类型推断填充默认值）
    │
    ▼
通过 rust-opengauss 执行：
    EXPLAIN (ANALYZE, COSTS, BUFFERS, TIMING, FORMAT TEXT)
    <SQL>
    │
    ▼
ogexplain-core::parser::parse() 解析 TEXT 为 ExplainPlan
    │
    ▼
ogexplain-core::analyze(&plan) 执行 28 条 DiagnosticRule
    + openGauss 特有诊断（Vector 算子 / CStore 列存 / Streaming / FQS 下推）
    │
    ▼
生成 DiagnosticReport { findings: Vec<Finding>, stats: GlobalStats }
```

#### 降级策略

| 场景 | 行为 |
|------|------|
| 数据库连接不可用 | 自动降级为 3.1 的静态规则匹配，报告顶部标注 `[EXPLAIN 降级：静态分析]` |
| SQL 含语法错误 | 阻断，不执行 EXPLAIN（避免数据库报错） |
| EXPLAIN 超时（>30s） | 终止连接，标记 `TIMEOUT`，建议人工复核 |
| 涉及临时表/CTE | 尝试执行；若依赖会话状态则降级为静态分析 |

#### 输出增强

```rust
// cr-audit-explain/src/types.rs

/// 含执行计划元数据的审核发现包装。
///
/// 包装 ogexplain-core 的 Finding，附加 EXPLAIN 特有指标。
#[non_exhaustive]  // M-TYP-03
pub struct ExplainFinding {
    /// 基础发现（来自 ogexplain-core）
    pub finding: Finding,
    /// 原始 EXPLAIN TEXT 输出
    pub plan_text: String,
    /// 总代价（来自 EXPLAIN 的 cost 字段）
    pub total_cost: f64,
    /// ANALYZE 模式下的实际执行时间（毫秒）
    pub actual_time_ms: Option<f64>,
    /// 优化器估计行数
    pub rows_estimated: i64,
    /// ANALYZE 模式下的实际行数
    pub rows_actual: Option<i64>,
    /// 共享缓冲区命中块数
    pub shared_hit_blocks: Option<i64>,
    /// 磁盘读取块数
    pub shared_read_blocks: Option<i64>,
}
```

Markdown 报告示例：

~~~~~markdown
### 🔴 [SCAN-001] 大表全表扫描（实际代价 48,231.5）
**SQL**: `SELECT * FROM orders WHERE create_time > '2024-01-01'`
**执行计划**:
```
Seq Scan on orders  (cost=0.00..48231.50 rows=5000000 width=244)
  Filter: (create_time > '2024-01-01 00:00:00'::timestamp)
  Rows Removed by Filter: 4990000
  Buffers: shared read=31245
```
**实际执行**: 12.3s（ANALYZE）| **磁盘读**: 31,245 块
**诊断**: `SCAN-001` + `MEM-001`
**建议**: 为 `orders(create_time)` 添加 B-tree 索引或按时间分区，预计代价降至 125.4
~~~~~

### 3.3 复杂度评估 — 直接复用 ogsql-complexity

| 输入 | 处理 | 输出 |
|------|------|------|
| 变更 SQL | 调用 `ogsql_complexity::analyze(sql)` | `ComplexityReport`（含 `overall_score`、`overall_level`） |
| GaussDB 存储过程 | 调用 `ogsql_complexity::gauss_analyze(sql, &config)` | `GaussDbComplexityReport`（含 11 维度评分） |

**API 签名**（来源：`ogsql-complexity/src/engine.rs`）：

```rust
/// 标准 SQL 复杂度分析。
pub fn analyze(sql: &str) -> Result<ComplexityReport, ComplexityError>;

/// GaussDB 专用复杂度分析（含存储过程 11 步评分公式）。
pub fn gauss_analyze(
    sql: &str,
    config: &ComplexityConfig,
) -> Result<GaussDbComplexityReport, ComplexityError>;
```

**基线对比**：记录 `main` 分支上同一文件的复杂度分数，计算 **增量 Δ**：

~~~~~markdown
### 🟡 [COMPLEX-003] 存储过程复杂度上升 +23 分
**文件**: `src/sql/proc_calc_order.sql`
**基线复杂度**: 34（低）→ **当前复杂度**: 57（中）
**变更**: 新增 2 层 `FOR` 循环嵌套 + 1 个游标
**建议**: 考虑将循环内逻辑提取为独立过程，或改用集合操作（`INSERT ... SELECT`）
~~~~~

---

## 四、错误处理设计

遵循 CONTRIBUTING.md 的强制错误处理规则（M-ERR-01 ~ M-ERR-06）。

### 4.1 错误类型层次

```rust
// cr-core/src/error.rs — 使用 thiserror，不依赖 anyhow（M-ERR-01）
/// CodeRoughcollie 统一的错误类型。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RoughcollieError {
    /// 配置解析失败。
    #[error("配置错误: {0}")]
    Config(String),

    /// SQL 语法解析失败。
    #[error("SQL 解析错误 (行 {line}, 列 {col}): {message}")]
    Parse { line: usize, col: usize, message: String },

    /// 数据库连接或执行错误。
    #[error("数据库错误: {0}")]
    Database(#[from] DbError),

    /// ogexplain-analyzer 规则引擎错误。
    #[error("规则引擎错误: {0}")]
    RuleEngine(String),

    /// EXPLAIN 执行超时。
    #[error("EXPLAIN 超时 ({timeout_sec}s): {sql}")]
    ExplainTimeout { timeout_sec: u64, sql: String },

    /// 插件加载错误。
    #[error("插件加载错误: {0}")]
    Plugin(String),

    /// IO 错误（文件读写等）。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 数据库层错误（cr-db 定义）。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DbError {
    /// 连接失败。
    #[error("连接 GaussDB 失败 ({host}:{port}): {reason}")]
    ConnectionFailed { host: String, port: u16, reason: String },

    /// 权限不足。
    #[error("权限不足: 用户 '{user}' 缺少 {required_priv} 权限")]
    PermissionDenied { user: String, required_priv: String },

    /// EXPLAIN 被安全策略拒绝。
    #[error("SQL 被安全策略拒绝: {reason}")]
    SecurityRejected { reason: String },
}
```

### 4.2 错误处理约定

| 层级 | 约定 |
|------|------|
| `cr-core` | 定义 `RoughcollieError`，禁止 `anyhow`、禁止 `unwrap()`（M-ERR-01、M-ERR-02）。必须使用 `expect()` 时提供失败原因（M-ERR-03）。 |
| `cr-db` / audit crates | 使用 `thiserror` 定义本 crate 错误，通过 `From` 转换到 `RoughcollieError`（M-ERR-06）。 |
| `cr-cli`（bin） | 可使用 `anyhow` / `eyre` 做最终错误兜底与上下文传播（R-ERR-02）。 |

---

## 五、可观测性设计

遵循 CONTRIBUTING.md 的强制日志规则（M-LOG-01 ~ M-LOG-06）和 BEST-PRATICE.md 的建议（R-OBS-01 ~ R-OBS-05）。

### 5.1 日志规范

| 级别 | 语义 | 示例 |
|------|------|------|
| `ERROR` | 需人工告警的故障 | 数据库连接断开、EXPLAIN 执行异常、配置解析失败 |
| `WARN` | 可自愈或可降级的异常 | EXPLAIN 降级为静态分析、单条 SQL 审核超时 |
| `INFO` | 关键生命周期事件 | 审核任务开始/结束、连接池状态、Finding 数量统计 |
| `DEBUG` | 开发调试信息 | 单条规则匹配详情、AST 遍历路径 |
| `TRACE` | 极度详细的诊断 | 每条 SQL 的完整 EXPLAIN 输出 |

```rust
// 日志示例（使用 tracing）
use tracing::{info, warn, error, instrument};

/// 审核一批文件，返回所有 Finding。
///
/// # Errors
///
/// 当数据库连接不可用且运行时配置未启用降级时返回错误。
#[instrument(skip(ctx), fields(
    file_count = files.len(),
    db_enabled = ctx.db_enabled,
    trace_id = %ctx.trace_id  // M-LOG-05: 入口 Span 含 trace_id
))]
pub async fn audit_files(
    ctx: &AuditContext,
    files: &[FilePath],
) -> Result<Vec<Finding>, RoughcollieError> {
    info!("开始审核 {} 个文件", files.len());

    if ctx.db_enabled {
        // ERROR 必须包含可行动的上下文（M-LOG-06）
        error!(
            db_host = ?ctx.db_host,
            error = %e,
            "无法连接审核数据库，连接已重试 3 次"
        );
    }

    // ...
}
```

### 5.2 指标暴露

```rust
/// 审核指标（使用 metrics crate，R-OBS-03）。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AuditMetrics {
    /// 审核任务总数
    pub audits_total: Counter,
    /// 审核耗时分布（P50/P95/P99）
    pub audit_duration_seconds: Histogram,
    /// Finding 按严重度分布
    pub findings_by_severity: Gauge,
    /// EXPLAIN 执行成功率
    pub explain_success_rate: Gauge,
}
```

---

## 六、测试策略

### 6.1 测试金字塔

| 层级 | 占比 | 内容 | 工具 |
|------|------|------|------|
| 单元测试 | 50% | `cr-core` 类型与 trait、各 audit crate 的规则匹配逻辑 | `cargo test` |
| 集成测试 | 30% | 端到端审核流程（准备 SQL 文件 → 执行审核 → 校验 Finding） | `cargo test --test '*'` |
| 属性测试 | 10% | AST 解析不变性、规则匹配幂等性 | `proptest`（R-TST-02） |
| 模糊测试 | 5% | `unsafe` 代码路径（预计极少）、EXPLAIN 解析器 | `cargo-fuzz`（R-TST-03） |
| 性能基准 | 5% | 大批量 SQL 审核吞吐、EXPLAIN 连接池效率 | `criterion` |

### 6.2 CI 质量门禁

```yaml
# .github/workflows/ci.yml（参考 ogsql-parser / metamorphosis CI 模式）
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace
  clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy }
      - run: cargo clippy --workspace -- -D warnings   # M-FMT-01
  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt }
      - run: cargo fmt --all -- --check
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/install-action@cargo-audit
      - run: cargo audit
```

### 6.3 依赖注入与可测试性

所有 audit crate 通过 `cr-core` 中定义的 trait 与外部依赖解耦，便于测试时注入 Mock 实现（R-TST-01）：

```rust
/// 数据库连接抽象（cr-core 定义，零 IO 依赖）。
///
/// 使用 async_trait 因为 EXPLAIN 执行涉及网络 IO。
#[async_trait::async_trait]
pub trait DbConnection: Send + Sync {
    /// 执行 EXPLAIN 并返回原始 TEXT 输出。
    async fn execute_explain(&self, sql: &str) -> Result<String, DbError>;
    /// 校验连接用户权限（启动时调用）。
    async fn validate_permissions(&self) -> Result<(), DbError>;
}

/// 测试时注入 Mock 实现。
#[cfg(test)]
mod tests {
    struct MockDb { plans: HashMap<String, String> }

    #[async_trait::async_trait]
    impl DbConnection for MockDb {
        async fn execute_explain(&self, sql: &str) -> Result<String, DbError> {
            self.plans.get(sql).cloned().ok_or(DbError::ConnectionFailed {
                host: "mock".into(), port: 0, reason: "no plan".into(),
            })
        }
        async fn validate_permissions(&self) -> Result<(), DbError> { Ok(()) }
    }
}
```

---

## 七、配置与连接安全

### 7.1 配置文件 `.roughcollie.toml`

```toml
[project]
name = "my-gauss-app"
root = "/path/to/repo"

# === 数据库预置连接（核心配置）===
[database]
enabled = true                      # 是否启用真实 EXPLAIN
host = "10.0.1.100"
port = 5432
database = "roughcollie_audit"      # 建议专用审核库，与生产隔离
username = "roughcollie"
# 密码优先级：环境变量 GAUSSDB_PASSWORD > OS 密钥链 > 配置文件（不推荐明文）
password_env = "GAUSSDB_PASSWORD"
ssl_mode = "verify-full"            # 或 "disable"（内网测试）
auth_method = "sha256"              # sha256 / sm3 / scram-sha-256

[database.explain]
timeout_seconds = 30                # EXPLAIN 超时
max_cost_warning = 10000            # 代价超过此值报 Warning
max_cost_critical = 50000           # 代价超过此值报 Critical，阻断 CI
buffers_threshold = 100000          # 磁盘读块数阈值
enable_analyze = true               # 是否执行 EXPLAIN ANALYZE（有副作用，默认 true）
enable_buffers = true               # 是否显示 Buffers

[database.security]
# 启动时强制检查：连接用户不得拥有 DML/DDL 权限
enforce_readonly = true
allowed_commands = ["EXPLAIN", "SET", "SHOW", "SELECT pg_sleep"]

# === 审核规则 ===
[rules.ogexplain]
# 继承 ogexplain-core 全部 28 条规则
preset = "all"
# 或自定义：preset = ["SCAN-001", "TYPE-001", "JOIN-001"]
severity_override = { "SCAN-001" = "critical", "TYPE-001" = "warning" }

[rules.astgrep]
preset = ["gaussdb-security", "java-sql-injection", "mybatis-xss"]
severity_threshold = "warning"

[rules.complexity]
enabled = true
baseline_compare = true             # 与基线对比增量
warning_delta = 10                  # 复杂度上升 10 分报 Warning
critical_delta = 25                 # 复杂度上升 25 分报 Critical

[output]
format = "markdown"                 # markdown / json / sarif
path = "./audit-report.md"
exit_code_on_critical = 1           # Critical 时退出码非 0（阻断 CI）
exit_code_on_warning = 0            # Warning 时退出码 0（仅报告）
```

### 7.2 安全加固

| 措施 | 实现 |
|------|------|
| **最小权限** | 连接用户仅授予 `CONNECT`、`EXPLAIN` 权限（通过 `cr-db/security.rs` 启动时执行 `SELECT has_database_privilege()` 等校验） |
| **密码管理** | 优先使用 OS 密钥链（macOS Keychain / Windows Credential / Linux secret-tool），次选环境变量，禁止日志打印（M-LOG-04） |
| **SQL 注入防护** | EXPLAIN 执行前，用 `ogsql-parser` 解析确认语句为纯 DML/SELECT，拒绝含 `;` 的多语句或注释注入 |
| **超时熔断** | `EXPLAIN` 超时自动取消连接，防止长查询拖垮审核库 |
| **网络隔离** | 建议审核库为独立测试实例，通过 VPN/专线连接，禁止直连生产 |

---

## 八、命令设计

### 8.1 命令行接口

```bash
# 基础用法（需预置 .roughcollie.toml）
coderc audit \
  --baseline main \
  --files $(git diff --name-only origin/main) \
  --output-format markdown

# 显式指定数据库连接（覆盖配置文件）
coderc audit \
  --baseline main \
  --files src/mapper/OrderMapper.xml,src/sql/proc.sql \
  --db-host 10.0.1.100 \
  --db-name roughcollie_audit \
  --db-user roughcollie \
  --db-password-env GAUSSDB_PASSWORD

# 纯静态模式（无数据库连接）
coderc audit \
  --baseline main \
  --files src/mapper/OrderMapper.xml \
  --no-db                         # 强制禁用 EXPLAIN，仅静态规则
```

### 8.2 退出码约定

Severity 枚举仅有 `Critical` / `Warning` / `Info` 三级（来自 ogexplain-core，不引入 `Error`），退出码映射如下：

| 退出码 | 含义 | 触发条件 |
|--------|------|---------|
| 0 | 通过 | 无 Finding，或所有 Finding 严重度 ≤ `Info` |
| 1 | 阻断 | 存在 `Critical` 级别 Finding（`exit_code_on_critical = 1`） |
| 2 | 工具错误 | 配置解析失败、连接超时等工具自身故障 |
| 3 | 降级执行 | 审核完成但发生了降级（如 EXPLAIN 不可用，回退静态分析） |

> `Warning` 级别默认不阻断（退出码 0），可通过配置 `exit_code_on_warning = 1` 改为阻断。

---

## 九、企业级扩展设计

以下从插件体系、多数据库、API 化、规则市场、合规、高可用等维度描述企业级扩展方向。

### 9.1 插件化规则体系

允许企业在不修改 CodeRoughcollie 源码的前提下，以插件形式加载自定义审核规则。

#### 插件 ABI 安全策略（关键设计决策）

Rust trait 跨动态库边界是不安全的——vtable 布局与编译器版本绑定，`&'static str` 生命周期在 dlopen 后无效。CodeRoughcollie 采用 **C ABI 封装层** 解决此问题：

```
┌─────────────────────────────────────────────┐
│  宿主进程 (coderc)                           │
│  ┌──────────────────┐  ┌─────────────────┐  │
│  │ cr-plugin        │  │ C ABI 兼容层     │  │
│  │ (Rust)           │──│ abi.rs          │  │
│  │ loader.rs        │  │ - 版本协商       │  │
│  │ registry.rs      │  │ - JSON 序列化    │  │
│  └──────────────────┘  │ - 内存隔离       │  │
│                        └────┬────────────┘  │
└─────────────────────────────┼───────────────┘
                              │ C FFI (extern "C")
                    ┌─────────▼─────────┐
                    │ 插件 .so/.dylib    │
                    │ 可用任意 Rust 版本 │
                    │ 编译，只要实现     │
                    │ C ABI 入口函数     │
                    └───────────────────┘
```

**C ABI 入口约定**：

```rust
// cr-plugin/src/abi.rs

/// 插件必须导出的 C 函数（固定签名，跨 Rust 版本兼容）。
///
/// 输入：JSON 序列化的 AuditContext
/// 输出：JSON 序列化的 Vec<Finding>
/// 返回值：0 = 成功，非 0 = 错误码
pub type cr_plugin_run_t =
    unsafe extern "C" fn(ctx_json: *const u8, ctx_len: usize,
                         out_buf: *mut *mut u8, out_len: *mut usize) -> i32;

/// 插件元信息查询函数。
pub type cr_plugin_info_t =
    unsafe extern "C" fn() -> *const CrPluginInfo;

#[repr(C)]
pub struct CrPluginInfo {
    /// ABI 版本（用于兼容性校验，不匹配则拒绝加载）
    pub abi_version: u32,
    /// 插件名称
    pub name: *const c_char,
    /// 规则数量
    pub rule_count: u32,
}
```

**安全保障**：

| 风险 | 缓解措施 |
|------|---------|
| ABI 版本不匹配 | 加载时检查 `abi_version`，不匹配则拒绝并报错 |
| 插件 panic | C FFI 边界使用 `catch_unwind`（M-FFI-01），panic 不传播到宿主 |
| 内存安全 | 输入/输出通过 JSON 序列化传递，不跨边界传递 Rust 指针 |
| 恶意代码 | 插件沙箱（`sandbox.rs`）限制文件系统/网络访问（三期完善） |

#### 规则注册

插件内部实现规则逻辑后，通过 C ABI 导出。`cr-plugin` 的 `loader.rs` 在宿主侧将 C 函数包装回 Rust trait 对象：

```rust
// cr-plugin/src/loader.rs

/// 加载单个插件动态库。
///
/// # Safety
///
/// 调用者必须确保 path 指向可信的动态库文件。
///
/// # Errors
///
/// 返回 `RoughcollieError::Plugin` 当 ABI 版本不匹配或导出函数缺失。
pub unsafe fn load_plugin(path: &Path) -> Result<PluginInstance, RoughcollieError> {
    // SAFETY: 调用者负责确保 path 安全（M-UNS-02）
    let lib = unsafe { libloading::Library::new(path) }
        .map_err(|e| RoughcollieError::Plugin(format!("加载失败: {e}")))?;

    // 查询 ABI 版本
    let info: cr_plugin_info_t = unsafe {
        *lib.get(b"cr_plugin_info\0")
            .map_err(|e| RoughcollieError::Plugin(format!("无 info 函数: {e}")))?
    };
    let plugin_info = unsafe { &*info() };

    if plugin_info.abi_version != CR_ABI_VERSION {
        return Err(RoughcollieError::Plugin(format!(
            "ABI 版本不匹配: 期望 {}, 实际 {}",
            CR_ABI_VERSION, plugin_info.abi_version
        )));
    }

    // 包装为 trait 对象...
    Ok(PluginInstance { lib, info: plugin_info.clone() })
}
```

```toml
# .roughcollie.toml — 插件配置
[plugins]
# 插件搜索路径（系统路径使用 coderc 命名）
paths = ["./cr-plugins/", "/usr/local/share/coderc/plugins/"]

# 按名称启用/禁用
enabled = ["my-company-rules", "pci-dss-checks"]
disabled = ["experimental-*"]
```

### 9.2 多数据库支持

| 数据库 | 支持阶段 | 说明 |
|--------|---------|------|
| **openGauss / GaussDB** | 一期核心 | 原生支持，完整 EXPLAIN + 专属算子诊断 |
| **PostgreSQL** | 二期 | 共享 `ogsql-parser` 基础，适配 EXPLAIN 输出格式差异 |
| **MySQL** | 二期 | 使用 `mysql` crate，适配 `EXPLAIN FORMAT=JSON` |
| **Oracle** | 三期 | 需专用 parser 适配 PL/SQL 语法 |

架构中的 `trait DbConnection`（§ 6.3）保证了数据库层的可替换性：

```rust
// cr-db 可以针对不同数据库提供不同实现
pub struct GaussDbConnection { /* ... */ }  // impl DbConnection
pub struct PostgresConnection { /* ... */ } // impl DbConnection
pub struct MysqlConnection { /* ... */ }    // impl DbConnection
```

### 9.3 API 与 MCP Server

除了 CLI，提供以下集成入口：

| 接口形式 | 用途 | 优先级 |
|---------|------|--------|
| **MCP Server** | AI 编程助手（Claude/Cursor）直接调用审核 | 二期 |
| **gRPC API** | 微服务间高性能调用，支持流式审核结果 | 三期 |
| **REST API** | Web Dashboard、第三方平台集成 | 三期 |
| **GitHub App** | 原生 PR 审核集成，无需配置 CI workflow | 三期 |

#### MCP Server 设计（对齐 metamorphosis mcp-server 模式）

使用 `rmcp` v0.16 SDK，参考 metamorphosis 的 `#[rmcp::tool_router]` 宏模式：

```rust
// cr-mcp-server/src/server.rs

/// 无状态 MCP Server，每次请求独立执行审核。
pub struct CodeRoughcollieServer {
    pub(crate) tool_router: ToolRouter<Self>,
}

#[rmcp::tool_router(vis = "pub(crate)")]
impl CodeRoughcollieServer {
    #[rmcp::tool(
        name = "audit_files",
        description = "审核指定文件列表，返回所有 Finding"
    )]
    async fn audit_files(&self, Parameters(params): Parameters<AuditParams>) -> String {
        match tools::audit_files(params).await {
            Ok(result) => serde_json::to_string_pretty(&result)
                .unwrap_or_else(|e| format!(r#"{{"error": "序列化失败: {e}"}}"#)),
            Err(e) => serde_json::to_string_pretty(&ErrorResponse { error: e.to_string() })
                .unwrap_or_else(|_| r#"{"error": "unknown"}"#.into()),
        }
    }

    #[rmcp::tool(
        name = "explain_sql",
        description = "对单条 SQL 执行 EXPLAIN 并返回诊断"
    )]
    async fn explain_sql(&self, Parameters(params): Parameters<SqlParams>) -> String { /* ... */ }

    #[rmcp::tool(
        name = "list_rules",
        description = "列出所有可用规则及其元数据"
    )]
    async fn list_rules(&self) -> String { /* ... */ }

    #[rmcp::tool(
        name = "compare_baseline",
        description = "对比两个版本间的复杂度变化"
    )]
    async fn compare_baseline(&self, Parameters(params): Parameters<BaselineParams>) -> String { /* ... */ }

    #[rmcp::tool(
        name = "suggest_fix",
        description = "对 Finding 生成修复建议（对接 metamorphosis）"
    )]
    async fn suggest_fix(&self, Parameters(params): Parameters<FixParams>) -> String { /* ... */ }
}

#[rmcp::tool_handler]
impl ServerHandler for CodeRoughcollieServer {}

/// 启动 stdio 传输（供 Claude Desktop / Cursor 调用）。
pub async fn run_stdio() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = CodeRoughcollieServer::default();
    let transport = rmcp::transport::io::stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
```

MCP Server 文件布局（对齐 metamorphosis）：

```
cr-mcp-server/
├── src/
│   ├── lib.rs           # 模块导出
│   ├── server.rs        # rmcp tool_router + ServerHandler impl
│   ├── tools.rs         # 工具实现（调用 cr-audit-* crates）
│   └── types.rs         # 工具参数/响应类型（JsonSchema derive）
└── Cargo.toml           # 依赖 rmcp + cr-audit-* + schemars
```

### 9.4 合规规则包

为企业提供预配置的合规规则包，满足行业监管要求：

| 规则包 | 覆盖内容 | 示例规则 |
|--------|---------|---------|
| **PCI-DSS** | 支付卡数据安全 | 敏感字段加密检测、审计日志完整性 |
| **SOC 2** | 服务组织控制 | 访问控制审计、变更管理追溯 |
| **HIPAA** | 医疗数据保护 | PHI 字段脱敏、最小权限原则 |
| **GDPR** | 个人数据保护 | PII 数据匿名化、数据最小化 |
| **内部安全基线** | 企业自定义 | SQL 注入检测、权限提升检测 |

```toml
# .roughcollie.toml — 合规规则包配置
[rules.compliance]
enabled = true
packages = ["PCI-DSS", "SOC2"]
severity = "critical"  # 合规规则违反默认报 Critical，阻断 CI
```

### 9.5 审核结果存储与分析

```rust
/// 持久化的审核记录（cr-core 定义）。
#[non_exhaustive]
pub struct AuditRecord {
    /// 唯一审核 ID
    pub audit_id: Uuid,
    /// 关联的 Git commit SHA
    pub commit_sha: String,
    /// 分支名
    pub branch: String,
    /// 审核时间
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// 所有 Finding
    pub findings: Vec<Finding>,
    /// 审核耗时统计
    pub duration: std::time::Duration,
    /// 是否发生降级
    pub degraded: bool,
    /// 使用的规则集版本
    pub rules_version: String,
}
```

支持存储后端：

| 后端 | 适用场景 |
|------|---------|
| **本地 JSON Lines** | 单机开发、小团队 |
| **SQLite** | 中等规模，无需外部依赖 |
| **PostgreSQL** | 企业级，支持复杂趋势查询 |

### 9.6 通知与集成

```toml
# .roughcollie.toml — 通知配置
[notifications]
enabled = true

[notifications.slack]
webhook_url_env = "SLACK_WEBHOOK_URL"
channel = "#db-audit-alerts"
# 仅通知 Critical 级别
min_severity = "critical"

[notifications.email]
smtp_host = "smtp.company.com"
recipients = ["dba-team@company.com", "security@company.com"]

[notifications.webhook]
url = "https://internal.company.com/hooks/audit"
# 发送完整 JSON 报告
include_full_report = true
```

### 9.7 高可用与水平扩展

| 场景 | 方案 |
|------|------|
| **审核库只读副本** | 支持配置多个只读 GaussDB 端点，连接池自动故障转移 |
| **审核任务队列** | 大批量（如全量仓库扫描）通过 Redis 队列异步执行，CLI 提交后轮询结果 |
| **结果缓存** | 对同一 commit SHA 的重复审核命中缓存，避免重复 EXPLAIN 执行 |
| **并发控制** | 可配置最大并发 EXPLAIN 数，防止打爆审核库 |

---

## 十、版本与发布策略

### 10.1 版本号

遵循语义化版本（SemVer）：`MAJOR.MINOR.PATCH`

| 变更类型 | 版本递增 | 示例 |
|---------|---------|------|
| 公开 API 不兼容变更 | MAJOR | `0.2.0 → 1.0.0` |
| 向后兼容的功能新增 | MINOR | `0.2.0 → 0.3.0` |
| 向后兼容的 Bug 修复 | PATCH | `0.2.0 → 0.2.1` |

### 10.2 发布渠道

参考 ogsql-parser / metamorphosis 的 release.yml 跨平台构建模式：

| 渠道 | 命令 | 适用场景 |
|------|------|---------|
| **GitHub Release**（二进制） | 从 Release 页下载 `coderc-{version}-linux-x86_64.tar.gz` | CI/CD 集成、无 Rust 工具链的环境 |
| **Cargo 安装** | `cargo install coderc` | 开发者本地使用 |
| **Docker 镜像** | `docker pull ghcr.io/c2j/coderc:{version}` | 预置 GaussDB 连接依赖的容器化 CI |

跨平台构建使用 `cargo-zigbuild`（参考 ogsql-parser release.yml）：

| 目标平台 | 构建方式 |
|---------|---------|
| linux-x86_64 (glibc ≥ 2.17) | `cargo zigbuild --target x86_64-unknown-linux-gnu.2.17` |
| linux-arm64 | `cargo zigbuild --target aarch64-unknown-linux-gnu.2.17` |
| windows-x86_64 | `cargo build --target x86_64-pc-windows-gnu`（静态链接 CRT） |

### 10.3 提交规范

遵循 Conventional Commits（与 CONTRIBUTING.md 一致）：

```
feat(audit-explain): add Vector operator detection
fix(db): handle connection timeout gracefully
docs(design): update architecture with plugin system
test(core): add proptest for severity aggregation
refactor(config): extract TOML parsing to cr-config
chore(deps): update rust-opengauss to 0.4
```

---

## 十一、实施路线（分四期，共 32 周）

### 首期：CLI 核心 + 静态审核（Week 1-8）

**目标**：可运行、可集成 CI，覆盖静态反模式 + 安全扫描。

| 周 | 模块 | 任务 | 产出 |
|----|------|------|------|
| 1-2 | 骨架 | 初始化 Workspace（按 2.1 布局）、CLI 参数解析（clap）、Git diff 解析、CI 门禁（ci.yml） | `coderc --help` 可用，CI 门禁就绪 |
| 3-4 | 静态 SQL | 集成 `ogsql-parser` + `ogexplain-core` 规则库（静态部分），实现 AST 反模式检测 | 支持 28 条规则中的纯静态规则（如 `TYPE-001`、`SUBQ-001`） |
| 5-6 | 安全扫描 | 集成 `astgrep` crate，对 Java/MyBatis 做增量安全扫描 | 支持 SQL 注入、XSS 检测 |
| 7-8 | 报告 + CI | Markdown/JSON 输出、退出码策略、Dogfood workflow、`cargo test --workspace` 全绿 | 可接入 CI 做门禁，单元测试覆盖 ≥ 80% |

**里程碑**：`v0.1.0` — 纯静态审核可用，无数据库连接依赖。

---

### 二期：真实 EXPLAIN + 复杂度（Week 9-16）

**目标**：接入 GaussDB 预置连接，实现"静态 + 动态"混合审核。

| 周 | 模块 | 任务 | 产出 |
|----|------|------|------|
| 9-10 | 数据库连接 | `cr-db` 模块：封装 `rust-opengauss`，连接池、认证、权限校验 | 可安全连接 GaussDB，启动时校验只读权限 |
| 11-12 | EXPLAIN 执行器 | 参数占位符推断填充、EXPLAIN 执行、超时熔断、错误降级 | 对变更 SQL 执行真实 EXPLAIN |
| 13-14 | 执行计划分析 | 调用 `ogexplain-core::analyze()` 解析执行计划，28 条规则 + openGauss 特有算子诊断 | 输出含实际代价、Buffers、算子类型的 Finding |
| 15-16 | 复杂度 + 配置 + 可观测性 | 集成 `ogsql-complexity::gauss_analyze()`、基线对比（Δ 计算）、TOML 配置系统、`tracing` + `metrics` | 复杂度增量门禁可用，日志结构化输出 |

**里程碑**：`v0.2.0` — 混合审核完整可用，需预置 GaussDB 连接。

---

### 三期：插件化 + 生态编排 + MCP（Week 17-24）

**目标**：支持企业级自定义规则、成为生态编排中心、AI 助手可调用。

| 周 | 模块 | 任务 | 产出 |
|----|------|------|------|
| 17-18 | 影响分析 | 子进程调用 `codeweb` CLI（`codeweb impact --file`），变更 Java/Mapper 时查询上下游调用链 | 输出 `IMPACT` 类型 Finding |
| 19-20 | MCP Server | `cr-mcp-server` crate：基于 `rmcp` v0.16，暴露 5 个工具（audit_files / explain_sql / list_rules / compare_baseline / suggest_fix） | AI 助手（Claude/Cursor）可直接调用审核 |
| 21-22 | 插件系统 | `cr-plugin` crate: C ABI 封装层（`libloading` + `catch_unwind`）、版本协商、规则注册表 | 企业可通过 `.so` 插件增加自定义规则 |
| 23-24 | SARIF + 自动修复 | SARIF 输出（兼容 GitHub Advanced Security）、对接 `metamorphosis` 生成补丁建议 | 平台集成与自动修复原型 |

**里程碑**：`v0.3.0` — MCP Server + 插件系统上线。

---

### 四期：企业级特性（Week 25-32）

**目标**：多数据库、合规检查、API 化、通知系统、趋势分析。

| 周 | 模块 | 任务 | 产出 |
|----|------|------|------|
| 25-26 | 多数据库 | PostgreSQL EXPLAIN 适配、MySQL 基础支持 | 三个数据库上均可执行静态审核 + EXPLAIN |
| 27-28 | 合规规则包 | PCI-DSS / SOC2 / 内部安全基线预置规则、`rules.compliance` 配置段 | 企业可一键启用合规检查 |
| 29-30 | API + 通知 | gRPC / REST API、Slack / Email / Webhook 通知、结果持久化（SQLite / PostgreSQL） | 平台集成与自动化告警 |
| 31-32 | 趋势分析 | 审核历史趋势、团队级统计 | Dashboard 原型 |

**里程碑**：`v1.0.0` — 企业级代码审核平台正式版。

---

## 十二、CI/CD 集成

CI 和 Release 工作流参考 ogsql-parser / metamorphosis 的成熟模式。

### 12.1 CI 流水线（`.github/workflows/ci.yml`）

4 个并行 Job：`test` / `clippy` / `fmt` / `audit`，任一失败即阻断合并。

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: true       # 拉取 ogexplain-analyzer 等子模块
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --workspace

  clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { submodules: true }
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy }
      - run: cargo clippy --workspace -- -D warnings

  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt }
      - run: cargo fmt --all -- --check

  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: taiki-e/install-action@cargo-audit
      - run: cargo audit
```

### 12.2 发布流水线（`.github/workflows/release.yml`）

Tag `v*` 触发，跨平台构建（`cargo-zigbuild`），自动创建 GitHub Release。

```yaml
name: Build & Release
on:
  push:
    tags: ["v*"]
  workflow_dispatch:
    inputs:
      version: { description: "Release version (e.g. v0.1.0)", required: true }

permissions:
  contents: write

env:
  BINARY_NAME: coderc

jobs:
  build-linux-x86_64:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
        with: { submodules: true }
      - uses: dtolnay/rust-toolchain@stable
      - uses: mlugg/setup-zig@v1
        with: { version: "0.13.0" }
      - run: cargo install cargo-zigbuild
      - run: cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.17
      - run: strip target/x86_64-unknown-linux-gnu/release/${{ env.BINARY_NAME }}
      - run: |
          cd target/x86_64-unknown-linux-gnu/release
          tar czf ../../../coderc-${{ github.ref_name }}-linux-x86_64.tar.gz ${{ env.BINARY_NAME }}
      - uses: actions/upload-artifact@v4
        with:
          name: coderc-linux-x86_64
          path: coderc-${{ github.ref_name }}-linux-x86_64.tar.gz

  build-linux-arm64:
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
        with: { submodules: true }
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: aarch64-unknown-linux-gnu }
      - uses: mlugg/setup-zig@v1
        with: { version: "0.13.0" }
      - run: cargo install cargo-zigbuild
      - run: cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.17
      - run: |
          sudo apt-get install -y binutils-aarch64-linux-gnu
          aarch64-linux-gnu-strip target/aarch64-unknown-linux-gnu/release/${{ env.BINARY_NAME }}
      - run: |
          cd target/aarch64-unknown-linux-gnu/release
          tar czf ../../../coderc-${{ github.ref_name }}-linux-arm64.tar.gz ${{ env.BINARY_NAME }}
      - uses: actions/upload-artifact@v4
        with:
          name: coderc-linux-arm64
          path: coderc-${{ github.ref_name }}-linux-arm64.tar.gz

  build-windows-x86_64:
    runs-on: windows-2022
    steps:
      - uses: actions/checkout@v4
        with: { submodules: true }
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: x86_64-pc-windows-gnu }
      - run: cargo build --release --target x86_64-pc-windows-gnu
        env:
          RUSTFLAGS: "-C target-feature=+crt-static"
      - shell: pwsh
        run: |
          Compress-Archive `
            -Path "target/x86_64-pc-windows-gnu/release/${{ env.BINARY_NAME }}.exe" `
            -DestinationPath "coderc-${{ github.ref_name }}-windows-x86_64.zip"
      - uses: actions/upload-artifact@v4
        with:
          name: coderc-windows-x86_64
          path: coderc-${{ github.ref_name }}-windows-x86_64.zip

  release:
    needs: [build-linux-x86_64, build-linux-arm64, build-windows-x86_64]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with: { path: artifacts }
      - uses: softprops/action-gh-release@v2
        with:
          tag_name: ${{ github.ref_name }}
          generate_release_notes: true
          files: |
            artifacts/**/*.tar.gz
            artifacts/**/*.zip
```

### 12.3 Dogfood（`.github/workflows/dogfood.yml`）

CodeRoughcollie 审核自身代码变更：

```yaml
name: Dogfood
on: [pull_request]

jobs:
  self-audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0, submodules: true }

      - name: Install coderc
        run: |
          curl -L https://github.com/c2j/CodeRoughcollie/releases/latest/download/coderc-linux-x86_64.tar.gz | tar xz
          chmod +x coderc && sudo mv coderc /usr/local/bin/

      - name: Run self-audit
        run: |
          coderc audit \
            --baseline origin/${{ github.base_ref }} \
            --files $(git diff --name-only origin/${{ github.base_ref }}) \
            --output-format markdown \
            --output-path audit-report.md

      - name: Comment PR
        if: always()
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

---

## 十三、关键设计决策总结

| 决策 | 理由 |
|------|------|
| **反模式规则复用 `ogexplain-core`** | 避免重复造规则，确保诊断口径与执行计划分析一致 |
| **真实 EXPLAIN 为核心竞争力** | 静态工具无法感知统计信息、索引选择性、实际行数；预置连接让审核从"猜测"变为"测量" |
| **复杂度评分复用 `ogsql-complexity`** | 生态统一评分标准，`gauss_analyze()` 提供存储过程 11 维度评分 |
| **Severity 直接复用 ogexplain 三级** | 不引入 `Error` 级别。`Critical` / `Warning` / `Info` 覆盖所有场景，退出码策略通过配置映射 |
| **连接安全优先** | 只读权限校验 + 超时熔断 + 密码环境变量，确保 CI 服务器不会误操作数据库 |
| **降级策略** | 无连接时自动回退静态分析，保证工具在任何环境都能输出有价值的结果 |
| **Trait 抽象解耦外部依赖** | `DbConnection`、`AuditRule` 等 trait 定义在 `cr-core`（零 IO），使得测试、多数据库、插件化均可无痛扩展 |
| **Cargo Workspace 单向依赖** | 遵循 M-ARCH-01，`cr-core` 位于最底层，禁止被任何上层 crate 反向依赖 |
| **thiserror + 禁止 anyhow 在 lib** | 遵循 M-ERR-01，确保库使用者可以获得精确的错误类型进行分支处理 |
| **插件 C ABI 封装层** | Rust trait vtable 跨动态库不安全，C ABI + JSON 序列化确保插件可用不同 Rust 版本编译 |
| **MCP Server 优先于 REST API** | AI 辅助编程是当前趋势，MCP 协议让 Claude/Cursor 原生调用审核能力，生态价值最高 |

---

## 十四、相关文档索引

| 文档 | 用途 |
|------|------|
| [CONTRIBUTING.md](./CONTRIBUTING.md) | 强制编码规则 + 项目结构与贡献流程 |
| [BEST-PRATICE.md](./BEST-PRATICE.md) | 推荐最佳实践（Code Review 参考） |
