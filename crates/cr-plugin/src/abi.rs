//! C ABI 兼容层定义。
//!
//! 插件必须导出两个 C 函数：`cr_plugin_info` 和 `cr_plugin_run`。
//! 宿主通过 `libloading` 加载动态库，调用这两个函数。

use std::ffi::c_char;

/// 当前 C ABI 版本号。
///
/// 加载插件时校验此版本，不匹配则拒绝加载。
pub const CR_ABI_VERSION: u32 = 1;

/// 插件元信息（C ABI 兼容结构体）。
///
/// # Safety
///
/// 此结构体使用 `#[repr(C)]`，保证跨编译器布局一致。
/// 所有字符串字段为 C 字符串（以 null 结尾），由插件负责分配和保持有效。
#[repr(C)]
pub struct CrPluginInfo {
    /// ABI 版本，必须等于 `CR_ABI_VERSION`。
    pub abi_version: u32,
    /// 插件名称（C 字符串）。
    pub name: *const c_char,
    /// 插件版本（C 字符串）。
    pub version: *const c_char,
    /// 包含的规则数量。
    pub rule_count: u32,
}

/// 插件审计函数签名。
///
/// 输入：JSON 序列化的审计上下文（包含 SQL 文本）。
/// 输出：JSON 序列化的 `Vec<cr_core::Finding>`。
///
/// 宿主分配输出缓冲区，插件写入后返回大小。
///
/// # Safety
///
/// 此类型仅用于 C FFI 边界。调用者必须确保指针有效。
pub type CrPluginRunFn =
    unsafe extern "C" fn(ctx_json: *const u8, ctx_len: usize, out_buf: *mut *mut u8, out_len: *mut usize) -> i32;

/// 插件信息查询函数签名。
///
/// # Safety
///
/// 此类型仅用于 C FFI 边界。
pub type CrPluginInfoFn = unsafe extern "C" fn() -> *const CrPluginInfo;

/// 插件加载错误。
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PluginError {
    /// 动态库加载失败。
    #[error("加载插件失败: {0}")]
    LoadFailed(String),
    /// 导出函数缺失。
    #[error("插件缺少必需的导出函数: {0}")]
    MissingSymbol(String),
    /// ABI 版本不匹配。
    #[error("ABI 版本不匹配: 期望 {expected}, 实际 {actual}")]
    AbiVersionMismatch { expected: u32, actual: u32 },
    /// 插件执行错误。
    #[error("插件执行错误: {0}")]
    ExecutionFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_version_positive() {
        assert!(CR_ABI_VERSION > 0);
    }

    #[test]
    fn test_load_failed_display() {
        assert!(PluginError::LoadFailed("test".into()).to_string().contains("test"));
    }

    #[test]
    fn test_version_mismatch_display() {
        let e = PluginError::AbiVersionMismatch { expected: 1, actual: 2 };
        assert!(e.to_string().contains('1'));
        assert!(e.to_string().contains('2'));
    }
}
