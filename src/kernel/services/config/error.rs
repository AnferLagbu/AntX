#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯类型定义。
//! 配置校验结果类型 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/config/error.rs, 2026-06-16 提取到 services.
//! 纯类型定义, 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export.

//! 配置校验结果类型

use core::fmt;

/// 配置校验结果.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// 检测到的 CPU 数超过 `MAX_CPUS`.
    CpuCountExceedsMax { actual: u32, max: usize },
    /// 内存布局不一致 (例如 `PAGE_SIZE` 不是 2 的幂).
    MemoryLayoutInvalid,
    /// 检测到的中断控制器 (APIC/IOAPIC/PIC) 未初始化.
    IrqControllerUnavailable,
    /// 跨模块常量冲突.
    InconsistentConstant {
        name: &'static str,
        lhs: u64,
        rhs: u64,
    },
    /// 驱动特定配置非法.
    DriverConfigInvalid(&'static str),
    /// Slab 默认大小不是 2 的幂.
    SlabNotPowerOfTwo,
    /// Slab 默认大小未与页大小对齐.
    SlabMisaligned,
    /// Slab 默认大小超过合理上限.
    SlabTooLarge,
    /// 栈大小不是页大小的整数倍.
    StackMisaligned,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::CpuCountExceedsMax { actual, max } => {
                write!(f, "CPU count {actual} exceeds MAX_CPUS {max}")
            }
            ConfigError::MemoryLayoutInvalid => write!(f, "memory layout invalid"),
            ConfigError::IrqControllerUnavailable => {
                write!(f, "no interrupt controller initialized")
            }
            ConfigError::InconsistentConstant { name, lhs, rhs } => {
                write!(
                    f,
                    "constant {name} mismatch: config.rs={lhs} vs submodule={rhs}"
                )
            }
            ConfigError::DriverConfigInvalid(name) => {
                write!(f, "driver {name} misconfigured")
            }
            ConfigError::SlabNotPowerOfTwo => write!(f, "slab default size is not power of two"),
            ConfigError::SlabMisaligned => write!(f, "slab default size is not page-aligned"),
            ConfigError::SlabTooLarge => write!(f, "slab default size exceeds upper bound"),
            ConfigError::StackMisaligned => write!(f, "stack size is not a multiple of page size"),
        }
    }
}
