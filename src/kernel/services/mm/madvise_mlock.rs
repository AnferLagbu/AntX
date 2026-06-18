#![deny(unsafe_code)]
//! madvise / mlock / mincore 系统调用实现 — services 层策略主体
//!
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! ## 迁移记录
//!
//! 策略代码于 2026-06-17 从 framework::proc::madvise_mlock 迁移至此。
//! framework 层仅保留 re-export 保持调用方兼容。
//!
//! ## 职责
//!
//! - madvise: 内存访问模式建议 (参数验证 + 委托 framework)
//! - mlock/munlock: 物理页锁定/解锁 (参数验证 + 委托 framework)
//! - mlockall/munlockall: 进程级锁定 (委托 framework)
//! - mincore: 每页驻留性查询 (参数验证 + copy_to_user)

use crate::kernel::framework::mm::vma_get_current_mm;
use crate::kernel::framework::mm::copy_user::copy_to_user;
use crate::kernel::framework::userptr;
use crate::kernel::framework::errno::Errno;

// ============================================================================
// madvise advice 常量
// ============================================================================

pub const MADV_NORMAL: u32 = 0;
pub const MADV_RANDOM: u32 = 1;
pub const MADV_SEQUENTIAL: u32 = 2;
pub const MADV_WILLNEED: u32 = 3;
pub const MADV_DONTNEED: u32 = 4;
pub const MADV_FREE: u32 = 5;
pub const MADV_REMOVE: u32 = 9;
pub const MADV_DONTFORK: u32 = 10;
pub const MADV_DOFORK: u32 = 11;
pub const MADV_MERGEABLE: u32 = 12;
pub const MADV_UNMERGEABLE: u32 = 13;
pub const MADV_HUGEPAGE: u32 = 14;
pub const MADV_NOHUGEPAGE: u32 = 15;
pub const MADV_DONTDUMP: u32 = 16;
pub const MADV_DODUMP: u32 = 17;
pub const MADV_WIPEONFORK: u32 = 18;
pub const MADV_KEEPONFORK: u32 = 19;
pub const MADV_SOFT_OFFLINE: u32 = 101;
pub const MADV_COLD: u32 = 20;
pub const MADV_PAGEOUT: u32 = 21;
pub const MADV_POPULATE_READ: u32 = 22;
pub const MADV_POPULATE_WRITE: u32 = 23;

// ============================================================================
// mlockall 标志
// ============================================================================

pub const MCL_CURRENT: u32 = 1;
pub const MCL_FUTURE: u32 = 2;
pub const MCL_ONFAULT: u32 = 4;

const PAGE_SIZE: u64 = 4096;

// ============================================================================
// madvise
// ============================================================================

/// `sys_madvise(addr, len, advice) -> 0/-errno`
pub fn sys_madvise(addr: u64, len: u64, advice: u64) -> i64 {
    let advice = advice as u32;
    if addr == 0 && len == 0 {
        return Errno::EINVAL.as_ret();
    }
    if addr & (PAGE_SIZE - 1) != 0 {
        return Errno::EINVAL.as_ret();
    }
    if advice > 23 && advice != MADV_SOFT_OFFLINE {
        return Errno::EINVAL.as_ret();
    }
    if advice == MADV_REMOVE {
        return 0;
    }
    if advice == MADV_POPULATE_READ || advice == MADV_POPULATE_WRITE {
        return 0;
    }

    let mm = match vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::EFAULT.as_ret(),
    };

    let len_usize = match (len as usize).checked_add(0usize) {
        Some(v) => v,
        None => return Errno::ENOMEM.as_ret(),
    };
    match mm.madvise_range(addr as usize, len_usize, advice) {
        Ok(_) => 0,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// mlock
// ============================================================================

/// `sys_mlock(addr, len) -> 0/-errno`
pub fn sys_mlock(addr: u64, len: u64) -> i64 {
    if addr & (PAGE_SIZE - 1) != 0 {
        return Errno::EINVAL.as_ret();
    }
    if len == 0 {
        return 0;
    }
    let len_usize = len as usize;

    let mm = match vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::EFAULT.as_ret(),
    };

    match mm.mlock_range(addr as usize, len_usize) {
        Ok(_) => 0,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// munlock
// ============================================================================

/// `sys_munlock(addr, len) -> 0/-errno`
pub fn sys_munlock(addr: u64, len: u64) -> i64 {
    if addr & (PAGE_SIZE - 1) != 0 {
        return Errno::EINVAL.as_ret();
    }
    if len == 0 {
        return 0;
    }
    let len_usize = len as usize;

    let mm = match vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::EFAULT.as_ret(),
    };

    match mm.munlock_range(addr as usize, len_usize) {
        Ok(_) => 0,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// mlockall
// ============================================================================

/// `sys_mlockall(flags) -> 0/-errno`
pub fn sys_mlockall(flags: u64) -> i64 {
    let flags = flags as u32;
    let mm = match vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::EFAULT.as_ret(),
    };

    match mm.mlock_all(flags) {
        Ok(_) => 0,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// munlockall
// ============================================================================

/// `sys_munlockall() -> 0/-errno`
pub fn sys_munlockall() -> i64 {
    let mm = match vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::EFAULT.as_ret(),
    };

    match mm.munlock_all() {
        Ok(()) => 0,
        Err(e) => e.as_ret(),
    }
}

// ============================================================================
// mincore
// ============================================================================

/// `sys_mincore(addr, len, vec_ptr) -> 0/-errno`
pub fn sys_mincore(addr: u64, len: u64, vec_ptr: u64) -> i64 {
    if addr & (PAGE_SIZE - 1) != 0 {
        return Errno::EINVAL.as_ret();
    }
    if len == 0 {
        return 0;
    }
    if vec_ptr == 0 {
        return Errno::EFAULT.as_ret();
    }

    let page_size = PAGE_SIZE as usize;
    let n_pages = ((len as usize) + page_size - 1) / page_size;
    let buf_bytes = n_pages;

    if !userptr::validate_user_buf(vec_ptr, buf_bytes as u64) {
        return Errno::EFAULT.as_ret();
    }

    let mm = match vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::EFAULT.as_ret(),
    };

    const MAX_STACK_PAGES: usize = 1024;
    if n_pages > MAX_STACK_PAGES {
        return Errno::ENOMEM.as_ret();
    }
    let mut stack_buf = [0u8; MAX_STACK_PAGES];

    match mm.mincore_range(addr as usize, len as usize, &mut stack_buf[..n_pages]) {
        Ok(_resident) => {
            // 使用 framework 的 safe copy_to_user 替代 unsafe copy_nonoverlapping
            match copy_to_user(vec_ptr, &stack_buf[..buf_bytes], buf_bytes) {
                Ok(_) => 0,
                Err(_) => Errno::EFAULT.as_ret(),
            }
        }
        Err(e) => e.as_ret(),
    }
}
