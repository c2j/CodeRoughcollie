# AGENTS.md

CodeRoughcollie 仓库的 AI Agent 与人类协作约定。

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
