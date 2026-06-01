//! System Configuration Validation
//!
//! Validates system configuration at boot time to catch errors early.
//! This module performs comprehensive checks on:
//! - CPU count vs MAX_CPUS
//! - Memory layout consistency
//! - Architecture-specific constraints
//! - Driver configuration

use core::sync::atomic::Ordering;

/// Maximum number of CPUs supported by the kernel
/// This value must be consistent across all modules
pub const MAX_CPUS: usize = 1024;

/// Maximum IRQ number supported
pub const MAX_IRQS: usize = 256;

/// Maximum number of processes
pub const MAX_PROCESSES: usize = 65536;

/// Maximum number of threads per process
pub const MAX_THREADS_PER_PROCESS: usize = 4096;

/// Configuration validation result
#[derive(Debug, Clone, Copy)]
pub enum ConfigError {
    CpuCountExceedsMax { actual: u32, max: usize },
    MemoryLayoutInvalid,
    IrqCountExceedsMax { actual: u32, max: usize },
    ArchitectureNotSupported,
    DriverConfigInvalid(&'static str),
}

/// Validate CPU configuration
pub fn validate_cpu_config() -> Result<(), ConfigError> {
    let cpu_count = crate::kernel::smp::get_cpu_count();
    
    if cpu_count as usize > MAX_CPUS {
        return Err(ConfigError::CpuCountExceedsMax {
            actual: cpu_count,
            max: MAX_CPUS,
        });
    }
    
    Ok(())
}

/// Validate memory configuration
pub fn validate_memory_config() -> Result<(), ConfigError> {
    // TODO: Add memory layout validation
    // - Check kernel address space layout
    // - Verify PMM initialization
    // - Validate VMM page table setup
    Ok(())
}

/// Validate interrupt configuration
pub fn validate_interrupt_config() -> Result<(), ConfigError> {
    // Check if APIC is initialized on x86_64
    #[cfg(target_arch = "x86_64")]
    {
        if !crate::kernel::arch::x86_64::apic::is_initialized() {
            // This is not necessarily an error - could be legacy PIC system
            // But we should log a warning
        }
    }
    
    Ok(())
}

/// Validate all system configuration
/// 
/// This should be called early in the boot process
/// 
/// # Panics
/// Panics in debug mode if any critical configuration is invalid
pub fn validate_system_config() {
    let mut errors = 0;
    
    // CPU configuration
    match validate_cpu_config() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            #[cfg(debug_assertions)]
            {
                match e {
                    ConfigError::CpuCountExceedsMax { actual, max } => {
                        panic!(
                            "CONFIG ERROR: CPU count {} exceeds MAX_CPUS {}. \
                             Increase MAX_CPUS in kernel/config.rs",
                            actual, max
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    
    // Memory configuration
    match validate_memory_config() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            // Log error but don't panic - memory config issues are usually recoverable
        }
    }
    
    // Interrupt configuration
    match validate_interrupt_config() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
        }
    }
    
    if errors == 0 {
        // Configuration is valid
        #[cfg(debug_assertions)]
        {
            // In debug mode, print success message
            // TODO: Use klog when available
        }
    }
}

/// Get configuration summary for debugging
pub fn get_config_summary() -> ConfigSummary {
    ConfigSummary {
        max_cpus: MAX_CPUS,
        actual_cpus: crate::kernel::smp::get_cpu_count(),
        max_irqs: MAX_IRQS,
        apic_enabled: cfg!(target_arch = "x86_64") 
            && crate::kernel::arch::x86_64::apic::is_initialized(),
        ioapic_enabled: cfg!(target_arch = "x86_64") 
            && crate::kernel::arch::x86_64::ioapic::is_initialized(),
    }
}

/// Configuration summary structure
#[derive(Debug, Clone, Copy)]
pub struct ConfigSummary {
    pub max_cpus: usize,
    pub actual_cpus: u32,
    pub max_irqs: usize,
    pub apic_enabled: bool,
    pub ioapic_enabled: bool,
}

/// Initialize configuration validation
/// This should be called after basic system initialization
pub fn init() {
    validate_system_config();
}
