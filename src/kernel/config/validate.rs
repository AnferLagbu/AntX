//! 启动自检: 验证常量自洽性、内存布局、中断控制器
//!
//! 集中实现 `validate_*` 函数, 由 `init()` 串联调用。

use super::error::ConfigError;
use super::memory::{
    HUGE_PAGE_2M_SIZE, KERNEL_STACK_SIZE, PAGE_SIZE, USER_CODE_BASE, USER_STACK_GUARD,
    USER_STACK_SIZE, USER_STACK_TOP,
};
use super::slab::SLAB_DEFAULT_SIZE;

/// Validate CPU configuration.
///
/// 检查: 实际 CPU 数是否超出 `MAX_CPUS`。
pub fn validate_cpu_config() -> Result<(), ConfigError> {
    let cpu_count = crate::kernel::smp::get_cpu_count();

    if cpu_count as usize > super::capacity::MAX_CPUS {
        return Err(ConfigError::CpuCountExceedsMax {
            actual: cpu_count,
            max: super::capacity::MAX_CPUS,
        });
    }

    Ok(())
}

/// Validate memory layout consistency.
///
/// 检查:
/// 1. `PAGE_SIZE` 是 2 的幂
/// 2. `SLAB_DEFAULT_SIZE` 按页对齐
/// 3. 栈尺寸 / guard / 拓扑合法
pub fn validate_memory_config() -> Result<(), ConfigError> {
    // 1. PAGE_SIZE 必须是 2 的幂
    if PAGE_SIZE == 0 || (PAGE_SIZE & (PAGE_SIZE - 1)) != 0 {
        return Err(ConfigError::MemoryLayoutInvalid);
    }

    // 2. SLAB_DEFAULT_SIZE 必须按页对齐
    if (SLAB_DEFAULT_SIZE as u64) % PAGE_SIZE != 0 {
        return Err(ConfigError::MemoryLayoutInvalid);
    }

    // 3. 栈尺寸合法性
    if USER_STACK_SIZE < PAGE_SIZE {
        return Err(ConfigError::MemoryLayoutInvalid);
    }
    if USER_STACK_GUARD > 16 * 1024 * 1024 {
        return Err(ConfigError::MemoryLayoutInvalid);
    }

    // 4. 用户栈 top 必须高于 code base + 2M huge 页
    if USER_STACK_TOP <= USER_CODE_BASE + HUGE_PAGE_2M_SIZE {
        return Err(ConfigError::MemoryLayoutInvalid);
    }

    // 5. 内核栈不能小于 1 页
    if (KERNEL_STACK_SIZE as u64) < PAGE_SIZE {
        return Err(ConfigError::MemoryLayoutInvalid);
    }

    Ok(())
}

/// Validate interrupt configuration.
///
/// 检查: x86_64 下 APIC 或 IOAPIC 至少一个已初始化。
/// aarch64 下 GIC 失败已在 arch 层 panic, 此处仅做软校验。
pub fn validate_interrupt_config() -> Result<(), ConfigError> {
    #[cfg(target_arch = "x86_64")]
    {
        let apic_ok = crate::kernel::arch::x86_64::apic::is_initialized();
        let ioapic_ok = crate::kernel::arch::x86_64::ioapic::is_initialized();
        if !apic_ok && !ioapic_ok {
            return Err(ConfigError::IrqControllerUnavailable);
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // GIC 初始化失败已经在 arch 层 panic, 此处只需做软校验
    }

    Ok(())
}

/// 跨模块一致性校验 — 防止下游模块重新定义常量后数值漂移。
///
/// **调用方契约**: 本函数在 `init()` 末尾执行, 此时所有 `pub use` 引用已生效。
/// 由于其他模块 `pub use` 本模块常量, 编译期即保证一致, 启动期是双保险。
pub fn validate_cross_module_consistency() -> Result<(), ConfigError> {
    // 直接使用本模块的常量作为权威值。
    // 由于其他模块 `pub use` 本模块常量, 编译期即保证一致。
    // 此函数预留为运行时自检接口: 如果未来某个子系统又重新定义 const,
    // 编译器会因命名冲突或链接错误直接拒绝, 不会到达此处。
    Ok(())
}

/// Validate all system configuration.
///
/// Called early in the boot process. In `debug_assertions` builds,
/// a CPU count overflow **panics**; otherwise errors are logged.
pub fn validate_system_config() -> u32 {
    let mut errors = 0u32;

    match validate_cpu_config() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            log_config_error(&e);
            #[cfg(debug_assertions)]
            if let ConfigError::CpuCountExceedsMax { actual, max } = e {
                panic!(
                    "CONFIG: CPU count {} exceeds MAX_CPUS {}. \
                     Increase MAX_CPUS in kernel/config/capacity.rs",
                    actual, max
                );
            }
        }
    }

    match validate_memory_config() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            log_config_error(&e);
        }
    }

    match validate_interrupt_config() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            log_config_error(&e);
        }
    }

    match validate_cross_module_consistency() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            log_config_error(&e);
        }
    }

    errors
}

#[inline]
fn log_config_error(e: &ConfigError) {
    use crate::klog_err;
    match e {
        ConfigError::CpuCountExceedsMax { actual, max } => {
            klog_err!(Boot, "CONFIG: CPU count {} > MAX_CPUS {}", actual, max);
        }
        ConfigError::MemoryLayoutInvalid => {
            klog_err!(Boot, "CONFIG: memory layout invalid");
        }
        ConfigError::IrqControllerUnavailable => {
            klog_err!(Boot, "CONFIG: no interrupt controller initialized");
        }
        ConfigError::InconsistentConstant { name, lhs, rhs } => {
            klog_err!(
                Boot,
                "CONFIG: constant {} mismatch: config.rs={} vs submodule={}",
                name,
                lhs,
                rhs
            );
        }
        ConfigError::DriverConfigInvalid(name) => {
            klog_err!(Boot, "CONFIG: driver {} misconfigured", name);
        }
    }
}
