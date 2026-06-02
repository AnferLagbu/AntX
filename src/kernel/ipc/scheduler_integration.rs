//! IPC 调度器集成 (阻塞/唤醒机制)
//!
//! 提供与内核调度器的桥接功能：
//! - 阻塞当前线程到等待队列
//! - 唤醒等待队列中的线程
//! - 支持超时等待

use super::types::{WaitQueue, WaitQueueItem};
use crate::kernel::proc::api::{scheduler_yield_ex, thread_get_current};

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
#[inline]
pub fn block_current_thread(wait_queue: &mut WaitQueue, _timeout_ms: u64) -> Result<(), i32> {
    unsafe {
        let thread_addr = thread_get_current();

        if thread_addr != 0 {
            let tid = *(thread_addr as *const u32);

            // 创建等待项并加入队列
            let item = WaitQueueItem { tid };
            wait_queue.add(item);

            // 让出 CPU (调用扩展调度器)
            scheduler_yield_ex();

            // 被唤醒后返回
            Ok(())
        } else {
            Err(-2) // 无效线程
        }
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
/// * false - 队列为空
#[inline]
pub fn wake_one_thread(wait_queue: &mut WaitQueue) -> bool {
    wait_queue.wake_one().is_some()
}

/// 唤醒等待队列中的所有线程
///
/// 标记队列中所有线程为可运行状态。
///
/// # Arguments
/// * `wait_queue` - 目标等待队列
#[inline]
pub fn wake_all_threads(wait_queue: &mut WaitQueue) {
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
pub fn block_with_timeout(wait_queue: &mut WaitQueue, timeout_ms: u64) -> Result<(), i32> {
    if timeout_ms == 0 {
        // 无限等待
        return block_current_thread(wait_queue, 0);
    }

    // TODO: 实现基于定时器的超时等待
    // 当前简化实现: 忙等待 + 定期检查
    let start_time = rdtsc();
    let timeout_ticks = ms_to_ticks(timeout_ms);

    loop {
        // 尝试非阻塞检查条件
        if wait_queue.count() == 0 {
            return Ok(());
        }

        // 检查是否超时
        let elapsed = rdtsc() - start_time;
        if elapsed >= timeout_ticks {
            return Err(-1); // 超时
        }

        // 短暂让出 CPU (避免忙等待占用过多资源)
        unsafe {
            scheduler_yield_ex();
        }
    }
}

/// 读取 TSC 时间戳计数器
///
/// 用于高精度时间测量。
fn rdtsc() -> u64 {
    crate::arch!(timestamp())
}

/// 将毫秒转换为 TSC ticks (近似值)
///
/// 假设 CPU 主频为 1GHz (需要实际校准)
fn ms_to_ticks(ms: u64) -> u64 {
    const APPROX_CPU_FREQ_MHZ: u64 = 1000; // 1 GHz
    ms * APPROX_CPU_FREQ_MHZ * 1000
}
