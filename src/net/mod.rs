/// 网络子系统 Rust 模块
/// 
/// 提供 lwIP 协议栈的操作系统抽象层（OSAL）和 AntX 内核集成。
/// 
/// ## 架构概览
/// 
/// ```text
/// src/net/
/// ├── mod.rs          # 模块导出
/// ├── sys_arch.rs     # lwIP OS 抽象层 (信号量/互斥锁/邮箱/线程)
/// ├── init.rs         # 网络子系统初始化
/// ├── netif.rs        # 网络接口管理 (DHCP/IPv6)
/// └── types.rs        # 类型定义和常量
/// ```
/// 
/// ## 设计原则
/// 
/// - **不修改 lwIP 源码** - 保持第三方协议栈原样
/// - **FFI 兼容层** - 提供与 C 版本相同的 ABI 接口
/// - **类型安全** - 使用 Rust 的所有权系统增强安全性
/// - **零成本抽象** - 关键路径无额外开销
/// 
/// ## 安全性改进 (相比 C 版本)
/// 
/// - **原子操作**: 所有全局状态使用 Atomic 类型，消除 data race
/// - **状态机**: 初始化过程使用有限状态机，防止重复初始化
/// - **RAII**: 资源自动清理，防止内存泄漏
/// - **边界检查**: 数组访问和指针操作都有安全保证

pub mod types;
pub mod sys_arch;
pub mod init;      // ✅ Phase 2: 已启用
pub mod netif;     // ✅ Phase 2: 已启用

// 重新导出核心类型 (方便其他模块使用)
pub use types::*;
pub use init::{is_network_initialized, get_init_state};
pub use netif::{is_dhcp_done, reset_dhcp_state};
