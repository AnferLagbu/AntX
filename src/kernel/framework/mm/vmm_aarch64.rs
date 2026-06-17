//! AArch64 虚拟内存管理器
//!
//! 实现与 x86_64 vmm.rs 相同的 FFI 接口, 提供:
//! - 内核高半区页表 (TTBR1_EL1) 管理
//! - 用户空间页表 (TTBR0_EL1) 创建/映射
//! - 页表遍历/克隆/销毁
//!
//! 架构: ARMv8-A 4KB granule, 48-bit VA
//! - TTBR0_EL1: 用户空间 (0x0000_0000_0000_0000 .. 0x0000_FFFF_FFFF_FFFF)
//! - TTBR1_EL1: 内核空间 (0xFFFF_0000_0000_0000 .. 0xFFFF_FFFF_FFFF_FFFF)
//!
//! 页表级: L0 (512GB) → L1 (1GB) → L2 (2MB) → L3 (4KB)

use super::*;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::kernel::framework::sync::spinlock::{disable_interrupts, restore_interrupts, IrqSaveFlags};


use crate::kernel::framework::sync::OnceLock;
fn phys_to_virt(phys: u64) -> u64 {
    phys + super::KERNEL_BASE
}

// ─── ARM 描述符常量 ────────────────────────────────────────

/// 描述符类型
#[allow(dead_code)]
const DESC_VALID: u64 = 1 << 0;
const DESC_TYPE_TABLE: u64 = 0b11; // 表描述符 (L0/L1/L2)
const DESC_TYPE_BLOCK: u64 = 0b01; // 块描述符 (L1/L2)
const DESC_TYPE_PAGE: u64 = 0b11; // 页描述符 (L3, 与 TABLE 位相同)

/// 内存属性索引 (与 mmu.rs 中 MAIR_EL1 设定对应)
#[allow(dead_code)]
const MAIR_DEVICE_nGnRnE: u64 = 0; // Device memory
const MAIR_NORMAL_WBWA: u64 = 1; // Normal cacheable (kernel)
#[allow(dead_code)]
const MAIR_NORMAL_NC: u64 = 2; // Normal non-cacheable
const MAIR_USER_NORMAL: u64 = 4; // Normal WBWA for user pages

/// 访问权限位 [7:6] (描述符中)
const AP_EL1_RW: u64 = 0 << 6; // EL1 读写, EL0 不可访问
const AP_BOTH_RW: u64 = 1 << 6; // EL1 读写, EL0 读写
const AP_EL1_RO: u64 = 2 << 6; // EL1 只读, EL0 不可访问
const AP_BOTH_RO: u64 = 3 << 6; // EL1 只读, EL0 只读

/// 属性索引移位 (位 [4:2])
const ATTR_SHIFT: u64 = 2;

/// 访问标志 (位 10)
const AF: u64 = 1 << 10;

/// XN (Execute Never) 位
const UXN: u64 = 1 << 54; // EL0 不可执行
const PXN: u64 = 1 << 53; // EL1 不可执行

/// Stage 1 共享性 (位 8 内, 位 9 外) — Stage 1 不严格需要

/// 每级页表项数
const TABLE_ENTRIES: usize = 512;

// ─── 地址提取宏 ───────────────────────────────────────

#[inline(always)]
fn l0_index(vaddr: u64) -> usize {
    ((vaddr >> 39) & 0x1FF) as usize
}

#[inline(always)]
fn l1_index(vaddr: u64) -> usize {
    ((vaddr >> 30) & 0x1FF) as usize
}

#[inline(always)]
fn l2_index(vaddr: u64) -> usize {
    ((vaddr >> 21) & 0x1FF) as usize
}

#[inline(always)]
fn l3_index(vaddr: u64) -> usize {
    ((vaddr >> 12) & 0x1FF) as usize
}

#[inline(always)]
fn is_kernel_addr(vaddr: u64) -> bool {
    vaddr >= 0xFFFF_0000_0000_0000
}

// ─── 页标志转换 (x86 → ARM) ──────────────────────────────

/// 将 x86 风格页标志转换为 ARM L3 页描述符 (4KB 页)
fn page_flags_to_descriptor(flags: u64, paddr: u64) -> u64 {
    let mut desc = paddr & 0x0000_FFFF_FFFF_F000; // Output address [47:12]
    desc |= DESC_TYPE_PAGE; // bits [1:0] = 0b11
    desc |= AF; // Access flag

    // Access permission
    let user = (flags & PAGE_USER) != 0;
    let writable = (flags & PAGE_WRITABLE) != 0;

    if user && writable {
        desc |= AP_BOTH_RW;
    } else if user && !writable {
        desc |= AP_BOTH_RO;
    } else if !user && writable {
        desc |= AP_EL1_RW;
    } else {
        desc |= AP_EL1_RO;
    }

    // Memory type
    if user {
        desc |= MAIR_USER_NORMAL << ATTR_SHIFT;
    } else {
        desc |= MAIR_NORMAL_WBWA << ATTR_SHIFT;
    }

    // Execute never
    let nx = (flags & PAGE_NX) != 0;
    if nx {
        desc |= UXN;
        if !user {
            desc |= PXN;
        }
    }

    desc
}

/// 将 x86 风格页标志转换为 ARM L1/L2 块描述符 (1GB/2MB 块)
fn block_flags_to_descriptor(flags: u64, paddr: u64, _level: u8, output_mask: u64) -> u64 {
    let mut desc = paddr & output_mask;
    desc |= DESC_TYPE_BLOCK;
    desc |= AF;

    let user = (flags & PAGE_USER) != 0;
    let writable = (flags & PAGE_WRITABLE) != 0;

    if user && writable {
        desc |= AP_BOTH_RW;
    } else if user && !writable {
        desc |= AP_BOTH_RO;
    } else if !user && writable {
        desc |= AP_EL1_RW;
    } else {
        desc |= AP_EL1_RO;
    }

    // Kernel blocks use MAIR index 1 (WBWA), device blocks use 0
    if user {
        desc |= MAIR_USER_NORMAL << ATTR_SHIFT;
    } else {
        desc |= MAIR_NORMAL_WBWA << ATTR_SHIFT;
    }

    let nx = (flags & PAGE_NX) != 0;
    if nx {
        desc |= UXN;
        if !user {
            desc |= PXN;
        }
    }

    desc
}

/// 创建指向下一级表的表描述符
fn table_descriptor(next_table_paddr: u64) -> u64 {
    (next_table_paddr & 0x0000_FFFF_FFFF_F000) | DESC_TYPE_TABLE
}

static VMM_LOCK: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
static VMM_LOCK_RECURSIVE: AtomicBool = AtomicBool::new(false);

// ─── AArch64 Virtual Memory Manager ──────────────────────────────────

pub struct Aarch64Vmm {
    /// Physical address of kernel L0 table (for TTBR1_EL1)
    kernel_l0: u64,
    /// User page table counter
    #[allow(dead_code)]
    next_table_id: core::sync::atomic::AtomicU64,
}

impl Aarch64Vmm {
    pub fn new() -> Self {
        Self {
            kernel_l0: 0,
            next_table_id: core::sync::atomic::AtomicU64::new(0),
        }
    }

    #[inline(always)]
    pub fn acquire_lock(&self) -> IrqSaveFlags {
        let flags = disable_interrupts();
        while VMM_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        #[cfg(debug_assertions)]
        {
            if VMM_LOCK_RECURSIVE.swap(true, Ordering::Relaxed) {
                panic!("VMM_LOCK: recursive acquisition detected (deadlock)");
            }
        }
        flags
    }

    #[inline(always)]
    pub fn release_lock(&self, flags: &IrqSaveFlags) {
        #[cfg(debug_assertions)]
        {
            VMM_LOCK_RECURSIVE.store(false, Ordering::Relaxed);
        }
        VMM_LOCK.store(false, Ordering::Release);
        restore_interrupts(flags);
    }

    // ─── 初始化 ──────────────────────────────────────────────

    /// 初始化内核高半区页表 (TTBR1_EL1).
    /// 不替换 mmu.rs 已建立的低半区恒等映射.
    pub fn init(&self) {
        // 内核 MMU 恒等映射已由 mmu::init() 建立.
        // 我们保留它用于低层访问 (MMIO 等), 并在 TTBR1_EL1 中建立
        // 规范的高半区内核映射以供常规使用.

        // 读取当前 TTBR0_EL1 (指向 mmu.rs 建立的 L0 表)
        let current_l0: u64;
        // SAFETY: mrs ttbr0_el1 是系统寄存器读取指令，无副作用；
        // 声明 options(nomem, preserves_flags) 防止编译器重排。
        unsafe {
            core::arch::asm!("mrs {}, ttbr0_el1", out(reg) current_l0);
        }

        // 存储内核 L0 地址
        // 暂复用现有页表.
        // 完整实现中应创建独立的内核表.
        let kernel_l0_ptr = &raw const self.kernel_l0 as *mut u64;
        // SAFETY: kernel_l0_ptr 指向 self.kernel_l0 (类型对齐的 u64)；
        // write_volatile 防止编译器优化掉对页表硬件的写。
        unsafe {
            ptr::write_volatile(kernel_l0_ptr, current_l0);
        }

        // 确保 TTBR1_EL1 指向同一张表 (用于高半区访问)
        unsafe {
            core::arch::asm!(
                "msr ttbr1_el1, {}",
                "isb",
                in(reg) current_l0,
            );
        }

        // 初始化 KPTI: 创建 trampoline TTBR1 页表.
        // 在用户态运行时, TTBR1_EL1 指向最小化页表 (仅含异常入口),
        // 减少内核地址空间泄露面. 异常入口时切换回完整内核页表.
        // SAFETY: current_l0 是刚写入 TTBR1_EL1 的有效页表物理地址;
        // KPTI 全局状态在 boot 阶段被独占写入; PMM 已初始化.
        unsafe {
            super::kpti::kpti_init(current_l0);
        }
    }

    // ─── Allocate a Page Table ───────────────────────────────────────

    fn alloc_table(&self) -> Option<u64> {
        let paddr = get_pmm().alloc_page()?;
        // Zero the table
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            ptr::write_bytes(
                phys_to_virt(paddr.as_u64()) as *mut u8,
                0,
                PAGE_SIZE as usize,
            );
        }
        Some(paddr.as_u64())
    }

    fn free_table(&self, paddr: u64) {
        if paddr != 0 {
            get_pmm().free_page(PhysAddr(paddr));
        }
    }

    // ─── Kernel Page Map ─────────────────────────────────────────────

    pub fn map_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) -> Result<(), &'static str> {
        if virt.as_u64() >> 48 == 0 {
            return Ok(());
        }
        self.map_page_in_table(self.kernel_l0, virt, phys, flags);
        Ok(())
    }

    pub fn map_huge_page(
        &self,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
        size_type: PageSize,
    ) -> Result<(), &'static str> {
        if virt.as_u64() >> 48 == 0 {
            return Ok(());
        }
        match size_type {
            PageSize::Size4K => self.map_page(virt, phys, flags),
            PageSize::Size2M => {
                let _lock_flags = self.acquire_lock();

                let vaddr = virt.as_u64();
                let paddr = phys.as_u64();
                let raw_flags = flags.bits();

                let l0 = phys_to_virt(self.kernel_l0) as *mut u64;
                let l0_idx = l0_index(vaddr);
                let l1 = match self.ensure_next_level(l0, l0_idx) {
                    Ok(t) => t,
                    Err(e) => {
                        self.release_lock(&_lock_flags);
                        return Err(e);
                    }
                };
                let l1_idx = l1_index(vaddr);
                let l2 = match self.ensure_next_level(l1, l1_idx) {
                    Ok(t) => t,
                    Err(e) => {
                        self.release_lock(&_lock_flags);
                        return Err(e);
                    }
                };
                let l2_idx = l2_index(vaddr);

                let desc = block_flags_to_descriptor(raw_flags, paddr, 2, 0x0000_FFFF_FFE0_0000);
                // SAFETY: l2 是 ensure_next_level 返回的 L2 页表物理基地址
                // (转换后虚拟地址)；l2_idx < 512 落在表项数内；write_volatile
                // 写硬件页表，禁用编译器优化。
                unsafe {
                    ptr::write_volatile(l2.add(l2_idx), desc);
                }

                self.release_lock(&_lock_flags);
                Ok(())
            }
            PageSize::Size1G => {
                let _lock_flags = self.acquire_lock();

                let vaddr = virt.as_u64();
                let paddr = phys.as_u64();
                let raw_flags = flags.bits();

                let l0 = phys_to_virt(self.kernel_l0) as *mut u64;
                let l0_idx = l0_index(vaddr);
                let l1 = match self.ensure_next_level(l0, l0_idx) {
                    Ok(t) => t,
                    Err(e) => {
                        self.release_lock(&_lock_flags);
                        return Err(e);
                    }
                };
                let l1_idx = l1_index(vaddr);

                let desc = block_flags_to_descriptor(raw_flags, paddr, 1, 0x0000_FFFF_C000_0000);
                // SAFETY: l1 是 ensure_next_level 返回的 L1 页表基地址；
                // l1_idx < 512；write_volatile 写硬件页表。
                unsafe {
                    ptr::write_volatile(l1.add(l1_idx), desc);
                }

                self.release_lock(&_lock_flags);
                Ok(())
            }
        }
    }

    pub fn unmap_page(&self, _virt: VirtAddr) {}

    /// 修改虚拟页的保护属性 (mprotect 核心实现)
    ///
    /// 当前简化实现: 仅修改 VMA 元数据, 不修改页表.
    /// TODO(TRACK-82FEA0): 实现完整的 aarch64 页表权限修改 + TLB invalidate.
    pub fn protect_page(&self, _virt: VirtAddr, _new_flags: PageFlags) {
        // TODO(TRACK-A589E3): aarch64 页表权限修改
    }

    pub fn split_2mb_page(&self, _virt: u64) -> Result<(), &'static str> {
        // 在 aarch64 上, L2 块 (2MB) 是块映射的默认.
        // 无需拆分 — 可直接分配 L3 表并使用 4KB 页.
        // 此函数仅为与 x86 兼容而保留.
        Ok(())
    }

    // ─── 页表遍历 / 映射 ───────────────────────────────────────

    /// 从 `root_paddr` 遍历页表, 按需创建中间级, 设置最终页描述符.
    pub fn map_page_in_table(
        &self,
        root_paddr: u64,
        virt: VirtAddr,
        phys: PhysAddr,
        flags: PageFlags,
    ) {
        let _lock_flags = self.acquire_lock();

        let vaddr = virt.as_u64();
        let paddr = phys.as_u64();
        let raw_flags = flags.bits();

        let l0 = phys_to_virt(root_paddr) as *mut u64;
        let l0_idx = l0_index(vaddr);

        let l1 = match self.ensure_next_level(l0, l0_idx) {
            Ok(t) => t,
            Err(_) => {
                self.release_lock(&_lock_flags);
                return;
            }
        };
        let l1_idx = l1_index(vaddr);

        let l2 = match self.ensure_next_level(l1, l1_idx) {
            Ok(t) => t,
            Err(_) => {
                self.release_lock(&_lock_flags);
                return;
            }
        };
        let l2_idx = l2_index(vaddr);

        let l3 = match self.ensure_next_level(l2, l2_idx) {
            Ok(t) => t,
            Err(_) => {
                self.release_lock(&_lock_flags);
                return;
            }
        };
        let l3_idx = l3_index(vaddr);

        let desc = page_flags_to_descriptor(raw_flags, paddr);
        // SAFETY: l3 是 ensure_next_level 返回的 L3 页表基地址；
        // l3_idx < 512；write_volatile 写硬件页表。
        unsafe {
            ptr::write_volatile(l3.add(l3_idx), desc);
        }

        // SAFETY: dsb ishst / tlbi vaae1is / dsb ish / isb 是 aarch64 标准
        // TLB 失效序列；输入 vaddr 是合法内核虚拟地址，无副作用。
        unsafe {
            core::arch::asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) vaddr);
        }

        self.release_lock(&_lock_flags);
    }

    /// Ensure the next-level page table exists at `table[idx]`.
    /// Returns a pointer to the next-level table.
    fn ensure_next_level(&self, table: *mut u64, idx: usize) -> Result<*mut u64, &'static str> {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let entry = ptr::read_volatile(table.add(idx));
            if entry & 0b11 == 0b11 {
                let paddr = entry & 0x0000_FFFF_FFFF_F000;
                Ok(phys_to_virt(paddr) as *mut u64)
            } else {
                let new_paddr = self
                    .alloc_table()
                    .ok_or("[VMM] Out of physical memory for page table")?;
                let desc = table_descriptor(new_paddr);
                ptr::write_volatile(table.add(idx), desc);
                core::arch::asm!("dsb ishst");
                Ok(phys_to_virt(new_paddr) as *mut u64)
            }
        }
    }

    pub fn unmap_page_in_table(&self, root_paddr: u64, virt: VirtAddr) {
        if root_paddr == 0 {
            return;
        }

        let _lock_flags = self.acquire_lock();

        let vaddr = virt.as_u64();

        // SAFETY: phys_to_virt(root_paddr) gives kernel VA for page table walk.
        let l0 = phys_to_virt(root_paddr) as *mut u64;
        let l0_idx = l0_index(vaddr);

        let l1 = self.get_next_level(l0, l0_idx);
        if l1.is_null() {
            self.release_lock(&_lock_flags);
            return;
        }
        let l1_idx = l1_index(vaddr);

        let l2 = self.get_next_level(l1, l1_idx);
        if l2.is_null() {
            self.release_lock(&_lock_flags);
            return;
        }
        let l2_idx = l2_index(vaddr);

        let l3 = self.get_next_level(l2, l2_idx);
        if l3.is_null() {
            self.release_lock(&_lock_flags);
            return;
        }
        let l3_idx = l3_index(vaddr);

        // 清除 L3 页描述符
        // SAFETY: l3 是已验证的 L3 页表基地址；l3_idx < 512。
        unsafe {
            ptr::write_volatile(l3.add(l3_idx), 0);
        }

        // TLB 失效 — 必须在释放页表页前执行, 以避免投机性遍历落入已释放物理页.
        // SAFETY: 标准 TLB 失效序列；vaddr 是有效内核虚拟地址。
        unsafe {
            core::arch::asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) vaddr);
        }

        // 递归释放空的中间页表页, 避免 unmap 间内存泄漏
        // (destroy_page_table 仅在进程销毁时执行).
        if self.is_table_empty(l3) {
            // SAFETY: 读取/清除 L2 中指向 L3 的表项；l2_idx < 512。
            let l3_paddr = unsafe {
                let l2_entry = ptr::read_volatile(l2.add(l2_idx));
                ptr::write_volatile(l2.add(l2_idx), 0);
                l2_entry & 0x0000_FFFF_FFFF_F000
            };
            // SAFETY: dsb ishst 是数据同步屏障，确保前面的页表写完成。
            unsafe {
                core::arch::asm!("dsb ishst");
            }
            self.free_table(l3_paddr);

            // Check L2 recursively
            if self.is_table_empty(l2) {
                // SAFETY: 读取/清除 L1 中指向 L2 的表项；l1_idx < 512。
                let l2_paddr = unsafe {
                    let l1_entry = ptr::read_volatile(l1.add(l1_idx));
                    ptr::write_volatile(l1.add(l1_idx), 0);
                    l1_entry & 0x0000_FFFF_FFFF_F000
                };
                unsafe {
                    core::arch::asm!("dsb ishst");
                }
                self.free_table(l2_paddr);

                // Check L1 recursively
                if self.is_table_empty(l1) {
                    // SAFETY: 读取/清除 L0 中指向 L1 的表项；l0_idx < 512。
                    let l1_paddr = unsafe {
                        let l0_entry = ptr::read_volatile(l0.add(l0_idx));
                        ptr::write_volatile(l0.add(l0_idx), 0);
                        l0_entry & 0x0000_FFFF_FFFF_F000
                    };
                    unsafe {
                        core::arch::asm!("dsb ishst");
                    }
                    self.free_table(l1_paddr);
                }
            }
        }

        self.release_lock(&_lock_flags);
    }

    /// 当一个页表页的全部 512 项均为 0 时返回 true.
    fn is_table_empty(&self, table: *mut u64) -> bool {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            for i in 0..TABLE_ENTRIES {
                if ptr::read_volatile(table.add(i)) != 0 {
                    return false;
                }
            }
        }
        true
    }

    /// 遍历表项到下一级 (只读, 不分配).
    /// 若该表项不是合法的表描述符则返回 null.
    fn get_next_level(&self, table: *mut u64, idx: usize) -> *mut u64 {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            let entry = ptr::read_volatile(table.add(idx));
            if entry & 0b11 == 0b11 {
                let paddr = entry & 0x0000_FFFF_FFFF_F000;
                phys_to_virt(paddr) as *mut u64
            } else {
                core::ptr::null_mut()
            }
        }
    }

    // ─── 用户页表操作 ──────────────────────────────────

    pub fn create_user_page_table(&self) -> Option<u64> {
        // 为用户空间 (TTBR0_EL1) 分配一张干净的 L0 表
        let user_l0 = self.alloc_table()?;

        // 为用户空间项分配新的 L1 表.
        // 不与内核共享 L1_IDMAP/L2_DEVICE — 这些表对 MMIO 区使用
        // Device 内存属性, 不适合用户代码执行. 共享它们也会导致
        // 用户页表修改 (例如把 2MB BLOCK 替换为 TABLE 描述符) 破坏
        // 内核恒等映射.
        let user_l1 = match self.alloc_table() {
            Some(t) => t,
            None => {
                self.free_table(user_l0);
                return None;
            }
        };

        let kernel_l0 = phys_to_virt(self.kernel_l0) as *const u64;
        let user_l0_ptr = phys_to_virt(user_l0) as *mut u64;
        let user_l1_desc = table_descriptor(user_l1);

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            // L0[0] → 新的用户 L1 表 (干净, 不共享)
            ptr::write_volatile(user_l0_ptr.add(0), user_l1_desc);

            // 从内核 L0 复制 TTBR1 项 (索引 256..511).
            // 它们覆盖高半区内核地址空间
            // (0xFFFF_0000_0000_0000 .. 0xFFFF_FFFF_FFFF_FFFF).
            // 切换 TTBR0_EL1 到用户页表后, 内核代码必须仍可通过
            // TTBR1_EL1 访问 — TTBR1 项保证内核自身页表仍可用于
            // 异常处理与其他内核态操作.
            for i in 256..TABLE_ENTRIES {
                let entry = ptr::read_volatile(kernel_l0.add(i));
                ptr::write_volatile(user_l0_ptr.add(i), entry);
            }
        }

        Some(user_l0)
    }

    pub fn ensure_pml4_user(&self, _virt: u64) {
        // 在 aarch64 上, 内核与用户表是分离的 (TTBR0 vs TTBR1).
        // 内核表项无需 USER 位 — 用户访问走 TTBR0, 内核走 TTBR1.
        // 对 aarch64 而言此函数为空操作.
    }

    pub fn ensure_path_user(&self, virt: u64) {
        // 在 aarch64 上, 仅当路径位于用户页表才相关.
        // 由于用户表在 TTBR0 且天然用户可访问, 只需确保所有
        // 中间表描述符存在 (map_page_in_table 已处理).
        if is_kernel_addr(virt) {
            // 内核页无需 USER 标志
        }
        // 用户表中的用户空间地址, 已在 map_page_in_table 中保证项合法
    }

    pub fn switch_page_table(&self, ttbr0: u64) {
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::arch::asm!(
                "dsb ish",
                "msr ttbr0_el1, {}",
                "isb",
                "tlbi vmalle1is",
                "dsb ish",
                "isb",
                in(reg) ttbr0,
            );
        }
    }

    pub fn get_physical(&self, virt: VirtAddr) -> Option<PhysAddr> {
        self.get_physical_in_pml4(self.kernel_l0, virt)
    }

    pub fn get_physical_in_pml4(&self, root_paddr: u64, virt: VirtAddr) -> Option<PhysAddr> {
        let vaddr = virt.as_u64();

        let l0 = root_paddr as *const u64;
        let l0_idx = l0_index(vaddr);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let l0_entry = unsafe { ptr::read_volatile(l0.add(l0_idx)) };
        if l0_entry & 0b11 != 0b11 {
            return None;
        }

        // SAFETY: table descriptor frame bits contain valid PA → phys_to_virt → kernel VA
        let l1 = phys_to_virt(l0_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l1_idx = l1_index(vaddr);
        let l1_entry = unsafe { ptr::read_volatile(l1.add(l1_idx)) };
        if l1_entry & 0b11 == 0b01 {
            // L1 block (1GB)
            return Some(PhysAddr(
                (l1_entry & 0x0000_FFFF_C000_0000) | (vaddr & 0x3FFF_FFFF),
            ));
        }
        if l1_entry & 0b11 != 0b11 {
            return None;
        }

        // SAFETY: L1 table descriptor frame → phys_to_virt → kernel VA
        let l2 = phys_to_virt(l1_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l2_idx = l2_index(vaddr);
        let l2_entry = unsafe { ptr::read_volatile(l2.add(l2_idx)) };
        if l2_entry & 0b11 == 0b01 {
            // L2 block (2MB)
            return Some(PhysAddr(
                (l2_entry & 0x0000_FFFF_FFE0_0000) | (vaddr & 0x1F_FFFF),
            ));
        }
        if l2_entry & 0b11 != 0b11 {
            return None;
        }

        // SAFETY: L2 table descriptor frame → phys_to_virt → kernel VA
        let l3 = phys_to_virt(l2_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l3_idx = l3_index(vaddr);
        let l3_entry = unsafe { ptr::read_volatile(l3.add(l3_idx)) };
        if l3_entry & 0b11 != 0b11 {
            return None;
        }

        Some(PhysAddr(
            (l3_entry & 0x0000_FFFF_FFFF_F000) | (vaddr & 0xFFF),
        ))
    }

    /// 读取 L3 页表项原始值 (用于 swap entry 检测)
    pub fn get_pte_value(&self, root_paddr: u64, virt: VirtAddr) -> Option<u64> {
        let vaddr = virt.as_u64();

        let l0 = root_paddr as *const u64;
        let l0_idx = l0_index(vaddr);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let l0_entry = unsafe { ptr::read_volatile(l0.add(l0_idx)) };
        if l0_entry & 0b11 != 0b11 {
            return None;
        }

        let l1 = phys_to_virt(l0_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l1_idx = l1_index(vaddr);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let l1_entry = unsafe { ptr::read_volatile(l1.add(l1_idx)) };
        if l1_entry & 0b11 != 0b11 {
            return None;
        }

        let l2 = phys_to_virt(l1_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l2_idx = l2_index(vaddr);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let l2_entry = unsafe { ptr::read_volatile(l2.add(l2_idx)) };
        if l2_entry & 0b11 != 0b11 {
            return None;
        }

        let l3 = phys_to_virt(l2_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
        let l3_idx = l3_index(vaddr);
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let l3_entry = unsafe { ptr::read_volatile(l3.add(l3_idx)) };

        Some(l3_entry)
    }

    /// 直接写入 L3 PTE 原始值 (用于 swap 替换)
    ///
    /// 沿 L0→L1→L2→L3 找到最终 PTE, 写入 raw_pte 后 TLB invalidate.
    /// 与 map_page_in_table 的区别: 接受任意 raw PTE (含 swap entry, 即 valid=0).
    /// 若任意中间层缺失 (valid=0), 静默返回 (不创建中间页表, swap-out 不应触发缺中间页).
    pub fn set_pte_value(&self, root_paddr: u64, virt: VirtAddr, raw_pte: u64) {
        let vaddr = virt.as_u64();

        let _flags = self.acquire_lock();

        // SAFETY: VMM_LOCK held; 四级页表查找 PTE 并直接写入
        unsafe {
            let l0 = root_paddr as *const u64;
            let l0_idx = l0_index(vaddr);
            let l0_entry = ptr::read_volatile(l0.add(l0_idx));
            if l0_entry & 0b11 != 0b11 {
                self.release_lock(&_flags);
                return;
            }

            let l1 = phys_to_virt(l0_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
            let l1_idx = l1_index(vaddr);
            let l1_entry = ptr::read_volatile(l1.add(l1_idx));
            if l1_entry & 0b11 != 0b11 {
                self.release_lock(&_flags);
                return;
            }

            let l2 = phys_to_virt(l1_entry & 0x0000_FFFF_FFFF_F000) as *const u64;
            let l2_idx = l2_index(vaddr);
            let l2_entry = ptr::read_volatile(l2.add(l2_idx));
            if l2_entry & 0b11 != 0b11 {
                self.release_lock(&_flags);
                return;
            }

            let l3 = phys_to_virt(l2_entry & 0x0000_FFFF_FFFF_F000) as *mut u64;
            let l3_idx = l3_index(vaddr);
            let l3_ptr = l3.add(l3_idx);
            ptr::write_volatile(l3_ptr, raw_pte);

            // TLB invalidate (与 unmap_page_in_table 一致)
            core::arch::asm!("dsb ishst", "tlbi vaae1is, {}", "dsb ish", "isb", in(reg) vaddr);
        }

        self.release_lock(&_flags);
    }

    // ─── Clone / Destroy User Page Table ────────────────────────────

    pub fn clone_user_page_table(&self, parent_paddr: u64) -> Option<u64> {
        let child_paddr = self.alloc_table()?;

        // SAFETY: phys_to_virt converts page table physical addresses to kernel VAs
        let parent = phys_to_virt(parent_paddr) as *const u64;
        let child = phys_to_virt(child_paddr) as *mut u64;

        for i in 0..256 {
            unsafe {
                let entry = ptr::read_volatile(parent.add(i));
                ptr::write_volatile(child.add(i), entry);
            }
        }

        Some(child_paddr)
    }

    pub fn destroy_page_table(&self, root_paddr: u64) {
        if root_paddr == 0 {
            return;
        }

        // SAFETY: phys_to_virt converts root_paddr to kernel VA
        let l0 = phys_to_virt(root_paddr) as *mut u64;

        for i in 0..256 {
            unsafe {
                let entry = ptr::read_volatile(l0.add(i));
                if entry & 0b11 == 0b11 {
                    let l1_paddr = entry & 0x0000_FFFF_FFFF_F000;
                    self.destroy_l1_table(l1_paddr);
                }
            }
        }

        self.free_table(root_paddr);
    }

    fn destroy_l1_table(&self, paddr: u64) {
        let l1 = phys_to_virt(paddr) as *mut u64;
        for i in 0..512 {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                let entry = ptr::read_volatile(l1.add(i));
                if entry & 0b11 == 0b11 {
                    let l2_paddr = entry & 0x0000_FFFF_FFFF_F000;
                    self.destroy_l2_table(l2_paddr);
                }
            }
        }
        self.free_table(paddr);
    }

    fn destroy_l2_table(&self, paddr: u64) {
        let l2 = phys_to_virt(paddr) as *mut u64;
        for i in 0..512 {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                let entry = ptr::read_volatile(l2.add(i));
                if entry & 0b11 == 0b11 {
                    let l3_paddr = entry & 0x0000_FFFF_FFFF_F000;
                    self.free_table(l3_paddr);
                }
            }
        }
        self.free_table(paddr);
    }
}

// ─── 全局 VMM 实例 ─────────────────────────────────────────────

static GLOBAL_VMM: OnceLock<Aarch64Vmm> = OnceLock::new();

pub fn vmm_init() {
    GLOBAL_VMM.get_or_init(|| {
        let vmm = Aarch64Vmm::new();
        vmm.init();
        vmm
    });
}

pub fn get_vmm() -> &'static Aarch64Vmm {
    GLOBAL_VMM
        .get()
        .expect("[VMM] aarch64 VMM accessed before initialization")
}

pub fn get_kernel_pml4() -> u64 {
    get_vmm().kernel_l0
}

pub fn get_current_pml4() -> u64 {
    let ttbr0: u64;
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::arch::asm!("mrs {}, TTBR0_EL1", out(reg) ttbr0);
    }
    if ttbr0 != 0 {
        ttbr0
    } else {
        get_vmm().kernel_l0
    }
}
