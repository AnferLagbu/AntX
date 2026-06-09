//! io_uring 异步 I/O 框架 (C4)
//!
//! 提供基于共享内存环形缓冲区的异步 I/O 接口, 类似 Linux io_uring.
//!
//! ## 架构
//!
//! ```text
//! services/io/iouring.rs (safe 代理)
//!     │
//!     ▼
//! framework/io/iouring.rs (本文件, TCB)
//!     │
//!     ▼
//! framework/fs/vfs (文件操作) / framework/net (网络操作)
//! ```
//!
//! ## 核心概念
//!
//! - **SQ (Submission Queue)**: 用户态写入 I/O 请求, 内核消费
//! - **CQ (Completion Queue)**: 内核写入完成结果, 用户态消费
//! - **SQE (Submission Queue Entry)**: 单个 I/O 请求
//! - **CQE (Completion Queue Entry)**: 单个完成结果
//!
//! ## 数据流
//!
//! ```text
//! 用户态                    内核态
//!   │                        │
//!   ├── SQE → SQ ring ──────┤
//!   │                        ├── 取出 SQE, 执行 I/O
//!   │                        ├── 写入 CQE → CQ ring
//!   ├── CQ ring ────────────┤
//!   ├── 取出 CQE            │
//! ```

use core::sync::atomic::{AtomicU32, Ordering};

use alloc::vec::Vec;

use spin::Mutex;

use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// 常量
// ============================================================================

/// 最大 io_uring 实例数
pub const MAX_URING_INSTANCES: usize = 16;

/// 默认队列深度 (2 的幂)
pub const DEFAULT_RING_SIZE: u32 = 256;

// ============================================================================
// 操作码
// ============================================================================

/// io_uring 操作码
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IoOpCode {
    /// 无操作
    Nop = 0,
    /// 读
    Read = 1,
    /// 写
    Write = 2,
    /// fsync
    Fsync = 3,
    /// accept (网络)
    Accept = 4,
    /// connect (网络)
    Connect = 5,
    /// 发送
    Send = 6,
    /// 接收
    Recv = 7,
    /// 超时等待
    Timeout = 8,
}

impl IoOpCode {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Nop),
            1 => Some(Self::Read),
            2 => Some(Self::Write),
            3 => Some(Self::Fsync),
            4 => Some(Self::Accept),
            5 => Some(Self::Connect),
            6 => Some(Self::Send),
            7 => Some(Self::Recv),
            8 => Some(Self::Timeout),
            _ => None,
        }
    }
}

// ============================================================================
// SQE / CQE
// ============================================================================

/// 提交队列条目 (SQE)
///
/// 用户态写入, 内核消费.
#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct Sqe {
    /// 操作码
    pub opcode: u8,
    /// 标志位
    pub flags: u8,
    /// 用户数据 (原样传递到 CQE)
    pub user_data: u64,
    /// 文件描述符
    pub fd: i32,
    /// 偏移量 (文件 I/O)
    pub offset: u64,
    /// 地址 (缓冲区指针)
    pub addr: u64,
    /// 长度
    pub len: u32,
}

/// 完成队列条目 (CQE)
///
/// 内核写入, 用户态消费.
#[derive(Debug, Clone, Default)]
#[repr(C)]
pub struct Cqe {
    /// 用户数据 (从 SQE 复制)
    pub user_data: u64,
    /// 结果 (成功为正数/0, 失败为负 errno)
    pub result: i32,
    /// 标志位
    pub flags: u32,
}

// ============================================================================
// 环形缓冲区
// ============================================================================

/// 环形缓冲区 (SQ 或 CQ 共用结构)
pub struct RingBuffer<T> {
    /// 环形存储
    entries: Vec<T>,
    /// 容量 (2 的幂)
    capacity: u32,
    /// 掩码 (capacity - 1)
    mask: u32,
    /// 头索引 (消费位置)
    head: AtomicU32,
    /// 尾索引 (生产位置)
    tail: AtomicU32,
}

impl<T: Clone + Default> RingBuffer<T> {
    /// 创建指定容量的环形缓冲区
    pub fn new(capacity: u32) -> Self {
        let cap = capacity.next_power_of_two();
        let mut entries = Vec::with_capacity(cap as usize);
        for _ in 0..cap {
            entries.push(T::default());
        }
        Self {
            entries,
            capacity: cap,
            mask: cap - 1,
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    /// 当前条目数
    pub fn len(&self) -> u32 {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        tail.wrapping_sub(head)
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 是否已满
    pub fn is_full(&self) -> bool {
        self.len() >= self.capacity
    }

    /// 生产: 写入一个条目 (成功返回 true)
    pub fn push(&mut self, entry: T) -> bool {
        if self.is_full() {
            return false;
        }
        let tail = self.tail.load(Ordering::Acquire);
        let idx = (tail & self.mask) as usize;
        self.entries[idx] = entry;
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// 消费: 读取一个条目 (成功返回 Some)
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let head = self.head.load(Ordering::Acquire);
        let idx = (head & self.mask) as usize;
        let entry = self.entries[idx].clone();
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(entry)
    }

    /// 窥视 (不移动 head)
    pub fn peek(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let head = self.head.load(Ordering::Acquire);
        let idx = (head & self.mask) as usize;
        Some(self.entries[idx].clone())
    }
}

// ============================================================================
// io_uring 实例
// ============================================================================

/// io_uring 实例
pub struct IoUring {
    /// 实例 ID
    pub id: u32,
    /// 队列深度
    pub ring_size: u32,
    /// 提交队列
    pub sq: Mutex<RingBuffer<Sqe>>,
    /// 完成队列
    pub cq: Mutex<RingBuffer<Cqe>>,
    /// 所属进程 PID
    pub owner_pid: u32,
    /// 标志位
    pub flags: AtomicU32,
}

impl IoUring {
    /// 创建新的 io_uring 实例
    pub fn new(id: u32, ring_size: u32, owner_pid: u32) -> Self {
        Self {
            id,
            ring_size,
            sq: Mutex::new(RingBuffer::new(ring_size)),
            cq: Mutex::new(RingBuffer::new(ring_size)),
            owner_pid,
            flags: AtomicU32::new(0),
        }
    }

    /// 提交 SQE
    pub fn submit_sqe(&self, sqe: Sqe) -> Result<(), Errno> {
        let mut sq = self.sq.lock();
        if sq.push(sqe) {
            Ok(())
        } else {
            Err(Errno::EBUSY)
        }
    }

    /// 取出下一个 SQE (内核消费)
    pub fn consume_sqe(&self) -> Option<Sqe> {
        let mut sq = self.sq.lock();
        sq.pop()
    }

    /// 推送 CQE (内核生产)
    pub fn push_cqe(&self, cqe: Cqe) -> Result<(), Errno> {
        let mut cq = self.cq.lock();
        if cq.push(cqe) {
            Ok(())
        } else {
            Err(Errno::EBUSY)
        }
    }

    /// 取出下一个 CQE (用户态消费)
    pub fn reap_cqe(&self) -> Option<Cqe> {
        let mut cq = self.cq.lock();
        cq.pop()
    }

    /// 处理所有待处理的 SQE
    ///
    /// 取出 SQE, 执行操作, 推送 CQE.
    pub fn process_pending(&self) -> u32 {
        let mut processed = 0u32;
        loop {
            let sqe = match self.consume_sqe() {
                Some(s) => s,
                None => break,
            };

            let result = self.execute_op(&sqe);
            let cqe = Cqe {
                user_data: sqe.user_data,
                result,
                flags: 0,
            };

            if self.push_cqe(cqe).is_err() {
                // CQ 满了, 放回 SQE
                let _ = self.submit_sqe(sqe);
                break;
            }
            processed += 1;
        }
        processed
    }

    /// 执行单个操作
    fn execute_op(&self, sqe: &Sqe) -> i32 {
        let opcode = match IoOpCode::from_u8(sqe.opcode) {
            Some(op) => op,
            None => return -(Errno::EINVAL as i32),
        };

        match opcode {
            IoOpCode::Nop => 0,
            IoOpCode::Read | IoOpCode::Write | IoOpCode::Fsync => {
                // TODO: 集成 VFS fd 表, 通过 fd 查找文件并执行 I/O
                // 当前返回 ENOSYS, 待 VFS fd 表统一后实现
                -(Errno::ENOSYS as i32)
            }
            IoOpCode::Accept
            | IoOpCode::Connect
            | IoOpCode::Send
            | IoOpCode::Recv => {
                // TODO: 实现网络异步操作
                -(Errno::ENOSYS as i32)
            }
            IoOpCode::Timeout => {
                // TODO: 实现超时等待
                -(Errno::ENOSYS as i32)
            }
        }
    }
}

// ============================================================================
// 全局 io_uring 管理
// ============================================================================

/// 全局 io_uring 实例表
static URING_TABLE: Mutex<Vec<Option<IoUring>>> = Mutex::new(Vec::new());
static NEXT_URING_ID: AtomicU32 = AtomicU32::new(0);

/// 创建 io_uring 实例
pub fn io_uring_setup(entries: u32, owner_pid: u32) -> Result<u32, Errno> {
    let ring_size = if entries == 0 { DEFAULT_RING_SIZE } else { entries.next_power_of_two() };

    let mut table = URING_TABLE.lock();
    if table.len() >= MAX_URING_INSTANCES {
        return Err(Errno::ENOMEM);
    }

    let id = NEXT_URING_ID.fetch_add(1, Ordering::SeqCst);
    let uring = IoUring::new(id, ring_size, owner_pid);
    table.push(Some(uring));
    Ok(id)
}

/// 销毁 io_uring 实例
pub fn io_uring_destroy(id: u32) -> Result<(), Errno> {
    let mut table = URING_TABLE.lock();
    let pos = table.iter().position(|u| u.as_ref().map_or(false, |u| u.id == id));
    match pos {
        Some(i) => {
            table.remove(i);
            Ok(())
        }
        None => Err(Errno::EBADF),
    }
}

/// 提交 SQE 到指定实例
pub fn io_uring_submit(id: u32, sqe: Sqe) -> Result<(), Errno> {
    let table = URING_TABLE.lock();
    let uring = table
        .iter()
        .find(|u| u.as_ref().map_or(false, |u| u.id == id))
        .and_then(|u| u.as_ref());
    match uring {
        Some(u) => u.submit_sqe(sqe),
        None => Err(Errno::EBADF),
    }
}

/// 进入 io_uring (处理待处理请求 + 可选等待)
///
/// 返回处理的 CQE 数量
pub fn io_uring_enter(id: u32, to_submit: u32, _min_complete: u32) -> Result<u32, Errno> {
    let table = URING_TABLE.lock();
    let uring = table
        .iter()
        .find(|u| u.as_ref().map_or(false, |u| u.id == id))
        .and_then(|u| u.as_ref());

    match uring {
        Some(u) => {
            // 处理 to_submit 个 SQE
            let to_process = to_submit.min(u.sq.lock().len());
            drop(table); // 释放表锁, process_pending 内部获取实例锁

            // 重新获取 (因为 drop 了 table)
            let table = URING_TABLE.lock();
            let uring = table
                .iter()
                .find(|u| u.as_ref().map_or(false, |u| u.id == id))
                .and_then(|u| u.as_ref());

            match uring {
                Some(u) => {
                    let mut processed = 0u32;
                    for _ in 0..to_process {
                        let sqe = match u.consume_sqe() {
                            Some(s) => s,
                            None => break,
                        };
                        let result = u.execute_op(&sqe);
                        let cqe = Cqe {
                            user_data: sqe.user_data,
                            result,
                            flags: 0,
                        };
                        if u.push_cqe(cqe).is_err() {
                            let _ = u.submit_sqe(sqe);
                            break;
                        }
                        processed += 1;
                    }
                    Ok(processed)
                }
                None => Err(Errno::EBADF),
            }
        }
        None => Err(Errno::EBADF),
    }
}

/// 收割 CQE
pub fn io_uring_reap(id: u32) -> Option<Cqe> {
    let table = URING_TABLE.lock();
    let uring = table
        .iter()
        .find(|u| u.as_ref().map_or(false, |u| u.id == id))
        .and_then(|u| u.as_ref());
    uring.and_then(|u| u.reap_cqe())
}

// ============================================================================
// Syscall 接口
// ============================================================================

/// sys_io_uring_setup — 创建 io_uring 实例
///
/// # 参数
/// - a0: entries (队列深度)
///
/// # 返回
/// - 成功: uring ID (非负)
/// - 失败: 负 errno
pub fn sys_io_uring_setup(entries: u64) -> i64 {
    let pid = crate::kernel::framework::proc::api::process_get_current_pid();
    match io_uring_setup(entries as u32, pid as u32) {
        Ok(id) => id as i64,
        Err(e) => -(e as i64),
    }
}

/// sys_io_uring_enter — 进入 io_uring (提交 + 等待完成)
///
/// # 参数
/// - a0: uring ID
/// - a1: to_submit (要提交的 SQE 数量)
/// - a2: min_complete (最少等待完成的 CQE 数量)
pub fn sys_io_uring_enter(id: u64, to_submit: u64, min_complete: u64) -> i64 {
    match io_uring_enter(id as u32, to_submit as u32, min_complete as u32) {
        Ok(n) => n as i64,
        Err(e) => -(e as i64),
    }
}

/// sys_io_uring_register — 注册缓冲区/文件 (当前桩实现)
///
/// # 参数
/// - a0: uring ID
/// - a1: opcode
/// - a2: arg
/// - a3: nr_args
pub fn sys_io_uring_register(_id: u64, _opcode: u64, _arg: u64, _nr_args: u64) -> i64 {
    // TODO: 实现缓冲区注册 / 文件注册
    -(Errno::ENOSYS as i64)
}

/// sys_io_uring_submit_sqe — 提交单个 SQE
///
/// # 参数
/// - a0: uring ID
/// - a1: opcode
/// - a2: flags
/// - a3: user_data
/// - a4: fd
/// - a5: (offset << 32) | len
///
/// 简化版: 通过寄存器传递关键字段而非用户态指针
pub fn sys_io_uring_submit_sqe(id: u64, opcode: u64, flags: u64, user_data: u64, fd: u64, offset_len: u64) -> i64 {
    let sqe = Sqe {
        opcode: opcode as u8,
        flags: flags as u8,
        user_data,
        fd: fd as i32,
        offset: offset_len >> 32,
        addr: 0, // 简化版不传地址
        len: (offset_len & 0xFFFF_FFFF) as u32,
    };

    match io_uring_submit(id as u32, sqe) {
        Ok(()) => 0,
        Err(e) => -(e as i64),
    }
}
