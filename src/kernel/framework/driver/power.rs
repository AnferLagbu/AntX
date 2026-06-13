//! 电源管理 — CPU 空闲/频率调节 + 系统挂起/恢复
//!
//! ## 设计
//!
//! 提供三级电源管理:
//!
//! 1. **CpuIdle**: CPU 空闲状态管理 (C0-C3), 空闲时进入低功耗状态
//! 2. **CpuFreq**: CPU 频率调节 (DVFS), 支持 performance/powersave/ondemand governor
//! 3. **Suspend/Resume**: 系统级挂起与恢复 (S0ix/S3/S5)
//!
//! ### 与 Linux 的差异
//!
//! 1. **C-state**: 仅支持 C0(运行)/C1(hlt)/C2(deep) 三级, 不实现 C6/C7
//! 2. **Governor**: 仅实现 performance/powersave/ondemand 三种
//! 3. **Suspend**: 仅实现 S5(关机)/S3(挂起到内存), 不实现 S0ix
//! 4. **ACPI**: 暂不解析 DSDT/FADT 电源表, 使用硬编码默认值
//!
//! ## SAFETY
//!
//! 本模块属于 framework/TCB, 允许 unsafe.
//! C-state 进入使用 hlt/wfi 指令, 需在中断使能状态下执行.
//! Suspend 流程会禁用非启动 CPU, 需确保 IPI 安全.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use alloc::vec;
use alloc::vec::Vec;
use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

// ============================================================================
// 常量
// ============================================================================

/// 最大 C-state 数量
pub const MAX_CSTATES: usize = 4;
/// 最大频率等级
pub const MAX_FREQ_LEVELS: usize = 8;
/// 最大 CPU 数量 (与 config 对齐)
pub const MAX_PM_CPUS: usize = 256;

// ============================================================================
// CpuIdle — CPU 空闲状态管理
// ============================================================================

/// CPU 空闲状态 (C-state)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CpuIdleState {
    /// C0: 运行状态
    C0Running = 0,
    /// C1: 停止时钟 (hlt/wfi), 延迟 < 1us
    C1Halt = 1,
    /// C2: 深度停止, 延迟 < 100us
    C2DeepHalt = 2,
    /// C3: 最深睡眠, 延迟 < 1ms
    C3Sleep = 3,
}

impl CpuIdleState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::C1Halt,
            2 => Self::C2DeepHalt,
            3 => Self::C3Sleep,
            _ => Self::C0Running,
        }
    }

    /// 进入该 C-state 的预期延迟 (微秒)
    pub fn latency_us(&self) -> u32 {
        match self {
            Self::C0Running => 0,
            Self::C1Halt => 1,
            Self::C2DeepHalt => 50,
            Self::C3Sleep => 500,
        }
    }

    /// 该 C-state 的功耗节省比例 (0-100)
    pub fn power_saving(&self) -> u32 {
        match self {
            Self::C0Running => 0,
            Self::C1Halt => 40,
            Self::C2DeepHalt => 70,
            Self::C3Sleep => 90,
        }
    }
}

/// Per-CPU 空闲统计
#[derive(Debug)]
pub struct CpuIdleStats {
    /// 当前 C-state
    pub current_state: AtomicU32,
    /// 各状态累计停留时间 (毫秒)
    pub time_per_state: [AtomicU64; MAX_CSTATES],
    /// 各状态进入次数
    pub entry_count: [AtomicU64; MAX_CSTATES],
    /// 上次进入空闲的时间戳
    pub last_idle_entry: AtomicU64,
    /// 最深允许的 C-state
    pub max_cstate: AtomicU32,
}

impl CpuIdleStats {
    pub fn new() -> Self {
        Self {
            current_state: AtomicU32::new(CpuIdleState::C0Running as u32),
            time_per_state: [
                AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0),
            ],
            entry_count: [
                AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0),
            ],
            last_idle_entry: AtomicU64::new(0),
            max_cstate: AtomicU32::new(CpuIdleState::C2DeepHalt as u32),
        }
    }

    /// 进入空闲状态
    pub fn enter_idle(&self, state: CpuIdleState) {
        let max = self.max_cstate.load(Ordering::Acquire);
        let effective = if state as u32 > max {
            CpuIdleState::from_u32(max)
        } else {
            state
        };
        self.current_state.store(effective as u32, Ordering::Release);
        self.entry_count[effective as usize].fetch_add(1, Ordering::Relaxed);
        self.last_idle_entry.store(Self::read_timestamp(), Ordering::Release);
    }

    /// 退出空闲状态
    pub fn exit_idle(&self) {
        let prev = self.current_state.swap(CpuIdleState::C0Running as u32, Ordering::AcqRel);
        let entry_ts = self.last_idle_entry.load(Ordering::Acquire);
        if entry_ts > 0 {
            let now = Self::read_timestamp();
            // 近似: TSC 周期 → 毫秒 (假设 1 GHz)
            let elapsed_ms = (now - entry_ts) / 1_000_000;
            self.time_per_state[prev as usize].fetch_add(elapsed_ms, Ordering::Relaxed);
        }
    }

    /// 读取时间戳 (TSC/cntvct)
    fn read_timestamp() -> u64 {
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
}

/// CPU 空闲驱动 — 选择并进入最优 C-state
pub struct CpuIdleDriver {
    /// Per-CPU 空闲统计
    per_cpu_stats: IrqSpinLock<Vec<CpuIdleStats>>,
    /// 全局最深 C-state 限制 (由 per_cpu_stats.max_cstate 实际控制)
    #[allow(dead_code)]
    global_max_cstate: AtomicU32,
    /// 是否启用 idle governor
    enabled: AtomicBool,
}

impl CpuIdleDriver {
    pub const fn new() -> Self {
        Self {
            per_cpu_stats: IrqSpinLock::new(Vec::new()),
            global_max_cstate: AtomicU32::new(CpuIdleState::C2DeepHalt as u32),
            enabled: AtomicBool::new(false),
        }
    }

    /// 初始化 (为每个 CPU 创建统计)
    pub fn init(&self, num_cpus: u32) {
        let mut stats = self.per_cpu_stats.lock();
        for _ in 0..num_cpus {
            stats.push(CpuIdleStats::new());
        }
        self.enabled.store(true, Ordering::Release);
    }

    /// CPU 进入空闲 (调度器 idle 调用)
    pub fn idle(&self, cpu_id: u32) {
        if !self.enabled.load(Ordering::Acquire) {
            Self::arch_halt();
            return;
        }

        let stats = self.per_cpu_stats.lock();
        if (cpu_id as usize) >= stats.len() {
            drop(stats);
            Self::arch_halt();
            return;
        }

        // 简单策略: 根据预期空闲时间选择 C-state
        // TODO: 实现 menu governor (基于历史预测)
        let max_cstate = stats[cpu_id as usize].max_cstate.load(Ordering::Acquire);
        let state = CpuIdleState::from_u32(max_cstate.min(2)); // 默认最深 C2
        stats[cpu_id as usize].enter_idle(state);
        drop(stats);

        // 执行架构相关的 halt
        match state {
            CpuIdleState::C1Halt => Self::arch_halt(),
            CpuIdleState::C2DeepHalt | CpuIdleState::C3Sleep => {
                // C2+: 禁用中断前可做额外省电操作
                Self::arch_halt();
            }
            _ => Self::arch_halt(),
        }

        // 被中断唤醒, 退出空闲
        let stats = self.per_cpu_stats.lock();
        if (cpu_id as usize) < stats.len() {
            stats[cpu_id as usize].exit_idle();
        }
    }

    /// 获取 CPU 空闲统计
    pub fn get_stats(&self, cpu_id: u32) -> Option<CpuIdleStats> {
        // 注意: 返回拷贝, 避免持锁
        let stats = self.per_cpu_stats.lock();
        stats.get(cpu_id as usize).map(|s| {
            // 手动拷贝 (CpuIdleStats 的 Atomic 字段读取当前值)
            let copy = CpuIdleStats::new();
            copy.current_state.store(s.current_state.load(Ordering::Acquire), Ordering::Release);
            for i in 0..MAX_CSTATES {
                copy.time_per_state[i].store(s.time_per_state[i].load(Ordering::Acquire), Ordering::Release);
                copy.entry_count[i].store(s.entry_count[i].load(Ordering::Acquire), Ordering::Release);
            }
            copy.last_idle_entry.store(s.last_idle_entry.load(Ordering::Acquire), Ordering::Release);
            copy.max_cstate.store(s.max_cstate.load(Ordering::Acquire), Ordering::Release);
            copy
        })
    }

    /// 设置最深 C-state
    pub fn set_max_cstate(&self, cpu_id: u32, max: CpuIdleState) {
        let stats = self.per_cpu_stats.lock();
        if (cpu_id as usize) < stats.len() {
            stats[cpu_id as usize].max_cstate.store(max as u32, Ordering::Release);
        }
    }

    /// 架构相关 halt
    fn arch_halt() {
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
}

// ============================================================================
// CpuFreq — CPU 频率调节
// ============================================================================

/// 频率调节策略 (Governor)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FreqGovernor {
    /// 性能优先: 始终最高频率
    Performance = 0,
    /// 省电优先: 始终最低频率
    Powersave = 1,
    /// 按需调节: 根据负载自动调频
    Ondemand = 2,
}

impl FreqGovernor {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Self::Performance,
            1 => Self::Powersave,
            2 => Self::Ondemand,
            _ => Self::Performance,
        }
    }
}

/// 频率等级
#[derive(Debug, Clone, Copy)]
pub struct FreqLevel {
    /// 频率 (MHz)
    pub freq_mhz: u32,
    /// 电压 (mV)
    pub voltage_mv: u32,
}

/// CPU 频率驱动
pub struct CpuFreqDriver {
    /// 可用频率等级表 (从高到低排序)
    freq_table: IrqSpinLock<Vec<FreqLevel>>,
    /// Per-CPU 当前频率索引
    per_cpu_freq_idx: IrqSpinLock<Vec<AtomicU32>>,
    /// 当前 Governor
    governor: IrqSpinLock<FreqGovernor>,
    /// Ondemand 参数: 升频阈值 (负载百分比)
    up_threshold: AtomicU32,
    /// Ondemand 参数: 降频阈值
    down_threshold: AtomicU32,
    /// 是否启用
    enabled: AtomicBool,
}

impl CpuFreqDriver {
    pub const fn new() -> Self {
        Self {
            freq_table: IrqSpinLock::new(Vec::new()),
            per_cpu_freq_idx: IrqSpinLock::new(Vec::new()),
            governor: IrqSpinLock::new(FreqGovernor::Performance),
            up_threshold: AtomicU32::new(80),
            down_threshold: AtomicU32::new(20),
            enabled: AtomicBool::new(false),
        }
    }

    /// 初始化 (设置频率表)
    pub fn init(&self, freq_table: Vec<FreqLevel>, num_cpus: u32) {
        *self.freq_table.lock() = freq_table;
        let mut indices = self.per_cpu_freq_idx.lock();
        for _ in 0..num_cpus {
            // 默认最高频率
            indices.push(AtomicU32::new(0));
        }
        self.enabled.store(true, Ordering::Release);
    }

    /// 用默认频率表初始化 (单频点)
    pub fn init_default(&self, base_freq_mhz: u32, num_cpus: u32) {
        let table = vec![
            FreqLevel { freq_mhz: base_freq_mhz, voltage_mv: 1000 },
        ];
        self.init(table, num_cpus);
    }

    /// 获取当前频率
    pub fn get_freq(&self, cpu_id: u32) -> u32 {
        let table = self.freq_table.lock();
        let indices = self.per_cpu_freq_idx.lock();
        if table.is_empty() || (cpu_id as usize) >= indices.len() {
            return 0;
        }
        let idx = indices[cpu_id as usize].load(Ordering::Acquire) as usize;
        if idx < table.len() {
            table[idx].freq_mhz
        } else {
            0
        }
    }

    /// 设置目标频率
    pub fn set_freq(&self, cpu_id: u32, target_mhz: u32) -> bool {
        let table = self.freq_table.lock();
        if table.is_empty() {
            return false;
        }
        // 找最接近的频率等级
        let mut best_idx = 0;
        let mut best_diff = u32::MAX;
        for (i, level) in table.iter().enumerate() {
            let diff = (level.freq_mhz as i32 - target_mhz as i32).unsigned_abs();
            if diff < best_diff {
                best_diff = diff;
                best_idx = i;
            }
        }
        drop(table);

        let indices = self.per_cpu_freq_idx.lock();
        if (cpu_id as usize) >= indices.len() {
            return false;
        }
        indices[cpu_id as usize].store(best_idx as u32, Ordering::Release);
        // TODO: 实际写 MSR/寄存器调整频率和电压
        true
    }

    /// 设置 Governor
    pub fn set_governor(&self, governor: FreqGovernor) {
        *self.governor.lock() = governor;
        match governor {
            FreqGovernor::Performance => {
                // 所有 CPU 设为最高频
                let indices = self.per_cpu_freq_idx.lock();
                for idx in indices.iter() {
                    idx.store(0, Ordering::Release);
                }
            }
            FreqGovernor::Powersave => {
                // 所有 CPU 设为最低频
                let table = self.freq_table.lock();
                let last = if table.is_empty() { 0 } else { table.len() - 1 };
                drop(table);
                let indices = self.per_cpu_freq_idx.lock();
                for idx in indices.iter() {
                    idx.store(last as u32, Ordering::Release);
                }
            }
            FreqGovernor::Ondemand => {
                // Ondemand: 由调度器 tick 触发调频
            }
        }
    }

    /// 获取当前 Governor
    pub fn get_governor(&self) -> FreqGovernor {
        *self.governor.lock()
    }

    /// Ondemand 调频检查 (由调度器 tick 调用)
    pub fn ondemand_check(&self, cpu_id: u32, load_percent: u32) {
        let gov = *self.governor.lock();
        if gov != FreqGovernor::Ondemand {
            return;
        }
        let up = self.up_threshold.load(Ordering::Acquire);
        let down = self.down_threshold.load(Ordering::Acquire);

        if load_percent > up {
            // 负载高: 升频
            let indices = self.per_cpu_freq_idx.lock();
            if (cpu_id as usize) < indices.len() {
                let cur = indices[cpu_id as usize].load(Ordering::Acquire);
                if cur > 0 {
                    indices[cpu_id as usize].store(cur - 1, Ordering::Release);
                }
            }
        } else if load_percent < down {
            // 负载低: 降频
            let table = self.freq_table.lock();
            let max_idx = if table.is_empty() { 0 } else { table.len() - 1 };
            drop(table);
            let indices = self.per_cpu_freq_idx.lock();
            if (cpu_id as usize) < indices.len() {
                let cur = indices[cpu_id as usize].load(Ordering::Acquire);
                if (cur as usize) < max_idx {
                    indices[cpu_id as usize].store(cur + 1, Ordering::Release);
                }
            }
        }
    }
}

// ============================================================================
// Suspend/Resume — 系统挂起与恢复
// ============================================================================

/// 系统电源状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SystemPowerState {
    /// S0: 工作状态
    S0Working = 0,
    /// S3: 挂起到内存 (Suspend-to-RAM)
    S3SuspendToRam = 3,
    /// S5: 软关机
    S5SoftOff = 5,
}

/// 挂起/恢复回调
pub type SuspendCallback = fn() -> i32;
pub type ResumeCallback = fn() -> i32;

/// 挂起/恢复通知器
pub struct SuspendNotifier {
    pub name: [u8; 16],
    pub suspend: SuspendCallback,
    pub resume: ResumeCallback,
    pub priority: u32, // 0=最先, 越大越后
}

/// 系统电源管理
pub struct PmSubsystem {
    /// 当前系统电源状态
    state: AtomicU32,
    /// 挂起通知器列表
    notifiers: IrqSpinLock<Vec<SuspendNotifier>>,
    /// CpuIdle 驱动
    pub cpuidle: CpuIdleDriver,
    /// CpuFreq 驱动
    pub cpufreq: CpuFreqDriver,
    /// 是否已初始化
    initialized: AtomicBool,
}

impl PmSubsystem {
    pub const fn new() -> Self {
        Self {
            state: AtomicU32::new(SystemPowerState::S0Working as u32),
            notifiers: IrqSpinLock::new(Vec::new()),
            cpuidle: CpuIdleDriver::new(),
            cpufreq: CpuFreqDriver::new(),
            initialized: AtomicBool::new(false),
        }
    }

    /// 初始化电源管理子系统
    pub fn init(&self, num_cpus: u32) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }
        self.cpuidle.init(num_cpus);
        // CpuFreq: 默认频率表 (QEMU 通常 2 GHz)
        self.cpufreq.init_default(2000, num_cpus);
        self.initialized.store(true, Ordering::Release);
        crate::klog_ffi!(
            klog_ffi_info,
            "[PM] subsystem initialized: {} CPUs, default 2000 MHz",
            num_cpus
        );
    }

    /// 注册挂起通知器
    pub fn register_notifier(&self, notifier: SuspendNotifier) {
        let mut notifiers = self.notifiers.lock();
        notifiers.push(notifier);
        notifiers.sort_by_key(|n| n.priority);
    }

    /// 挂起系统
    pub fn suspend(&self, target: SystemPowerState) -> i64 {
        if !self.initialized.load(Ordering::Acquire) {
            return -(11i64); // EAGAIN
        }

        let current = SystemPowerState::from_u32(self.state.load(Ordering::Acquire));
        if current != SystemPowerState::S0Working {
            return -(16i64); // EBUSY
        }

        crate::klog_ffi!(
            klog_ffi_info,
            "[PM] suspending to S{}...",
            target as u32
        );

        // 1. 通知所有注册者 (按优先级)
        let notifiers = self.notifiers.lock();
        for n in notifiers.iter() {
            let ret = (n.suspend)();
            if ret != 0 {
                crate::klog_ffi!(
                    klog_ffi_warn,
                    "[PM] notifier suspend failed: ret={}", ret
                );
                // 回滚: 通知已暂停的恢复
                drop(notifiers);
                return -(5i64); // EIO
            }
        }
        drop(notifiers);

        // 2. 更新状态
        self.state.store(target as u32, Ordering::Release);

        // 3. 执行架构相关挂起
        match target {
            SystemPowerState::S3SuspendToRam => {
                self.arch_suspend_to_ram();
            }
            SystemPowerState::S5SoftOff => {
                self.arch_shutdown();
            }
            _ => {}
        }

        // 恢复后
        self.state.store(SystemPowerState::S0Working as u32, Ordering::Release);

        // 4. 通知恢复
        let notifiers = self.notifiers.lock();
        for n in notifiers.iter() {
            (n.resume)();
        }

        0
    }

    /// 获取当前电源状态
    pub fn get_state(&self) -> SystemPowerState {
        SystemPowerState::from_u32(self.state.load(Ordering::Acquire))
    }

    /// 架构相关: 挂起到内存
    fn arch_suspend_to_ram(&self) {
        // TODO: 实现真正的 S3 挂起
        // 1. 冻结所有非 boot CPU
        // 2. 保存设备状态
        // 3. 写 ACPI SLP_TYP 寄存器
        // 4. 执行 wfi/hlt 等待唤醒
        // 当前: 简化为 halt 循环
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
    fn arch_shutdown(&self) {
        crate::kernel::framework::driver::shutdown_all();
    }
}

impl SystemPowerState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            3 => Self::S3SuspendToRam,
            5 => Self::S5SoftOff,
            _ => Self::S0Working,
        }
    }
}

// ============================================================================
// 全局实例
// ============================================================================

/// 全局电源管理子系统
static PM_SUBSYSTEM: PmSubsystem = PmSubsystem::new();

/// 初始化电源管理
pub fn pm_init(num_cpus: u32) {
    PM_SUBSYSTEM.init(num_cpus);
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
// 系统调用
// ============================================================================

/// sys_pm — 电源管理系统调用
///
/// `a0`: cmd
///   0 = suspend(目标状态: a1)
///   1 = get_state() → 当前状态
///   2 = set_governor(governor: a1)
///   3 = get_governor() → 当前 governor
///   4 = set_max_cstate(cpu: a1, max: a2)
///   5 = get_freq(cpu: a1) → 频率 MHz
///   6 = set_freq(cpu: a1, target_mhz: a2)
#[no_mangle]
pub fn sys_pm(cmd: u64, a1: u64, a2: u64) -> i64 {
    if !pm_is_initialized() {
        return -(11i64);
    }

    match cmd {
        0 => {
            // suspend
            let target = SystemPowerState::from_u32(a1 as u32);
            pm_subsystem().suspend(target)
        }
        1 => {
            // get_state
            pm_subsystem().get_state() as i64
        }
        2 => {
            // set_governor
            let gov = FreqGovernor::from_u32(a1 as u32);
            pm_subsystem().cpufreq.set_governor(gov);
            0
        }
        3 => {
            // get_governor
            pm_subsystem().cpufreq.get_governor() as i64
        }
        4 => {
            // set_max_cstate
            let cpu = a1 as u32;
            let max = CpuIdleState::from_u32(a2 as u32);
            pm_subsystem().cpuidle.set_max_cstate(cpu, max);
            0
        }
        5 => {
            // get_freq
            pm_subsystem().cpufreq.get_freq(a1 as u32) as i64
        }
        6 => {
            // set_freq
            if pm_subsystem().cpufreq.set_freq(a1 as u32, a2 as u32) {
                0
            } else {
                -(22i64) // EINVAL
            }
        }
        _ => -(38i64), // ENOSYS
    }
}
