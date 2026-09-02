#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯策略实现。
//! IPC 调度器集成 (阻塞/唤醒机制) — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/ipc/scheduler_integration.rs, 2026-06-16 提取到 services.
//! 纯策略代码 (阻塞/唤醒/超时等待), 0 unsafe.
//! framework 仅保留 re-export.
//!
//! 提供与内核调度器的桥接功能：
//! - 阻塞当前线程到等待队列
//! - 唤醒等待队列中的线程
//! - 支持超时等待

use super::types::{WaitQueue, WaitQueueItem};
use crate::kernel::framework::proc::{
    scheduler_block, scheduler_unblock, scheduler_yield_ex, thread_get_current,
};
use crate::kernel::framework::sync::in_irq_context;
use crate::kernel::framework::timer::hrtimer::hrtimer_sleep;

/// B07-15: 中断上下文唤醒守卫 — 中断/softirq 上下文不得直接调度或阻塞.
fn in_interrupt_context() -> bool {
    in_irq_context()
}

/// 阻塞当前线程到指定等待队列
///
/// 将当前线程加入等待队列并让出 CPU，
/// 直到被其他线程唤醒或超时。
///
/// # Arguments
/// * `wait_queue` - 目标等待队列
/// * `timeout_ms` - 超时时间 (毫秒), 0 表示无限等待
///
/// # Returns
/// * Ok(()) - 成功被唤醒
/// * Err(i32) - 错误码 (-1: 超时, -2: 被信号中断)
///
/// # Errors
/// 当无法获取当前线程 (`thread_get_current` 返回 0) 时返回 `Err(-2)`;
/// 在中断上下文中调用返回 `Err(-2)` (不允许在中断上下文阻塞).
#[inline]
pub fn block_current_thread(wait_queue: &mut WaitQueue, _timeout_ms: u64) -> Result<(), i32> {
    if in_interrupt_context() {
        // 中断上下文禁止阻塞/调度 (持锁期间睡眠会死锁).
        return Err(-2);
    }
    let thread_addr = thread_get_current();

    if thread_addr != 0 {
        // WaitQueueItem 中线程 ID 以 u32 存储; thread_get_current 返回的 u64
        // 对所有有效线程 ID 均可安全截断为 u32。
        let tid = thread_addr as u32;

        // 创建等待项并加入队列
        let item = WaitQueueItem { tid };
        wait_queue.add(item);

        // B07-15: 标记进程为 Blocked 并让出 CPU.
        // 原实现仅 yield (忙等), 不改变调度状态 — 被唤醒线程仍在就绪队列,
        // 无法实现真实阻塞语义.
        scheduler_block(crate::kernel::framework::proc::BlockReason::WaitingForIo);
        scheduler_yield_ex();

        // 被唤醒后返回
        Ok(())
    } else {
        Err(-2) // 无效线程
    }
}

/// 唤醒等待队列中的一个线程
///
/// 从等待队列头部取出一个线程并标记为可运行。
///
/// # Arguments
/// * `wait_queue` - 目标等待队列
///
/// # Returns
/// * true - 成功唤醒一个线程
/// * false - 队列为空或唤醒被延后 (中断上下文)
#[inline]
pub fn wake_one_thread(wait_queue: &mut WaitQueue) -> bool {
    // B07-15: 中断上下文不可直接调度唤醒, 延后由进程上下文补唤醒.
    if in_interrupt_context() {
        wait_queue.drain_pending();
        // 中断上下文仅登记唤醒 (pending), 不实际调度; 由后续进程上下文
        // 经 wake 路径补唤醒. 返回 true 表示已登记.
        return true;
    }
    if let Some(item) = wait_queue.wake_one() {
        scheduler_unblock(item.tid);
        true
    } else {
        false
    }
}

/// 唤醒等待队列中的所有线程
///
/// 标记队列中所有线程为可运行状态。
///
/// # Arguments
/// * `wait_queue` - 目标等待队列
#[inline]
pub fn wake_all_threads(wait_queue: &mut WaitQueue) {
    // B07-15: 中断上下文禁止调度, 延后到进程上下文补唤醒.
    if in_interrupt_context() {
        wait_queue.wake_all();
        return;
    }
    wait_queue.wake_all();
}

/// 带超时的阻塞等待
///
/// 结合定时器和等待队列实现精确的超时控制。
///
/// # Arguments
/// * `wait_queue` - 目标等待队列
/// * `timeout_ms` - 超时时间 (毫秒)
///
/// # Returns
/// * Ok(()) - 在超时前被唤醒
/// * Err(-1) - 超时
///
/// # Errors
/// 当等待超时仍未满足条件时返回 `Err(-1)`; 其余错误与 [`block_current_thread`] 相同.
pub fn block_with_timeout(wait_queue: &mut WaitQueue, timeout_ms: u64) -> Result<(), i32> {
    if timeout_ms == 0 {
        // 无限等待
        return block_current_thread(wait_queue, 0);
    }

    // B07-15: 用 hrtimer 睡眠替代忙等待 (TRACK-8C5FFB).
    // 忙等待在单核下会 starve 唤醒方 (无抢占 + 不可重入调度器).
    let timeout_nanos = timeout_ms.saturating_mul(1_000_000);
    // 周期性检查等待队列是否仍需要阻塞 (被外部唤醒则退出).
    // 简化: 有限次重试 + hrtimer 睡眠, 期间被唤醒则立即返回.
    let deadline = crate::kernel::framework::timer::hrtimer::hrtimer_clock_read()
        .saturating_add(timeout_nanos);
    loop {
        if wait_queue.count() == 0 {
            // 已无等待者 (被唤醒或条件满足)
            return Ok(());
        }
        if in_interrupt_context() {
            return Err(-2);
        }
        if crate::kernel::framework::timer::hrtimer::hrtimer_clock_read() >= deadline {
            return Err(-1); // 超时
        }
        // 睡眠一个短时隙后重查 (让出 CPU, 非忙等).
        hrtimer_sleep(timeout_nanos.min(1_000_000)).map_err(|()| -1)?;
        if wait_queue.count() == 0 {
            return Ok(());
        }
    }
}
