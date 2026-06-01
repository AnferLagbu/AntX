//! Configuration validation result types

use core::fmt;

/// Configuration validation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Detected CPU count exceeds `MAX_CPUS`.
    CpuCountExceedsMax { actual: u32, max: usize },
    /// Memory layout inconsistency (e.g. PAGE_SIZE not power of 2).
    MemoryLayoutInvalid,
    /// Detected IRQ controller (APIC/IOAPIC/PIC) is not initialized.
    IrqControllerUnavailable,
    /// Cross-module constant conflict.
    InconsistentConstant {
        name: &'static str,
        lhs: u64,
        rhs: u64,
    },
    /// Driver-specific configuration is invalid.
    DriverConfigInvalid(&'static str),
    /// Slab default size is not power of two.
    SlabNotPowerOfTwo,
    /// Slab default size is not aligned to page size.
    SlabMisaligned,
    /// Slab default size exceeds a sane upper bound.
    SlabTooLarge,
    /// Stack size is not a multiple of page size.
    StackMisaligned,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::CpuCountExceedsMax { actual, max } => {
                write!(f, "CPU count {} exceeds MAX_CPUS {}", actual, max)
            }
            ConfigError::MemoryLayoutInvalid => write!(f, "memory layout invalid"),
            ConfigError::IrqControllerUnavailable => write!(f, "no interrupt controller initialized"),
            ConfigError::InconsistentConstant { name, lhs, rhs } => {
                write!(
                    f,
                    "constant {} mismatch: config.rs={} vs submodule={}",
                    name, lhs, rhs
                )
            }
            ConfigError::DriverConfigInvalid(name) => {
                write!(f, "driver {} misconfigured", name)
            }
            ConfigError::SlabNotPowerOfTwo => write!(f, "slab default size is not power of two"),
            ConfigError::SlabMisaligned => write!(f, "slab default size is not page-aligned"),
            ConfigError::SlabTooLarge => write!(f, "slab default size exceeds upper bound"),
            ConfigError::StackMisaligned => write!(f, "stack size is not a multiple of page size"),
        }
    }
}
