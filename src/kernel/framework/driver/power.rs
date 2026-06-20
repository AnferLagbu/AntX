//! 电源管理 — framework 层硬件操作 + re-export
//!
//! ## T4-4 迁移记录
//!
//! 策略代码 (类型定义 + governor 选择 + C-state 决策 + 通知器链 + syscall 分发)
//! 已于 2026-06-16 迁移到 services::driver::power.
//! 本文件仅保留 unsafe 硬件操作 (arch_halt/read_timestamp/arch_suspend_to_ram/arch_shutdown)
//! + 全局实例 + re-export.

use core::sync::atomic::Ordering;

// Re-export services 层策略主体
pub use crate::kernel::services::driver::power::{
    MAX_CSTATES, MAX_FREQ_LEVELS, MAX_PM_CPUS,
    CpuIdleState, CpuIdleStats, CpuIdleDriver,
    FreqGovernor, FreqLevel, CpuFreqDriver,
    SystemPowerState, SuspendNotifier, SuspendCallback, ResumeCallback,
    PmSubsystem,
};

// ============================================================================
// 全局实例
// ============================================================================

/// 全局电源管理子系统
static PM_SUBSYSTEM: PmSubsystem = PmSubsystem::new();

/// 初始化电源管理
pub fn pm_init(num_cpus: u32) {
    PM_SUBSYSTEM.init(num_cpus);
    crate::klog_ffi!(
        klog_ffi_info,
        "[PM] subsystem initialized: {} CPUs, default 2000 MHz",
        num_cpus
    );
}

/// 获取全局 PM 子系统
pub fn pm_subsystem() -> &'static PmSubsystem {
    &PM_SUBSYSTEM
}

/// PM 是否已初始化
pub fn pm_is_initialized() -> bool {
    PM_SUBSYSTEM.initialized.load(Ordering::Acquire)
}

// ============================================================================
// 硬件操作 (unsafe)
// ============================================================================

/// 读取时间戳 (TSC/cntvct)
pub fn read_timestamp() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: rdtsc 是用户态安全指令
        unsafe { core::arch::x86_64::_rdtsc() }
    }
    #[cfg(target_arch = "aarch64")]
    {
        let cnt: u64;
        // SAFETY: cntvct_el0 是 EL0 可读虚拟计数器
        unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt) };
        cnt
    }
}

/// 架构相关 halt (C1)
pub fn arch_halt() {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: hlt 使 CPU 进入 C1 直到中断, 中断已使能
        unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: wfi 等待中断, 中断已使能
        unsafe { core::arch::asm!("wfi", options(nomem, nostack)) };
    }
}

/// CPU 进入空闲 (调度器 idle 调用)
///
/// 组合策略选择 (services) + 硬件 halt (framework)
pub fn pm_idle(cpu_id: u32) {
    let state = match PM_SUBSYSTEM.cpuidle.select_cstate(cpu_id) {
        Some(s) => s,
        None => CpuIdleState::C1Halt,
    };

    let stats = PM_SUBSYSTEM.cpuidle.per_cpu_stats.lock();
    if (cpu_id as usize) < stats.len() {
        stats[cpu_id as usize].enter_idle(state);
        stats[cpu_id as usize].set_idle_entry_ts(read_timestamp());
    }
    drop(stats);

    // 执行架构相关的 halt
    arch_halt();

    // 被中断唤醒, 退出空闲
    let stats = PM_SUBSYSTEM.cpuidle.per_cpu_stats.lock();
    if (cpu_id as usize) < stats.len() {
        let entry_ts = stats[cpu_id as usize].last_idle_entry.load(Ordering::Acquire);
        let elapsed_ms = if entry_ts > 0 {
            (read_timestamp() - entry_ts) / 1_000_000
        } else {
            0
        };
        stats[cpu_id as usize].exit_idle(elapsed_ms);
    }
}

/// 挂起系统 (组合策略 + 硬件操作)
pub fn pm_suspend(target: SystemPowerState) -> i64 {
    if let Err(e) = PM_SUBSYSTEM.suspend_prepare(target) {
        return e;
    }

    crate::klog_ffi!(
        klog_ffi_info,
        "[PM] suspending to S{}...",
        target as u32
    );

    // 执行架构相关挂起
    match target {
        SystemPowerState::S3SuspendToRam => {
            arch_suspend_to_ram();
        }
        SystemPowerState::S5SoftOff => {
            arch_shutdown();
        }
        _ => {}
    }

    // 恢复后通知
    PM_SUBSYSTEM.suspend_resume_notify();
    0
}

/// 架构相关: 挂起到内存
fn arch_suspend_to_ram() {
    // TODO(TRACK-6F7A9A): 实现真正的 S3 挂起
    crate::klog_ffi!(
        klog_ffi_info,
        "[PM] S3: entering halt loop (placeholder)"
    );
    loop {
        #[cfg(target_arch = "x86_64")]
        {
            // SAFETY: hlt 等待中断唤醒
            unsafe { core::arch::asm!("hlt", options(nomem, nostack)) };
        }
        #[cfg(target_arch = "aarch64")]
        {
            // SAFETY: wfi 等待中断唤醒
            unsafe { core::arch::asm!("wfi", options(nomem, nostack)) };
        }
    }
}

/// 架构相关: 关机
fn arch_shutdown() {
    crate::kernel::framework::driver::shutdown_all();
}

// ============================================================================
// 系统调用入口
// ============================================================================

/// sys_pm — 电源管理系统调用
#[no_mangle]
pub fn sys_pm(cmd: u64, a1: u64, a2: u64) -> i64 {
    crate::kernel::services::driver::power::sys_pm_dispatch(&PM_SUBSYSTEM, cmd, a1, a2)
}
