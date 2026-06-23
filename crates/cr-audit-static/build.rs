//! 静态审核 crate 构建脚本。
//!
//! 仅在 `embed-astgrep` feature 启用时执行嵌入工作：
//! 1. 过滤 `lib/cr-rules/**/rules/*.yaml` 到 `$OUT_DIR/cr-rules/`，剔除 `cases/`、`examples/`、`checklist/`。
//! 2. 定位 `astgrep` release 二进制（环境变量 `ASTGREP_RELEASE_BIN` 优先；否则按候选路径探查），
//!    拷贝到 `$OUT_DIR/astgrep-bin`。
//! 3. 通过 `cargo:rustc-env` 暴露两个路径给 `include_dir!` / `include_bytes!`。
//!
//! 二进制缺失时：dev 场景发出警告并跳过嵌入（runner 运行时走外部查找）；
//! CI/release 场景应通过 `ASTGREP_RELEASE_BIN` 显式指定，缺失即报错（`CARGO_BUILD_EMBED_REQUIRED=1`）。

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

fn main() {
    // 始终 watch 规则目录与 astgrep 源以触发增量重建
    println!("cargo:rerun-if-changed=../../lib/cr-rules");
    println!("cargo:rerun-if-env-changed=ASTGREP_RELEASE_BIN");

    let embed_enabled = std::env::var("CARGO_FEATURE_EMBED_ASTGREP").is_ok();
    if !embed_enabled {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by Cargo"));
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let workspace_root = manifest_dir.ancestors().nth(2).expect("crate sits at <workspace>/crates/cr-audit-static");

    // ── 1. 过滤规则文件 ────────────────────────────────────────────────
    let rules_src = workspace_root.join("lib/cr-rules");
    let rules_dst = out_dir.join("cr-rules");
    if rules_dst.exists() {
        fs::remove_dir_all(&rules_dst).ok();
    }
    fs::create_dir_all(&rules_dst).expect("create cr-rules dst");

    let mut rule_count = 0usize;
    copy_rules_tree(&rules_src, &rules_dst, &mut rule_count);
    println!("cargo:warning=cr-audit-static: embedded {rule_count} rule files");

    // ── 2. 定位 astgrep 二进制 ─────────────────────────────────────────
    let bin_dst = out_dir.join("astgrep-bin");
    match resolve_astgrep_binary(workspace_root) {
        Ok(src_path) => {
            copy_binary(&src_path, &bin_dst).expect("copy astgrep binary");
            println!(
                "cargo:warning=cr-audit-static: embedded astgrep binary from {} ({} bytes)",
                src_path.display(),
                fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0)
            );
        }
        Err(reason) => {
            let required = std::env::var("CARGO_BUILD_EMBED_REQUIRED").is_ok();
            if required {
                panic!(
                    "embed-astgrep 要求 astgrep 二进制，但 {reason}。
                     请预先构建：`(cd lib/astgrep && cargo build --release)`
                     或显式指定：`ASTGREP_RELEASE_BIN=/path/to/astgrep cargo build`"
                );
            }
            println!("cargo:warning=cr-audit-static: embed-astgrep 启用但未找到 astgrep 二进制（{reason}）；runner 将在运行时降级到外部查找");
            fs::write(&bin_dst, b"").expect("write placeholder");
        }
    }

    // ── 3. 生成包含字面路径的 .rs 供 include_dir!/include_bytes! 使用 ──
    // include_dir! 在宏展开前检查 token，拒绝 concat!(env!(...))；改由 build.rs 把
    // 绝对路径写入生成的源文件，运行侧用 include!() 拉取。
    let generated_path = out_dir.join("astgrep_embed_statics.rs");
    let rules_dst_display = rules_dst.display().to_string().replace('\\', "\\\\");
    let bin_dst_display = bin_dst.display().to_string().replace('\\', "\\\\");
    let generated = format!(
        "pub static EMBEDDED_BIN: &[u8] = include_bytes!(\"{bin_dst_display}\");\n\
         pub static EMBEDDED_RULES: include_dir::Dir<'static> =\n\
         \x20   include_dir::include_dir!(\"{rules_dst_display}\");\n"
    );
    fs::write(&generated_path, generated).expect("write generated statics file");
}

/// 递归拷贝 `lib/cr-rules/**/rules/*.yaml`（保留 `{lang}/{category}/rules/...` 相对结构）。
fn copy_rules_tree(src: &Path, dst: &Path, rule_count: &mut usize) {
    let entries = match fs::read_dir(src) {
        Ok(e) => e,
        Err(err) => {
            println!("cargo:warning=cr-audit-static: 无法读取规则目录 {}: {err}", src.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // 命中 `rules` 目录：整树拷贝其中所有 .yaml
            if path.file_name().map(|n| n == "rules").unwrap_or(false) {
                let rel = path.strip_prefix(src).unwrap_or(&path);
                let target = dst.join(rel);
                if let Err(err) = copy_dir(&path, &target, rule_count) {
                    println!("cargo:warning=cr-audit-static: 拷贝规则 {} 失败: {err}", path.display());
                }
            } else {
                copy_rules_tree(&path, dst, rule_count);
            }
        }
    }
}

fn copy_dir(src: &Path, dst: &Path, rule_count: &mut usize) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &dst.join(entry.file_name()), rule_count)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            fs::copy(&path, dst.join(entry.file_name()))?;
            *rule_count += 1;
        }
    }
    Ok(())
}

fn copy_binary(src: &Path, dst: &Path) -> io::Result<()> {
    let mut src_file = fs::File::open(src)?;
    let mut dst_file = fs::File::create(dst)?;
    // 标准库的 fs::copy 不会保留 0600 -> 0755 权限语义；显式设置可执行位
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = src_file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst_file.write_all(&buf[..n])?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dst, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// 按 ASTGREP_RELEASE_BIN > lib/astgrep/target/{release,debug}/astgrep[.exe] > 同级 astgrep 仓库候选。
fn resolve_astgrep_binary(workspace_root: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("ASTGREP_RELEASE_BIN") {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("ASTGREP_RELEASE_BIN={p} 不是文件"));
    }
    let exe_ext = if cfg!(windows) { ".exe" } else { "" };
    let mut candidates: Vec<PathBuf> =
        vec![workspace_root.join(format!("lib/astgrep/target/release/astgrep{exe_ext}"))];
    if let Some(parent) = workspace_root.parent() {
        candidates.push(parent.join(format!("astgrep/target/release/astgrep{exe_ext}")));
    }
    candidates.push(workspace_root.join(format!("lib/astgrep/target/debug/astgrep{exe_ext}")));
    for c in &candidates {
        if c.is_file() {
            return Ok(c.clone());
        }
    }
    Err(format!(
        "未在候选路径找到 astgrep 二进制：{}",
        candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
    ))
}
