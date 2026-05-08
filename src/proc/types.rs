pub type Pid = u32;
pub type Tid = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(pub Pid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(pub Tid);

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
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => ProcessState::Created,
            1 => ProcessState::Ready,
            2 => ProcessState::Running,
            3 => ProcessState::Blocked,
            4 => ProcessState::Zombie,
            5 => ProcessState::Terminated,
            6 => ProcessState::Frozen,
            _ => ProcessState::Created,
        }
    }
    pub fn from_u32(value: u32) -> Self {
        Self::from_u8(value as u8)
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
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rflags: u64,
    pub cr3: u64,
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub ss: u16,
}

impl ProcessContext {
    pub const fn new() -> Self {
        Self {
            rip: 0,
            rsp: 0,
            rbp: 0,
            rflags: 0x202,
            cr3: 0,
            rbx: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            cs: 0x08,
            ds: 0x10,
            es: 0x10,
            fs: 0x10,
            gs: 0x10,
            ss: 0x10,
        }
    }
    
    pub fn set_user_mode(&mut self) {
        self.cs = 0x1B;
        self.ds = 0x23;
        self.es = 0x23;
        self.fs = 0x23;
        self.gs = 0x23;
        self.ss = 0x23;
        self.rflags = 0x202;
    }
}

impl Default for ProcessContext {
    fn default() -> Self {
        Self::new()
    }
}

pub const MAX_PROCESSES: usize = 256;
pub const MAX_OPEN_FILES: usize = 32;
pub const KERNEL_STACK_SIZE: usize = 65536;
pub const USER_STACK_SIZE: usize = 65536;
