//! CPU 缓存信息检测模块
//!
//! 提供缓存层级、大小、关联性等信息的检测功能。
//! 详细实现在 cpu/mod.rs 中的 `detect_cache()` 函数。

/// 缓存信息结构体 (定义在 cpu/mod.rs)
/// 此文件作为模块占位符, 实际逻辑由父模块统一管理

// ✅ P0-4 修复: 移除错误的模板占位符 ($module)
// cache.rs 和 topology.rs 作为子模块声明存在,
// 但核心检测逻辑集中在 mod.rs 中以保持一致性

#[cfg(test)]
mod tests {
    #[test]
    fn test_cache_module_exists() {
        // 验证模块可正常编译
        assert!(true);
    }
}
