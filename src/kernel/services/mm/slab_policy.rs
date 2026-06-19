#![deny(unsafe_code)]
//! Slab 策略 — services 层
//!
//! ## 框架责任分离
//!
//! - **framework**: slab 页面分配/释放, 位图操作, 链表管理, 锁操作 (机制)
//! - **services** (本模块): 缓存大小选择, 对象数计算, 分配优先级, 大小限制 (策略)
//!
//! ## 策略表
//!
//! | 策略 | 默认行为 | 可调参数 |
//! |------|---------|---------|
//! | find_cache_index | 线性扫描 cache_sizes, 取首个 >= size | cache_sizes 数组 |
//! | calculate_objects_per_slab | (slab_size - header - bitmap) / obj_size | slab_size |
//! | select_alloc_source | partial → free → new | 优先级顺序 |
//! | normalize_object_size | max(requested, MIN_SIZE), 限制 MAX_SIZE | MIN/MAX |
//!
//! ## 关联
//!
//! - T2-3: Slab 策略提取 (2026-06-19)
//! - 互补: pmm_trait::PmmPolicy (物理页分配策略)

use crate::kernel::framework::mm::slab_trait::{
    SlabPolicy, SlabPolicyContext, SlabAllocSource,
};
use crate::kernel::framework::config::{
    SLAB_MIN_OBJECT_SIZE, SLAB_MAX_OBJECT_SIZE,
};

// ============================================================================
// 默认 Slab 策略 — 标准 slab 分配器行为
// ============================================================================

/// 默认 Slab 策略 — 与原 framework/mm/slab.rs 硬编码行为一致
///
/// 在 `services::mm::init()` 中通过 `register_slab_policy()` 注册.
pub struct DefaultSlabPolicy;

impl SlabPolicy for DefaultSlabPolicy {
    /// 缓存大小选择: 线性扫描, 取首个 >= 请求大小的缓存
    fn find_cache_index(&self, size: usize, cache_sizes: &[usize]) -> Option<usize> {
        for (i, &cache_size) in cache_sizes.iter().enumerate() {
            if size <= cache_size {
                return Some(i);
            }
        }
        None
    }

    /// 计算每个 Slab 可容纳的对象数
    ///
    /// 公式: (slab_size - header_size - bitmap_bytes) / object_size
    /// 其中 bitmap_bytes = estimated_objects.div_ceil(8)
    fn calculate_objects_per_slab(
        &self,
        slab_size: usize,
        header_size: usize,
        object_size: usize,
    ) -> u32 {
        let usable_space = slab_size - header_size;
        let estimated_objects = usable_space / object_size;
        let bitmap_bytes = estimated_objects.div_ceil(8);
        let actual_usable = usable_space - bitmap_bytes;
        (actual_usable / object_size) as u32
    }

    /// Slab 选择策略: partial → free → new
    ///
    /// 优先从 partial 链表分配 (减少碎片),
    /// 其次从 free 链表分配 (复用空闲 Slab),
    /// 最后新建 Slab (扩展缓存).
    fn select_alloc_source(&self, ctx: SlabPolicyContext) -> SlabAllocSource {
        if ctx.partial_slabs > 0 {
            SlabAllocSource::Partial
        } else if ctx.free_slabs > 0 {
            SlabAllocSource::Free
        } else {
            SlabAllocSource::NewSlab
        }
    }

    /// 对象大小规范化
    ///
    /// - 请求 0 或超过 MAX → None (回退堆分配器)
    /// - 请求 < MIN → 提升到 MIN (16 字节)
    fn normalize_object_size(&self, requested_size: usize) -> Option<usize> {
        if requested_size == 0 || requested_size > SLAB_MAX_OBJECT_SIZE {
            return None;
        }
        Some(requested_size.max(SLAB_MIN_OBJECT_SIZE))
    }
}

/// 注册默认 Slab 策略到 framework
///
/// 由 `services::mm::init()` 调用. 只能注册一次.
pub fn register_default_slab_policy() -> Result<(), ()> {
    static POLICY: DefaultSlabPolicy = DefaultSlabPolicy;
    crate::kernel::framework::mm::register_slab_policy(&POLICY).map_err(|_| ())
}
