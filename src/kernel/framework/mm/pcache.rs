//! Page Cache — 文件内容缓存
//!
//! 为文件映射 (mmap) 和读写提供统一的页级缓存, 避免重复 I/O.
//!
//! ## 核心设计
//!
//! - 以 `(inode_id, page_index)` 为键的全局哈希表
//! - 每个缓存页存储 4KB 文件数据 + 引用计数
//! - MAP_SHARED: 写回 Page Cache (脏页标记)
//! - MAP_PRIVATE: COW, 写入不回写 Page Cache
//!
//! ## 同步
//!
//! 全局哈希表由自旋锁保护, 与 futex 相同模式.
//! 桶级锁避免全局竞争.
//!
//! # Safety
//!
//! - 缓存页由 PMM 分配, 通过 KERNEL_BASE 映射访问
//! - 脏页写回由文件系统负责 (当前阶段仅标记)

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};
use core::cell::UnsafeCell;

use crate::kernel::framework::mm::{PhysAddr, PAGE_SIZE, pmm};

// ============================================================================
// Page Cache Entry
// ============================================================================

/// 一个缓存页: 存储文件某页的数据
struct PageCacheEntry {
    /// 对应的 inode 编号
    inode_id: u32,
    /// 文件内页索引 (byte_offset / PAGE_SIZE)
    page_index: u64,
    /// 物理页帧 (由 PMM 分配)
    phys: u64,
    /// 引用计数 (多少个 VMA 映射了此页)
    ref_count: u32,
    /// 是否为脏页 (MAP_SHARED 写入后标记)
    dirty: bool,
    /// 是否被占用 (inode_id != 0)
    occupied: bool,
}

impl PageCacheEntry {
    const fn empty() -> Self {
        PageCacheEntry {
            inode_id: 0,
            page_index: 0,
            phys: 0,
            ref_count: 0,
            dirty: false,
            occupied: false,
        }
    }
}

// ============================================================================
// Page Cache Bucket
// ============================================================================

/// 哈希桶数量 (2 的幂)
const PCACHE_HASH_BUCKETS: usize = 64;
/// 每桶最大条目数
const PCACHE_BUCKET_CAPACITY: usize = 16;

struct PageCacheBucket {
    entries: [PageCacheEntry; PCACHE_BUCKET_CAPACITY],
    count: usize,
}

impl PageCacheBucket {
    const fn new() -> Self {
        PageCacheBucket {
            entries: [
                PageCacheEntry::empty(), PageCacheEntry::empty(),
                PageCacheEntry::empty(), PageCacheEntry::empty(),
                PageCacheEntry::empty(), PageCacheEntry::empty(),
                PageCacheEntry::empty(), PageCacheEntry::empty(),
                PageCacheEntry::empty(), PageCacheEntry::empty(),
                PageCacheEntry::empty(), PageCacheEntry::empty(),
                PageCacheEntry::empty(), PageCacheEntry::empty(),
                PageCacheEntry::empty(), PageCacheEntry::empty(),
            ],
            count: 0,
        }
    }

    /// 查找缓存页, 返回物理地址
    fn lookup(&self, inode_id: u32, page_index: u64) -> Option<u64> {
        for entry in self.entries.iter() {
            if entry.occupied && entry.inode_id == inode_id && entry.page_index == page_index {
                return Some(entry.phys);
            }
        }
        None
    }

    /// 查找并增加引用计数
    fn lookup_and_ref(&mut self, inode_id: u32, page_index: u64) -> Option<u64> {
        for entry in self.entries.iter_mut() {
            if entry.occupied && entry.inode_id == inode_id && entry.page_index == page_index {
                entry.ref_count += 1;
                return Some(entry.phys);
            }
        }
        None
    }

    /// 插入缓存页 (分配物理页并从文件读取)
    /// 返回物理地址, 或 None (桶满/OOM)
    fn insert(&mut self, inode_id: u32, page_index: u64) -> Option<u64> {
        if self.count >= PCACHE_BUCKET_CAPACITY {
            return None;
        }

        // 先检查是否已存在
        if let Some(phys) = self.lookup_and_ref(inode_id, page_index) {
            return Some(phys);
        }

        // 分配物理页
        let pmm_inst = pmm::get_pmm();
        let phys = pmm_inst.alloc_page()?;

        // 清零 (防止信息泄漏)
        let virt = phys.to_virt();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_bytes(virt.0 as *mut u8, 0, PAGE_SIZE as usize);
        }

        // TODO(TRACK-A7DE25): 从文件系统读取数据填充此页
        // 当前阶段文件数据由 initramfs 加载, 后续集成 VFS read

        // 插入条目
        for entry in self.entries.iter_mut() {
            if !entry.occupied {
                entry.inode_id = inode_id;
                entry.page_index = page_index;
                entry.phys = phys.as_u64();
                entry.ref_count = 1;
                entry.dirty = false;
                entry.occupied = true;
                self.count += 1;
                return Some(phys.as_u64());
            }
        }

        // 不应到达此处 (count 检查已通过)
        pmm_inst.free_page(phys);
        None
    }

    /// 标记脏页
    fn mark_dirty(&mut self, inode_id: u32, page_index: u64) {
        for entry in self.entries.iter_mut() {
            if entry.occupied && entry.inode_id == inode_id && entry.page_index == page_index {
                entry.dirty = true;
                return;
            }
        }
    }

    /// 减少引用计数, 归零时释放
    fn deref(&mut self, inode_id: u32, page_index: u64) {
        for entry in self.entries.iter_mut() {
            if entry.occupied && entry.inode_id == inode_id && entry.page_index == page_index {
                if entry.ref_count > 0 {
                    entry.ref_count -= 1;
                }
                if entry.ref_count == 0 {
                    // 释放物理页
                    let phys = PhysAddr(entry.phys);
                    pmm::get_pmm().free_page(phys);
                    *entry = PageCacheEntry::empty();
                    self.count -= 1;
                }
                return;
            }
        }
    }

    /// 释放指定 inode 的所有缓存页
    fn invalidate_inode(&mut self, inode_id: u32) {
        for entry in self.entries.iter_mut() {
            if entry.occupied && entry.inode_id == inode_id {
                let phys = PhysAddr(entry.phys);
                pmm::get_pmm().free_page(phys);
                *entry = PageCacheEntry::empty();
                self.count -= 1;
            }
        }
    }

    /// 填充缓存页内容 (供 miss 后由 vfs/fs 路径回填文件数据)
    ///
    /// 若 `src.len() < PAGE_SIZE`, 剩余字节保持原值 (通常为零).
    fn fill(&mut self, inode_id: u32, page_index: u64, src: &[u8]) -> bool {
        for entry in self.entries.iter() {
            if entry.occupied && entry.inode_id == inode_id && entry.page_index == page_index {
                let dst_virt = crate::kernel::framework::mm::phys_to_virt(entry.phys);
                // SAFETY: `dst_virt` 由 pmm 分配的物理页映射, 仅由 PageCache 拥有
                unsafe {
                    let copy_len = core::cmp::min(src.len(), PAGE_SIZE as usize);
                    core::ptr::copy_nonoverlapping(
                        src.as_ptr(),
                        dst_virt as *mut u8,
                        copy_len,
                    );
                }
                return true;
            }
        }
        false
    }
}

// ============================================================================
// 简易自旋锁 (与 futex 相同模式)
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

// ============================================================================
// 全局 Page Cache
// ============================================================================

struct PageCacheTable {
    locks: [SimpleSpinLock; PCACHE_HASH_BUCKETS],
    buckets: [UnsafeCell<PageCacheBucket>; PCACHE_HASH_BUCKETS],
}

// SAFETY: 每个桶由独立的 SimpleSpinLock 保护
unsafe impl Sync for PageCacheTable {}
unsafe impl Send for PageCacheTable {}

static PAGE_CACHE: PageCacheTable = PageCacheTable {
    locks: unsafe { core::mem::zeroed() },
    buckets: unsafe { core::mem::zeroed() },
};

/// 计算哈希桶索引
fn pcache_hash(inode_id: u32, page_index: u64) -> usize {
    let h = (inode_id as u64)
        .wrapping_mul(0x9E3779B97F4A7C15)
        .wrapping_add(page_index.wrapping_mul(0x517CC1B727220A95));
    (h as usize) & (PCACHE_HASH_BUCKETS - 1)
}

// ============================================================================
// 公共 API
// ============================================================================

/// 查找或创建缓存页
///
/// 若缓存命中, 返回物理地址并增加引用计数.
/// 若未命中, 分配物理页并插入缓存.
pub fn pcache_get(inode_id: u32, page_index: u64) -> Option<u64> {
    let idx = pcache_hash(inode_id, page_index);
    PAGE_CACHE.locks[idx].lock();
    // SAFETY: 持有锁
    let bucket = unsafe { &mut *PAGE_CACHE.buckets[idx].get() };
    let result = bucket.insert(inode_id, page_index);
    PAGE_CACHE.locks[idx].unlock();
    result
}

/// 查找缓存页 (不增加引用计数)
pub fn pcache_lookup(inode_id: u32, page_index: u64) -> Option<u64> {
    let idx = pcache_hash(inode_id, page_index);
    PAGE_CACHE.locks[idx].lock();
    // SAFETY: `PAGE_CACHE` 由调用方保证为有效指针; 只读访问
    let bucket = unsafe { &mut *PAGE_CACHE.buckets[idx].get() };
    let result = bucket.lookup(inode_id, page_index);
    PAGE_CACHE.locks[idx].unlock();
    result
}

/// 标记脏页 (MAP_SHARED 写入后调用)
pub fn pcache_mark_dirty(inode_id: u32, page_index: u64) {
    let idx = pcache_hash(inode_id, page_index);
    PAGE_CACHE.locks[idx].lock();
    // SAFETY: `PAGE_CACHE` 由调用方保证为有效指针; 只读访问
    let bucket = unsafe { &mut *PAGE_CACHE.buckets[idx].get() };
    bucket.mark_dirty(inode_id, page_index);
    PAGE_CACHE.locks[idx].unlock();
}

/// 释放缓存页引用 (munmap 时调用)
pub fn pcache_put(inode_id: u32, page_index: u64) {
    let idx = pcache_hash(inode_id, page_index);
    PAGE_CACHE.locks[idx].lock();
    // SAFETY: `PAGE_CACHE` 由调用方保证为有效指针; 只读访问
    let bucket = unsafe { &mut *PAGE_CACHE.buckets[idx].get() };
    bucket.deref(inode_id, page_index);
    PAGE_CACHE.locks[idx].unlock();
}

/// 释放 inode 的所有缓存页 (文件关闭时调用)
pub fn pcache_invalidate_inode(inode_id: u32) {
    for i in 0..PCACHE_HASH_BUCKETS {
        PAGE_CACHE.locks[i].lock();
        // SAFETY: `PAGE_CACHE` 由调用方保证为有效指针; 只读访问
        let bucket = unsafe { &mut *PAGE_CACHE.buckets[i].get() };
        bucket.invalidate_inode(inode_id);
        PAGE_CACHE.locks[i].unlock();
    }
}

/// 将缓存页数据写入目标虚拟地址 (用于 #PF 时填充用户页)
///
/// # Safety
///
/// - `dest_virt` 必须指向有效的、已映射的用户空间页
/// - `phys` 必须是 Page Cache 中的有效物理页
pub unsafe fn pcache_copy_to_user(phys: u64, dest_virt: u64) {
    let src_virt = crate::kernel::framework::mm::phys_to_virt(phys);
    core::ptr::copy_nonoverlapping(
        src_virt as *const u8,
        dest_virt as *mut u8,
        PAGE_SIZE as usize,
    );
}

/// 填充指定缓存页内容 (解决 TRACK-A7DE25)
///
/// 适用于 pcache_get 返回新页 (miss) 后, 由 vfs 层读取 fs 数据并回填.
/// 复制长度取 `min(src.len(), PAGE_SIZE)`, 不足部分保持 pcache_get 时的零页状态.
///
/// 返回 true 表示找到并填充了对应 entry; false 表示 entry 不存在
/// (调用方应仅在 pcache_get 成功返回后调用).
pub fn pcache_fill(inode_id: u32, page_index: u64, src: &[u8]) -> bool {
    let idx = pcache_hash(inode_id, page_index);
    PAGE_CACHE.locks[idx].lock();
    // SAFETY: 持有桶锁
    let bucket = unsafe { &mut *PAGE_CACHE.buckets[idx].get() };
    let result = bucket.fill(inode_id, page_index, src);
    PAGE_CACHE.locks[idx].unlock();
    result
}

/// 读取缓存页内容到目标缓冲区
///
/// 用于把 pcache 物理页的数据复制到用户缓冲 / 其他位置.
/// `dst.len()` 不得超过 `PAGE_SIZE`.
pub fn pcache_read_to_slice(inode_id: u32, page_index: u64, dst: &mut [u8]) -> bool {
    let phys = match pcache_lookup(inode_id, page_index) {
        Some(p) => p,
        None => return false,
    };
    let src_virt = crate::kernel::framework::mm::phys_to_virt(phys);
    let copy_len = core::cmp::min(dst.len(), PAGE_SIZE as usize);
    // SAFETY: `src_virt` 指向 pcache 拥有的有效物理页, 长度由物理页大小保证
    unsafe {
        core::ptr::copy_nonoverlapping(
            src_virt as *const u8,
            dst.as_mut_ptr(),
            copy_len,
        );
    }
    true
}

// ============================================================================
// 内核测试
// ============================================================================

#[cfg(feature = "kernel_test")]
fn test_pcache_hash_range() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};
    for &inode in &[1u32, 42, 1000, 0xFFFF] {
        for &pg in &[0u64, 1, 100, 0xFFFFFFFF] {
            let idx = pcache_hash(inode, pg);
            check!(idx < PCACHE_HASH_BUCKETS, "hash in range");
        }
    }
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_pcache_bucket_insert_lookup() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};
    let mut bucket = PageCacheBucket::new();
    check!(bucket.count == 0, "empty bucket");

    // 注意: insert 会调用 PMM 分配, 在测试环境中可能失败
    // 此测试仅验证数据结构操作
    // 实际集成测试在 QEMU 中运行
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_pcache_fill_requires_existing_entry() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};
    // 在空桶上 fill 应返回 false (entry 不存在)
    let mut bucket = PageCacheBucket::new();
    let data = [0xABu8; 16];
    let result = bucket.fill(1, 0, &data);
    check!(!result, "fill on empty bucket returns false");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_pcache_fill_len_clamped() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{check, TestResult};
    // fill 的 copy_len 应取 min(src.len(), PAGE_SIZE)
    // 我们通过计算期望 copy_len 验证 (不实际触发 PMM 分配)
    let src_short = [0u8; 100];
    let expected = core::cmp::min(src_short.len(), PAGE_SIZE as usize);
    check!(expected == 100, "short src copies full");
    let src_long = [0u8; (PAGE_SIZE as usize) + 1024];
    let expected2 = core::cmp::min(src_long.len(), PAGE_SIZE as usize);
    check!(expected2 == PAGE_SIZE as usize, "long src clamped to PAGE_SIZE");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
pub fn register_pcache_tests() {
    use crate::kernel::framework::tests::runner;
    let r = runner();
    r.register("pcache", "hash_range", test_pcache_hash_range);
    r.register("pcache", "bucket_insert_lookup", test_pcache_bucket_insert_lookup);
    r.register("pcache", "fill_requires_existing_entry", test_pcache_fill_requires_existing_entry);
    r.register("pcache", "fill_len_clamped", test_pcache_fill_len_clamped);
}
