/// 网络子系统类型定义
/// 
/// 提供 lwIP OS 抽象层所需的类型别名和常量。
/// 同时包含公共的 FFI 声明（日志函数等）。

use core::sync::atomic::{AtomicU32, Ordering};

// ============================================================================
// 公共 FFI 声明 - 日志输出函数 (供所有子模块使用)
// ============================================================================

extern "C" {
    /// 日志输出: 网络信息
    #[link_name = "klog_net"]
    pub fn klog_net(fmt: *const i8, ...);
    
    /// 日志输出: 网络错误
    #[link_name = "klog_net_err"]
    pub fn klog_net_err(fmt: *const i8, ...);
    
    /// 日志输出: 初始化消息
    #[link_name = "klog_init_msg"]
    pub fn klog_init_msg(fmt: *const i8, ...);
}

// ============================================================================
// lwIP 错误码
// ============================================================================

/// lwIP 错误码 (与 C 版本兼容)
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LwipErr {
    /// 无错误
    Ok = 0,
    /// 内存溢出
    Mem = -1,
    /// 缓冲区错误
    Buf = -2,
    /// 超时
    Timeout = -3,
    /// 路由不可达
    Rte = -4,
    /// 操作进行中
    Inprogress = -5,
    /// 值非法
    Val = -6,
    /// 操作将阻塞
    Wouldblock = -7,
    /// 地址已使用
    Addrinuse = -8,
    /// 连接已关闭
    Already = -9,
    /// 连接中
    Isconn = -10,
    /// 未连接
    Notconn = -11,
    /// 连接重置
    Aborted = -12,
    /// 连接已关闭
    Connrst = -13,
    /// 不支持的协议/操作
    Nobufs = -14,
    /// 数据包被丢弃
    Udp = -15,
    /// TCP 层错误
    Tcp = -16,
    /// DNS 错误
    Dns = -17,
    /// 其他错误
    If = -18,
}

impl Default for LwipErr {
    fn default() -> Self {
        Self::Ok
    }
}

impl From<i32> for LwipErr {
    fn from(code: i32) -> Self {
        match code {
            0 => Self::Ok,
            -1 => Self::Mem,
            -2 => Self::Buf,
            -3 => Self::Timeout,
            _ => Self::If, // 通用错误
        }
    }
}

// ============================================================================
// 时间管理
// ============================================================================

/// 全局 tick 计数器 (100Hz)
static SYS_TICKS: AtomicU32 = AtomicU32::new(0);

/// 系统初始化 - 同时供 Rust 和 FFI 使用
#[no_mangle]
pub extern "C" fn sys_init() {
    SYS_TICKS.store(0, Ordering::Relaxed);
    unsafe { klog_net("sys_arch ready\0".as_ptr() as *const i8); }
}

/// 获取当前时间 (毫秒) - 同时供 Rust 和 FFI 使用
#[no_mangle]
pub extern "C" fn sys_now() -> u32 {
    SYS_TICKS.load(Ordering::Relaxed) * 10 // 100Hz → ms
}

/// 增加 tick 计数 (由 timer ISR 调用) - 同时供 Rust 和 FFI 使用
#[no_mangle]
pub extern "C" fn sys_tick_inc() {
    SYS_TICKS.fetch_add(1, Ordering::Relaxed);
}

// ============================================================================
// 临界区保护
// ============================================================================

/// 临界区保护标志
#[derive(Clone, Copy, Debug)]
pub struct SysProt(pub u64);

/// 进入临界区
#[no_mangle]
pub extern "C" fn sys_arch_protect() -> SysProt {
    let flags: u64;
    
    unsafe {
        core::arch::asm!(
            "pushfq",
            "cli",
            "pop {}",
            out(reg) flags,
            options(nostack, nomem)
        );
    }
    
    SysProt(flags)
}

/// 退出临界区
#[no_mangle]
pub extern "C" fn sys_arch_unprotect(pval: SysProt) {
    unsafe {
        core::arch::asm!(
            "push {}",
            "popfq",
            in(reg) pval.0,
            options(nostack, nomem)
        );
    }
}

// ============================================================================
// 日志输出 (FFI 桩函数)
// ============================================================================

/// 网络日志输出 (简化版，不支持格式化参数)
#[no_mangle]
pub unsafe extern "C" fn rust_klog_net(fmt: *const i8) {
    // TODO: 实现完整的日志输出
    // 目前仅作为桩函数
    let _ = fmt;
}
