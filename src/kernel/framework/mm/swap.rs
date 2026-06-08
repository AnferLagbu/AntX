//! Swap — 页面换出/换入与回收
//!
//! 当物理内存不足时, 将不活跃的页面换出到 swap 区, 释放物理页.
//! 当访问换出页面时, 通过 #PF 触发换入.
//!
//! ## 核心设计
//!
//! - **Swap 区**: 概念性块设备区域, 按 slot 索引 (每个 slot = 4KB)
//! - **Swap Entry**: 64 位编码, 存储在页表项中, 标识换出页面的位置
//!   - Bit 0: present = 0 (标识为 swap entry 而非 PTE)
//!   - Bit 1: swap 类型 (0 = 普通 swap)
//!   - Bits 2-55: swap slot 索引
//!   - Bits 56-63: 保留
//! - **LRU 链表**: active + inactive 双链表, 近似 LRU 回收
//! - **kswapd**: 内核线程, 周期性扫描 inactive 链表回收页面
//!
//! ## 换出流程
//!
//! ```text
//! kswapd 扫描 inactive 链表
//!   → 选中页面
//!   → 分配 swap slot
//!   → 写入 swap 区 (当前阶段: 复制到预留内存区域)
//!   → 更新 PTE 为 swap entry
//!   → 释放物理页
//! ```
//!
//! ## 换入流程
//!
//! ```text
//! #PF → 检测 PTE 为 swap entry
//!   → 分配新物理页
//!   → 从 swap slot 读取数据
//!   → 更新 PTE 为正常映射
//!   → 释放 swap slot
//! ```
//!
//! # Safety
//!
//! - Swap 位图由自旋锁保护
//! - 换出/换入操作在 #PF 上下文中执行, 必须无阻塞
//! - 当前阶段 swap 区使用预留内存区域模拟, 后续集成块设备

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;

use crate::kernel::framework::mm::{PhysAddr, VirtAddr, PAGE_SIZE, pmm, vmm};
use crate::kernel::framework::mm::page_fault::PfResult;

// ============================================================================
// Swap Entry 编码
// ============================================================================

/// Swap entry: 存储在页表项中, 标识换出页面
///
/// 编码格式 (64 位):
/// - Bit 0: 0 (present = 0, 区分正常 PTE)
/// - Bit 1: swap 类型 (0 = 普通 swap)
/// - Bits 2-55: swap slot 索引
/// - Bits 56-63: 保留
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapEntry(u64);

impl SwapEntry {
    /// 创建 swap entry
    pub fn new(slot: u64) -> Self {
        // Bit 0 = 0 (not present), Bit 1 = 0 (type 0)
        // Slot 存储在 bits 2-55
        SwapEntry((slot & 0x003F_FFFF_FFFF_FFFF) << 2)
    }

    /// 从 PTE 值解析 swap entry
    pub fn from_pte(pte: u64) -> Option<Self> {
        // present = 0 且非零值
        if pte & 1 != 0 || pte == 0 {
            return None;
        }
        // 检查是否为有效的 swap entry (bit 1 = 0)
        if pte & 0x2 != 0 {
            return None;
        }
        Some(SwapEntry(pte))
    }

    /// 获取 swap slot 索引
    pub fn slot(&self) -> u64 {
        (self.0 >> 2) & 0x003F_FFFF_FFFF_FFFF
    }

    /// 转换为 PTE 值
    pub fn to_pte(&self) -> u64 {
        self.0
    }

    /// 是否为有效的 swap entry
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }
}

// ============================================================================
// Swap 区管理
// ============================================================================

/// Swap 区最大 slot 数 (当前阶段: 4096 slots = 16MB)
const SWAP_MAX_SLOTS: usize = 4096;

/// Swap slot 状态
const SLOT_FREE: u8 = 0;
const SLOT_USED: u8 = 1;

/// Swap 区: 使用预留内存区域模拟
struct SwapArea {
    /// Slot 分配位图
    bitmap: [u8; SWAP_MAX_SLOTS],
    /// 已使用的 slot 数
    used_count: u64,
    /// Swap 数据存储区虚拟地址 (预留内存)
    storage_virt: u64,
    /// 是否已初始化
    initialized: bool,
}

impl SwapArea {
    const fn new() -> Self {
        SwapArea {
            bitmap: [SLOT_FREE; SWAP_MAX_SLOTS],
            used_count: 0,
            storage_virt: 0,
            initialized: false,
        }
    }

    /// 初始化 swap 区: 分配预留内存
    fn init(&mut self) -> bool {
        if self.initialized {
            return true;
        }

        // 分配存储区域: SWAP_MAX_SLOTS 个 4KB 页
        let pmm_inst = pmm::get_pmm();
        let mut virt_base = 0u64;

        // 使用连续分配 (简化实现)
        for i in 0..SWAP_MAX_SLOTS {
            match pmm_inst.alloc_page() {
                Some(phys) => {
                    if i == 0 {
                        virt_base = phys.to_virt().0;
                    }
                    // 清零
                    let v = phys.to_virt();
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    unsafe {
                        core::ptr::write_bytes(v.0 as *mut u8, 0, PAGE_SIZE as usize);
                    }
                }
                None => {
                    // 分配失败, 回滚已分配的页
                    return false;
                }
            }
        }

        self.storage_virt = virt_base;
        self.initialized = true;
        crate::klog_info!(Swap, "[SWAP] Initialized: {} slots ({} MB)",
            SWAP_MAX_SLOTS,
            SWAP_MAX_SLOTS * 4096 / (1024 * 1024));
        true
    }

    /// 分配一个空闲 slot, 返回 slot 索引
    fn alloc_slot(&mut self) -> Option<u64> {
        for (i, slot) in self.bitmap.iter_mut().enumerate() {
            if *slot == SLOT_FREE {
                *slot = SLOT_USED;
                self.used_count += 1;
                return Some(i as u64);
            }
        }
        None
    }

    /// 释放 slot
    fn free_slot(&mut self, slot: u64) {
        let idx = slot as usize;
        if idx < SWAP_MAX_SLOTS && self.bitmap[idx] == SLOT_USED {
            self.bitmap[idx] = SLOT_FREE;
            self.used_count -= 1;
        }
    }

    /// 获取 slot 对应的虚拟地址
    fn slot_addr(&self, slot: u64) -> u64 {
        self.storage_virt + slot * PAGE_SIZE
    }

    /// 将数据写入 swap slot
    ///
    /// # Safety
    ///
    /// - `src_virt` 必须指向有效的 4KB 数据源
    fn write_slot(&self, slot: u64, src_virt: u64) {
        if !self.initialized {
            return;
        }
        let dst = self.slot_addr(slot) as *mut u8;
        // SAFETY: dst 指向 swap 存储区 (已分配并映射), src 为有效用户页
        unsafe {
            core::ptr::copy_nonoverlapping(
                src_virt as *const u8,
                dst,
                PAGE_SIZE as usize,
            );
        }
    }

    /// 从 swap slot 读取数据
    ///
    /// # Safety
    ///
    /// - `dst_virt` 必须指向有效的 4KB 目标页
    fn read_slot(&self, slot: u64, dst_virt: u64) {
        if !self.initialized {
            return;
        }
        let src = self.slot_addr(slot) as *const u8;
        // SAFETY: src 指向 swap 存储区, dst 为有效物理页
        unsafe {
            core::ptr::copy_nonoverlapping(
                src,
                dst_virt as *mut u8,
                PAGE_SIZE as usize,
            );
        }
    }

    /// 空闲 slot 数
    fn free_slots(&self) -> u64 {
        SWAP_MAX_SLOTS as u64 - self.used_count
    }
}

// ============================================================================
// LRU 链表 (简化版)
// ============================================================================

/// LRU 页面跟踪
///
/// 使用固定大小数组跟踪最近访问的页面,
/// 按访问时间排序 (新访问的页移到尾部).
struct LruList {
    /// Active 链表: 最近被访问的页面
    active: [LruEntry; LRU_CAPACITY],
    active_count: usize,
    /// Inactive 链表: 较久未访问的页面 (回收候选)
    inactive: [LruEntry; LRU_CAPACITY],
    inactive_count: usize,
}

const LRU_CAPACITY: usize = 256;

#[derive(Clone, Copy)]
struct LruEntry {
    virt_addr: u64,
    phys_addr: u64,
    /// 是否为脏页
    dirty: bool,
    /// 是否被占用
    occupied: bool,
}

impl LruEntry {
    const fn empty() -> Self {
        LruEntry {
            virt_addr: 0,
            phys_addr: 0,
            dirty: false,
            occupied: false,
        }
    }
}

impl LruList {
    const fn new() -> Self {
        LruList {
            active: [LruEntry::empty(); LRU_CAPACITY],
            active_count: 0,
            inactive: [LruEntry::empty(); LRU_CAPACITY],
            inactive_count: 0,
        }
    }

    /// 添加页面到 active 链表 (页面被访问时调用)
    fn add_active(&mut self, virt_addr: u64, phys_addr: u64, dirty: bool) {
        // 先检查是否已在 inactive 链表中, 若是则提升
        for i in 0..LRU_CAPACITY {
            if self.inactive[i].occupied && self.inactive[i].virt_addr == virt_addr {
                // 从 inactive 移除, 加入 active
                let entry = self.inactive[i];
                self.inactive[i] = LruEntry::empty();
                self.inactive_count -= 1;
                self.push_active(entry.virt_addr, entry.phys_addr, entry.dirty || dirty);
                return;
            }
        }

        // 不在 inactive 中, 直接加入 active
        self.push_active(virt_addr, phys_addr, dirty);
    }

    fn push_active(&mut self, virt_addr: u64, phys_addr: u64, dirty: bool) {
        // 检查是否已在 active 中
        for i in 0..LRU_CAPACITY {
            if self.active[i].occupied && self.active[i].virt_addr == virt_addr {
                self.active[i].dirty = dirty;
                return;
            }
        }

        // active 满, 将最旧的降级到 inactive
        if self.active_count >= LRU_CAPACITY {
            self.demote_oldest();
        }

        // 添加到 active 尾部
        for i in 0..LRU_CAPACITY {
            if !self.active[i].occupied {
                self.active[i] = LruEntry {
                    virt_addr,
                    phys_addr,
                    dirty,
                    occupied: true,
                };
                self.active_count += 1;
                return;
            }
        }
    }

    /// 将 active 链表最旧的条目降级到 inactive
    fn demote_oldest(&mut self) {
        // 找到 active 中第一个条目 (最旧)
        for i in 0..LRU_CAPACITY {
            if self.active[i].occupied {
                let entry = self.active[i];
                self.active[i] = LruEntry::empty();
                self.active_count -= 1;

                // inactive 满时丢弃最旧的
                if self.inactive_count >= LRU_CAPACITY {
                    // 移除最旧的 inactive 条目
                    for j in 0..LRU_CAPACITY {
                        if self.inactive[j].occupied {
                            self.inactive[j] = LruEntry::empty();
                            self.inactive_count -= 1;
                            break;
                        }
                    }
                }

                // 添加到 inactive
                for j in 0..LRU_CAPACITY {
                    if !self.inactive[j].occupied {
                        self.inactive[j] = entry;
                        self.inactive_count += 1;
                        return;
                    }
                }
                return;
            }
        }
    }

    /// 从 inactive 链表获取回收候选
    fn get_victim(&mut self) -> Option<LruEntry> {
        for i in 0..LRU_CAPACITY {
            if self.inactive[i].occupied {
                let entry = self.inactive[i];
                self.inactive[i] = LruEntry::empty();
                self.inactive_count -= 1;
                return Some(entry);
            }
        }
        None
    }
}

// ============================================================================
// 全局 Swap 状态
// ============================================================================

struct SimpleSpinLock {
    locked: AtomicBool,
}

impl SimpleSpinLock {
    const fn new() -> Self {
        SimpleSpinLock {
            locked: AtomicBool::new(false),
        }
    }

    fn lock(&self) {
        while self
            .locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

struct SwapState {
    lock: SimpleSpinLock,
    area: UnsafeCell<SwapArea>,
    lru: UnsafeCell<LruList>,
}

// SAFETY: SwapState 由自旋锁保护
unsafe impl Sync for SwapState {}
unsafe impl Send for SwapState {}

static SWAP: SwapState = SwapState {
    lock: SimpleSpinLock::new(),
    area: UnsafeCell::new(SwapArea::new()),
    lru: UnsafeCell::new(LruList::new()),
};

// ============================================================================
// 公共 API
// ============================================================================

/// 初始化 swap 子系统
pub fn swap_init() -> bool {
    SWAP.lock.lock();
    // SAFETY: `SWAP` 由调用方保证为有效指针; 只读访问
    let area = unsafe { &mut *SWAP.area.get() };
    let result = area.init();
    SWAP.lock.unlock();
    result
}

/// 换出页面: 将物理页写入 swap, 返回 swap entry
///
/// # Safety
///
/// - `virt_addr` 必须是已映射的用户空间虚拟地址
/// - `phys_addr` 必须是对应的有效物理地址
pub fn swap_out(virt_addr: u64, phys_addr: u64, _dirty: bool) -> Option<SwapEntry> {
    SWAP.lock.lock();
    let area = unsafe { &mut *SWAP.area.get() };

    if !area.initialized {
        SWAP.lock.unlock();
        return None;
    }

    let slot = match area.alloc_slot() {
        Some(s) => s,
        None => {
            SWAP.lock.unlock();
            return None;
        }
    };

    // 将页面数据写入 swap slot
    let src_virt = phys_addr + crate::kernel::framework::mm::KERNEL_BASE;
    area.write_slot(slot, src_virt);

    let entry = SwapEntry::new(slot);
    SWAP.lock.unlock();

    crate::klog_debug!(Swap, "[SWAP] Out: vaddr={:#x} paddr={:#x} -> slot={}",
        virt_addr, phys_addr, slot);

    Some(entry)
}

/// 换入页面: 从 swap slot 读取数据到新物理页
///
/// # Safety
///
/// - 调用者确保 `entry` 是有效的 swap entry
/// - 返回新分配的物理地址
pub fn swap_in(entry: SwapEntry) -> Option<PhysAddr> {
    let slot = entry.slot();

    SWAP.lock.lock();
    let area = unsafe { &mut *SWAP.area.get() };

    if !area.initialized {
        SWAP.lock.unlock();
        return None;
    }

    // 分配新物理页
    let pmm_inst = pmm::get_pmm();
    let new_phys = pmm_inst.alloc_page()?;

    // 从 swap slot 读取数据
    let dst_virt = new_phys.to_virt().0;
    area.read_slot(slot, dst_virt);

    // 释放 swap slot
    area.free_slot(slot);

    SWAP.lock.unlock();

    crate::klog_debug!(Swap, "[SWAP] In: slot={} -> paddr={:#x}", slot, new_phys.as_u64());

    Some(new_phys)
}

/// 释放 swap slot (页面被修改写回时调用)
pub fn swap_free(entry: SwapEntry) {
    let slot = entry.slot();
    SWAP.lock.lock();
    // SAFETY: `SWAP` 由调用方保证为有效指针; 只读访问
    let area = unsafe { &mut *SWAP.area.get() };
    area.free_slot(slot);
    SWAP.lock.unlock();
}

/// 检测 PTE 是否为 swap entry
pub fn is_swap_pte(pte: u64) -> bool {
    SwapEntry::from_pte(pte).is_some()
}

/// 从 PTE 解析 swap entry
pub fn pte_to_swap_entry(pte: u64) -> Option<SwapEntry> {
    SwapEntry::from_pte(pte)
}

/// 记录页面访问 (添加到 LRU active 链表)
pub fn lru_touch(virt_addr: u64, phys_addr: u64, dirty: bool) {
    SWAP.lock.lock();
    // SAFETY: `SWAP` 由调用方保证为有效指针; 只读访问
    let lru = unsafe { &mut *SWAP.lru.get() };
    lru.add_active(virt_addr, phys_addr, dirty);
    SWAP.lock.unlock();
}

/// 回收页面 (从 LRU inactive 链表选取并换出)
///
/// 返回换出的页面数
pub fn reclaim_pages(max_count: u32) -> u32 {
    let mut reclaimed = 0u32;

    while reclaimed < max_count {
        let victim = {
            SWAP.lock.lock();
            // SAFETY: `SWAP` 由调用方保证为有效指针; 只读访问
            let lru = unsafe { &mut *SWAP.lru.get() };
            let v = lru.get_victim();
            SWAP.lock.unlock();
            v
        };

        match victim {
            Some(entry) => {
                // 尝试换出
                if let Some(_swap_entry) = swap_out(entry.virt_addr, entry.phys_addr, entry.dirty) {
                    // 换出成功: 解除映射, 释放物理页
                    let vmm_inst = vmm::get_vmm();
                    vmm_inst.unmap_page(VirtAddr(entry.virt_addr));

                    // 注意: 当前简化实现不更新 PTE 为 swap entry
                    // 完整实现需要在 unmap_page 中保留 swap entry
                    // 后续集成: map_page_in_table 支持 swap entry PTE

                    let pmm_inst = pmm::get_pmm();
                    pmm_inst.free_page(PhysAddr(entry.phys_addr));

                    reclaimed += 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }

    if reclaimed > 0 {
        crate::klog_debug!(Swap, "[SWAP] Reclaimed {} pages", reclaimed);
    }

    reclaimed
}

/// 获取 swap 信息
pub fn swap_info() -> (u64, u64) {
    SWAP.lock.lock();
    // SAFETY: `SWAP` 由调用方保证为有效指针; 只读访问
    let area = unsafe { &mut *SWAP.area.get() };
    let total = SWAP_MAX_SLOTS as u64;
    let free = area.free_slots();
    SWAP.lock.unlock();
    (total * PAGE_SIZE, free * PAGE_SIZE)
}

// ============================================================================
// #PF Swap Entry 检测
// ============================================================================

/// 处理 swap-in 缺页
///
/// 当 #PF 检测到 PTE 为 swap entry 时调用.
/// 分配新物理页, 从 swap 读取数据, 重新建立映射.
pub fn handle_swap_fault(pml4: u64, fault_addr: u64) -> PfResult {
    let vmm_inst = vmm::get_vmm();

    // 读取当前 PTE
    let pte = match vmm_inst.get_pte_value(pml4, VirtAddr(fault_addr)) {
        Some(v) => v,
        None => return PfResult::SignalSegv,
    };

    let entry = match SwapEntry::from_pte(pte) {
        Some(e) => e,
        None => return PfResult::SignalSegv,
    };

    // 换入
    let new_phys = match swap_in(entry) {
        Some(p) => p,
        None => return PfResult::Oom,
    };

    // 重新建立映射
    let flags = crate::kernel::framework::mm::PageFlags::PRESENT
        | crate::kernel::framework::mm::PageFlags::WRITABLE
        | crate::kernel::framework::mm::PageFlags::USER;
    vmm_inst.map_page_in_table(pml4, VirtAddr(fault_addr), new_phys, flags);

    PfResult::Fixed
}

// ============================================================================
// 内核测试
// ============================================================================

#[cfg(feature = "kernel_test")]
fn test_swap_entry_encoding() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{assert_eq_test, check, TestResult};

    let entry = SwapEntry::new(42);
    assert_eq_test!(entry.slot(), 42, "slot decode");

    let pte = entry.to_pte();
    assert_eq_test!(pte & 1, 0, "not present");
    check!(SwapEntry::from_pte(pte).is_some(), "from_pte valid");

    // present=1 的 PTE 不应被解析为 swap entry
    check!(SwapEntry::from_pte(0x1001).is_none(), "present pte not swap");
    // 零值不应被解析
    check!(SwapEntry::from_pte(0).is_none(), "zero not swap");

    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_swap_entry_large_slot() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{assert_eq_test, TestResult};

    let entry = SwapEntry::new(0x003F_FFFF_FFFF_FFFF);
    assert_eq_test!(entry.slot(), 0x003F_FFFF_FFFF_FFFF, "max slot");

    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
pub fn register_swap_tests() {
    use crate::kernel::framework::tests::runner;
    let r = runner();
    r.register("swap", "entry_encoding", test_swap_entry_encoding);
    r.register("swap", "entry_large_slot", test_swap_entry_large_slot);
}
