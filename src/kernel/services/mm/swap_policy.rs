#![deny(unsafe_code)]
//! Swap 策略 — services 层
//!
//! ## 框架责任分离
//!
//! - **framework**: swap 区 I/O, slot 分配/释放, PTE 操作, 页面换出/换入 (机制)
//! - **services** (本模块): LRU 管理决策, 回收决策, kswapd 触发策略 (策略)
//!
//! ## 策略表
//!
//! | 策略 | 默认行为 | 可调参数 |
//! |------|---------|---------|
//! | reclaim_batch_size | 8 页/次 | 批量大小 |
//! | should_wakeup_kswapd | free<10% 或 swap>80% | 阈值 |
//! | should_demote_active | active >= capacity | 容量 |
//! | should_evict_inactive | inactive >= capacity | 容量 |
//! | select_victim | 首个非 locked 条目 | 选择算法 |
//!
//! ## 关联
//!
//! - T2-4: Swap 策略完整迁移 (2026-06-19)
//! - 互补: pmm_trait::PmmPolicy (物理页分配策略)

use crate::kernel::framework::mm::swap_trait::{
    SwapPolicy, SwapPolicyContext, LruPageInfo,
};

// ============================================================================
// 默认 Swap 策略 — 标准 swap 行为
// ============================================================================

/// 默认 Swap 策略 — 与原 framework/mm/swap.rs 硬编码行为一致
///
/// 在 `services::mm::init()` 中通过 `register_swap_policy()` 注册.
pub struct DefaultSwapPolicy;

impl SwapPolicy for DefaultSwapPolicy {
    /// kswapd 每次唤醒回收 8 页
    fn reclaim_batch_size(&self, _ctx: SwapPolicyContext) -> u32 {
        8
    }

    /// 唤醒条件: 空闲页 < 10% 或 swap 使用率 > 80%
    fn should_wakeup_kswapd(&self, ctx: SwapPolicyContext) -> bool {
        let free_ratio = if ctx.total_pages > 0 {
            ctx.free_pages as f64 / ctx.total_pages as f64
        } else {
            1.0
        };
        let swap_usage = if ctx.total_slots > 0 {
            ctx.used_slots as f64 / ctx.total_slots as f64
        } else {
            0.0
        };
        free_ratio < 0.1 || swap_usage > 0.8
    }

    /// active 链表满时降级最旧条目
    fn should_demote_active(&self, active_count: usize, capacity: usize) -> bool {
        active_count >= capacity
    }

    /// inactive 链表满时丢弃最旧非锁定条目
    fn should_evict_inactive(&self, inactive_count: usize, capacity: usize) -> bool {
        inactive_count >= capacity
    }

    /// 选择第一个非 locked 的 inactive 条目作为回收候选
    fn select_victim(&self, entries: &[Option<LruPageInfo>]) -> Option<usize> {
        for (i, entry) in entries.iter().enumerate() {
            if let Some(e) = entry {
                if !e.locked {
                    return Some(i);
                }
            }
        }
        None
    }
}

/// 注册默认 Swap 策略到 framework
///
/// 由 `services::mm::init()` 调用. 只能注册一次.
pub fn register_default_swap_policy() -> Result<(), ()> {
    static POLICY: DefaultSwapPolicy = DefaultSwapPolicy;
    crate::kernel::framework::mm::register_swap_policy(&POLICY).map_err(|_| ())
}
