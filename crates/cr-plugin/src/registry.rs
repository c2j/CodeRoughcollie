//! 规则注册表。
//!
//! 管理内置规则和已加载的插件规则。

use std::path::PathBuf;

use crate::loader::PluginInstance;

/// 规则注册表。
#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<PluginInstance>,
}

impl PluginRegistry {
    /// 创建空注册表。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 从指定目录加载所有插件。
    ///
    /// # Errors
    ///
    /// 当任何插件加载失败时返回 `PluginError`。
    pub fn load_from_dir(&mut self, dir: &PathBuf) -> Result<usize, crate::abi::PluginError> {
        let mut count = 0;
        if !dir.exists() {
            return Ok(0);
        }
        for entry in std::fs::read_dir(dir).map_err(|e| crate::abi::PluginError::LoadFailed(e.to_string()))? {
            let entry = entry.map_err(|e| crate::abi::PluginError::LoadFailed(e.to_string()))?;
            let path = entry.path();
            if is_shared_lib(&path) {
                match PluginInstance::load(&path) {
                    Ok(plugin) => {
                        tracing::info!(name = plugin.name(), rules = plugin.rule_count(), "插件已加载");
                        self.plugins.push(plugin);
                        count += 1;
                    }
                    Err(e) => {
                        tracing::warn!(path = %path.display(), error = %e, "跳过插件");
                    }
                }
            }
        }
        Ok(count)
    }

    /// 已加载的插件数量。
    #[must_use]
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// 是否为空。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// 获取所有已加载插件。
    #[must_use]
    pub fn plugins(&self) -> &[PluginInstance] {
        &self.plugins
    }
}

fn is_shared_lib(path: &std::path::Path) -> bool {
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy();
        ext == "so" || ext == "dylib" || ext == "dll"
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_registry() {
        let r = PluginRegistry::new();
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn test_load_nonexistent_dir() {
        let mut r = PluginRegistry::new();
        let result = r.load_from_dir(&std::path::PathBuf::from("/nonexistent/path"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0);
    }
}
