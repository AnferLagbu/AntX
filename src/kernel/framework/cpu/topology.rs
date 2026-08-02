//! CPU 多核拓扑信息检测模块
//!
//! 提供物理核心数、逻辑线程数、APIC ID 等拓扑信息的检测功能。
//! 详细实现在 cpu/mod.rs 中的 `detect_topology()` 函数。

/// 拓扑信息结构体 (定义在 cpu/mod.rs)
/// 此文件作为模块占位符, 实际逻辑由父模块统一管理

// ✅ P0-4 修复: 移除错误的模板占位符 ($module)
// topology.rs 作为子模块声明存在,
// 但核心检测逻辑集中在 mod.rs 中以保持一致性

#[cfg(test)]
mod tests {
    #[test]
    fn test_topology_module_exists() {
        // 验证模块可正常编译
        assert!(true);
    }
}
