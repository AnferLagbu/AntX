//! epoll — 事件轮询机制 (TCB)
//!
//! 实现 Linux epoll API: epoll_create / epoll_ctl / epoll_wait.
//!
//! ## 架构
//!
//! ```text
//! epoll_instance (红黑树/哈希表存储 fd→event 映射)
//!   ├── interest_list: 注册的所有 fd + 事件
//!   └── ready_list:    就绪的 fd + 事件 (epoll_wait 返回)
//!
//! epoll_ctl(ADD):  fd → interest_list
//! epoll_ctl(MOD):  修改 interest_list 中的事件
//! epoll_ctl(DEL):  从 interest_list 移除
//! epoll_wait:      ready_list → 用户空间 (阻塞直到有事件或超时)
//! ```
//!
//! ## 与 Linux 的差异
//!
//! - 当前使用 Vec 而非红黑树 (简化实现, fd 数量有限)
//! - 就绪列表使用 spin::Mutex 保护
//! - 不支持 EPOLLEXCLUSIVE / EPOLLWAKEUP
//!
//! # Safety
//!
//! - epoll 实例通过全局 ID 分配, 避免指针悬挂
//! - 就绪回调在中断上下文调用, 不可睡眠

#![allow(dead_code)]

use alloc::vec::Vec;
use spin::Mutex;

use crate::kernel::framework::syscall::types::Errno;

// ============================================================================
// epoll 常量
// ============================================================================

/// EPOLLIN: 可读事件
pub const EPOLLIN: u32 = 0x001;
/// EPOLLOUT: 可写事件
pub const EPOLLOUT: u32 = 0x004;
/// EPOLLERR: 错误事件
pub const EPOLLERR: u32 = 0x008;
/// EPOLLHUP: 挂断事件
pub const EPOLLHUP: u32 = 0x010;
/// EPOLLRDHUP: 对端关闭
pub const EPOLLRDHUP: u32 = 0x2000;
/// EPOLLET: 边沿触发
pub const EPOLLET: u32 = 1 << 31;
/// EPOLLONESHOT: 一次性事件
pub const EPOLLONESHOT: u32 = 1 << 30;

/// epoll_ctl 操作
pub const EPOLL_CTL_ADD: i32 = 1;
pub const EPOLL_CTL_DEL: i32 = 2;
pub const EPOLL_CTL_MOD: i32 = 3;

// ============================================================================
// epoll 数据结构
// ============================================================================

/// epoll_event — 用户空间事件结构
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct EpollEvent {
    /// 事件掩码 (EPOLLIN | EPOLLOUT | ...)
    pub events: u32,
    /// 用户数据 (64-bit, 透传)
    pub data: u64,
}

/// 内核跟踪的 fd 项
#[derive(Debug, Clone, Copy)]
struct EpollItem {
    /// 监控的 fd
    fd: i32,
    /// 事件掩码
    events: u32,
    /// 用户数据
    data: u64,
    /// 是否边沿触发
    is_et: bool,
    /// 是否一次性
    is_oneshot: bool,
    /// 是否已就绪 (oneshot 用)
    ready: bool,
}

/// epoll 实例
struct EpollInstance {
    /// 感兴趣列表 (所有注册的 fd)
    interest_list: Vec<EpollItem>,
    /// 就绪列表 (有事件待处理的 fd)
    ready_list: Vec<EpollEvent>,
    /// 实例 ID
    id: u64,
}

// ============================================================================
// 全局状态
// ============================================================================

/// epoll 实例表
static EPOLL_INSTANCES: Mutex<Vec<EpollInstance>> = Mutex::new(Vec::new());
/// 下一个 epoll 实例 ID
static NEXT_EPOLL_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

// ============================================================================
// epoll 系统调用实现
// ============================================================================

/// epoll_create — 创建 epoll 实例
///
/// `size` 参数在 Linux 2.6.8+ 中被忽略, 但必须 > 0.
/// 返回 epoll fd (当前用实例 ID 代替).
pub fn sys_epoll_create(size: i32) -> i64 {
    if size <= 0 {
        return Errno::EINVAL.as_ret();
    }

    let id = NEXT_EPOLL_ID.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
    let instance = EpollInstance {
        interest_list: Vec::new(),
        ready_list: Vec::new(),
        id,
    };

    EPOLL_INSTANCES.lock().push(instance);

    crate::klog_debug!(Sync, "[epoll] Created instance id={}", id);
    id as i64
}

/// epoll_ctl — 控制 epoll 实例
///
/// - EPOLL_CTL_ADD: 注册 fd
/// - EPOLL_CTL_MOD: 修改 fd 的事件
/// - EPOLL_CTL_DEL: 移除 fd
pub fn sys_epoll_ctl(epfd: i64, op: i32, fd: i32, event: *const EpollEvent) -> i64 {
    if epfd <= 0 {
        return Errno::EBADF.as_ret();
    }
    if fd < 0 {
        return Errno::EBADF.as_ret();
    }

    let epfd_id = epfd as u64;
    let mut instances = EPOLL_INSTANCES.lock();

    // 查找 epoll 实例
    let idx = match instances.iter().position(|i| i.id == epfd_id) {
        Some(i) => i,
        None => return Errno::EBADF.as_ret(),
    };

    match op {
        EPOLL_CTL_ADD => {
            // 检查 fd 是否已存在
            if instances[idx].interest_list.iter().any(|item| item.fd == fd) {
                return Errno::EEXIST.as_ret();
            }

            // SAFETY: event 指针由 syscall 入口验证
            let ev = if !event.is_null() {
                unsafe { core::ptr::read(event) }
            } else {
                return Errno::EFAULT.as_ret();
            };

            let ev_events = ev.events;
            let ev_data = ev.data;

            let item = EpollItem {
                fd,
                events: ev_events & !EPOLLET & !EPOLLONESHOT,
                data: ev_data,
                is_et: (ev_events & EPOLLET) != 0,
                is_oneshot: (ev_events & EPOLLONESHOT) != 0,
                ready: false,
            };

            instances[idx].interest_list.push(item);
            crate::klog_debug!(Sync, "[epoll] ADD fd={} events=0x{:X} to epfd={}", fd, ev_events, epfd_id);
        }
        EPOLL_CTL_MOD => {
            let item = match instances[idx].interest_list.iter_mut().find(|i| i.fd == fd) {
                Some(i) => i,
                None => return Errno::ENOENT.as_ret(),
            };

            let ev = if !event.is_null() {
                unsafe { core::ptr::read(event) }
            } else {
                return Errno::EFAULT.as_ret();
            };

            let ev_events = ev.events;
            let ev_data = ev.data;

            item.events = ev_events & !EPOLLET & !EPOLLONESHOT;
            item.data = ev_data;
            item.is_et = (ev_events & EPOLLET) != 0;
            item.is_oneshot = (ev_events & EPOLLONESHOT) != 0;
            item.ready = false;

            crate::klog_debug!(Sync, "[epoll] MOD fd={} events=0x{:X} in epfd={}", fd, ev_events, epfd_id);
        }
        EPOLL_CTL_DEL => {
            let len_before = instances[idx].interest_list.len();
            instances[idx].interest_list.retain(|i| i.fd != fd);
            if instances[idx].interest_list.len() == len_before {
                return Errno::ENOENT.as_ret();
            }

            // 同时从就绪列表移除
            instances[idx].ready_list.retain(|e| e.data != fd as u64);

            crate::klog_debug!(Sync, "[epoll] DEL fd={} from epfd={}", fd, epfd_id);
        }
        _ => {
            return Errno::EINVAL.as_ret();
        }
    }

    0
}

/// epoll_wait — 等待事件
///
/// `maxevents` 必须大于 0.
/// `timeout`: -1=无限等待, 0=非阻塞, >0=毫秒超时.
/// 返回就绪事件数, 或错误码.
pub fn sys_epoll_wait(epfd: i64, events: *mut EpollEvent, maxevents: i32, timeout: i32) -> i64 {
    if epfd <= 0 || events.is_null() || maxevents <= 0 {
        return Errno::EINVAL.as_ret();
    }

    let epfd_id = epfd as u64;
    let mut instances = EPOLL_INSTANCES.lock();

    // 查找 epoll 实例
    let idx = match instances.iter().position(|i| i.id == epfd_id) {
        Some(i) => i,
        None => return Errno::EBADF.as_ret(),
    };

    // 扫描 interest_list, 检查哪些 fd 就绪
    // 当前简化实现: 轮询所有注册的 fd
    // TODO: 集成 VFS poll 机制, 由驱动回调唤醒
    let mut ready_events = Vec::new();

    for item in &instances[idx].interest_list {
        // 简化: 假设所有 fd 都有 EPOLLIN 就绪
        // 真实实现需要调用 vfs_poll(fd) 检查实际状态
        let revents = check_fd_ready(item.fd, item.events);

        if revents != 0 {
            ready_events.push(EpollEvent {
                events: revents,
                data: item.data,
            });

            // oneshot: 标记已就绪, 不再报告
            // (简化: 直接从 interest_list 中标记)
        }

        if ready_events.len() as i32 >= maxevents {
            break;
        }
    }

    // 如果没有就绪事件且 timeout != 0
    if ready_events.is_empty() && timeout != 0 {
        // 简化: 非阻塞返回
        // TODO: 阻塞等待, 使用 WaitQueue
        if timeout > 0 {
            // 等待 timeout 毫秒
            // 当前简化: 直接返回 0 (无事件)
        }
    }

    // 复制到用户空间
    let count = ready_events.len().min(maxevents as usize);
    for i in 0..count {
        // SAFETY: events 指针由 syscall 入口验证
        unsafe {
            core::ptr::write(events.add(i), ready_events[i]);
        }
    }

    crate::klog_debug!(Sync, "[epoll] WAIT epfd={} returned {} events", epfd_id, count);
    count as i64
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 检查 fd 是否就绪
///
/// 简化实现: 总是返回 EPOLLIN (可读).
/// 真实实现需要调用 VFS poll 操作.
fn check_fd_ready(_fd: i32, events: u32) -> u32 {
    // TODO: 集成 VFS poll
    // 当前: 假设 pipe/socket 可读
    let mut revents = 0u32;
    if events & EPOLLIN != 0 {
        revents |= EPOLLIN;
    }
    if events & EPOLLOUT != 0 {
        revents |= EPOLLOUT;
    }
    revents
}

/// 销毁 epoll 实例
pub fn epoll_destroy(epfd: u64) {
    let mut instances = EPOLL_INSTANCES.lock();
    instances.retain(|i| i.id != epfd);
    crate::klog_debug!(Sync, "[epoll] Destroyed instance id={}", epfd);
}

// ============================================================================
// 内核测试
// ============================================================================

#[cfg(feature = "kernel_test")]
fn test_epoll_create() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};

    let fd = sys_epoll_create(1);
    check!(fd > 0, "epoll_create returns positive fd");

    let fd2 = sys_epoll_create(0);
    check!(fd2 < 0, "epoll_create(0) returns error");

    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_epoll_ctl_add_del() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};

    let epfd = sys_epoll_create(4);
    check!(epfd > 0, "epoll_create ok");

    let ev = EpollEvent { events: EPOLLIN, data: 42 };
    let ret = sys_epoll_ctl(epfd, EPOLL_CTL_ADD, 3, &ev as *const EpollEvent);
    check!(ret == 0, "epoll_ctl ADD ok");

    // 重复添加应失败
    let ret2 = sys_epoll_ctl(epfd, EPOLL_CTL_ADD, 3, &ev as *const EpollEvent);
    check!(ret2 < 0, "epoll_ctl ADD duplicate fails");

    // 删除
    let ret3 = sys_epoll_ctl(epfd, EPOLL_CTL_DEL, 3, core::ptr::null());
    check!(ret3 == 0, "epoll_ctl DEL ok");

    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
pub fn register_epoll_tests() {
    use crate::kernel::framework::tests::runner;
    let r = runner();
    r.register("epoll", "create", test_epoll_create);
    r.register("epoll", "ctl_add_del", test_epoll_ctl_add_del);
}
