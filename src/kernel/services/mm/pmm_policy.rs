#![deny(unsafe_code)]
//! PMM 策略 — services 层
//!
//! ## 框架责任分离
//!
//! - **framework**: buddy 位图操作、物理页分配/释放、锁操作 (机制)
//! - **services** (本模块): 阶数选择、碎片化评估、回收阈值、水位线 (策略)
//!
//! ## 策略表
//!
//! | 策略 | 默认行为 | 可调参数 |
//! |------|---------|---------|
//! | count_to_order | 向上取整到 2^n | max_order |
//! | fragmentation_score | (1-free_ratio)×0.7 + fail_ratio×0.3 | 权重 |
//! | reclaim_threshold | max(total×10%, 64) | 百分比/最小值 |
//! | watermarks | min=1.25%×total, low=1.5×min, high=2×min | 比例系数 |
//!
//! ## 关联
//!
//! - T2-2: PMM 策略提取 (2026-06-19)
//! - 互补: alloc_trait::FrameAllocDecision (分配前决策)

use crate::kernel::framework::mm::pmm_trait::{PmmPolicy, PmmPolicyContext, Watermarks};

// ============================================================================
// 默认 PMM 策略 — 标准 buddy 分配器行为
// ============================================================================

/// 默认 PMM 策略 — 与原 framework/mm/pmm.rs 硬编码行为一致
///
/// 在 `services::mm::init()` 中通过 `register_pmm_policy()` 注册.
pub struct DefaultPmmPolicy;

impl PmmPolicy for DefaultPmmPolicy {
    /// Buddy 阶数选择: 向上取整到 2^n
    ///
    /// 阶数 = ceil(log2(count)) = floor(log2(count-1)) + 1
    /// 受 max_order 约束 (当前为 9, 即 2MB).
    fn count_to_order(&self, count: usize, max_order: u8) -> u8 {
        if count <= 1 {
            return 0;
        }
        let order = (usize::BITS - (count - 1).leading_zeros()) as u8;
        if order > max_order {
            max_order
        } else {
            order
        }
    }

    /// 碎片化评估: 综合空闲比例和分配失败率
    ///
    /// 评分公式: (1 - free_ratio) × 0.7 + fail_ratio × 0.3
    /// - 空闲比例低 → 高碎片化 (权重 0.7)
    /// - 分配失败率高 → 高碎片化 (权重 0.3)
    fn fragmentation_score(&self, ctx: PmmPolicyContext) -> f64 {
        if ctx.total_pages == 0 {
            return 0.0;
        }
        let free_ratio = ctx.free_pages as f64 / ctx.total_pages as f64;
        let fail_ratio = if ctx.total_allocs > 0 {
            ctx.failed_allocs as f64 / ctx.total_allocs as f64
        } else {
            0.0
        };
        (1.0 - free_ratio) * 0.7 + fail_ratio * 0.3
    }

    /// 回收阈值: 当空闲页低于 max(total×10%, 64) 时触发 kswapd
    fn reclaim_threshold_pages(&self, total_pages: u64) -> u64 {
        (total_pages * 10 / 100).max(64)
    }

    /// 水位线计算: Linux 风格三级阈值
    ///
    /// - min ≈ 1.25% × total (最低 16 页)
    /// - low = 1.5 × min
    /// - high = 2 × min
    fn watermarks(&self, total_pages: u64) -> Watermarks {
        let min = (total_pages * 125 / 10000).max(16);
        let low = min * 3 / 2;
        let high = min * 2;
        Watermarks { high, low, min }
    }
}

/// 注册默认 PMM 策略到 framework
///
/// 由 `services::mm::init()` 调用. 只能注册一次.
pub fn register_default_pmm_policy() -> Result<(), ()> {
    static POLICY: DefaultPmmPolicy = DefaultPmmPolicy;
    crate::kernel::framework::mm::register_pmm_policy(&POLICY).map_err(|_| ())
}
