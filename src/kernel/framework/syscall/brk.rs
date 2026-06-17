//! brk — framework 层实现 (TCB)
//!
//! 堆内存扩展/收缩系统调用.
//! 涉及 VMA 操作和页表更新, 属于 TCB.

use core::sync::atomic::AtomicU64;

use crate::kernel::framework::syscall::Errno;
use crate::kernel::framework::mm::api;

/// 用户空间最大地址
#[cfg(target_arch = "x86_64")]
const USER_ADDR_MAX: u64 = 0x7FFF_FFFF_FFFF;

#[cfg(target_arch = "aarch64")]
const USER_ADDR_MAX: u64 = 0x0000_FFFF_FFFF_FFFF;

/// 全局静态 brk 回退 (无 MmStruct 时使用)
static BRK: AtomicU64 = AtomicU64::new(0x400000 + 65536);

/// brk 系统调用实现
///
/// # Safety
///
/// - 访问当前进程的 MmStruct (TCB 操作)
/// - 可能分配物理页 (raw::alloc_pages)
pub fn sys_brk(addr: u64) -> i64 {
    if addr == 0 {
        // 返回当前 brk (VMA 优先)
        if let Some(mm) = api::vma_get_current_mm() {
            return mm.brk.load(core::sync::atomic::Ordering::Acquire) as i64;
        }
        return BRK.load(core::sync::atomic::Ordering::SeqCst) as i64;
    }

    if addr > USER_ADDR_MAX {
        return Errno::ENOMEM.as_ret();
    }

    // VMA 路径: 通过 MmStruct 扩展/收缩堆
    if let Some(mm) = api::vma_get_current_mm() {
        match mm.set_brk(addr as usize) {
            Ok(new_brk) => return new_brk as i64,
            Err(_) => return Errno::ENOMEM.as_ret(),
        }
    }

    // 回退: 全局静态 brk (无 MmStruct 时使用)
    let current = BRK.load(core::sync::atomic::Ordering::SeqCst);
    if addr > current {
        let extra = addr - current;
        let pages = extra.div_ceil(4096);
        let ptr = crate::kernel::framework::syscall::raw::alloc_pages(pages);
        if ptr.is_null() {
            return Errno::ENOMEM.as_ret();
        }
    }
    BRK.store(addr, core::sync::atomic::Ordering::SeqCst);
    addr as i64
}
