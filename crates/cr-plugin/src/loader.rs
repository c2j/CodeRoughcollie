//! 动态库加载器。
//!
//! 使用 `libloading` 加载插件 `.so` / `.dylib`，通过 C ABI 调用。

use std::path::Path;

use crate::abi::{CrPluginInfoFn, CrPluginRunFn, PluginError, CR_ABI_VERSION};

/// 已加载的插件实例。
pub struct PluginInstance {
    info: crate::abi::CrPluginInfo,
    run_fn: CrPluginRunFn,
    _lib: libloading::Library,
}

impl PluginInstance {
    /// 加载插件动态库。
    ///
    /// # Errors
    ///
    /// 当文件不存在、ABI 版本不匹配或导出函数缺失时返回 `PluginError`。
    pub fn load(path: &Path) -> Result<Self, PluginError> {
        let lib = unsafe { libloading::Library::new(path) }.map_err(|e| PluginError::LoadFailed(e.to_string()))?;

        let info_fn: CrPluginInfoFn =
            unsafe { *lib.get(b"cr_plugin_info\0").map_err(|e| PluginError::MissingSymbol(e.to_string()))? };

        // SAFETY: 调用者负责确保 path 指向可信的动态库（M-UNS-02）
        let info_ptr = unsafe { info_fn() };
        // SAFETY: info_ptr 由插件返回，指向静态数据
        let info = unsafe { &*info_ptr };

        if info.abi_version != CR_ABI_VERSION {
            return Err(PluginError::AbiVersionMismatch { expected: CR_ABI_VERSION, actual: info.abi_version });
        }

        let run_fn: CrPluginRunFn =
            unsafe { *lib.get(b"cr_plugin_run\0").map_err(|e| PluginError::MissingSymbol(e.to_string()))? };

        Ok(Self {
            info: crate::abi::CrPluginInfo {
                abi_version: info.abi_version,
                name: info.name,
                version: info.version,
                rule_count: info.rule_count,
            },
            run_fn,
            _lib: lib,
        })
    }

    /// 执行插件审计。
    ///
    /// # Errors
    ///
    /// 当插件执行返回错误码时返回 `PluginError::ExecutionFailed`。
    pub fn run(&self, ctx_json: &[u8]) -> Result<Vec<u8>, PluginError> {
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: usize = 0;

        // SAFETY: 插件承诺正确处理输入并分配输出缓冲区。
        // catch_unwind 防止插件 panic 传播到宿主进程（M-FFI-01）。
        let panic_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: 同上
            unsafe { (self.run_fn)(ctx_json.as_ptr(), ctx_json.len(), &mut out_ptr, &mut out_len) }
        }));

        let ret = match panic_result {
            Ok(code) => code,
            Err(_) => return Err(PluginError::ExecutionFailed("插件 panic（catch_unwind 捕获）".into())),
        };

        if ret != 0 {
            return Err(PluginError::ExecutionFailed(format!("返回码: {ret}")));
        }

        if out_ptr.is_null() || out_len == 0 {
            return Ok(Vec::new());
        }

        // SAFETY: 插件承诺 out_ptr 指向有效内存，长度为 out_len
        let result = unsafe { std::slice::from_raw_parts(out_ptr, out_len) }.to_vec();

        // SAFETY: 插件使用 malloc 分配，宿主使用 free 释放
        unsafe { libc::free(out_ptr as *mut libc::c_void) };

        Ok(result)
    }

    /// 插件名称。
    #[must_use]
    pub fn name(&self) -> &str {
        if self.info.name.is_null() {
            return "<unknown>";
        }
        // SAFETY: name 在加载时已验证为有效 C 字符串
        unsafe { std::ffi::CStr::from_ptr(self.info.name) }.to_str().unwrap_or("<invalid utf-8>")
    }

    /// 规则数量。
    #[must_use]
    pub const fn rule_count(&self) -> u32 {
        self.info.rule_count
    }
}
