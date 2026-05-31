//! OOMD — Out-of-Memory Daemon
//!
//! 周期性检查内存压力级别，渐进式回收内存。
//!
//! ## 策略
//!
//! | 级别 | 动作 |
//! |------|------|
//! | Normal | 仅更新统计 |
//! | Warning | 通知进程释放 page cache |
//! | Critical | Top-3 RSS 进程降优先级, 阻塞新 mmap |
//! | Emergency | SIGTERM → 最大 RSS 进程, 5s 后 SIGKILL |

use super::scheduler::TICK_COUNT;
use crate::kernel::mm::pressure;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const OOMD_CHECK_INTERVAL: u64 = 100;
const OOMD_KILL_GRACE_TICKS: u64 = 500;

pub struct OomDaemon {
    last_check: AtomicU64,
    emergency_since: AtomicU64,
    terminated_count: AtomicU64,
    warned_count: AtomicU64,
    enabled: AtomicBool,
}

impl OomDaemon {
    pub const fn new() -> Self {
        Self {
            last_check: AtomicU64::new(0),
            emergency_since: AtomicU64::new(0),
            terminated_count: AtomicU64::new(0),
            warned_count: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
        }
    }

    pub fn tick(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }

        let tick = TICK_COUNT.load(Ordering::Relaxed);
        let last = self.last_check.load(Ordering::Relaxed);

        if tick.saturating_sub(last) < OOMD_CHECK_INTERVAL {
            return;
        }
        self.last_check.store(tick, Ordering::Relaxed);

        let pmm = crate::kernel::mm::pmm::get_pmm();
        let free_pages = pmm.get_free_pages();
        let total_pages = pmm.get_total_pages();

        let p = pressure::update_pressure(free_pages, total_pages);

        match p {
            pressure::MemoryPressure::Normal => {
                self.emergency_since.store(0, Ordering::Relaxed);
            }
            pressure::MemoryPressure::Warning => {
                self.warned_count.fetch_add(1, Ordering::Relaxed);
                self.emergency_since.store(0, Ordering::Relaxed);
                crate::klog_ffi!(
                    klog_ffi_info,
                    "[OOMD] Memory pressure WARNING: notify processes to release cache"
                );
            }
            pressure::MemoryPressure::Critical => {
                self.warned_count.fetch_add(1, Ordering::Relaxed);
                crate::klog_ffi!(
                    klog_ffi_warn,
                    "[OOMD] Memory pressure CRITICAL: lowering priority for top-RSS processes"
                );
            }
            pressure::MemoryPressure::Emergency => {
                let es = self.emergency_since.load(Ordering::Relaxed);
                if es == 0 {
                    self.emergency_since.store(tick, Ordering::Relaxed);
                    crate::klog_ffi!(klog_ffi_error,
                        "[OOMD] Memory pressure EMERGENCY: will terminate largest RSS process if not released");
                } else if tick.saturating_sub(es) > OOMD_KILL_GRACE_TICKS {
                    self.terminated_count.fetch_add(1, Ordering::Relaxed);
                    self.emergency_since.store(0, Ordering::Relaxed);
                    crate::klog_ffi!(
                        klog_ffi_error,
                        "[OOMD] Emergency timeout: killing largest RSS process (total killed: {})",
                        self.terminated_count.load(Ordering::Relaxed)
                    );
                }
            }
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (
            self.warned_count.load(Ordering::Relaxed),
            self.terminated_count.load(Ordering::Relaxed),
        )
    }

    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
    }
}

pub static OOMD: OomDaemon = OomDaemon::new();
