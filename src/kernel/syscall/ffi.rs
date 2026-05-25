/// Syscall FFI 接口层
/// 
/// 提供 C 兼容的系统调用接口，允许 C 代码调用 Rust 实现的系统调用功能。
/// 同时也提供从 Rust 调用 C 实现的兼容性接口。

use crate::kernel::syscall::types::*;

// ==================== 从 C 导入的函数 ====================

extern "C" {
    pub fn klog_kern(fmt: *const core::ffi::c_char);
    pub fn klog_syscall(fmt: *const core::ffi::c_char);
    pub fn klog_info(fmt: *const core::ffi::c_char);
    pub fn strcmp(s1: *const core::ffi::c_char, s2: *const core::ffi::c_char) -> i32;
}

// ==================== 导出到 C 的接口 ====================

/// 初始化系统调用子系统 (C 入口点)
#[no_mangle]
pub extern "C" fn rust_syscall_init() {
    unsafe { super::syscall_init(); }
}

/// 执行系统调用 (C 入口点)
#[no_mangle]
pub unsafe extern "C" fn rust_syscall_dispatch(num: u64, arg0: u64, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    super::syscall_dispatch(num, arg0, arg1, arg2, arg3)
}

/// 注册自定义系统调用 (用于动态扩展)
#[no_mangle]
#[allow(deprecated)]
pub unsafe extern "C" fn rust_syscall_register(num: u64, handler: SyscallHandler) -> i64 {
    if num >= MAX_SYSCALLS {
        return SyscallError::E_INVAL.as_i64();
    }
    super::syscall_register(num, handler);
    0
}

// ==================== 辅助函数 ====================

/// 将错误码转换为可读字符串 (用于调试)
#[no_mangle]
pub extern "C" fn syscall_error_to_string(code: i64) -> *const core::ffi::c_char {
    static ERROR_MESSAGES: [&str; 39] = [
        "Operation not permitted",           // E_PERM (-1)
        "No such file or directory",         // E_NOTFOUND (-2)
        "Function not implemented",          // E_NOSYS (-3)
        "Interrupted system call",           // E_INTR (-4)
        "I/O error",                         // E_IO (-5)
        "",                                  // (-6)
        "",                                  // (-7)
        "Exec format error",                 // E_NOEXEC (-8)
        "Bad file descriptor",               // E_BADFD (-9)
        "No child processes",                // E_CHILD (-10)
        "Resource temporarily unavailable",  // E_AGAIN (-11)
        "Cannot allocate memory",            // E_NOMEM (-12)
        "Permission denied",                 // E_ACCES (-13)
        "Bad address",                       // E_FAULT (-14)
        "",                                  // (-15)
        "Device or resource busy",           // E_BUSY (-16)
        "File exists",                       // E_EXIST (-17)
        "",                                  // (-18)
        "",                                  // (-19)
        "Not a directory",                   // E_NOTDIR (-20)
        "Is a directory",                    // E_ISDIR (-21)
        "Invalid argument",                  // E_INVAL (-22)
        "",                                  // (-23)
        "",                                  // (-24)
        "",                                  // (-25)
        "",                                  // (-26)
        "",                                  // (-27)
        "No space left on device",           // E_NOSPC (-28)
        "",                                  // (-29)
        "Read-only file system",             // E_ROFS (-30)
        "",                                  // (-31)
        "",                                  // (-32)
        "",                                  // (-33)
        "Result too large",                  // E_RANGE (-34)
        "",                                  // (-35)
        "File name too long",                // E_NAMETOOLONG (-36)
        "",                                  // (-37)
        "",                                  // (-38)
        "Directory not empty",               // E_NOTEMPTY (-39)
    ];
    
    // 计算索引 (负数转正数)
    let idx = ((-code) as usize).saturating_sub(1);
    
    if idx < ERROR_MESSAGES.len() {
        ERROR_MESSAGES[idx].as_ptr() as *const core::ffi::c_char
    } else {
        "Unknown error\0".as_ptr() as *const core::ffi::c_char
    }
}

/// 验证系统调用号是否有效
#[no_mangle]
pub extern "C" fn rust_syscall_is_valid(num: u64) -> bool {
    num < MAX_SYSCALLS
}

/// 获取已注册的系统调用数量
#[no_mangle]
pub unsafe extern "C" fn rust_syscall_count() -> u64 {
    MAX_SYSCALLS as u64
}

// ==================== 测试辅助函数 ====================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_code_conversion() {
        assert_eq!(SyscallError::E_PERM.as_i64(), -1);
        assert_eq!(SyscallError::E_NOMEM.as_i64(), -12);
        assert_eq!(SyscallError::E_INVAL.as_i64(), -22);
    }
    
    #[test]
    fn test_error_from_i64() {
        assert_eq!(SyscallError::from_i64(-1), Some(SyscallError::E_PERM));
        assert_eq!(SyscallError::from_i64(-22), Some(SyscallError::E_INVAL));
        assert_eq!(SyscallError::from_i64(-999), None);
    }
    
    #[test]
    fn test_error_display() {
        assert_eq!(format!("{}", SyscallError::E_PERM), "Operation not permitted");
        assert_eq!(format!("{}", SyscallError::E_NOSYS), "Function not implemented");
    }
}

#[cfg(feature = "kernel_test")]
pub fn register_syscall_ffi_tests() {
    crate::kernel::tests::sys::register_syscall_ffi_tests();
}
