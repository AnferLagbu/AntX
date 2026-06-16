//! madvise / mlock / mincore 系统调用实现 (TCB)
//!
//! ## 编号 (760-765: 内存建议与锁定, P1 #15)
//!
//! - `QX_MADVISE`     (760): 内存访问模式建议
//! - `QX_MLOCK`       (761): 锁定 [addr, addr+len)
//! - `QX_MUNLOCK`     (762): 解除锁定
//! - `QX_MLOCKALL`    (763): 进程级锁定 (MCL_CURRENT/FUTURE/ONFAULT)
//! - `QX_MUNLOCKALL`  (764): 解除进程级所有锁定
//! - `QX_MINCORE`     (765): 每页驻留性查询
//!
//! ## 用户态布局
//!
//! ```c
//! // unsigned char vec[len/page_size]; // mincore
//! ```
//!
//! ## 安全
//!
//! - 用户态指针经 `check_user_buf` 校验
//! - mlock 受 RLIMIT_MEMLOCK 限制
//! - mlockall(MCL_CURRENT) 对 Guard/Device VMA 跳过
//! - 全程在 MmStruct.vmas 锁内进行 VMA 状态修改, 避免竞态

use crate::kernel::framework::mm::api::vma_get_current_mm;

// ============================================================================
// madvise advice → Linux 内核常量
// ============================================================================

/// Linux MADV_* 常量 (与 asm-generic/mman-common.h 一致)
pub const MADV_NORMAL: u32 = 0;
pub const MADV_RANDOM: u32 = 1;
pub const MADV_SEQUENTIAL: u32 = 2;
pub const MADV_WILLNEED: u32 = 3;
pub const MADV_DONTNEED: u32 = 4;
pub const MADV_FREE: u32 = 5;     // Linux 4.5+
pub const MADV_REMOVE: u32 = 9;   // Linux 2.6.16+
pub const MADV_DONTFORK: u32 = 10;
pub const MADV_DOFORK: u32 = 11;
pub const MADV_MERGEABLE: u32 = 12;
pub const MADV_UNMERGEABLE: u32 = 13;
pub const MADV_HUGEPAGE: u32 = 14;
pub const MADV_NOHUGEPAGE: u32 = 15;
pub const MADV_DONTDUMP: u32 = 16; // core dump 跳过
pub const MADV_DODUMP: u32 = 17;
pub const MADV_WIPEONFORK: u32 = 18; // fork 清零
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

// ============================================================================
// madvise
// ============================================================================

/// `sys_madvise(addr, len, advice) -> 0/-errno`  // POSIX 函数签名
///
/// `addr` 必须页对齐; `len` 向上页对齐
pub fn sys_madvise(addr: u64, len: u64, advice: u32) -> i64 {
    use crate::kernel::framework::errno::Errno;

    if addr == 0 && len == 0 {
        return Errno::EINVAL.as_ret();
    }
    if addr & (PAGE_SIZE - 1) != 0 {
        return Errno::EINVAL.as_ret();
    }
    if advice > 23 && advice != MADV_SOFT_OFFLINE {
        return Errno::EINVAL.as_ret();
    }
    // REMOVE 语义: 移除文件映射的页 (Linux 仅 shmem 实际支持)
    if advice == MADV_REMOVE {
        // 暂不支持, 但不阻塞
        return 0;
    }
    // POPULATE_* 需要触达所有页, 当前 LRU 没触达统计, 简化为零次触达成功
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

const PAGE_SIZE: u64 = 4096;

// ============================================================================
// mlock
// ============================================================================

/// `sys_mlock(addr, len) -> 0/-errno`  // POSIX 函数签名
///
/// 锁定 [addr, addr+len) 物理页, 禁止被 swap/reclaim.
pub fn sys_mlock(addr: u64, len: u64) -> i64 {
    use crate::kernel::framework::errno::Errno;

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

/// `sys_munlock(addr, len) -> 0/-errno`  // POSIX 函数签名
pub fn sys_munlock(addr: u64, len: u64) -> i64 {
    use crate::kernel::framework::errno::Errno;

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

/// `sys_mlockall(flags) -> 0/-errno`  // POSIX 函数签名
///
/// flags: MCL_CURRENT=1, MCL_FUTURE=2, MCL_ONFAULT=4 (可位或)
pub fn sys_mlockall(flags: u32) -> i64 {
    use crate::kernel::framework::errno::Errno;

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

/// `sys_munlockall() -> 0/-errno`  // POSIX 函数签名
pub fn sys_munlockall() -> i64 {
    use crate::kernel::framework::errno::Errno;

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

/// `sys_mincore(addr, len, vec_ptr) -> 0/-errno`  // POSIX 函数签名
///
/// `vec_ptr` 至少 len/page_size 字节长, 每字节 1 = 驻留, 0 = 未驻留
pub fn sys_mincore(addr: u64, len: u64, vec_ptr: u64) -> i64 {
    use crate::kernel::framework::errno::Errno;

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

    // 校验输出缓冲可写
    if !crate::kernel::framework::userptr::validate_user_buf(
        vec_ptr,
        buf_bytes as u64,
    ) {
        return Errno::EFAULT.as_ret();
    }

    let mm = match vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::EFAULT.as_ret(),
    };

    // 临时栈缓冲 (n_pages 通常很小, 默认 4KB 内核栈足够)
    // 大于 1024 页 (4MB) 时降级为 EINVAL 防止栈溢出
    const MAX_STACK_PAGES: usize = 1024;
    if n_pages > MAX_STACK_PAGES {
        return Errno::ENOMEM.as_ret();
    }
    let mut stack_buf = [0u8; MAX_STACK_PAGES];

    match mm.mincore_range(addr as usize, len as usize, &mut stack_buf[..n_pages]) {
        Ok(_resident) => {
            // 写回用户态
            // SAFETY: check_user_buf 已校验 vec_ptr [vec_ptr, vec_ptr+buf_bytes) 可写
            unsafe {
                core::ptr::copy_nonoverlapping(
                    stack_buf.as_ptr() as *const u8,
                    vec_ptr as *mut u8,
                    buf_bytes,
                );
            }
            0
        }
        Err(e) => e.as_ret(),
    }
}
