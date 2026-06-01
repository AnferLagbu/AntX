//! Configuration validation result types

/// Configuration validation result.
#[derive(Debug, Clone, Copy)]
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
}
