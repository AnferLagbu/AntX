//! mmap/munmap/mprotect 系统调用实现
//!
//! 与 VMA + Demand Paging + Page Cache 集成，实现 POSIX mmap 语义:
//!
//! - mmap: 创建 VMA，延迟物理页分配 (demand paging)
//!   - MAP_ANONYMOUS: 匿名映射 (malloc 等)
//!   - MAP_PRIVATE + fd: 文件私有映射 (COW, 写入不回写)
//!   - MAP_SHARED + fd: 文件共享映射 (写入回写 Page Cache)
//! - munmap: 删除 VMA，释放已映射物理页
//! - mprotect: 修改 VMA 保护属性
//! - brk: 扩展/收缩堆
//!
//! ## 文件映射 #PF 流程
//!
//! ```text
//! #PF(FileBacked VMA)
//!   ├── page_index = (fault_addr - vma.start) / PAGE_SIZE + vma.offset / PAGE_SIZE
//!   ├── pcache_get(inode_id, page_index)
//!   │   ├── Hit → map_page (MAP_SHARED: writable, MAP_PRIVATE: read-only+COW)
//!   │   └── Miss → alloc + fill from file → insert → map_page
//!   └── Return PfResult::Fixed
//! ```

use super::types::*;
use crate::kernel::framework::mm::vma::{MmStruct, Vma, VmaType};
use crate::kernel::framework::mm::{PageFlags as VmaFlags, PAGE_SIZE};

// ============================================================================
// mmap 标志位
// ============================================================================

/// MAP_SHARED: 写入回写文件
const MAP_SHARED: i32 = 0x01;
/// MAP_PRIVATE: 写入触发 COW, 不回写文件
const MAP_PRIVATE: i32 = 0x02;
/// MAP_ANONYMOUS: 匿名映射 (无文件后端)
const MAP_ANONYMOUS: i32 = 0x20;
/// MAP_FIXED: 强制使用指定地址
const MAP_FIXED: i32 = 0x10;

pub const SYS_MMAP_FLAGS: u64 = 0;

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

    // 必须指定 SHARED 或 PRIVATE (不可同时)
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

    // offset 必须页对齐
    if offset % PAGE_SIZE != 0 {
        return Err(Errno::EINVAL);
    }

    // 从 fd 获取 inode_id (通过 VFS)
    let inode_id = fd_to_inode_id(fd);
    if inode_id == 0 {
        return Err(Errno::EBADF);
    }

    let addr = find_or_allocate_addr(mm, addr_hint, len_aligned)?;
    let aligned_addr = addr & !(PAGE_SIZE as usize - 1);

    let final_flags = if map_shared {
        // MAP_SHARED: 可写时标记 WRITABLE, 写入回写 Page Cache
        page_flags | VmaFlags::PRESENT
    } else {
        // MAP_PRIVATE: 初始映射为只读, 写入时 COW
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
    );

    match mm.insert_vma(vma) {
        Ok(()) => {}
        Err(_) => return Err(Errno::ENOMEM),
    }

    // B2: 不预热 pcache. 传统 demand paging 语义: 用户 #PF 时由
    // page_fault::handle_file_fault miss 路径同步从 vfs 读 4KB 填 pcache.
    // 优势: 大文件 mmap 0 开销, 不预先浪费 I/O.

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

/// 从 fd 获取 inode_id
///
/// 通过进程文件描述符表查找对应的 inode 编号.
/// 当前简化实现: fd 直接映射为 inode_id + 1 (避免 0).
/// 后续集成完整 VFS fdtable 后替换.
fn fd_to_inode_id(fd: i32) -> u32 {
    if fd < 0 {
        return 0;
    }
    // TODO(TRACK-077F14): 从当前进程的 fdtable 获取 inode_id
    // 当前简化: fd + 1 作为 inode_id (0 表示无效)
    (fd as u32).wrapping_add(1)
}

#[inline]
pub fn munmap_syscall(mm: &MmStruct, addr: u64, length: u64) -> Result<(), Errno> {
    if addr == 0 || length == 0 {
        return Err(Errno::EINVAL);
    }

    let start = addr as usize;
    let end = start + length as usize;

    // 释放文件映射的 Page Cache 引用
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

        // 计算重叠区域对应的页范围
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prot_to_flags() {
        let r = prot_to_vma_flags(0x01); // PROT_READ
        assert!(r.contains(VmaFlags::PRESENT));
        assert!(!r.contains(VmaFlags::WRITABLE));
        assert!(r.contains(VmaFlags::USER));

        let rw = prot_to_vma_flags(0x03); // PROT_READ | PROT_WRITE
        assert!(rw.contains(VmaFlags::PRESENT));
        assert!(rw.contains(VmaFlags::WRITABLE));

        let rwx = prot_to_vma_flags(0x07); // PROT_READ | PROT_WRITE | PROT_EXEC
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
