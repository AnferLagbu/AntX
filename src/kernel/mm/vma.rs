//! Virtual Memory Area (VMA) — 用户地址空间管理
//!
//! 为每个进程的地址空间维护一组 VMA 描述符，提供：
//! - `mmap`/`munmap`/`mprotect`/`mremap` 语义
//! - 页错误 (page fault) 时的 demand paging 查找
//! - 地址空间布局 (stack/heap/mmap 方向)
//!
//! ## 数据结构
//!
//! 每个地址空间 (`MmStruct`) 维护一个按起始地址排序的 VMA 列表。
//! 当前使用 `Vec<Vma>` 实现，后续可升级为红黑树优化 O(log n) 查找。
//!
//! ## 安全
//!
//! `MmStruct` 内部保护由 `spin::Mutex` 提供。
//! `Vma` 中的物理页映射感知与 PMM 协作。

use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VmaType {
    Anonymous = 0,   // malloc / mmap(MAP_ANONYMOUS)
    FileBacked = 1,  // mmap file
    Stack = 2,       // 用户栈 (向下增长)
    Heap = 3,        // 堆 (brk/sbrk)
    Vdso = 4,        // vDSO
    Vsvar = 5,       // vsyscall / vvar
    Guard = 6,       // 保护页 (不可访问)
}

impl VmaType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Anonymous,
            1 => Self::FileBacked,
            2 => Self::Stack,
            3 => Self::Heap,
            4 => Self::Vdso,
            5 => Self::Vsvar,
            _ => Self::Guard,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Vma {
    pub start: usize,
    pub end: usize,
    pub flags: PageFlags,
    pub vma_type: VmaType,
    pub offset: u64,
}

impl Vma {
    pub fn new(start: usize, end: usize, flags: PageFlags, vma_type: VmaType) -> Self {
        Self { start, end, flags, vma_type, offset: 0 }
    }

    pub fn with_offset(start: usize, end: usize, flags: PageFlags, offset: u64) -> Self {
        Self { start, end, flags, vma_type: VmaType::FileBacked, offset }
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.end - self.start
    }

    #[inline]
    pub fn contains(&self, addr: usize) -> bool {
        addr >= self.start && addr < self.end
    }

    #[inline]
    pub fn is_guard(&self) -> bool {
        self.vma_type == VmaType::Guard
    }

    #[inline]
    pub fn is_stack(&self) -> bool {
        self.vma_type == VmaType::Stack
    }
}

pub struct MmStruct {
    pub vmas: Mutex<Vec<Vma>>,
    pub start_brk: AtomicUsize,
    pub brk: AtomicUsize,
    pub start_stack: usize,
    pub mmap_base: usize,
}

impl MmStruct {
    pub fn new() -> Self {
        Self {
            vmas: Mutex::new(Vec::new()),
            start_brk: AtomicUsize::new(0),
            brk: AtomicUsize::new(0),
            start_stack: 0,
            mmap_base: 0,
        }
    }

    /// 查找包含 `addr` 的 VMA
    pub fn find_vma(&self, addr: usize) -> Option<Vma> {
        let vmas = self.vmas.lock();
        vmas.iter().find(|v| v.contains(addr)).cloned()
    }

    /// 添加 VMA (合并相邻同类 VMA)
    pub fn insert_vma(&self, vma: Vma) -> Result<(), &'static str> {
        let mut vmas = self.vmas.lock();

        for existing in vmas.iter() {
            if vma.start < existing.end && vma.end > existing.start {
                if existing.vma_type != vma.vma_type || existing.flags != vma.flags {
                    return Err("VMA overlap with incompatible mapping");
                }
            }
        }

        let mut merged = vma;
        let mut i = 0;
        while i < vmas.len() {
            let existing = &vmas[i];
            if existing.vma_type != merged.vma_type || existing.flags != merged.flags {
                i += 1;
                continue;
            }
            let adjacent_or_overlap = merged.start <= existing.end && merged.end >= existing.start;
            if adjacent_or_overlap {
                let new_start = merged.start.min(existing.start);
                let new_end = merged.end.max(existing.end);
                merged.start = new_start;
                merged.end = new_end;
                vmas.remove(i);
                continue;
            }
            i += 1;
        }

        let pos = vmas.iter().position(|v| v.start > merged.start).unwrap_or(vmas.len());
        vmas.insert(pos, merged);

        Ok(())
    }

    /// 删除 [start, end) 范围内的 VMA 映射
    pub fn remove_range(&self, start: usize, end: usize) -> Result<(), &'static str> {
        let mut vmas = self.vmas.lock();

        let mut i = 0;
        while i < vmas.len() {
            let vma_start = vmas[i].start;
            let vma_end = vmas[i].end;
            let vma_flags = vmas[i].flags;
            let vma_type = vmas[i].vma_type;

            if vma_end <= start {
                i += 1;
                continue;
            }

            if vma_start >= end {
                break;
            }

            if start <= vma_start && end >= vma_end {
                let removed = vmas.remove(i);
                self.unmap_vma_pages(&removed);
            } else if start <= vma_start {
                let mut truncated = vmas.remove(i);
                truncated.start = end;
                vmas.insert(i, truncated);
                self.unmap_vma_pages(&Vma::new(start, end, vma_flags, vma_type));
                i += 1;
            } else if end >= vma_end {
                let mut truncated = vmas.remove(i);
                truncated.end = start;
                vmas.insert(i, truncated);
                self.unmap_vma_pages(&Vma::new(start, vma_end, vma_flags, vma_type));
                i += 1;
            } else {
                let left = Vma::new(vma_start, start, vma_flags, vma_type);
                let right = Vma::new(end, vma_end, vma_flags, vma_type);
                let mid = Vma::new(start, end, vma_flags, vma_type);
                vmas.remove(i);
                vmas.insert(i, right);
                vmas.insert(i, left);
                self.unmap_vma_pages(&mid);
                i += 2;
            }
        }

        Ok(())
    }

    fn unmap_vma_pages(&self, vma: &Vma) {
        let vmm = super::vmm::get_vmm();
        let mut addr = vma.start;
        while addr < vma.end {
            vmm.unmap_page(VirtAddr(addr as u64));
            addr += PAGE_SIZE as usize;
        }
    }

    /// 查找空闲地址范围 (用于 mmap 的 hint-less 分配)
    pub fn find_free_range(&self, size: usize) -> Option<usize> {
        let vmas = self.vmas.lock();
        let mut cursor = self.mmap_base;

        for vma in vmas.iter() {
            if cursor + size <= vma.start {
                return Some(cursor);
            }
            cursor = vma.end;
        }

        // TASK_SIZE: user address space upper bound for 64-bit
        let task_size: usize = 0x0000_7FFF_FFFF_F000;

        if cursor + size <= task_size {
            Some(cursor)
        } else {
            None
        }
    }

    /// 设置堆边界
    ///
    /// Uses AtomicUsize for brk/start_brk for lock-free thread-safe access.
    pub fn set_brk(&self, new_brk: usize) -> Result<usize, &'static str> {
        let page_aligned = (new_brk + PAGE_SIZE as usize - 1) & !(PAGE_SIZE as usize - 1);

        let start_brk = self.start_brk.load(Ordering::Acquire);
        let current_brk = self.brk.load(Ordering::Acquire);

        if page_aligned > start_brk {
            let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;
            let vma = Vma::new(current_brk, page_aligned, flags, VmaType::Heap);
            self.insert_vma(vma)?;
            self.brk.store(page_aligned, Ordering::Release);
        } else if page_aligned < current_brk {
            // 先更新 brk，防止其他 CPU 在 remove_range 后读到旧值
            // 去访问已被 unmap 的堆区域
            self.brk.store(page_aligned, Ordering::Release);
            self.remove_range(page_aligned, current_brk)?;
        }

        Ok(self.brk.load(Ordering::Acquire))
    }
}

// SAFETY: MmStruct uses Mutex for Vec<Vma> and AtomicUsize for brk/start_brk.
// All mutable fields go through proper synchronization primitives.
// start_stack/mmap_base are read-only after init.
// Cross-thread shared access is safe because all mutations are internally
// synchronized via atomic operations and locks.
unsafe impl Send for MmStruct {}
unsafe impl Sync for MmStruct {}

static mut CURRENT_MM: *const MmStruct = core::ptr::null();

pub fn set_current_mm(mm: *const MmStruct) {
    // SAFETY: CURRENT_MM 是当前 CPU 的 per-CPU 状态指针，
    // 仅在进程切换时由调度器写入，调用者保证无并发写入。
    unsafe { CURRENT_MM = mm; }
}

pub fn get_current_mm() -> Option<&'static MmStruct> {
    // SAFETY: CURRENT_MM 在 set_current_mm 中设置，
    // 要么为 null，要么指向有效的 MmStruct。
    // 返回 &'static 引用是安全的，因为 MmStruct 生命周期
    // 与进程一致，进程存在期间指针有效。
    unsafe {
        if CURRENT_MM.is_null() {
            None
        } else {
            Some(&*CURRENT_MM)
        }
    }
}

pub fn mm_struct_new() -> MmStruct {
    MmStruct::new()
}

#[no_mangle]
pub extern "C" fn vma_find(mm_ptr: *const MmStruct, addr: u64) -> u64 {
    if mm_ptr.is_null() {
        return 0;
    }
    let mm = unsafe { &*mm_ptr };
    match mm.find_vma(addr as usize) {
        Some(vma) => {
            let flags = vma.flags.bits();
            ((vma.start as u64) << 32) | flags
        }
        None => 0,
    }
}
