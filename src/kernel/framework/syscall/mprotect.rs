//! mprotect — 内存区域保护系统调用 (TCB)
//!
//! 修改进程地址空间中 [addr, addr+len) 范围的页保护属性.
//! 对应 POSIX `mprotect(addr, len, prot)`.
//!
//! ## prot 参数
//!
//! - PROT_READ  = 0x1
//! - PROT_WRITE = 0x2
//! - PROT_EXEC  = 0x4
//! - PROT_NONE  = 0x0
//!
//! ## 安全
//!
//! - 必须通过 services 层验证参数后再调用
//! - 操作页表需要 VMM 锁 + TLB flush

use crate::kernel::framework::mm::{PageFlags, vma};
use crate::kernel::framework::syscall::types::Errno;

/// PROT 常量
pub const PROT_NONE: i32 = 0x0;
pub const PROT_READ: i32 = 0x1;
pub const PROT_WRITE: i32 = 0x2;
pub const PROT_EXEC: i32 = 0x4;

/// 将 POSIX prot 转换为 PageFlags
pub fn prot_to_page_flags(prot: i32) -> PageFlags {
    let mut flags = PageFlags::empty();

    if prot & PROT_READ != 0 || prot & PROT_WRITE != 0 || prot & PROT_EXEC != 0 {
        flags.insert(PageFlags::PRESENT);
    }

    if prot & PROT_WRITE != 0 {
        flags.insert(PageFlags::WRITABLE);
    }

    // 用户空间映射总是设置 USER 位
    if prot != PROT_NONE {
        flags.insert(PageFlags::USER);
    }

    // 如果没有 PROT_EXEC, 设置 NX (No Execute)
    if prot & PROT_EXEC == 0 && prot != PROT_NONE {
        flags.insert(PageFlags::NX);
    }

    flags
}

/// mprotect 系统调用实现
///
/// 修改 [addr, addr+len) 范围的页保护属性.
pub fn sys_mprotect(addr: u64, len: u64, prot: i32) -> i64 {
    // 验证参数
    if addr & 0xFFF != 0 {
        return Errno::EINVAL.as_ret(); // addr 必须页对齐
    }
    if len == 0 {
        return Errno::EINVAL.as_ret();
    }

    // 验证 prot
    let valid_prot = PROT_NONE | PROT_READ | PROT_WRITE | PROT_EXEC;
    if prot & !valid_prot != 0 {
        return Errno::EINVAL.as_ret();
    }

    // 转换 prot → PageFlags
    let new_flags = prot_to_page_flags(prot);

    // 通过 VMA 操作修改权限
    if let Some(mm) = vma::get_current_mm() {
        match mm.mprotect(addr as usize, len as usize, new_flags) {
            Ok(()) => 0,
            Err(e) => e.as_ret(),
        }
    } else {
        Errno::ENOMEM.as_ret()
    }
}
