//! Memory Pressure Detection — 预测式内存管理
//!
//! 在 OOM 发生之前, 根据内存压力动态调整系统行为。
//!
//! ## 压力级别
//!
//! | 级别 | 阈值 | 动作 |
//! |------|------|------|
//! | Normal | >25% | 正常运行 |
//! | Warning | 10–25% | 通知进程释放 page cache |
//! | Critical | 3–10% | Top-3 RSS 进程降优先级, 阻塞新 mmap |
//! | Emergency | <3% | SIGTERM → 最大 RSS 进程, 5s 未释放 → SIGKILL |

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MemoryPressure {
    Normal = 0,
    Warning = 1,
    Critical = 2,
    Emergency = 3,
}

impl MemoryPressure {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Warning,
            2 => Self::Critical,
            3 => Self::Emergency,
            _ => Self::Normal,
        }
    }

    pub fn is_critical(&self) -> bool {
        matches!(self, Self::Critical | Self::Emergency)
    }

    pub fn is_emergency(&self) -> bool {
        matches!(self, Self::Emergency)
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Emergency => "emergency",
        }
    }
}

static CURRENT_PRESSURE: AtomicU8 = AtomicU8::new(MemoryPressure::Normal as u8);
static FREE_PAGES_THRESHOLD_WARNING: AtomicU64 = AtomicU64::new(256);
static FREE_PAGES_THRESHOLD_CRITICAL: AtomicU64 = AtomicU64::new(64);
static FREE_PAGES_THRESHOLD_EMERGENCY: AtomicU64 = AtomicU64::new(16);

pub fn set_thresholds(warning: u64, critical: u64, emergency: u64) {
    if warning > critical && critical > emergency {
        FREE_PAGES_THRESHOLD_WARNING.store(warning, Ordering::Release);
        FREE_PAGES_THRESHOLD_CRITICAL.store(critical, Ordering::Release);
        FREE_PAGES_THRESHOLD_EMERGENCY.store(emergency, Ordering::Release);
    }
}

pub fn current_pressure() -> MemoryPressure {
    MemoryPressure::from_u8(CURRENT_PRESSURE.load(Ordering::SeqCst))
}

pub fn update_pressure(free_pages: u64, total_pages: u64) -> MemoryPressure {
    let ratio = if total_pages > 0 {
        free_pages * 100 / total_pages
    } else {
        100
    };

    let (thr_warn, thr_crit, thr_emer) = (
        FREE_PAGES_THRESHOLD_WARNING.load(Ordering::Acquire),
        FREE_PAGES_THRESHOLD_CRITICAL.load(Ordering::Acquire),
        FREE_PAGES_THRESHOLD_EMERGENCY.load(Ordering::Acquire),
    );

    let new_pressure = if free_pages <= thr_emer || ratio <= 3 {
        MemoryPressure::Emergency
    } else if free_pages <= thr_crit || ratio <= 10 {
        MemoryPressure::Critical
    } else if free_pages <= thr_warn || ratio <= 25 {
        MemoryPressure::Warning
    } else {
        MemoryPressure::Normal
    };

    let prev = CURRENT_PRESSURE.swap(new_pressure as u8, Ordering::SeqCst);
    let prev_pressure = MemoryPressure::from_u8(prev);

    if prev_pressure != new_pressure {
        crate::klog_ffi!(klog_ffi_warn,
            "[PRESSURE] {} → {} (free={}, total={}, {}%)",
            prev_pressure.description(), new_pressure.description(),
            free_pages, total_pages, ratio);
    }

    new_pressure
}

pub fn is_pressure_critical() -> bool {
    current_pressure().is_critical()
}

pub fn is_pressure_emergency() -> bool {
    current_pressure().is_emergency()
}
