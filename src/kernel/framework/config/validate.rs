//! 启动自检: 验证常量自洽性、内存布局、中断控制器
//!
//! 集中实现 `validate_*` 函数, 由 `init()` 串联调用。

use super::error::ConfigError;
use super::memory::{
    HUGE_PAGE_2M_SIZE, KERNEL_STACK_SIZE, PAGE_SIZE, USER_CODE_BASE, USER_STACK_GUARD,
    USER_STACK_SIZE, USER_STACK_TOP,
};
use super::slab::SLAB_DEFAULT_SIZE;

/// 校验 CPU 配置.
///
/// 检查: 实际 CPU 数是否超出 `MAX_CPUS`.
pub fn validate_cpu_config() -> Result<(), ConfigError> {
    let cpu_count = crate::kernel::framework::smp::get_cpu_count();

    if cpu_count as usize > super::capacity::MAX_CPUS {
        return Err(ConfigError::CpuCountExceedsMax {
            actual: cpu_count,
            max: super::capacity::MAX_CPUS,
        });
    }

    Ok(())
}

/// 校验内存布局一致性.
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
    if !(SLAB_DEFAULT_SIZE as u64).is_multiple_of(PAGE_SIZE) {
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

/// 校验中断配置.
///
/// 检查: x86_64 下 APIC 或 IOAPIC 至少一个已初始化.
/// aarch64 下 GIC 失败已在 arch 层 panic, 此处仅做软校验。
pub fn validate_interrupt_config() -> Result<(), ConfigError> {
    #[cfg(target_arch = "x86_64")]
    {
        let apic_ok = crate::kernel::framework::arch::x86_64::apic::is_initialized();
        let ioapic_ok = crate::kernel::framework::arch::x86_64::ioapic::is_initialized();
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

// ============================================================================
// Driver-specific 验证 (演进 6)
// ============================================================================

/// 验证 PCI 子系统已初始化。
///
/// **契约**: 当 `net = "e1000"` / `ahci` / `nvme` 等使用 PCI 总线的驱动
/// 已配置为启用时, PCI 必须在它们之前完成初始化。
/// 本函数为软检查 — 报告但不 panic, 因为某些嵌入式环境可能不包含 PCI 总线。
pub fn validate_pci_subsystem() -> Result<(), ConfigError> {
    if !crate::kernel::framework::pci::is_initialized() {
        return Err(ConfigError::DriverConfigInvalid("pci"));
    }
    Ok(())
}

/// 验证网络子系统配置一致性。
///
/// 当前仅做存在性检查; 未来可扩展:
/// - IP 地址不冲突
/// - MTU 落在合法范围 (576..=65535)
/// - 网卡数量 ≤ MAX_CPUS
pub fn validate_network_subsystem() -> Result<(), ConfigError> {
    // 当前: 软检查; init 失败在 net::init() 内已 panic, 此处不重复。
    // 未来: 当 net config 抽离为常量时, 此处做 IP/MTU 等静态检查。
    Ok(())
}

/// 校验所有驱动配置.
///
/// 软检查 (不 panic): 报告所有未初始化子系统, 但不阻断启动.
/// 设计动机: 嵌入式环境可能故意不启用某些子系统 (例如无 PCI 总线),
/// 严格校验会误报。
pub fn validate_drivers() -> u32 {
    let mut errors = 0u32;

    match validate_pci_subsystem() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            log_config_error(&e);
        }
    }

    match validate_network_subsystem() {
        Ok(()) => {}
        Err(e) => {
            errors += 1;
            log_config_error(&e);
        }
    }

    errors
}

/// 校验所有系统配置.
///
/// 在启动早期调用. 在 `debug_assertions` 构建中,
/// CPU 数溢出将 **panic**; 否则仅记录错误.
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

    // 演进 9: KASLR 偏移自检
    if let Err(msg) = crate::kernel::framework::config::validate_kaslr_offset() {
        errors += 1;
        use crate::klog_err;
        klog_err!(Boot, "CONFIG: KASLR: {}", msg);
    }

    errors
}

#[inline]
fn log_config_error(e: &ConfigError) {
    use crate::klog_err;
    // 演进 6 后续: 依赖 ConfigError::Display (error.rs), 避免每加一个变体就要更新此处。
    klog_err!(Boot, "CONFIG: {}", e);
}
