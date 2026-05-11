//! 内核日志系统 (KLog)
//!
//! 提供多级别、分类的日志输出能力

/// 日志级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Note = 2,
    Warn = 3,
    Error = 4,
    Crit = 5,
}

/// 日志分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogCategory {
    Boot = 0,
    Kernel = 1,
    Memory = 2,
    Process = 3,
    FS = 4,
    Net = 5,
    Driver = 6,
}

/// 写入日志条目 (FFI 兼容)
///
/// # Arguments
/// * `level` - 日志级别
/// * `cat` - 分类
/// * `file` - 源文件名 (可为 null)
/// * `func` - 函数名 (可为 null)
/// * `line` - 行号
/// * `fmt` - 格式化字符串 (null 终止)
///
/// # Returns
/// 成功返回 0，失败返回 -1
#[no_mangle]
pub unsafe extern "C" fn klog_write(
    level: u8,
    _cat: u8,
    _file: *const i8,
    _func: *const i8,
    _line: u32,
    fmt: *const i8,
) -> i32 {
    // TODO: 完整实现日志系统（串口输出 + 环形缓冲区）
    // 当前：简单的空实现（避免编译错误）

    if fmt.is_null() {
        return -1;
    }

    // 检查日志级别是否启用
    match level {
        0..=5 => {},  // 所有级别暂时都接受
        _ => return -1,
    }

    // TODO: 实际的日志输出逻辑
    // 1. 格式化消息
    // 2. 写入串口 (COM1)
    // 3. 写入环形缓冲区

    0  // 成功
}

/// FFI: 便捷信息日志 (被多个内核模块通过 extern "C" 调用)
#[no_mangle]
pub unsafe extern "C" fn klog_ffi_info(_msg: *const u8) {}

/// FFI: 便捷警告日志
#[no_mangle]
pub unsafe extern "C" fn klog_ffi_warn(_msg: *const u8) {}

/// FFI: 网络日志
#[no_mangle]
pub unsafe extern "C" fn klog_net(_msg: *const i8) {}

/// FFI: 内核日志 (可变参数, 仅作桩)
#[no_mangle]
pub unsafe extern "C" fn klog_kern(_fmt: *const i8) {}

/// FFI: 系统调用日志
#[no_mangle]
pub unsafe extern "C" fn klog_syscall(_fmt: *const i8) {}

/// FFI: 通用信息日志
#[no_mangle]
pub unsafe extern "C" fn klog_info(_fmt: *const i8) {}

/// FFI: 网络错误日志
#[no_mangle]
pub unsafe extern "C" fn klog_net_err(_msg: *const i8) {}

/// FFI: 日志系统初始化 (C 兼容名)
#[no_mangle]
pub unsafe extern "C" fn klog_init_msg(_msg: *const i8) {}
