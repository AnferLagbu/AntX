#![deny(unsafe_code)]
//! mmap/munmap/mprotect 系统调用实现 — services 层策略主体
//!
//! @SAFE: 本文件不含 unsafe 代码。
//!
//! ## 迁移记录
//!
//! 策略代码于 2026-06-17 从 framework::syscall::mmap 迁移至此。
//! framework 层仅保留 re-export 保持调用方兼容。
//!
//! ## 职责
//!
//! - mmap/munmap/mprotect 策略逻辑 (参数验证、VMA 创建决策)
//! - VFS 交互: fd → inode_id 解析 (属于 services 层职责)
//! - 文件映射 Page Cache 引用释放

use crate::kernel::framework::syscall::Errno;
use crate::kernel::framework::mm::{MmStruct, Vma, VmaType};
use crate::kernel::framework::mm::{PageFlags as VmaFlags, PAGE_SIZE};

// ============================================================================
// mmap 标志位
// ============================================================================

/// MAP_SHARED: 写入回写文件
pub const MAP_SHARED: i32 = 0x01;
/// MAP_PRIVATE: 写入触发 COW, 不回写文件
pub const MAP_PRIVATE: i32 = 0x02;
/// MAP_ANONYMOUS: 匿名映射 (无文件后端)
pub const MAP_ANONYMOUS: i32 = 0x20;
/// MAP_FIXED: 强制使用指定地址
pub const MAP_FIXED: i32 = 0x10;

pub const SYS_MMAP_FLAGS: u64 = 0;

// ============================================================================
// VFS 交互 (services 层职责)
// ============================================================================

/// 从 fd 获取 inode_id
///
/// 通过进程文件描述符表查找对应的 inode 编号.
/// 此函数属于 services 层, 因为它涉及 VFS fdtable 查找.
pub fn fd_to_inode_id(fd: i32) -> u32 {
    if fd < 0 {
        return 0;
    }
    // TODO(TRACK-5B3EBC): 从当前进程的 fdtable 获取 inode_id
    (fd as u32).wrapping_add(1)
}

/// 通过 VFS_MANAGER 把 fd 反查为挂载点索引.
pub fn fd_to_mount_idx(fd: i32) -> Option<usize> {
    if fd < 0 {
        return None;
    }
    crate::kernel::framework::fs::VFS_MANAGER.get_fd_mount_idx(fd as usize)
}

// ============================================================================
// mmap 策略实现
// ============================================================================

#[inline]
pub fn mmap_syscall(
    mm: &MmStruct,
    addr_hint: u64,
    length: u64,
    prot: i32,
    flags: i32,
    fd: i32,
    offset: u64,
    pwm: u64,
) -> Result<usize, Errno> {
    if length == 0 {
        return Err(Errno::EINVAL);
    }

    let len_aligned = ((length as usize) + PAGE_SIZE as usize - 1) & !(PAGE_SIZE as usize - 1);

    let page_flags = prot_to_vma_flags(prot);

    let map_private = (flags & MAP_PRIVATE) != 0;
    let map_shared = (flags & MAP_SHARED) != 0;
    let map_anonymous = (flags & MAP_ANONYMOUS) != 0;

    if map_shared && map_private {
        return Err(Errno::EINVAL);
    }
    if !map_shared && !map_private {
        return Err(Errno::EINVAL);
    }

    // ── 匿名映射 ──
    if map_anonymous {
        let addr = find_or_allocate_addr(mm, addr_hint, len_aligned)?;
        let aligned_addr = addr & !(PAGE_SIZE as usize - 1);

        let final_flags = page_flags | VmaFlags::PRESENT;
        let vma = Vma::new(
            aligned_addr,
            aligned_addr + len_aligned,
            final_flags,
            VmaType::Anonymous,
        );
        match mm.insert_vma(vma) {
            Ok(()) => {}
            Err(_) => return Err(Errno::ENOMEM),
        }

        return Ok(aligned_addr);
    }

    // ── 文件映射 ──
    if fd < 0 {
        return Err(Errno::EBADF);
    }

    if !offset.is_multiple_of(PAGE_SIZE) {
        return Err(Errno::EINVAL);
    }

    let inode_id = fd_to_inode_id(fd);
    if inode_id == 0 {
        return Err(Errno::EBADF);
    }

    let mount_idx = fd_to_mount_idx(fd);

    let addr = find_or_allocate_addr(mm, addr_hint, len_aligned)?;
    let aligned_addr = addr & !(PAGE_SIZE as usize - 1);

    let final_flags = if map_shared {
        page_flags | VmaFlags::PRESENT
    } else {
        (page_flags | VmaFlags::PRESENT) & !VmaFlags::WRITABLE
    };

    let vma = Vma::file_backed(
        aligned_addr,
        aligned_addr + len_aligned,
        final_flags,
        offset,
        inode_id,
        pwm,
        map_shared,
        mount_idx,
    );

    match mm.insert_vma(vma) {
        Ok(()) => {}
        Err(_) => return Err(Errno::ENOMEM),
    }

    Ok(aligned_addr)
}

/// 查找或分配映射地址
fn find_or_allocate_addr(mm: &MmStruct, addr_hint: u64, len_aligned: usize) -> Result<usize, Errno> {
    if addr_hint != 0 && addr_hint < 0x0000_7FFF_FFFF_F000 {
        Ok(addr_hint as usize)
    } else {
        match mm.find_free_range(len_aligned) {
            Some(a) => Ok(a),
            None => Err(Errno::ENOMEM),
        }
    }
}

// ============================================================================
// munmap 策略实现
// ============================================================================

#[inline]
pub fn munmap_syscall(mm: &MmStruct, addr: u64, length: u64) -> Result<(), Errno> {
    if addr == 0 || length == 0 {
        return Err(Errno::EINVAL);
    }

    let start = addr as usize;
    let end = start + length as usize;

    release_file_pages(mm, start, end);

    mm.remove_range(start, end);
    Ok(())
}

/// 释放文件映射区域的 Page Cache 引用
fn release_file_pages(mm: &MmStruct, start: usize, end: usize) {
    let vmas = mm.vmas.lock();
    for vma in vmas.iter() {
        if vma.vma_type != VmaType::FileBacked || vma.inode_id == 0 {
            continue;
        }
        if vma.start >= end || vma.end <= start {
            continue;
        }

        let overlap_start = vma.start.max(start);
        let overlap_end = vma.end.min(end);

        let mut addr = overlap_start;
        while addr < overlap_end {
            let page_index = ((addr - vma.start) as u64 + vma.offset) / PAGE_SIZE;
            crate::kernel::framework::mm::pcache::pcache_put(vma.inode_id, page_index);
            addr += PAGE_SIZE as usize;
        }
    }
}

// ============================================================================
// mprotect 策略实现
// ============================================================================

#[inline]
pub fn mprotect_syscall(mm: &MmStruct, addr: u64, length: u64, prot: i32) -> Result<(), Errno> {
    if addr == 0 || length == 0 {
        return Err(Errno::EINVAL);
    }

    let start = addr as usize;
    let end = start + length as usize;
    let new_flags = prot_to_vma_flags(prot);

    let mut vmas = mm.vmas.lock();
    for vma in vmas.iter_mut() {
        if vma.start < end && vma.end > start {
            vma.flags = (vma.flags & VmaFlags::empty()) | new_flags;
        }
    }

    Ok(())
}

// ============================================================================
// 辅助函数
// ============================================================================

fn prot_to_vma_flags(prot: i32) -> VmaFlags {
    let mut flags = VmaFlags::USER;

    if prot & 0x01 != 0 {
        flags |= VmaFlags::PRESENT;
    }
    if prot & 0x02 != 0 {
        flags |= VmaFlags::WRITABLE;
    }
    if prot & 0x04 == 0 {
        flags |= VmaFlags::NX;
    }

    flags
}

// ============================================================================
// syscall 入口 — 从 framework::syscall::sys_mmap/sys_munmap 迁移的策略层
// ============================================================================

/// mmap syscall 策略入口
pub fn mmap_syscall_entry(addr: u64, size: u64, prot: i32, flags: i32, fd: i32, offset: u64) -> i64 {
    if size == 0 {
        return Errno::EINVAL.as_ret();
    }
    let pwm = crate::kernel::framework::credo::pwm_get_current();
    if !crate::kernel::framework::credo::pwm_has_capability(pwm, 7, 0x01) {
        return Errno::EACCES.as_ret();
    }

    // 无 mm 时走裸页分配路径
    if let Some(ptr) = crate::kernel::framework::syscall::api::mmap_get_mm_or_alloc(size) {
        return ptr as i64;
    }

    // 有 mm 时走 VMA 路径
    let mm = match crate::kernel::framework::mm::vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::ENOMEM.as_ret(),
    };

    match mmap_syscall(mm, addr, size, prot, flags, fd, offset, pwm) {
        Ok(a) => a as i64,
        Err(e) => e.as_ret(),
    }
}

/// munmap syscall 策略入口
pub fn munmap_syscall_entry(addr: u64, size: u64) -> i64 {
    if addr == 0 || size == 0 {
        return Errno::EINVAL.as_ret();
    }

    // 无 mm 时走裸页释放路径
    if crate::kernel::framework::mm::vma_get_current_mm().is_none() {
        crate::kernel::framework::syscall::api::munmap_free_pages(addr, size);
        return 0;
    }

    let mm = match crate::kernel::framework::mm::vma_get_current_mm() {
        Some(m) => m,
        None => return Errno::ENOMEM.as_ret(),
    };

    match munmap_syscall(mm, addr, size) {
        Ok(()) => 0,
        Err(e) => e.as_ret(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prot_to_flags() {
        let r = prot_to_vma_flags(0x01);
        assert!(r.contains(VmaFlags::PRESENT));
        assert!(!r.contains(VmaFlags::WRITABLE));
        assert!(r.contains(VmaFlags::USER));

        let rw = prot_to_vma_flags(0x03);
        assert!(rw.contains(VmaFlags::PRESENT));
        assert!(rw.contains(VmaFlags::WRITABLE));

        let rwx = prot_to_vma_flags(0x07);
        assert!(rwx.contains(VmaFlags::PRESENT));
        assert!(rwx.contains(VmaFlags::WRITABLE));
        assert!(!rwx.contains(VmaFlags::NX));
    }

    #[test]
    fn test_mmap_flags_constants() {
        assert_eq!(MAP_SHARED, 0x01);
        assert_eq!(MAP_PRIVATE, 0x02);
        assert_eq!(MAP_ANONYMOUS, 0x20);
    }
}
