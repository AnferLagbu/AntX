pub type Pid = u32;
pub type Tid = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(pub Pid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(pub Tid);

/// ✅ 优化后的进程状态模型 (七状态完整实现)
///
/// 状态生命周期:
/// Created → Ready → Running ↔ Blocked
///                  ↓         ↓
///                Frozen   Zombie → Terminated
///
/// 每个状态的含义:
/// - Created:    PCB 已分配, 资源初始化中 (尚未可运行)
/// - Ready:      除 CPU 外所有资源就绪, 在 MLFQ 队列中等待
/// - Running:    正在 CPU 上执行指令
/// - Blocked:    等待事件 (I/O/子进程/信号/睡眠)
/// - Zombie:     已调用 exit(), PCB 保留供父进程 wait()
/// - Terminated: PCB 已被回收, PID 可重用
/// - Frozen:     被挂起 (SIGSTOP/cgroup freezer/调试断点)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Created = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
    Terminated = 5,
    Frozen = 6,
}

impl ProcessState {
    /// 安全的从 u8 值转换为 ProcessState
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => ProcessState::Created,
            1 => ProcessState::Ready,
            2 => ProcessState::Running,
            3 => ProcessState::Blocked,
            4 => ProcessState::Zombie,
            5 => ProcessState::Terminated,
            6 => ProcessState::Frozen,
            _ => ProcessState::Created, // 无效值安全回退
        }
    }

    /// 从 u32 值转换 (兼容 AtomicU32 存储)
    pub fn from_u32(value: u32) -> Self {
        Self::from_u8(value as u8)
    }

    /// 获取状态名称 (用于日志和调试)
    pub fn name(&self) -> &'static str {
        match self {
            ProcessState::Created => "Created",
            ProcessState::Ready => "Ready",
            ProcessState::Running => "Running",
            ProcessState::Blocked => "Blocked",
            ProcessState::Zombie => "Zombie",
            ProcessState::Terminated => "Terminated",
            ProcessState::Frozen => "Frozen",
        }
    }

    /// ✅ 检查进程是否可调度 (在就绪队列或运行中)
    pub fn is_runnable(&self) -> bool {
        matches!(self, ProcessState::Ready | ProcessState::Running)
    }

    /// ✅ 检查进程是否存活 (未终止或僵尸)
    pub fn is_alive(&self) -> bool {
        !matches!(self, ProcessState::Zombie | ProcessState::Terminated)
    }

    /// ✅ 检查进程是否可以被冻结
    pub fn can_freeze(&self) -> bool {
        matches!(
            self,
            ProcessState::Running | ProcessState::Ready | ProcessState::Blocked
        )
    }

    /// ✅ 检查进程是否可以被唤醒 (从 Frozen 解冻后应转到的状态)
    pub fn thaw_target_state(&self) -> Option<ProcessState> {
        match self {
            ProcessState::Frozen => Some(ProcessState::Ready), // 默认解冻到 Ready
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    WaitingForIo = 0,
    WaitingForChild = 1,
    WaitingForSignal = 2,
    Sleeping = 3,
    Unknown = 255,
}

impl BlockReason {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => BlockReason::WaitingForIo,
            1 => BlockReason::WaitingForChild,
            2 => BlockReason::WaitingForSignal,
            3 => BlockReason::Sleeping,
            _ => BlockReason::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPriority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    RealTime = 4,
}

impl ProcessPriority {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => ProcessPriority::Idle,
            1 => ProcessPriority::Low,
            2 => ProcessPriority::Normal,
            3 => ProcessPriority::High,
            4 => ProcessPriority::RealTime,
            _ => ProcessPriority::Normal,
        }
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ProcessFlags: u32 {
        const IS_KERNEL = 1 << 0;
        const IS_TRACED = 1 << 1;
        const IS_STOPPED = 1 << 2;
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ProcessContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rax: u64, // ✅ fork 返回值控制
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    pub cr3: u64,
    pub cs: u64,
    pub ds: u64,
    pub es: u64,
    pub fs: u64,
    pub gs: u64,
    pub ss: u64,
}

impl ProcessContext {
    pub const fn new() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rax: 0,
            rip: 0,
            rsp: 0,
            rflags: 0,
            cr3: 0,
            cs: 0,
            ds: 0,
            es: 0,
            fs: 0,
            gs: 0,
            ss: 0,
        }
    }

    pub fn set_user_mode(&mut self) {
        self.cs = 0x23;
        self.ds = 0x1B;
        self.es = 0x1B;
        self.fs = 0x1B;
        self.gs = 0x1B;
        self.ss = 0x1B;
        self.rflags = 0x202;
    }
}

impl Default for ProcessContext {
    fn default() -> Self {
        Self::new()
    }
}

// === 进程规模常量 (统一从 config.rs 引用) ===
//
// 集中式 re-export: 所有 proc 子模块 (types/process/thread/user_proc) 共享同一组常量,
// 避免分散定义与影子覆盖。user_proc.rs 等子模块通过 `use super::types::*;` 引入。
pub use crate::kernel::framework::config::{
    // 内存页
    PAGE_SIZE,
    // 进程规模
    MAX_PROCESSES, MAX_OPEN_FILES,
    // 栈规模
    KERNEL_STACK_SIZE, USER_KSTACK_SIZE,
    USER_STACK_SIZE, USER_STACK_GUARD, USER_STACK_TOP, USER_STACK_MAX_SIZE,
    USER_CODE_BASE,
    // 调度参数
    SCHED_BOOST_INTERVAL,
    SCHED_LEVEL_0_QUANTUM, SCHED_LEVEL_1_QUANTUM, SCHED_LEVEL_2_QUANTUM, SCHED_LEVEL_3_QUANTUM,
    SCHED_RT_WATCHDOG_TICKS,
};

/// 线程调度优先级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreadPriority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Realtime = 4,
}

impl ThreadPriority {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => ThreadPriority::Idle,
            1 => ThreadPriority::Low,
            2 => ThreadPriority::Normal,
            3 => ThreadPriority::High,
            4 => ThreadPriority::Realtime,
            _ => ThreadPriority::Normal,
        }
    }
}

/// 线程状态 (七状态模型)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Created = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
    Terminated = 5,
    Frozen = 6,
}

impl ThreadState {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => ThreadState::Created,
            1 => ThreadState::Ready,
            2 => ThreadState::Running,
            3 => ThreadState::Blocked,
            4 => ThreadState::Zombie,
            5 => ThreadState::Terminated,
            6 => ThreadState::Frozen,
            _ => ThreadState::Created,
        }
    }

    pub fn is_runnable(&self) -> bool {
        matches!(self, ThreadState::Ready | ThreadState::Running)
    }

    pub fn is_alive(&self) -> bool {
        !matches!(self, ThreadState::Zombie | ThreadState::Terminated)
    }

    pub fn can_freeze(&self) -> bool {
        matches!(
            self,
            ThreadState::Running | ThreadState::Ready | ThreadState::Blocked
        )
    }
}
