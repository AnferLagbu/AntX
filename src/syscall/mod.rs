/// Syscall 模块 - 系统调用接口定义
/// 
/// 提供系统调用的类型定义和 FFI 接口。
/// 完整实现在 handlers.rs 中。

pub mod types;
// 暂时禁用完整实现以避免编译错误
// pub mod handlers;
// pub mod ffi;

use crate::syscall::types::*;

/// 全局系统调用表
static mut SYSCALL_TABLE: [Option<SyscallHandler>; 128] = [None; 128];

/// 初始化系统调用表
#[no_mangle]
pub unsafe extern "C" fn syscall_init() {
    for i in 0..MAX_SYSCALLS {
        SYSCALL_TABLE[i as usize] = None;
    }
    // TODO: 在此处注册所有 syscall handler
}

/// 分发系统调用
#[no_mangle]
pub unsafe extern "C" fn syscall_dispatch(num: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    if num >= MAX_SYSCALLS {
        return SyscallError::E_INVAL.as_i64();
    }
    
    match SYSCALL_TABLE[num as usize] {
        Some(handler) => handler(arg0, arg1, arg2, arg3),
        None => SyscallError::E_NOSYS.as_i64(),
    }
}

/// 注册系统调用
#[no_mangle]
pub unsafe extern "C" fn syscall_register(num: u64, handler: SyscallHandler) {
    if (num as usize) < SYSCALL_TABLE.len() {
        SYSCALL_TABLE[num as usize] = Some(handler);
    }
}

// ============================================================================
// 系统调用 FFI 桩函数 (供测试代码使用)
// ============================================================================

/// 打开文件
#[no_mangle]
pub unsafe extern "C" fn sys_fs_open(path: *const i8, flags: i32, _mode: i32) -> i64 {
    // TODO: 调用 VFS 实现
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    1 // 返回 fd=1 (stdout) 作为桩实现
}

/// 关闭文件
#[no_mangle]
pub unsafe extern "C" fn sys_fs_close(fd: i32) -> i64 {
    // TODO: 调用 VFS 实现
    if fd < 0 { return SyscallError::E_BADFD.as_i64(); }
    0
}

/// 读取文件
#[no_mangle]
pub unsafe extern "C" fn sys_fs_read(fd: i32, buf: *mut u8, count: u64) -> i64 {
    // TODO: 调用 VFS 实现
    if fd == 1 || fd == 2 { return SyscallError::E_BADFD.as_i64(); } // 不能读 stdout/stderr
    if buf.is_null() || count == 0 { return -1; }
    0 // 返回读取 0 字节
}

/// 写入文件
#[no_mangle]
pub unsafe extern "C" fn sys_fs_write(fd: i32, buf: *const u8, count: u64) -> i64 {
    // TODO: 调用 VFS 实现
    if buf.is_null() || count == 0 { return -1; }
    
    // stdout (fd=1) 或 stderr (fd=2): 输出到串口
    if fd == 1 || fd == 2 {
        // 简化版：直接返回写入字节数
        return count as i64;
    }
    
    0
}

/// 创建目录
#[no_mangle]
pub unsafe extern "C" fn sys_fs_mkdir(path: *const i8, _mode: i32) -> i64 {
    // TODO: 调用 VFS 实现
    if path.is_null() { return SyscallError::E_INVAL.as_i64(); }
    0
}

/// 获取进程 ID
#[no_mangle]
pub unsafe extern "C" fn sys_proc_getid() -> i64 {
    // TODO: 从当前进程 PCB 获取 PID
    1 // 返回 PID=1 作为桩实现
}

/// 让出 CPU
#[no_mangle]
pub unsafe extern "C" fn sys_proc_yield() -> i64 {
    // TODO: 调用调度器 yield
    0
}
