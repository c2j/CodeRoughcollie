# AGENTS.md

CodeRoughcollie 仓库的 AI Agent 与人类协作约定。

## TDD 工作流（Red → Green → Refactor）

本仓库采用测试驱动开发。一次循环只锁定一个行为：先写会失败的测试（Red），再写最小实现让它通过（Green），最后在测试全绿的前提下重构（Refactor）。探索草稿不得直接合入，必须按本文件用 TDD 重写。

### 先读再改
1. 确认改动落在哪个 crate（本仓库是 Cargo workspace，见「仓库地图」）。
2. 只用本文件列出的 cargo 命令；不要发明裸 `cargo update`、不要擅自切换 toolchain（以 `rust-toolchain.toml` 为准）。
3. 先跑与改动相关的最小测试；提交前再跑 workspace 门禁（fmt + clippy + test）。
4. 完成一个循环后按「完成标准与汇报」汇报，不要只说「做完了」。

### Never / Ask first / Always

**Never（不必请示，直接禁止）**
- 删除、注释、跳过已有测试：`#[ignore]`、注释掉 `#[test]`、把断言改成 `is_ok()` / `unwrap()` 了事
- 修改人类已有测试的断言来迁就实现
- 先提交无测试的业务行为，再「回头补」
- 写永真测试：无断言、只检查 `is_some()`、只 verify 调用次数不查参数与状态
- 用全量端到端测试覆盖本可单测完成的改动
- 提交半成品；每次对人类可见的结果必须能构建且相关测试为绿
- 把探索草稿、临时脚本、调试 `dbg!`/`println!` 留在主代码

**Ask first**
- 改人类已有测试（含断言、fixture、snapshot）
- 新增运行时依赖、`unsafe`、新的 workspace crate、新的外部服务
- 为不可测代码做超出当前改动路径的重构
- 接受/更新 snapshot（insta / golden file）且行为含义发生变化
- 关闭 clippy lint、新增 `#[allow]`

**Always**
- 改遗留路径前：先写特征测试，锁定当前可观察行为（允许丑，必须可重复）
- 新行为：先有会失败的行为断言，再写最少实现
- 难以测试时：先造接缝，再写测试（见「遗留代码与接缝」）
- 测试名描述行为：`should_reject_negative_amount`
- 现有测试因你的改动失败：修实现，不修测试（除非人类明确要求）

测试权限：

| 测试来源 | 权限 |
|---|---|
| 人类已有测试 | 只读 |
| 本任务新建测试 | 可改，直到该行为稳定 |
| 过时或环境偶发失败 | 只报告，不擅自跳过 |

### 工作流

**Red** — 写生产行为之前先写测试；测试必须能被收集且必须失败（断言失败，或因缺失 API 导致编译失败，二者都算合法 Red）。修改已有功能先写特征测试锁定当前输出。一次只加一个行为的测试。

**Green** — 只写让当前失败测试通过的最少代码。禁止删掉/改掉失败测试、一次引入多个未验证变更、用更宽断言或 `unwrap()` 换绿。

**Refactor** — 相关测试全绿后才重构；重构后立刻跑同一组测试；范围限于当前 crate。

**探索 vs 实现** — 需求或方案不清可写草稿验证；草稿不得合并；方案确定后必须走 TDD 重写。

### 遗留代码与接缝

**特征测试** — 锁定现有行为，不是证明它正确。用固定 fixture 或 `insta` snapshot。更新 snapshot 必须在汇报里写清 diff 含义；默认不接受「看起来差不多」。

**接缝（优先顺序，靠后的更差）**
1. trait + 泛型或 `impl Trait`，测试用假类型
2. 用类型去掉非法状态（enum / newtype），而不是在测试里补分支
3. 时钟、ID、熵、文件系统做成可注入依赖；测试用 `tempfile` / 内存实现
4. `unsafe` 不是接缝。新增 `unsafe` 必须 Ask first，并写 `SAFETY` 注释

只给即将修改的代码路径补测试，不要一次性给整个模块「补全覆盖率」。

### 测试分层

| 层级 | 位置 | 测什么 |
|---|---|---|
| 单元 | `src` 内 `#[cfg(test)] mod tests` | 模块不变量、错误类型、状态转换 |
| 集成 | `tests/*.rs` | 公共 API；不可访问私有项 |
| 文档测试 | `///` 示例 | 公共 API 必须可运行；禁止滥用 `no_run` |
| CLI/二进制 | 项目惯用方式 | 退出码与 stdout 契约 |
| 不变量 | `proptest`（项目已用时） | 往返解析、幂等、单调性 |
| 特征/快照 | `insta` 或固定 fixture | 遗留输出；接受 snapshot 必须说明 |

不要把本该测公共契约的内容塞进 `#[cfg(test)]` 去读私有字段。

Rust 的 Red 允许是：测试引用了尚不存在的类型/函数导致编译失败。不要为了先编译而写空 `todo!()` 实现再补测试——可以留 `todo!()` 仅作为 Green 的最小占位，且下一步必须替换。

### Rust Never 补遗
- 库代码（非 main/example/测试）用 `unwrap` / `expect` / `panic!` 做控制流
- 无必要 `unsafe`；有则必须 `SAFETY` 注释
- 一次性 `cargo update` 整个 lockfile
- 用 `#[allow(...)]` 静默应修复的 lint
- 为绿而改 snapshot 却不解释行为是否应该变

### 命令

```bash
# 单测（单 crate，按测试名过滤）
cargo test -p <crate> <test_name>

# 提交前门禁（CI 门禁，顺序：fmt → clippy → test）
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

> **CRITICAL**: TDD 在子模块目录内不适用——`lib/<name>/` 的改动必须走上游仓库（见「子模块策略」）。本文件的 TDD 工作流仅针对本仓库自有 crate（crates/）。子模块内禁止跑 `cargo fmt --all`（会污染子模块工作树），只用 `cargo fmt -p <crate>` 或 `cargo fmt --all -- --check`。

循环内只跑受影响 crate；提交前再 workspace。

### 完成标准与汇报

提交或交还人类前，确认：
- [ ] 新行为有失败→通过的测试
- [ ] 修改的遗留路径有特征测试
- [ ] 未删除、跳过、改写人类已有测试
- [ ] 已跑与改动匹配的门禁（fmt + clippy + test）
- [ ] `cargo fmt` 与 clippy 干净
- [ ] 没有把草稿、调试输出、无主 lockfile 大面积变更带上

每个 TDD 循环汇报：
1. 测试了什么行为（测试函数名）
2. 最小实现改了哪些文件
3. 是否重构、边界在哪
4. 实际执行的命令和结果（通过 / 失败原因；不要只写「测过了」）

### 质量判断（自我检查）
- 这条测试在实现写错时会失败吗？
- 我是否在测行为，而不是私有实现细节？
- 我是否用 skip、更宽断言、unwrap、snapshot 盲收换绿？
- 命令是否来自本文件，而不是我编的？

---

## 子模块（Submodule）策略 —— 强制规则

本仓库通过 git submodule 引用下列上游项目,**严禁在子模块目录内直接修改源码**:

| 子模块目录 | 上游仓库 | 用途 |
|---|---|---|
| `lib/ogsql-parser` | https://github.com/c2j/ogsql-parser | SQL 解析器 |
| `lib/ogexplain-analyzer` | https://github.com/c2j/ogexplain-analyzer | EXPLAIN 计划分析、复杂度评估 |
| `lib/metamorphosis` | https://github.com/c2j/metamorphosis | SQL 重写规则引擎 |
| `lib/rust-opengauss` | https://github.com/c2j/rust-opengauss | openGauss/GaussDB Rust 驱动（`gaussdb` facade + `tokio-opengauss`） |
| `lib/codeweb` | https://github.com/c2j/codeweb | 语义代码图谱分析器（Java/Mapper/SQL/存储过程 调用链 + 增量与影响分析） |
| `lib/astgrep` | https://github.com/c2j/astgrep | AST grep 引擎 |
| `lib/cr-rules` | git@github.com:c2j/cr-rules.git | 审核规则集 |

### 为什么不能直接改

子模块目录(`lib/<name>/`)的内容由上游仓库的某个 commit 固定。在本仓库内直接编辑这些文件会产生:

1. **脏工作树** — `git submodule status` 标 `+`,后续 `git submodule update` 可能丢失修改
2. **回退陷阱** — 任何人 clone 或更新子模块时,你的本地修改会被上游 commit 静默覆盖
3. **丢失归属** — 改动无法追溯,代码评审与发布流程失效
4. **升级冲突** — 下次升级子模块到新 main 时,本地修改与上游变更大面积冲突(本项目已因此丢弃过两份 stash,见历史分析)

### 正确流程

**如果在子模块里发现 bug 或需要新特性:**

1. 进入对应的上游仓库(见上表)
2. 在该仓库提 Issue 描述需求,或直接发 PR
3. 等待上游合并并发布新 commit / tag
4. 在本仓库执行升级流程(见下节),把子模块指针推进到上游新 commit
5. 在本仓库的 commit 里记录升级内容与新行为

**只在以下情况允许在子模块目录内执行 git 命令:**
- `git fetch` / `git log` / `git diff` 等只读操作
- `git checkout <commit>` / `git merge --ff-only origin/main` 等升级流程的一部分
- 临时调试(`git stash`),但**必须在离开前 drop 或 pop 回原状**

### 子模块升级流程

当需要把某个子模块推进到上游最新版本时:

```bash
# 1. 进入子模块目录
cd lib/<name>

# 2. 确认工作树干净(必须!)
git status --porcelain   # 应该为空

# 3. 拉取上游 main
git fetch origin
git checkout main
git merge --ff-only origin/main

# 4. 回到仓库根,记录指针变更
cd ../..
git add lib/<name>
git commit -m "chore(submodule): bump <name> to <new-short-sha>"
```

如果子模块工作树脏(有未提交修改),**必须先 stash 或 commit 到上游仓库**,然后再升级。绝不要带着脏工作树做 `git submodule update`。

### 避免污染子模块工作树(常见陷阱)

子模块通过 path dependency 被 workspace 引入,仓库根的若干命令会**静默波及**子模块源码,留下脏工作树。以下是已踩过的坑与防范:

| 陷阱 | 后果 | 防范 |
|---|---|---|
| 在仓库根跑 `cargo fmt --all`(不带 `--check`) | workspace 内所有 crate(含子模块)被格式化,产生大量 fmt-only 改动 | 只用 `cargo fmt -p <具体 crate>`,或提交前改用 `cargo fmt --all -- --check` 做只检查 |
| 在子模块内跑测试,insta 等库留下 `*.snap.new` | 子模块工作树出现未跟踪产物,父仓库标记 `modified content` | 测试后用 `cargo insta reject` 或手动清理;跑前确认子模块干净 |
| 误在子模块内创建嵌套目录(如 `lib/<name>/lib/<name>/`) | 留下未跟踪垃圾,父仓库持续显示子模块脏 | 调试脚本注意 CWD;离开前 `git -C lib/<name> status --porcelain` 必须为空 |

**在子模块内执行任何构建/测试/格式化之前,先确认 `git -C lib/<name> status --porcelain` 为空;操作之后再次确认。** 发现脏工作树立即 `git restore .` 并清理未跟踪产物,绝不要把脏状态留给下一次 `git submodule update`。

---

## 仓库其他约定

### 提交规范

- 语义化前缀:`feat:` / `fix:` / `chore:` / `refactor:` / `docs:` / `test:` / `perf:` / `ci:`
- 英文 commit message
- 一次 commit 只做一件事;多文件改动按目录/关注点拆分

### 提交前检查项（CI 强制门禁）

每次提交/推送前**必须**本地通过以下全部检查。CI（`.github/workflows/ci.yml`）在 push 时执行相同的 fmt/clippy/test 门禁，任一失败即阻断合并:

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

> **格式检查覆盖全部 crate**:`cargo fmt --all` 对 workspace 的**每一个**成员跑 fmt。即便本次只改一个 crate,历史格式债务（如某 crate 上一处 `pub use` 顺序）也会让 CI 红灯。养成提交前 `cargo fmt --all` 的习惯,不要只格式化当前改动文件。
>
> **Release 交叉编译**:`.github/workflows/release.yml` 用 `cargo zigbuild -p cr-cli` 只构建 coderc 二进制,避免拉入其他 workspace 成员的系统 OpenSSL 依赖（交叉编译环境无 libssl）。修改构建范围时务必保持 `-p cr-cli` 限定。

### 详细文档

- 设计文档:`docs/Design.md`
- 用户指南:`docs/UserGuide.md`
- 最佳实践:`docs/BEST-PRATICE.md`
