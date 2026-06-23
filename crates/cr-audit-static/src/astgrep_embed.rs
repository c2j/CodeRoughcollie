//! 内嵌资源释放（仅 `embed-astgrep` feature 启用）。
//!
//! `build.rs` 把 astgrep 二进制与 `cr-rules/**/*.yaml` 写入 `$OUT_DIR`，
//! 本模块在编译期将其嵌入 `coderc`，运行时首次释放到用户缓存目录并复用。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use include_dir::Dir;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EmbedError {
    #[error("无法确定缓存目录：{0}")]
    NoCacheDir(String),
    #[error("释放 {dest} 失败：{source}")]
    Release {
        dest: String,
        #[source]
        source: io::Error,
    },
    #[error("内嵌资源为空（构建时未找到 astgrep 二进制）。请启用 CARGO_BUILD_EMBED_REQUIRED=1 重新构建")]
    Empty,
}

include!(concat!(env!("OUT_DIR"), "/astgrep_embed_statics.rs"));

fn cache_root() -> Result<PathBuf, EmbedError> {
    let base = dirs::cache_dir().ok_or_else(|| EmbedError::NoCacheDir("dirs::cache_dir 返回 None".into()))?;
    Ok(base.join("coderc"))
}

fn astgrep_version_stamp() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 释放内嵌的 astgrep 二进制到缓存目录；如已存在且大小一致则复用。
pub fn materialize_astgrep_binary() -> Result<PathBuf, EmbedError> {
    if EMBEDDED_BIN.is_empty() {
        return Err(EmbedError::Empty);
    }
    let root = cache_root()?;
    let target_dir = root.join("astgrep");
    let target = target_dir.join(format!("astgrep-{}", astgrep_version_stamp()));
    if target.exists() && matches!(fs::metadata(&target), Ok(m) if m.len() == EMBEDDED_BIN.len() as u64) {
        return Ok(target);
    }
    fs::create_dir_all(&target_dir).map_err(|source| EmbedError::Release {
        dest: target_dir.display().to_string(),
        source,
    })?;
    let mut f = fs::File::create(&target).map_err(|source| EmbedError::Release {
        dest: target.display().to_string(),
        source,
    })?;
    f.write_all(EMBEDDED_BIN).map_err(|source| EmbedError::Release {
        dest: target.display().to_string(),
        source,
    })?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).map_err(|source| EmbedError::Release {
            dest: target.display().to_string(),
            source,
        })?;
    }
    Ok(target)
}

/// 释放内嵌规则树到缓存目录；目录名带版本戳记以避免冲突。
pub fn materialize_rules_root() -> Result<PathBuf, EmbedError> {
    let root = cache_root()?;
    let target = root.join("cr-rules").join(format!("v-{}", astgrep_version_stamp()));
    if target.join(".materialized").exists() {
        return Ok(target);
    }
    fs::create_dir_all(&target).map_err(|source| EmbedError::Release {
        dest: target.display().to_string(),
        source,
    })?;
    extract_dir(&EMBEDDED_RULES, &target)?;
    fs::write(target.join(".materialized"), b"").map_err(|source| EmbedError::Release {
        dest: target.display().to_string(),
        source,
    })?;
    Ok(target)
}

fn extract_dir(dir: &Dir<'_>, base: &Path) -> Result<(), EmbedError> {
    for entry in dir.entries() {
        let path = base.join(entry.path());
        match entry {
            include_dir::DirEntry::Dir(d) => {
                fs::create_dir_all(&path).map_err(|source| EmbedError::Release {
                    dest: path.display().to_string(),
                    source,
                })?;
                extract_dir(d, &path)?;
            }
            include_dir::DirEntry::File(f) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|source| EmbedError::Release {
                        dest: parent.display().to_string(),
                        source,
                    })?;
                }
                fs::write(&path, f.contents()).map_err(|source| EmbedError::Release {
                    dest: path.display().to_string(),
                    source,
                })?;
            }
        }
    }
    Ok(())
}
