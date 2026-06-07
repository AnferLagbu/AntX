#![deny(unsafe_code)]
//! mprotect — services 层安全代理
//!
//! 为 mprotect 系统调用提供参数验证:
//! - addr 必须页对齐
//! - len > 0
//! - prot 仅包含合法标志 (PROT_READ/WRITE/EXEC/NONE)
//!
//! ## 安全边界
//!
//! - services 层验证标量参数
//! - 页表修改委托给 framework 层 (TCB 操作)

use crate::kernel::framework::syscall::types::Errno;

/// mprotect 安全代理
///
/// 验证: addr 页对齐, len > 0, prot 合法
pub fn mprotect_syscall(addr: u64, len: u64, prot: i32) -> Result<usize, Errno> {
    // 验证 addr 页对齐
    if addr & 0xFFF != 0 {
        return Err(Errno::EINVAL);
    }
    // 验证 len > 0
    if len == 0 {
        return Err(Errno::EINVAL);
    }
    // 验证 prot 合法
    const PROT_NONE: i32 = 0x0;
    const PROT_READ: i32 = 0x1;
    const PROT_WRITE: i32 = 0x2;
    const PROT_EXEC: i32 = 0x4;
    let valid_prot = PROT_NONE | PROT_READ | PROT_WRITE | PROT_EXEC;
    if prot & !valid_prot != 0 {
        return Err(Errno::EINVAL);
    }

    let ret = crate::kernel::framework::syscall::mprotect::sys_mprotect(addr, len, prot);
    if ret < 0 { Err(Errno::from_ret(ret)) } else { Ok(ret as usize) }
}
