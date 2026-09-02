#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯类型定义和常量。
//! IPC 数据类型定义 — services 层策略主体
//!
//! ## T6-3 迁移记录
//!
//! 原属 framework/ipc/types.rs, 2026-06-16 提取到 services.
//! 纯数据定义 (IPC 类型/信号/管道/共享内存/消息队列/信号量), 0 unsafe.
//! framework 仅保留 re-export.

use core::sync::atomic::AtomicPtr;

/// IPC 资源 ID 类型 (全局唯一标识符)
pub type IpcId = u32;

// ============================================================================
// 常量定义
// ============================================================================

/// 最大管道数量
///
/// 测试模式下缩减至 2 以节省内存
#[cfg(not(feature = "kernel_test"))]
pub const IPC_MAX_PIPES: usize = 64;
#[cfg(feature = "kernel_test")]
pub const IPC_MAX_PIPES: usize = 2;
/// 最大信号数量
pub const IPC_MAX_SIGNALS: usize = 32;
/// 最大共享内存段数量
///
/// 测试模式下缩减至 2 以节省内存
#[cfg(not(feature = "kernel_test"))]
pub const IPC_MAX_SHM_SEGS: usize = 16;
#[cfg(feature = "kernel_test")]
pub const IPC_MAX_SHM_SEGS: usize = 2;
/// 最大消息队列数量
///
/// 测试模式下缩减至 2 以节省内存
#[cfg(not(feature = "kernel_test"))]
pub const IPC_MAX_MSG_QUEUES: usize = 32;
#[cfg(feature = "kernel_test")]
pub const IPC_MAX_MSG_QUEUES: usize = 2;
/// 最大信号量数量
///
/// 测试模式下缩减至 2 以节省内存
#[cfg(not(feature = "kernel_test"))]
pub const IPC_MAX_SEMAPHORES: usize = 64;
#[cfg(feature = "kernel_test")]
pub const IPC_MAX_SEMAPHORES: usize = 2;

/// 管道缓冲区大小 (4KB)
pub const PIPE_BUFFER_SIZE: usize = 4096;
/// 最大共享内存大小 (16MB)
pub const SHM_MAX_SIZE: u64 = 16 * 1024 * 1024;
/// 最大单条消息大小 (4KB)
pub const MSG_MAX_SIZE: usize = 4096;
/// 单个队列最大消息数
pub const MSG_QUEUE_MAX_MSGS: u32 = 64;

// ============================================================================
// 枚举定义
// ============================================================================

/// IPC 资源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IpcType {
    /// 管道
    Pipe = 1,
    /// 信号
    Signal,
    /// 共享内存
    Shm,
    /// 消息队列
    MsgQ,
    /// 信号量
    Sem,
}

/// 信号编号 (POSIX 兼容)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SignalNum {
    /// 无效/空信号
    None = 0,
    /// 中断 (Ctrl+C)
    Int = 1,
    /// 非法指令
    Ill = 2,
    /// 浮点异常
    Fpe = 3,
    /// 段错误
    Segv = 4,
    /// 终止请求
    Term = 5,
    /// 强制终止
    Kill = 6,
    /// 停止进程
    Stop = 7,
    /// 继续执行
    Cont = 8,
    /// 子进程状态改变
    Chld = 9,
    /// 用户自定义信号 1
    Usr1 = 10,
    /// 用户自定义信号 2
    Usr2 = 11,
    /// 定时器到期
    Alarm = 12,
    /// 管道断裂
    Pipe = 13,
}

impl From<u8> for SignalNum {
    #[expect(
        clippy::match_same_arms,
        reason = "match_same_arms: match arm 重复是为可读性/调试断点; 当前优先 expect"
    )]
    fn from(val: u8) -> Self {
        match val {
            0 => Self::None,
            1 => Self::Int,
            2 => Self::Ill,
            3 => Self::Fpe,
            4 => Self::Segv,
            5 => Self::Term,
            6 => Self::Kill,
            7 => Self::Stop,
            8 => Self::Cont,
            9 => Self::Chld,
            10 => Self::Usr1,
            11 => Self::Usr2,
            12 => Self::Alarm,
            13 => Self::Pipe,
            _ => Self::None,
        }
    }
}

/// 信号处理动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SignalAction {
    /// 默认处理 (终止/忽略)
    Default = 0,
    /// 忽略信号
    Ignore,
    /// 用户自定义处理器
    Handler,
    /// 阻塞信号
    Block,
}

// ============================================================================
// 核心数据结构
// ============================================================================

/// 等待队列项 (简化版，用于阻塞等待)
///
/// 在完整实现中应使用内核的 `wait_queue` 机制
#[derive(Debug, Clone, Copy)]
pub struct WaitQueueItem {
    /// 线程 ID
    pub tid: u32,
}

/// 简化版等待队列 (环形缓冲区) — B07-15: 中断安全化
///
/// 由 `IrqSpinLock` 保护队列结构; wake 路径在中断上下文用 `try_lock`
/// (仿 `services/net/wait_queue.rs::SocketWaitQueue` 范式), 避免在 ISR/
/// softirq 中持锁阻塞导致死锁. `try_lock` 失败时置 `wake_pending` 标志,
/// 由后续进程上下文路径补唤醒 (见 `drain_pending`).
#[derive(Debug)]
pub struct WaitQueue {
    items: [Option<WaitQueueItem>; 4],
    count: u32,
    /// 中断上下文 try_lock 失败时置位, 由进程上下文路径补唤醒
    wake_pending: bool,
    /// 队列锁 (中断安全)
    lock: crate::kernel::framework::sync::IrqSpinLock<()>,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            items: [const { None }; 4],
            count: 0,
            wake_pending: false,
            lock: crate::kernel::framework::sync::IrqSpinLock::new(()),
        }
    }

    pub fn init(&mut self) {
        self.items = [None; 4];
        self.count = 0;
        self.wake_pending = false;
    }

    pub fn count(&self) -> u32 {
        let _g = self.lock.lock();
        self.count
    }

    pub fn add(&mut self, item: WaitQueueItem) {
        let _g = self.lock.lock();
        if self.count < 4 {
            for i in 0..4 {
                if self.items[i].is_none() {
                    self.items[i] = Some(item);
                    self.count += 1;
                    return;
                }
            }
        }
    }

    /// 唤醒一个线程 (中断上下文安全)
    ///
    /// 在中断/softirq 上下文 (无法安全持锁) 下用 `try_lock`;
    /// 失败时置 pending 标志由进程上下文路径补唤醒. 返回被唤醒的项 (若有).
    pub fn wake_one(&mut self) -> Option<WaitQueueItem> {
        let Some(g) = self.lock.try_lock() else {
            // 中断上下文获取锁失败: 记 pending, 由进程上下文路径补唤醒.
            self.wake_pending = true;
            return None;
        };
        for i in 0..4 {
            if let Some(item) = self.items[i].take() {
                self.count -= 1;
                drop(g);
                return Some(item);
            }
        }
        drop(g);
        None
    }

    /// 唤醒所有线程 (中断上下文安全, 语义同 [`WaitQueue::wake_one`]).
    pub fn wake_all(&mut self) {
        let Some(g) = self.lock.try_lock() else {
            self.wake_pending = true;
            return;
        };
        for item in &mut self.items {
            *item = None;
        }
        self.count = 0;
        drop(g);
    }

    /// 补唤醒: 若存在 pending 唤醒请求, 在进程上下文持锁执行实际唤醒.
    ///
    /// 返回是否实际执行了唤醒 (有 pending 且成功取锁).
    pub fn drain_pending(&mut self) -> bool {
        if !self.wake_pending {
            return false;
        }
        let _g = self.lock.lock();
        if self.wake_pending {
            self.wake_pending = false;
            true
        } else {
            false
        }
    }
}

/// 管道结构体
///
/// 实现字节流通信，支持阻塞式读写
#[derive(Debug)]
pub struct Pipe {
    /// 全局唯一 ID
    pub id: IpcId,
    /// 环形缓冲区
    pub buffer: [u8; PIPE_BUFFER_SIZE],
    /// 读位置
    pub read_pos: u32,
    /// 写位置
    pub write_pos: u32,
    /// 当前缓冲区中的字节数
    pub count: u32,

    /// 创建者 PID (读端)
    pub read_pid: u32,
    /// 写端 PID
    pub write_pid: u32,

    /// 文件描述符 (读端)
    pub read_fd: i32,
    /// 文件描述符 (写端)
    pub write_fd: i32,

    /// 当前读者数量
    pub readers: i32,
    /// 当前写者数量
    pub writers: i32,

    /// 读等待队列
    pub read_wait: WaitQueue,
    /// 写等待队列
    pub write_wait: WaitQueue,

    /// 标志位 (`O_NONBLOCK` 等)
    pub flags: i32,
}

impl Pipe {
    pub const fn new() -> Self {
        Self {
            id: 0,
            buffer: [0u8; PIPE_BUFFER_SIZE],
            read_pos: 0,
            write_pos: 0,
            count: 0,
            read_pid: 0,
            write_pid: 0,
            read_fd: 0,
            write_fd: 0,
            readers: 0,
            writers: 0,
            read_wait: WaitQueue::new(),
            write_wait: WaitQueue::new(),
            flags: 0,
        }
    }
}

/// 信号处理函数签名
pub type SignalHandlerFn = extern "C" fn(i32);

/// 信号处理结构
#[derive(Debug, Clone, Copy)]
pub struct SignalHandler {
    /// 处理函数指针 (C ABI)
    pub handler: Option<SignalHandlerFn>,
    /// 处理函数地址 (用户空间)
    pub handler_addr: u64,
    /// 栈地址 (用户空间)
    pub stack_addr: u64,
    /// 标志位
    pub flags: u32,
    /// 信号掩码
    pub mask: u32,
}

/// 待处理信号状态
#[derive(Debug)]
pub struct SignalPending {
    /// 待处理信号位图
    pub pending: u32,
    /// 已屏蔽信号位图
    pub blocked: u32,
    /// 已注册的处理函数表
    pub handlers: [SignalHandler; IPC_MAX_SIGNALS],
}

impl SignalPending {
    pub const fn new() -> Self {
        Self {
            pending: 0,
            blocked: 0,
            handlers: [SignalHandler {
                handler: None,
                handler_addr: 0,
                stack_addr: 0,
                flags: 0,
                mask: 0,
            }; IPC_MAX_SIGNALS],
        }
    }
}

/// 共享内存段
#[derive(Debug)]
pub struct ShmSegment {
    /// 全局唯一 ID
    pub id: IpcId,
    /// 物理起始地址
    pub phys_addr: u64,
    /// 段大小 (字节)
    pub size: u64,

    /// 创建者 PID
    pub creator: u32,
    /// 引用计数
    pub ref_count: u32,

    /// 已附加的 PID 列表
    pub attached_pids: [u32; 16],
    /// 当前附加进程数
    pub attach_count: u32,

    /// 标志位
    pub flags: i32,
    /// 权限 (如 0666)
    pub perm: i32,
}

impl ShmSegment {
    pub const fn new() -> Self {
        Self {
            id: 0,
            phys_addr: 0,
            size: 0,
            creator: 0,
            ref_count: 0,
            attached_pids: [0u32; 16],
            attach_count: 0,
            flags: 0,
            perm: 0,
        }
    }
}

/// 消息结构
#[derive(Debug)]
pub struct Message {
    /// 消息类型 (用户自定义)
    pub type_: u64,
    /// 发送者 PID
    pub sender: u64,
    /// 数据长度
    pub size: u64,
    /// 消息数据
    pub data: [u8; MSG_MAX_SIZE],
    /// 下一条消息 (侵入式链表, AtomicPtr 保证 Send/Sync; 队列操作在持锁下执行, Relaxed 足够)
    pub next: AtomicPtr<Message>,
}

impl Message {
    pub const fn new() -> Self {
        Self {
            type_: 0,
            sender: 0,
            size: 0,
            data: [0u8; MSG_MAX_SIZE],
            next: AtomicPtr::new(core::ptr::null_mut()),
        }
    }
}

/// 消息队列
#[derive(Debug)]
pub struct MsgQueue {
    /// 全局唯一 ID
    pub id: IpcId,
    /// 所有者 PID
    pub owner: u32,

    /// 队列头指针 (AtomicPtr: B03-24 根治, 使 IpcNamespace 满足 Send)
    pub head: AtomicPtr<Message>,
    /// 队列尾指针
    pub tail: AtomicPtr<Message>,
    /// 当前消息数
    pub count: u32,
    /// 最大消息数
    pub max_msgs: u32,
    /// 单条最大字节数
    pub max_size: u32,

    /// 发送等待队列
    pub send_wait: WaitQueue,
    /// 接收等待队列
    pub recv_wait: WaitQueue,

    /// 标志位
    pub flags: i32,
    /// 权限
    pub perm: i32,
}

impl MsgQueue {
    pub const fn new() -> Self {
        Self {
            id: 0,
            owner: 0,
            head: AtomicPtr::new(core::ptr::null_mut()),
            tail: AtomicPtr::new(core::ptr::null_mut()),
            count: 0,
            max_msgs: MSG_QUEUE_MAX_MSGS,
            max_size: MSG_MAX_SIZE as u32,
            send_wait: WaitQueue::new(),
            recv_wait: WaitQueue::new(),
            flags: 0,
            perm: 0,
        }
    }
}

/// 信号量
#[derive(Debug)]
pub struct Semaphore {
    /// 全局唯一 ID
    pub id: IpcId,
    /// 所有者 PID
    pub owner: u32,

    /// 当前计数 (可为负表示等待线程数)
    pub count: i32,
    /// 最大计数值
    pub max_count: u32,

    /// 等待队列
    pub wait: WaitQueue,

    /// 标志位
    pub flags: i32,
    /// 权限
    pub perm: i32,
}

impl Semaphore {
    pub const fn new() -> Self {
        Self {
            id: 0,
            owner: 0,
            count: 0,
            max_count: 0,
            wait: WaitQueue::new(),
            flags: 0,
            perm: 0,
        }
    }
}

/// IPC 命名空间 (全局资源容器)
///
/// 存储所有 IPC 资源的静态数组。
/// 内部 NonNull 链表指针仅在持锁时访问 (Mutex 保护每个数组元素),
/// 故 IPC 命名空间整体可安全跨线程传递。
#[derive(Debug)]
pub struct IpcNamespace {
    /// 管道数组
    pub pipes: [Pipe; IPC_MAX_PIPES],
    /// 共享内存段数组
    pub shm_segs: [ShmSegment; IPC_MAX_SHM_SEGS],
    /// 消息队列数组
    pub msg_queues: [MsgQueue; IPC_MAX_MSG_QUEUES],
    /// 信号量数组
    pub semaphores: [Semaphore; IPC_MAX_SEMAPHORES],
}
