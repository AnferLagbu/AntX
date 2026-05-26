//! mmap/munmap/mprotect 系统调用实现
//!
//! 与 VMA + Demand Paging 集成，实现 POSIX mmap 语义:
//!
//! - mmap: 创建 VMA，延迟物理页分配 (demand paging)
//! - munmap: 删除 VMA，释放已映射物理页
//! - mprotect: 修改 VMA 保护属性
//! - brk: 扩展/收缩堆
//!
//! ## 与旧版区别
//!
//! | 特性 | 旧版 (直接分配) | 新版 (VMA+Demand) |
//! |------|-----------------|-------------------|
//! | 物理页分配 | mmap 时立即分配 | #PF 时懒惰分配 |
//! | 地址空间管理 | 无跟踪 | VMA 红黑树/链表 |
//! | munmap | 仅释放物理页 | VMA 删除 + TLB flush |
//! | COW | 不支持 | 支持 |

use super::types::*;
use crate::kernel::mm::vma::{Vma, VmaType, MmStruct};
use crate::kernel::mm::{PageFlags as VmaFlags, PAGE_SIZE};

pub const SYS_MMAP_FLAGS: u64 = 0;

#[inline]
pub fn mmap_syscall(
    mm: &MmStruct,
    addr_hint: u64,
    length: u64,
    prot: i32,
    flags: i32,
) -> Result<usize, Errno> {
    if length == 0 {
        return Err(Errno::EINVAL);
    }

    let len_aligned = ((length as usize) + PAGE_SIZE as usize - 1) & !(PAGE_SIZE as usize - 1);

    let page_flags = prot_to_vma_flags(prot);

    let map_private = (flags & 0x02) != 0;  // MAP_PRIVATE
    let map_anonymous = (flags & 0x20) != 0; // MAP_ANONYMOUS

    if !map_anonymous {
        return Err(Errno::ENOSYS);
    }

    let addr = if addr_hint != 0 && addr_hint < 0x0000_7FFF_FFFF_F000 {
        addr_hint as usize
    } else {
        match mm.find_free_range(len_aligned) {
            Some(a) => a,
            None => return Err(Errno::ENOMEM),
        }
    };

    let aligned_addr = addr & !(PAGE_SIZE as usize - 1);

    if map_private {
        let final_flags = page_flags | VmaFlags::PRESENT;
        let vma = Vma::new(aligned_addr, aligned_addr + len_aligned, final_flags, VmaType::Anonymous);
        match mm.insert_vma(vma) {
            Ok(()) => {}
            Err(_) => return Err(Errno::ENOMEM),
        }
    }

    Ok(aligned_addr)
}

#[inline]
pub fn munmap_syscall(mm: &MmStruct, addr: u64, length: u64) -> Result<(), Errno> {
    if addr == 0 || length == 0 {
        return Err(Errno::EINVAL);
    }

    let start = addr as usize;
    let end = start + length as usize;

    match mm.remove_range(start, end) {
        Ok(()) => Ok(()),
        Err(_) => Err(Errno::EINVAL),
    }
}

#[inline]
pub fn mprotect_syscall(
    mm: &MmStruct,
    addr: u64,
    length: u64,
    prot: i32,
) -> Result<(), Errno> {
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

    if prot & 0x01 != 0 { flags |= VmaFlags::PRESENT; }
    if prot & 0x02 != 0 { flags |= VmaFlags::WRITABLE; }
    if prot & 0x04 == 0 { flags |= VmaFlags::NX; }

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
}