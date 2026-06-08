//! Slab-Backed kmalloc (小对象 Slab, 大对象堆)
//!
//! 将 Kmalloc 与 Slab 分配器集成:
//!
//! - 分配 ≤ 2048 bytes → Slab 缓存 (O(1), 无碎片)
//! - 分配 > 2048 bytes → 堆分配器 (first-fit)
//!
//! ## SAFETY
//!
//! `SLAB_CACHES` 使用 `[Option<KmemCache>; 8]` 数组存储，每个缓存独立初始化。
//! 如果某个缓存创建失败，对应的数组元素为 `None`，分配时会自动回退到堆分配器。
//! 这种设计避免了未初始化内存访问，确保内存安全。
//!
//! `SLAB_CACHES` 访问由 `SLAB_LOCK` 自旋锁保护。
//! `slab_init()` 在内核早期初始化阶段（单核）调用，因此初始化路径无竞态。

use super::slab::KmemCache;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::klog_error;
use crate::klog_info_simple;

const CACHE_SIZES: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];

/// Slab caches array - using Option to safely handle initialization failures
/// None indicates that cache creation failed and should not be used
static mut SLAB_CACHES: [Option<KmemCache>; 8] = [None, None, None, None, None, None, None, None];
static SLAB_LOCK: AtomicBool = AtomicBool::new(false);
static SLAB_READY: AtomicBool = AtomicBool::new(false);

pub fn slab_init() {
    let mut success_count = 0;
    
    for i in 0..8 {
        let name = match i {
            0 => "kmalloc-16",
            1 => "kmalloc-32",
            2 => "kmalloc-64",
            3 => "kmalloc-128",
            4 => "kmalloc-256",
            5 => "kmalloc-512",
            6 => "kmalloc-1024",
            _ => "kmalloc-2048",
        };
        
        match KmemCache::create(name, CACHE_SIZES[i]) {
            Ok(cache) => {
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe { SLAB_CACHES[i] = Some(cache); }
                success_count += 1;
            }
            Err(_) => {
                klog_error!(
                    "[SLAB] CRITICAL: Failed to create cache {} (size {}). \
                     This size class will fall back to heap allocator.",
                    name,
                    CACHE_SIZES[i]
                );
                // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                unsafe { SLAB_CACHES[i] = None; }
            }
        }
    }
    
    if success_count == 0 {
        klog_error!("[SLAB] CRITICAL: All slab caches failed to initialize!");
    } else {
        klog_info_simple!(
            "[SLAB] Initialized {}/8 caches successfully",
            success_count
        );
    }
    
    SLAB_READY.store(true, Ordering::Release);
}

fn cache_index(size: usize) -> Option<usize> {
    for i in 0..CACHE_SIZES.len() {
        if size <= CACHE_SIZES[i] {
            return Some(i);
        }
    }
    None
}

#[inline(always)]
fn slab_lock() {
    while SLAB_LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn slab_unlock() {
    SLAB_LOCK.store(false, Ordering::Release);
}

pub fn slab_kmalloc(size: usize) -> Option<*mut u8> {
    if size == 0 || !SLAB_READY.load(Ordering::Acquire) {
        return super::kmalloc::get_kmalloc().allocate(size);
    }

    if let Some(idx) = cache_index(size) {
        slab_lock();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        let result = unsafe {
            match &mut SLAB_CACHES[idx] {
                Some(cache) => cache.allocate(),
                None => {
                    // Cache creation failed, fall back to heap allocator
                    super::kmalloc::get_kmalloc().allocate(size)
                }
            }
        };
        slab_unlock();
        result
    } else {
        super::kmalloc::get_kmalloc().allocate(size)
    }
}

pub fn slab_kfree(ptr: *mut u8, size: usize) {
    if ptr.is_null() {
        return;
    }

    if !SLAB_READY.load(Ordering::Acquire) {
        super::kmalloc::get_kmalloc().deallocate(ptr);
        return;
    }

    if let Some(idx) = cache_index(size) {
        slab_lock();
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            match &mut SLAB_CACHES[idx] {
                Some(cache) => cache.deallocate(ptr),
                None => {
                    // Cache creation failed, use heap deallocator
                    super::kmalloc::get_kmalloc().deallocate(ptr);
                }
            }
        }
        slab_unlock();
    } else {
        super::kmalloc::get_kmalloc().deallocate(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_index_selection() {
        assert_eq!(cache_index(8), Some(0));
        assert_eq!(cache_index(16), Some(0));
        assert_eq!(cache_index(20), Some(1));
        assert_eq!(cache_index(100), Some(3));
        assert_eq!(cache_index(2048), Some(7));
        assert_eq!(cache_index(2049), None);
    }
}
