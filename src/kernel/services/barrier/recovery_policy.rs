#![deny(unsafe_code)]
//! 恢复策略决策器 — services/barrier/ 业务层
//!
//! ## 职责
//!
//! 接收故障信号 (`FaultAttribution` + 重试次数 + 心跳丢失), 决策应该走哪条
//! 恢复路径:
//!
//! - **Noop**: 一次性瞬时错误, 不需动作
//! - **BBR** (Barrier Base Recovery): 域内 undo log 回滚, ~1μs
//! - **BSR** (Barrier Soft Reset): 域级软重置 + 设备快照恢复, ~50ms
//! - **BHR** (Barrier Hard Reset): 整机硬重置, ~120ms
//! - **Quarantine**: 隔离域, 不再尝试恢复
//!
//! ## 决策矩阵
//!
//! ```text
//! fault_kind × retry_count × heartbeat_gap × dependents → action
//! ────────────────────────────────────────────────────────────
//! Service     1-2 次      <100 ticks    0          → BBR
//! Service     3 次        100-500       0-2        → BSR
//! Service     ≥5 次       any           any        → Quarantine
//! Service     any         >500 ticks    any        → BSR (心跳丢失)
//! Tcb         any         any           any        → BHR (TCB 不可恢复)
//! CrossLayer  any         any           any        → BSR + 报告调用方
//! ```
//!
//! ## @SAFE
//!
//! 本文件不含 `unsafe`. 通过 `framework::barrier::*` 的安全公开 API
//! (`recoverable::Recoverable`, `reset::RecoveryLayer`, `reset::RecoveryResult`)
//! 与 TCB 交互, 不直接接触 `spin::Mutex`/`AtomicU64`.

use super::attribution::{FaultAttribution, TcbModule};

/// 决策动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    /// 不需要恢复 (一次瞬时错误, 已自愈)
    Noop,
    /// 域内 undo log 回滚 (BBR)
    BarrierBaseRecovery,
    /// 域级软重置 (BSR)
    BarrierSoftReset,
    /// 整机硬重置 (BHR)
    BarrierHardReset,
    /// 隔离该域, 不再尝试
    Quarantine,
}

impl RecoveryAction {
    /// 转为 `framework::barrier::reset::RecoveryLayer`
    pub fn to_framework_layer(self) -> Option<u32> {
        // 1 = Layer1 (BBR 栏基), 2 = Layer2 (BSR 栏软), 3 = Layer3 (BHR 栏硬)
        match self {
            Self::BarrierBaseRecovery => Some(1),
            Self::BarrierSoftReset => Some(2),
            Self::BarrierHardReset => Some(3),
            Self::Noop | Self::Quarantine => None,
        }
    }

    /// 决策是否需要执行硬件级重置 (BHR)
    pub fn is_hardware_reset(&self) -> bool {
        matches!(self, Self::BarrierHardReset)
    }
}

/// 故障信号 (decision 输入)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultSignal {
    /// 故障归属
    pub attribution: FaultAttribution,
    /// 连续失败次数
    pub retry_count: u32,
    /// 心跳丢失 ticks
    pub heartbeat_gap: u64,
    /// 该域被多少其他域依赖
    pub dependents: u32,
    /// 故障时 wall tick
    pub tick: u64,
}

impl FaultSignal {
    /// 构造服务域故障信号
    pub const fn service(
        domain_id: u64,
        retry_count: u32,
        heartbeat_gap: u64,
        dependents: u32,
        tick: u64,
    ) -> Self {
        Self {
            attribution: FaultAttribution::Service { domain_id, recoverable: true },
            retry_count,
            heartbeat_gap,
            dependents,
            tick,
        }
    }

    /// 构造 TCB 故障信号
    pub const fn tcb(module: TcbModule, tick: u64) -> Self {
        Self {
            attribution: FaultAttribution::Tcb { module },
            retry_count: 0,
            heartbeat_gap: 0,
            dependents: 0,
            tick,
        }
    }

    /// 构造跨层故障信号
    pub const fn cross_layer(caller: u64, module: TcbModule, tick: u64) -> Self {
        Self {
            attribution: FaultAttribution::CrossLayer { caller, callee: module },
            retry_count: 0,
            heartbeat_gap: 0,
            dependents: 0,
            tick,
        }
    }
}

/// 策略决策器
///
/// 无状态 (纯函数式), 通过 `decide()` 给定 `FaultSignal` 输出 `RecoveryAction`.
pub struct RecoveryPolicy;

impl RecoveryPolicy {
    /// 默认策略
    pub const DEFAULT: Self = Self;

    /// 给定故障信号, 输出恢复动作
    pub fn decide(signal: &FaultSignal) -> RecoveryAction {
        match signal.attribution {
            FaultAttribution::Tcb { .. } => {
                // TCB 内部故障, 不可恢复, 走 BHR
                RecoveryAction::BarrierHardReset
            }
            FaultAttribution::CrossLayer { .. } => {
                // 跨层故障: 走 BSR 重置服务域 + 报告调用方
                if signal.dependents > 0 {
                    // 有依赖者, 先 BBR 再 BSR 谨慎升级
                    RecoveryAction::BarrierBaseRecovery
                } else {
                    RecoveryAction::BarrierSoftReset
                }
            }
            FaultAttribution::Service { recoverable: false, .. } => {
                // 显式标记不可恢复
                RecoveryAction::Quarantine
            }
            FaultAttribution::Service { .. } | FaultAttribution::Unknown => {
                // 服务域或未知地址 → 按重试/心跳判定
                Self::decide_service(signal)
            }
        }
    }

    /// 服务域决策
    fn decide_service(signal: &FaultSignal) -> RecoveryAction {
        // 心跳丢失 > 500 ticks → 软重置
        if signal.heartbeat_gap > 500 {
            return RecoveryAction::BarrierSoftReset;
        }

        // 连续失败次数驱动决策
        match signal.retry_count {
            0 => RecoveryAction::Noop,
            1..=2 => {
                // 1-2 次连续失败 → 域内 undo 回滚
                if signal.dependents == 0 {
                    RecoveryAction::BarrierBaseRecovery
                } else {
                    // 有依赖者, 谨慎走 BSR (避免级联回滚)
                    RecoveryAction::BarrierSoftReset
                }
            }
            3..=4 => RecoveryAction::BarrierSoftReset,
            _ => RecoveryAction::Quarantine,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcb_fault_always_bhr() {
        let s = FaultSignal::tcb(TcbModule::Barrier, 100);
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::BarrierHardReset);
    }

    #[test]
    fn service_no_dep_bbr() {
        let s = FaultSignal::service(2, 1, 0, 0, 100);
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::BarrierBaseRecovery);
    }

    #[test]
    fn service_with_deps_bsr() {
        // 1-2 次失败但有依赖者 → BSR
        let s = FaultSignal::service(2, 2, 0, 3, 100);
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::BarrierSoftReset);
    }

    #[test]
    fn service_3_failures_bsr() {
        let s = FaultSignal::service(2, 3, 0, 0, 100);
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::BarrierSoftReset);
    }

    #[test]
    fn service_5_failures_quarantine() {
        let s = FaultSignal::service(2, 5, 0, 0, 100);
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::Quarantine);
    }

    #[test]
    fn service_heartbeat_loss_bsr() {
        // 0 次重试但心跳丢失 → BSR
        let s = FaultSignal::service(2, 0, 1000, 0, 100);
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::BarrierSoftReset);
    }

    #[test]
    fn service_zero_retry_noop() {
        let s = FaultSignal::service(2, 0, 0, 0, 100);
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::Noop);
    }

    #[test]
    fn cross_layer_with_dependents_bbr() {
        let s = FaultSignal::cross_layer(42, TcbModule::Barrier, 100);
        // 默认 dependents=0 → BSR
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::BarrierSoftReset);
    }

    #[test]
    fn non_recoverable_service_quarantine() {
        let s = FaultSignal {
            attribution: FaultAttribution::Service { domain_id: 2, recoverable: false },
            retry_count: 0,
            heartbeat_gap: 0,
            dependents: 0,
            tick: 100,
        };
        assert_eq!(RecoveryPolicy::decide(&s), RecoveryAction::Quarantine);
    }

    #[test]
    fn action_to_framework_layer() {
        assert_eq!(RecoveryAction::BarrierBaseRecovery.to_framework_layer(), Some(1));
        assert_eq!(RecoveryAction::BarrierSoftReset.to_framework_layer(), Some(2));
        assert_eq!(RecoveryAction::BarrierHardReset.to_framework_layer(), Some(3));
        assert_eq!(RecoveryAction::Noop.to_framework_layer(), None);
        assert_eq!(RecoveryAction::Quarantine.to_framework_layer(), None);
    }

    #[test]
    fn action_hardware_reset_check() {
        assert!(RecoveryAction::BarrierHardReset.is_hardware_reset());
        assert!(!RecoveryAction::BarrierSoftReset.is_hardware_reset());
    }
}
