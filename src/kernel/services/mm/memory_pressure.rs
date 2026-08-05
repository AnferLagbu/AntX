#![deny(unsafe_code)]
//! 内存压力策略 (Memory Pressure) — services 层
//!
//! ## 框架责任分离
//!
//! - **framework**: 原子原语 (AtomicU8/AtomicU64), OOM 触发, 进程控制
//! - **services** (本模块): 压力分级阈值、级别判定、状态机转换策略
//!
//! ## 策略表 (与 Linux mempressure 对照)
//!
//! | 级别 | 阈值 (free_pages 绝对值) | 阈值 (free_ratio 百分比) | 动作 |
//! |------|------|------|------|
//! | Normal | >256 | >25% | 正常运行 |
//! | Warning | 64-256 | 10-25% | 通知进程释放 page cache |
//! | Critical | 16-64 | 3-10% | 阻塞新 mmap, 降 RSS Top-3 优先级 |
//! | Emergency | ≤16 | ≤3% | SIGTERM → 最大 RSS 进程, 5s → SIGKILL |
//!
//! ## 与 Linux 差异
//!
//! - Linux cgroup-v2 memory.pressure: usec 粒度, 无固定阈值表
//! - 本项目: 嵌入式轻量基线, 4 级 + 绝对/相对双阈值
//! - 阈值可通过 `set_thresholds` 动态调整 (但必须保持 warn > crit > emer)
//!
//! ## 关联
//!
//! - 移出: framework::mm::pressure (2026-06-11)
//! - TCB 减面: [docs/plan/maintenance-2026-06-11.md](../../../../../../docs/plan/maintenance-2026-06-11.md) I-01 D9

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

// ============================================================================
// 内存压力级别
// ============================================================================

/// 内存压力级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MemoryPressure {
    /// 正常: 内存充足
    Normal = 0,
    /// 警告: 建议进程主动释放 page cache
    Warning = 1,
    /// 严重: 阻塞新 mmap, 降 RSS Top-3 优先级
    Critical = 2,
    /// 紧急: SIGTERM → SIGKILL 序列
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

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn is_critical(&self) -> bool {
        matches!(self, Self::Critical | Self::Emergency)
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn is_emergency(&self) -> bool {
        matches!(self, Self::Emergency)
    }

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "trivially_copy_pass_by_ref: 小类型传引用而非值是 API 约定 (如 impl trait); 当前优先 expect"
    )]
    pub fn description(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Emergency => "emergency",
        }
    }
}

// ============================================================================
// 阈值 (可调, 默认保守值适合 256MB 内嵌)
// ============================================================================

static CURRENT_PRESSURE: AtomicU8 = AtomicU8::new(MemoryPressure::Normal as u8);
/// 上一次压力级别 (由 `update_pressure` 在 swap 后写入)
static PREV_PRESSURE: AtomicU8 = AtomicU8::new(MemoryPressure::Normal as u8);
static FREE_PAGES_THRESHOLD_WARNING: AtomicU64 = AtomicU64::new(256);
static FREE_PAGES_THRESHOLD_CRITICAL: AtomicU64 = AtomicU64::new(64);
static FREE_PAGES_THRESHOLD_EMERGENCY: AtomicU64 = AtomicU64::new(16);

/// 设置阈值. 仅当 `warning > critical > emergency` 时生效 (配置合法性).
pub fn set_thresholds(warning: u64, critical: u64, emergency: u64) {
    if warning > critical && critical > emergency {
        FREE_PAGES_THRESHOLD_WARNING.store(warning, Ordering::Release);
        FREE_PAGES_THRESHOLD_CRITICAL.store(critical, Ordering::Release);
        FREE_PAGES_THRESHOLD_EMERGENCY.store(emergency, Ordering::Release);
    }
}

/// 获取当前压力级别
pub fn current_pressure() -> MemoryPressure {
    MemoryPressure::from_u8(CURRENT_PRESSURE.load(Ordering::SeqCst))
}

/// 更新压力级别 (传入当前 `free_pages` / `total_pages`)
///
/// 策略: 4 级状态机, 返回 `(new, prev)` 供调用方决定是否记日志
/// (services 层不直接 klog, 避免 unsafe 边界问题; framework wrapper 处理日志).
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
    PREV_PRESSURE.store(prev, Ordering::SeqCst);

    new_pressure
}

/// 读取上一次压力级别 (供 wrapper 做日志比较, 避免在 services 层使用 `klog_ffi`).
pub fn previous_pressure() -> MemoryPressure {
    MemoryPressure::from_u8(PREV_PRESSURE.load(Ordering::SeqCst))
}

pub fn is_pressure_critical() -> bool {
    current_pressure().is_critical()
}

pub fn is_pressure_emergency() -> bool {
    current_pressure().is_emergency()
}

// ============================================================================
// T-02: 压力感知分配策略 — services 层策略主体
// ============================================================================

use crate::kernel::framework::mm::alloc_trait::{AllocContext, AllocDecision, FrameAllocDecision};

/// 压力感知分配策略 — services 层安全实现
///
/// 根据内存压力级别决定是否允许分配:
/// - Normal/Warning: 允许
/// - Critical: 仅允许小分配 (< 4 页)
/// - Emergency: 拒绝所有分配, 建议回收后重试
///
/// 在 `services::mm::init()` 中通过 `register_alloc_decision()` 注册.
pub struct PressureAwareAllocPolicy;

impl FrameAllocDecision for PressureAwareAllocPolicy {
    fn decide_alloc(&self, ctx: AllocContext) -> AllocDecision {
        let pressure = current_pressure();
        match pressure {
            MemoryPressure::Normal | MemoryPressure::Warning => AllocDecision::Allow,
            MemoryPressure::Critical => {
                // 严重压力下仅允许小分配
                if ctx.requested_pages <= 4 {
                    AllocDecision::Allow
                } else {
                    AllocDecision::Deny
                }
            }
            MemoryPressure::Emergency => AllocDecision::RetryAfterReclaim,
        }
    }

    fn on_alloc_failed(&self, ctx: AllocContext) -> AllocDecision {
        let pressure = current_pressure();
        match pressure {
            MemoryPressure::Emergency => AllocDecision::RetryAfterReclaim,
            _ => {
                // 非紧急压力下分配失败, 建议回收后重试一次
                if ctx.free_pages < ctx.requested_pages as u64 {
                    AllocDecision::RetryAfterReclaim
                } else {
                    AllocDecision::Deny
                }
            }
        }
    }

    fn select_numa_node(&self, _ctx: AllocContext) -> Option<u8> {
        // 当前单节点系统, 不做 NUMA 选择
        None
    }
}

/// 注册压力感知分配策略到 framework
///
/// 由 `services::mm::init()` 调用. 只能注册一次.
///
/// # Errors
///
/// 当分配策略已被注册时返回 `Err(())`.
pub fn register_pressure_aware_policy() -> Result<(), ()> {
    static POLICY: PressureAwareAllocPolicy = PressureAwareAllocPolicy;
    crate::kernel::framework::mm::register_alloc_decision(&POLICY).map_err(|_| ())
}
