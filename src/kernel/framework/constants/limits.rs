//! TCB 内部容量上限常量
//!
//! 集中 framework 层自治容量常量, 与 `framework::config::*` 职责正交:
//!
//! | 模块 | 职责 | 暴露给 services |
//! |---|---|---|
//! | `framework::constants` | TCB 内部实现细节 (MMIO 上限, 锁深度, ...)| 否 |
//! | `framework::config` | services 公共 API 桥接 (sysctl, 调参)| 是 |
//!
//! 按 framekernel §4.2 资源分类, 敏感资源 (如硬件容量上限) 归 framework
//! 内部, 不暴露给 services. 强类型 (usize) const 不可变, 单点定义.
#![allow(dead_code)] // 各模块按需使用, 编译期 dead_code lint 不触发

/// MMIO 别名注册表最大容量
///
/// **超限行为**: `iomem::AliasRegistry::register` 返回
/// `Err("MMIO alias registry full")` (iomem.rs:55), 调用方按需处理.
pub const MAX_MMIO_MAPPINGS: usize = 64;

/// Lockdep 锁类最大数量
///
/// **超限行为**: lockdep 满时**静默截断** (放弃检测), 不 panic. 锁
/// 仍正常工作, 仅失去死锁检测覆盖.
pub const MAX_LOCK_CLASSES: usize = 64;

/// 每线程/每 CPU 最大持有锁深度
///
/// **超限行为**: 同 MAX_LOCK_CLASSES, 静默截断.
pub const MAX_HELD_LOCKS: usize = 8;
