//! 插件加载层。
//!
//! C ABI 安全封装层：通过 JSON 序列化在宿主和插件之间传递数据，
//! 避免 Rust trait vtable 跨动态库边界的 ABI 不兼容问题。

pub mod abi;
pub mod loader;
pub mod registry;
