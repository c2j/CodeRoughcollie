# Phase 1 骨架实施计划（Week 1-2）

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 搭建 CodeRoughcollie Workspace 骨架，12 个 crate 可编译，CLI 可运行 `--help`，CI 门禁就绪。

**Architecture:** Cargo Workspace 单向依赖，`cr-core` 零 IO，submodule 引入生态依赖（ogexplain-analyzer、ogsql-parser、rust-opengauss、astgrep）。

**Tech Stack:** Rust 2021 / clap / thiserror / tracing / ogexplain-core / ogsql-parser

---

## Task 1: Git Submodule 初始化

**Files:**
- Create: `.gitmodules`
- Init: `lib/ogsql-parser`, `lib/ogexplain-analyzer`, `lib/rust-opengauss`, `lib/astgrep`, `lib/metamorphosis`, `lib/codeweb`

**Steps:**
1. 写入 `.gitmodules`
2. `git submodule update --init --recursive`
3. Commit: `chore: initialize git submodules`

---

## Task 2: Workspace 根配置

**Files:**
- Create: `Cargo.toml`（Workspace 根）
- Create: `rustfmt.toml`
- Create: `clippy.toml`
- Create: `rust-toolchain.toml`

**Steps:**
1. 写入 Workspace Cargo.toml（12 members）
2. 写入 rustfmt.toml（参考 ogsql-parser）
3. 写入 clippy.toml
4. 写入 rust-toolchain.toml（MSRV 1.75）
5. Commit: `chore: workspace root configuration`

---

## Task 3: cr-core 骨架 + 核心类型

**Files:**
- Create: `crates/cr-core/Cargo.toml`
- Create: `crates/cr-core/src/lib.rs`
- Create: `crates/cr-core/src/types.rs` — Severity re-export, AuditContext
- Create: `crates/cr-core/src/traits.rs` — DbConnection trait, AuditRule trait
- Create: `crates/cr-core/src/error.rs` — RoughcollieError, DbError
- Create: `crates/cr-core/src/engine.rs` — placeholder
- Create: `crates/cr-core/src/baseline.rs` — placeholder
- Create: `crates/cr-core/src/scoring.rs` — placeholder

**Steps:**
1. Cargo.toml: 依赖 thiserror, serde, async-trait
2. error.rs: RoughcollieError + DbError
3. types.rs: re-export ogexplain Severity/Finding/DiagnosticCategory, 定义 AuditContext
4. traits.rs: DbConnection async trait
5. lib.rs: module 声明 + re-export
6. `cargo build -p cr-core` 通过
7. Commit: `feat(core): core types, traits, and error definitions`

---

## Task 4: cr-db / cr-git / cr-config / cr-report 骨架

**Files:**
- 各 crate 的 Cargo.toml + src/lib.rs（placeholder）

**Steps:**
1. 逐一创建 4 个 crate 的 Cargo.toml + lib.rs
2. `cargo build --workspace` 通过
3. Commit: `feat: scaffold cr-db, cr-git, cr-config, cr-report`

---

## Task 5: cr-audit-* 骨架（4 个 audit crate）

**Files:**
- cr-audit-static, cr-audit-explain, cr-audit-complexity, cr-audit-impact

**Steps:**
1. 各 crate Cargo.toml 依赖 cr-core
2. lib.rs placeholder
3. `cargo build --workspace` 通过
4. Commit: `feat: scaffold audit crates`

---

## Task 6: cr-plugin + cr-mcp-server 骨架（三期 placeholder）

**Files:**
- cr-plugin, cr-mcp-server（最小骨架，编译通过即可）

**Steps:**
1. Cargo.toml + lib.rs
2. Commit: `feat: scaffold cr-plugin and cr-mcp-server (phase 3 placeholders)`

---

## Task 7: cr-cli 命令行入口

**Files:**
- Create: `crates/cr-cli/Cargo.toml`
- Create: `crates/cr-cli/src/main.rs` — clap CLI

**Steps:**
1. Cargo.toml: clap + 依赖所有一期 crate
2. main.rs: clap derive 定义 audit 子命令
3. `cargo run -p cr-cli -- --help` 输出帮助信息
4. Commit: `feat(cli): clap CLI entry point with audit subcommand`

---

## Task 8: CI Workflow 文件

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`
- Create: `.github/workflows/dogfood.yml`

**Steps:**
1. ci.yml: test/clippy/fmt/audit 四 job
2. release.yml: cargo-zigbuild 跨平台构建
3. dogfood.yml: PR 自审核（v0.1.0 前仅模板）
4. Commit: `ci: add ci, release, and dogfood workflows`

---

## Task 9: 示例配置 + README

**Files:**
- Create: `.roughcollie.toml.example`
- Create: `README.md`

**Steps:**
1. 配置模板
2. README 基本信息表
3. Commit: `docs: add example config and README`
