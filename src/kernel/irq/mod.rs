//! 中断底部半 (Bottom-Half / Softirq) 机制
//!
//! 在硬中断退出时延迟执行非关键处理，减少中断禁用时间。
//! 参考 Linux softirq + tasklet 设计，但保持极简。
//!
//! ## 架构
//!
//! ```text
//! Hardware IRQ
//!   → hardirq_handler()       (快速路径: ACK/EOI, 关键数据搬运)
//!   → raise_softirq()         (标记 pending bit)
//!   → send_eoi()
//!   → do_softirq()            (开中断执行延后处理)
//!       ├── Softirq::Timer    → 定时器账本更新
//!       ├── Softirq::NetRx    → 网络包提交上层
//!       ├── Softirq::NetTx    → 网络发送完成回收
//!       ├── Softirq::Block    → 块设备 IO 完成
//!       └── Softirq::Tasklet  → 通用 tasklet
//! ```
//!
//! ## 安全性
//!
//! - `do_softirq()` 在开中断环境下运行，可被硬中断抢占
//! - `running` 标志防止重入
//! - handlers 在 `open_softirq()` 时一次性注册，运行时只读

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const MAX_SOFTIRQS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SoftirqVec {
    High = 0,
    Timer = 1,
    NetRx = 2,
    NetTx = 3,
    Block = 4,
    Tasklet = 5,
    Sched = 6,
    Count = 7,
}

impl SoftirqVec {
    #[inline]
    pub const fn to_idx(self) -> usize {
        self as usize
    }

    #[inline]
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::High),
            1 => Some(Self::Timer),
            2 => Some(Self::NetRx),
            3 => Some(Self::NetTx),
            4 => Some(Self::Block),
            5 => Some(Self::Tasklet),
            6 => Some(Self::Sched),
            _ => None,
        }
    }
}

pub type SoftirqHandler = fn();

struct SoftirqState {
    pending: AtomicU64,
    handlers: UnsafeCell<[Option<SoftirqHandler>; MAX_SOFTIRQS]>,
    running: AtomicBool,
}

unsafe impl Sync for SoftirqState {}

static SOFTIRQ: SoftirqState = SoftirqState {
    pending: AtomicU64::new(0),
    handlers: UnsafeCell::new([None; MAX_SOFTIRQS]),
    running: AtomicBool::new(false),
};

pub fn open_softirq(nr: SoftirqVec, handler: SoftirqHandler) {
    let handlers = unsafe { &mut *SOFTIRQ.handlers.get() };
    handlers[nr.to_idx()] = Some(handler);
}

#[inline]
pub fn raise_softirq(nr: SoftirqVec) {
    SOFTIRQ.pending.fetch_or(1u64 << nr.to_idx(), Ordering::Release);
}

#[inline]
pub fn raise_softirq_mask(mask: u64) {
    SOFTIRQ.pending.fetch_or(mask, Ordering::Release);
}

pub fn do_softirq() {
    if SOFTIRQ
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return;
    }

    let handlers = unsafe { &*SOFTIRQ.handlers.get() };

    loop {
        let pending = SOFTIRQ.pending.swap(0, Ordering::AcqRel);
        if pending == 0 {
            break;
        }

        crate::arch!(interrupt_enable());

        for i in 0..MAX_SOFTIRQS {
            let bit = 1u64 << i;
            if pending & bit != 0 {
                if let Some(handler) = handlers[i] {
                    handler();
                }
            }
        }

        crate::arch!(interrupt_disable());
    }

    SOFTIRQ.running.store(false, Ordering::Release);
}

#[inline]
pub fn in_softirq() -> bool {
    SOFTIRQ.running.load(Ordering::Acquire)
}

#[inline]
pub fn pending_softirq() -> bool {
    SOFTIRQ.pending.load(Ordering::Acquire) != 0
}

#[no_mangle]
pub extern "C" fn softirq_init() {
    // Default: no handlers registered. Subsystems call open_softirq() at init.
}

#[no_mangle]
pub extern "C" fn softirq_do() {
    do_softirq();
}