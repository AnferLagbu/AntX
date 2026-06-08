//! inotify — 文件系统事件通知机制 (TCB)
//!
//! 实现 Linux inotify API: inotify_init1 / inotify_add_watch / inotify_rm_watch.
//!
//! ## 架构
//!
//! ```text
//! InotifyInstance (全局实例表, 固定大小数组)
//!   ├── watches:    wd → (ino, mask) 映射 (每个 watch 监控一个 inode)
//!   ├── events:     事件环形队列 (FIFO, 溢出时丢弃最旧事件)
//!   └── epoll 集成: 有事件时 EPOLLIN 可读
//!
//! VFS 操作触发 inotify_notify():
//!   create / mkdir / unlink / rmdir / write / truncate / rename / ...
//!     → 查找该 inode 上的所有 watch
//!     → 生成 inotify_event 放入事件队列
//!     → 唤醒 epoll (如果 inotify fd 被 epoll 监控)
//!
//! sys_read(inotify_fd):
//!     → 从事件队列读取 inotify_event 结构体
//! ```
//!
//! ## 与 Linux 的差异
//!
//! - 使用固定大小数组 (8 实例, 每实例 16 watch, 64 事件) 而非动态分配
//! - 不支持 IN_DONT_FOLLOW / IN_ONLYDIR / IN_EXCL_UNLINK
//! - 不支持 IN_MOVE (仅 IN_MOVED_FROM / IN_MOVED_TO)
//! - 事件队列溢出时丢弃最旧事件 (Linux 设置 IN_Q_OVERFLOW)
//!
//! # Safety
//!
//! - inotify 实例通过全局 ID 分配, 避免指针悬挂
//! - inotify_notify 在 VFS 路径调用, 持锁时不可睡眠
//! - 事件队列使用 IrqSpinLock 保护, 中断安全

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock as Mutex;
use crate::kernel::framework::syscall::types::Errno;
use core::sync::atomic::Ordering;

// ============================================================================
// inotify 常量
// ============================================================================

/// IN_ACCESS: 文件被访问
pub const IN_ACCESS: u32 = 0x0000_0001;
/// IN_MODIFY: 文件被修改
pub const IN_MODIFY: u32 = 0x0000_0002;
/// IN_ATTRIB: 文件属性变化
pub const IN_ATTRIB: u32 = 0x0000_0004;
/// IN_CLOSE_WRITE: 可写文件被关闭
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
/// IN_CLOSE_NOWRITE: 不可写文件被关闭
pub const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
/// IN_OPEN: 文件被打开
pub const IN_OPEN: u32 = 0x0000_0020;
/// IN_MOVED_FROM: 文件被移出监控目录
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
/// IN_MOVED_TO: 文件被移入监控目录
pub const IN_MOVED_TO: u32 = 0x0000_0080;
/// IN_CREATE: 在监控目录中创建文件
pub const IN_CREATE: u32 = 0x0000_0100;
/// IN_DELETE: 在监控目录中删除文件
pub const IN_DELETE: u32 = 0x0000_0200;
/// IN_DELETE_SELF: 被监控文件自身被删除
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
/// IN_MOVE_SELF: 被监控文件自身被移动
pub const IN_MOVE_SELF: u32 = 0x0000_0800;

/// IN_ISDIR: 事件对象是目录
pub const IN_ISDIR: u32 = 0x4000_0000;
/// IN_Q_OVERFLOW: 事件队列溢出
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;
/// IN_IGNORED: watch 被移除 (内核自动发送)
pub const IN_IGNORED: u32 = 0x0000_8000;

/// IN_NONBLOCK: 非阻塞模式 (inotify_init1 标志)
pub const IN_NONBLOCK: i32 = 0x0800;
/// IN_CLOEXEC: 执行时关闭 (inotify_init1 标志)
pub const IN_CLOEXEC: i32 = 0x0200_0000;

/// IN_ALL_EVENTS: 所有事件的掩码
pub const IN_ALL_EVENTS: u32 = IN_ACCESS
    | IN_MODIFY
    | IN_ATTRIB
    | IN_CLOSE_WRITE
    | IN_CLOSE_NOWRITE
    | IN_OPEN
    | IN_MOVED_FROM
    | IN_MOVED_TO
    | IN_CREATE
    | IN_DELETE
    | IN_DELETE_SELF
    | IN_MOVE_SELF;

/// 最大 inotify 实例数
const INOTIFY_MAX_INSTANCES: usize = 8;
/// 每实例最大 watch 数
const INOTIFY_MAX_WATCHES: usize = 16;
/// 每实例最大事件队列深度
const INOTIFY_MAX_EVENTS: usize = 64;
/// 文件名最大长度 (inotify_event.name)
const INOTIFY_MAX_NAME: usize = 32;
/// inotify FD 空间起始
pub const INOTIFY_FD_BASE: i32 = 260;

// ============================================================================
// inotify 数据结构
// ============================================================================

/// inotify_event — 用户空间事件结构 (与 Linux ABI 兼容)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InotifyEvent {
    /// watch 描述符
    pub wd: i32,
    /// 事件掩码
    pub mask: u32,
    /// 关联的 cookie (用于关联 IN_MOVED_FROM/TO, v1 暂为 0)
    pub cookie: u32,
    /// name 字段长度 (含 \0)
    pub len: u32,
    /// 可选文件名 (目录事件时为被操作的文件名)
    pub name: [u8; INOTIFY_MAX_NAME],
}

impl Default for InotifyEvent {
    fn default() -> Self {
        Self::new()
    }
}

impl InotifyEvent {
    const fn new() -> Self {
        Self {
            wd: 0,
            mask: 0,
            cookie: 0,
            len: 0,
            name: [0; INOTIFY_MAX_NAME],
        }
    }
    /// 事件结构体的固定部分大小 (不含 name)
    pub const FIXED_SIZE: usize = 16; // wd(4) + mask(4) + cookie(4) + len(4)

    /// 总大小 (含 name)
    pub const FULL_SIZE: usize = Self::FIXED_SIZE + INOTIFY_MAX_NAME;

    /// 设置 name 字段
    fn set_name(&mut self, name: &str) {
        let bytes = name.as_bytes();
        let len = bytes.len().min(INOTIFY_MAX_NAME - 1);
        self.name[..len].copy_from_slice(&bytes[..len]);
        self.name[len] = 0;
        self.len = if name.is_empty() { 0 } else { (len + 1) as u32 };
    }
}

/// watch 条目
#[derive(Debug, Clone, Copy)]
struct WatchEntry {
    /// watch 描述符 (wd)
    wd: i32,
    /// 被监控的 inode 号
    ino: u32,
    /// 事件掩码
    mask: u32,
    /// 是否有效
    valid: bool,
}

impl Default for WatchEntry {
    fn default() -> Self {
        Self::new()
    }
}

impl WatchEntry {
    const fn new() -> Self {
        Self {
            wd: 0,
            ino: 0,
            mask: 0,
            valid: false,
        }
    }
}

/// inotify 实例
struct InotifyInstance {
    /// 实例 slot 索引 (fd = INOTIFY_FD_BASE + slot_idx)
    slot_idx: usize,
    /// watch 表
    watches: [WatchEntry; INOTIFY_MAX_WATCHES],
    /// watch 计数
    watch_count: usize,
    /// 下一个 wd (从 1 开始)
    next_wd: i32,
    /// 事件环形队列
    events: [InotifyEvent; INOTIFY_MAX_EVENTS],
    /// 队列头 (读位置)
    event_head: usize,
    /// 队列尾 (写位置)
    event_tail: usize,
    /// 队列中事件数
    event_count: usize,
    /// 是否有效
    valid: bool,
}

impl InotifyInstance {
    const fn new() -> Self {
        Self {
            slot_idx: 0,
            watches: [WatchEntry::new(); INOTIFY_MAX_WATCHES],
            watch_count: 0,
            next_wd: 1,
            events: [InotifyEvent::new(); INOTIFY_MAX_EVENTS],
            event_head: 0,
            event_tail: 0,
            event_count: 0,
            valid: false,
        }
    }

    fn init(&mut self, slot_idx: usize) {
        self.slot_idx = slot_idx;
        self.watch_count = 0;
        self.next_wd = 1;
        self.event_head = 0;
        self.event_tail = 0;
        self.event_count = 0;
        self.valid = true;
        for w in &mut self.watches {
            *w = WatchEntry::default();
        }
    }

    /// 获取该实例的 fd
    fn fd(&self) -> i32 {
        INOTIFY_FD_BASE + self.slot_idx as i32
    }

    /// 入队一个事件, 队列满时丢弃最旧事件
    fn push_event(&mut self, event: InotifyEvent) {
        if self.event_count == INOTIFY_MAX_EVENTS {
            // 队列满, 丢弃最旧事件
            self.event_head = (self.event_head + 1) % INOTIFY_MAX_EVENTS;
            self.event_count -= 1;
        }
        self.events[self.event_tail] = event;
        self.event_tail = (self.event_tail + 1) % INOTIFY_MAX_EVENTS;
        self.event_count += 1;
    }

    /// 出队一个事件
    fn pop_event(&mut self) -> Option<InotifyEvent> {
        if self.event_count == 0 {
            return None;
        }
        let event = self.events[self.event_head];
        self.event_head = (self.event_head + 1) % INOTIFY_MAX_EVENTS;
        self.event_count -= 1;
        Some(event)
    }

    /// 查找指定 inode 上的 watch
    fn find_watch_by_ino(&self, ino: u32) -> Option<usize> {
        self.watches
            .iter()
            .position(|w| w.valid && w.ino == ino)
    }

    /// 查找指定 wd 的 watch
    fn find_watch_by_wd(&self, wd: i32) -> Option<usize> {
        self.watches
            .iter()
            .position(|w| w.valid && w.wd == wd)
    }

    /// 添加 watch, 返回 wd
    fn add_watch(&mut self, ino: u32, mask: u32) -> Result<i32, Errno> {
        // 如果该 inode 已有 watch, 更新 mask
        if let Some(idx) = self.find_watch_by_ino(ino) {
            self.watches[idx].mask = mask;
            return Ok(self.watches[idx].wd);
        }

        // 找空闲槽位
        let idx = self
            .watches
            .iter()
            .position(|w| !w.valid)
            .ok_or(Errno::ENOSPC)?;

        let wd = self.next_wd;
        self.next_wd += 1;
        self.watches[idx] = WatchEntry {
            wd,
            ino,
            mask,
            valid: true,
        };
        self.watch_count += 1;
        Ok(wd)
    }

    /// 移除 watch
    fn remove_watch(&mut self, wd: i32) -> Result<(), Errno> {
        let idx = self
            .find_watch_by_wd(wd)
            .ok_or(Errno::EINVAL)?;
        self.watches[idx] = WatchEntry::default();
        self.watch_count -= 1;
        Ok(())
    }
}

// ============================================================================
// 全局状态
// ============================================================================

/// inotify 实例表
static INOTIFY_INSTANCES: Mutex<[InotifyInstance; INOTIFY_MAX_INSTANCES]> =
    Mutex::new([
        InotifyInstance::new(),
        InotifyInstance::new(),
        InotifyInstance::new(),
        InotifyInstance::new(),
        InotifyInstance::new(),
        InotifyInstance::new(),
        InotifyInstance::new(),
        InotifyInstance::new(),
    ]);

/// 统计: inotify 操作计数
static INOTIFY_OPS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

// ============================================================================
// inotify 系统调用实现
// ============================================================================

/// inotify_init1 — 创建 inotify 实例
///
/// `flags`: IN_NONBLOCK | IN_CLOEXEC
/// 返回 inotify fd.
pub fn sys_inotify_init1(flags: i32) -> i64 {
    INOTIFY_OPS.fetch_add(1, Ordering::Relaxed);

    // flags 只允许 IN_NONBLOCK | IN_CLOEXEC
    if flags & !(IN_NONBLOCK | IN_CLOEXEC) != 0 {
        return Errno::EINVAL.as_ret();
    }

    let mut instances = INOTIFY_INSTANCES.lock();
    let (slot_idx, fd) = match instances.iter_mut().enumerate().find(|(_, i)| !i.valid) {
        Some((idx, slot)) => {
            slot.init(idx);
            (idx, slot.fd())
        }
        None => return Errno::EMFILE.as_ret(),
    };

    crate::klog_debug!(FS, "[inotify] Created instance fd={} slot={}", fd, slot_idx);
    fd as i64
}

/// inotify_add_watch — 添加 watch
///
/// `fd`: inotify fd
/// `ino`: 被监控的 inode 号
/// `mask`: 事件掩码
/// 返回 watch 描述符 (wd).
pub fn sys_inotify_add_watch(fd: i64, ino: u32, mask: u32) -> i64 {
    INOTIFY_OPS.fetch_add(1, Ordering::Relaxed);

    if !is_inotify_fd(fd as i32) || ino == 0 {
        return Errno::EBADF.as_ret();
    }

    // mask 必须包含至少一个事件
    if mask & IN_ALL_EVENTS == 0 {
        return Errno::EINVAL.as_ret();
    }

    let slot = fd_to_slot(fd as i32);
    let mut instances = INOTIFY_INSTANCES.lock();

    let instance = match instances.iter_mut().find(|i| i.valid && i.slot_idx == slot) {
        Some(i) => i,
        None => return Errno::EBADF.as_ret(),
    };

    match instance.add_watch(ino, mask) {
        Ok(wd) => {
            crate::klog_debug!(FS, "[inotify] ADD_WATCH ino={} mask=0x{:X} wd={}", ino, mask, wd);
            wd as i64
        }
        Err(e) => e.as_ret(),
    }
}

/// inotify_rm_watch — 移除 watch
///
/// `fd`: inotify fd
/// `wd`: watch 描述符
pub fn sys_inotify_rm_watch(fd: i64, wd: i32) -> i64 {
    INOTIFY_OPS.fetch_add(1, Ordering::Relaxed);

    if !is_inotify_fd(fd as i32) {
        return Errno::EBADF.as_ret();
    }

    let slot = fd_to_slot(fd as i32);
    let mut instances = INOTIFY_INSTANCES.lock();

    let instance = match instances.iter_mut().find(|i| i.valid && i.slot_idx == slot) {
        Some(i) => i,
        None => return Errno::EBADF.as_ret(),
    };

    // 移除前发送 IN_IGNORED 事件
    if instance.find_watch_by_wd(wd).is_some() {
        let ignored_event = InotifyEvent {
            wd,
            mask: IN_IGNORED,
            cookie: 0,
            len: 0,
            name: [0; INOTIFY_MAX_NAME],
        };
        instance.push_event(ignored_event);
    }

    match instance.remove_watch(wd) {
        Ok(()) => {
            crate::klog_debug!(FS, "[inotify] RM_WATCH wd={}", wd);
            0
        }
        Err(e) => e.as_ret(),
    }
}

/// inotify_read — 从 inotify fd 读取事件
///
/// 返回读取的字节数, 或错误码.
pub fn sys_inotify_read(fd: i64, buf: *mut u8, count: usize) -> i64 {
    if !is_inotify_fd(fd as i32) || buf.is_null() || count < InotifyEvent::FULL_SIZE {
        return Errno::EINVAL.as_ret();
    }

    let slot = fd_to_slot(fd as i32);
    let mut instances = INOTIFY_INSTANCES.lock();

    let instance = match instances.iter_mut().find(|i| i.valid && i.slot_idx == slot) {
        Some(i) => i,
        None => return Errno::EBADF.as_ret(),
    };

    let mut written = 0usize;
    while written + InotifyEvent::FULL_SIZE <= count {
        let event = match instance.pop_event() {
            Some(e) => e,
            None => break,
        };

        // SAFETY: buf 非空且 count 已验证, written + FULL_SIZE <= count
        let dst = unsafe { buf.add(written) as *mut InotifyEvent };
        // SAFETY: dst 对齐且在 [buf, buf+count) 范围内
        unsafe { core::ptr::write(dst, event) };
        written += InotifyEvent::FULL_SIZE;
    }

    if written == 0 {
        Errno::EAGAIN.as_ret()
    } else {
        written as i64
    }
}

// ============================================================================
// VFS 事件通知接口
// ============================================================================

/// 判断 fd 是否属于 inotify FD 空间
pub fn is_inotify_fd(fd: i32) -> bool {
    fd >= INOTIFY_FD_BASE && fd < INOTIFY_FD_BASE + INOTIFY_MAX_INSTANCES as i32
}

/// fd → slot 索引
fn fd_to_slot(fd: i32) -> usize {
    (fd - INOTIFY_FD_BASE) as usize
}

/// 通知所有监控指定 inode 的 inotify 实例
///
/// 在 VFS 操作 (create/unlink/write/truncate 等) 完成后调用.
/// `ino`: 发生事件的 inode 号
/// `mask`: 事件类型 (IN_CREATE | IN_DELETE | ...)
/// `name`: 可选文件名 (目录事件时为被操作的文件名, 如 "foo.txt")
/// `is_dir`: 事件对象是否为目录
pub fn inotify_notify(ino: u32, mask: u32, name: &str, is_dir: bool) {
    if ino == 0 {
        return;
    }

    let mut instances = INOTIFY_INSTANCES.lock();
    let mut notified_fds = [false; INOTIFY_MAX_INSTANCES];
    let mut notified_count = 0usize;

    for (i, instance) in instances.iter_mut().enumerate() {
        if !instance.valid {
            continue;
        }

        // 查找该 inode 上的 watch
        let watch_idx = match instance.find_watch_by_ino(ino) {
            Some(idx) => idx,
            None => continue,
        };

        let watch = &instance.watches[watch_idx];

        // 检查 watch 是否关心此事件
        if watch.mask & mask == 0 {
            continue;
        }

        let mut event = InotifyEvent {
            wd: watch.wd,
            mask,
            cookie: 0,
            len: 0,
            name: [0; INOTIFY_MAX_NAME],
        };

        if is_dir {
            event.mask |= IN_ISDIR;
        }

        if !name.is_empty() {
            event.set_name(name);
        }

        instance.push_event(event);

        if notified_count < INOTIFY_MAX_INSTANCES {
            notified_fds[i] = true;
            notified_count += 1;
        }
    }

    if notified_count > 0 {
        // 唤醒 epoll 中监控 inotify fd 的等待者
        drop(instances);
        for i in 0..INOTIFY_MAX_INSTANCES {
            if notified_fds[i] {
                crate::kernel::framework::syscall::epoll::epoll_pwake(
                    INOTIFY_FD_BASE + i as i32,
                );
            }
        }
    }
}

/// 释放指定 inotify 实例的所有资源
///
/// 在 close(inotify_fd) 时调用.
pub fn inotify_release(fd: i64) {
    if !is_inotify_fd(fd as i32) {
        return;
    }

    let slot = fd_to_slot(fd as i32);
    let mut instances = INOTIFY_INSTANCES.lock();

    if let Some(instance) = instances.iter_mut().find(|i| i.valid && i.slot_idx == slot) {
        crate::klog_debug!(FS, "[inotify] Release instance fd={}", instance.fd());
        instance.valid = false;
        instance.watch_count = 0;
    }
}

/// 检查指定 inotify fd 是否有事件可读 (epoll 集成用)
pub fn inotify_fd_readable(fd: i64) -> bool {
    if !is_inotify_fd(fd as i32) {
        return false;
    }

    let slot = fd_to_slot(fd as i32);
    let instances = INOTIFY_INSTANCES.lock();

    instances
        .iter()
        .find(|i| i.valid && i.slot_idx == slot)
        .map_or(false, |i| i.event_count > 0)
}

/// 获取统计信息
pub fn inotify_stats() -> (u64, u64) {
    let instances = INOTIFY_INSTANCES.lock();
    let active = instances.iter().filter(|i| i.valid).count() as u64;
    let ops = INOTIFY_OPS.load(Ordering::Relaxed);
    (active, ops)
}
