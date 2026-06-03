//! Miri 测试: services/barrier/attribution.rs 的算法等价物
//!
//! 验证:
//! - 故障归属决策 (TCB / Services / CrossLayer / Unknown)
//! - 失败计数器 (连续失败 → tier 升级)
//! - 能力降级
//!
//! 限制: 不含真实 atomic, 用 u64 模拟以避免 Miri 性能问题.

#![allow(dead_code)]

/// 故障归属决策
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultAttribution {
    Service { domain_id: u64, recoverable: bool },
    Tcb { module: TcbModule },
    CrossLayer { caller: u64, callee: TcbModule },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcbModule {
    Barrier,
    Credo,
    Memory,
    Sync,
    Idt,
    Other,
}

pub const fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    if haystack.len() < needle.len() { return false; }
    let mut i = 0;
    while i < needle.len() {
        if haystack[i] != needle[i] { return false; }
        i += 1;
    }
    true
}

pub const fn contains_substr(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() { return true; }
    if haystack.len() < needle.len() { return false; }
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        let mut j = 0;
        let mut match_ = true;
        while j < needle.len() {
            if haystack[i + j] != needle[j] { match_ = false; break; }
            j += 1;
        }
        if match_ { return true; }
        i += 1;
    }
    false
}

impl TcbModule {
    pub const fn from_name(name: &[u8]) -> Self {
        if contains_substr(name, b"barrier") { Self::Barrier }
        else if contains_substr(name, b"credo") { Self::Credo }
        else if contains_substr(name, b"mm") || contains_substr(name, b"pmm") { Self::Memory }
        else if contains_substr(name, b"sync") { Self::Sync }
        else if contains_substr(name, b"idt") { Self::Idt }
        else { Self::Other }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AddrRange {
    pub start: u64,
    pub end: u64,
    pub name: &'static [u8],
}

impl AddrRange {
    pub const fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }
}

pub const TCB_RANGES: &[AddrRange] = &[
    AddrRange { start: 0xFFFF_FFFF_8000_0000, end: 0xFFFF_FFFF_8001_0000, name: b"framework::barrier" },
    AddrRange { start: 0xFFFF_FFFF_8001_0000, end: 0xFFFF_FFFF_8002_0000, name: b"framework::credo" },
    AddrRange { start: 0xFFFF_FFFF_8002_0000, end: 0xFFFF_FFFF_8003_0000, name: b"framework::mm" },
];

pub const SERVICE_RANGES: &[AddrRange] = &[
    AddrRange { start: 0xFFFF_FFFF_0000_0000, end: 0xFFFF_FFFF_1000_0000, name: b"services::credo" },
    AddrRange { start: 0xFFFF_FFFF_1000_0000, end: 0xFFFF_FFFF_2000_0000, name: b"services::fs" },
];

pub struct FaultAttributor;

impl core::fmt::Debug for FaultAttributor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FaultAttributor").finish()
    }
}

impl FaultAttributor {
    pub fn attribute(panic_rip: u64) -> FaultAttribution {
        let mut i = 0;
        while i < TCB_RANGES.len() {
            if TCB_RANGES[i].contains(panic_rip) {
                return FaultAttribution::Tcb { module: TcbModule::from_name(TCB_RANGES[i].name) };
            }
            i += 1;
        }
        let mut j = 0;
        while j < SERVICE_RANGES.len() {
            if SERVICE_RANGES[j].contains(panic_rip) {
                let domain_id = (panic_rip >> 12) & 0xFFFF;
                return FaultAttribution::Service { domain_id, recoverable: true };
            }
            j += 1;
        }
        FaultAttribution::Unknown
    }

    pub fn attribute_cross(caller_id: u64, callee_panic_rip: u64) -> FaultAttribution {
        let mut i = 0;
        while i < TCB_RANGES.len() {
            if TCB_RANGES[i].contains(callee_panic_rip) {
                return FaultAttribution::CrossLayer {
                    caller: caller_id,
                    callee: TcbModule::from_name(TCB_RANGES[i].name),
                };
            }
            i += 1;
        }
        Self::attribute(callee_panic_rip)
    }
}

/// 非原子版本用于 Miri
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainFailureRecord {
    pub domain_id: u64,
    pub consecutive_failures: u32,
    pub total_failures: u64,
    pub last_failure_tick: u64,
    pub downgraded: u32,
    pub current_tier: u32,
}

impl DomainFailureRecord {
    pub const fn new(domain_id: u64) -> Self {
        Self {
            domain_id,
            consecutive_failures: 0,
            total_failures: 0,
            last_failure_tick: 0,
            downgraded: 0,
            current_tier: 0,
        }
    }

    pub fn record_failure(&mut self, current_tick: u64) -> u32 {
        self.total_failures += 1;
        self.consecutive_failures += 1;
        self.last_failure_tick = current_tick;
        let n = self.consecutive_failures;
        let new_tier = if n >= 5 { 2 } else if n >= 3 { 1 } else { 0 };
        if new_tier > self.current_tier {
            self.current_tier = new_tier;
            self.downgraded += 1;
        }
        new_tier
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        if self.current_tier > 0 {
            self.current_tier -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcb_contains_start_not_end() {
        for r in TCB_RANGES {
            assert!(r.contains(r.start));
            assert!(!r.contains(r.end));
        }
    }

    #[test]
    fn attribute_tcb_barrier() {
        let attr = FaultAttributor::attribute(0xFFFF_FFFF_8000_5000);
        assert_eq!(attr, FaultAttribution::Tcb { module: TcbModule::Barrier });
    }

    #[test]
    fn attribute_tcb_credo() {
        let attr = FaultAttributor::attribute(0xFFFF_FFFF_8001_4000);
        assert_eq!(attr, FaultAttribution::Tcb { module: TcbModule::Credo });
    }

    #[test]
    fn attribute_service() {
        let attr = FaultAttributor::attribute(0xFFFF_FFFF_0500_0000);
        match attr {
            FaultAttribution::Service { recoverable, .. } => assert!(recoverable),
            _ => panic!("expected service"),
        }
    }

    #[test]
    fn attribute_unknown() {
        let attr = FaultAttributor::attribute(0x1234_5678);
        assert_eq!(attr, FaultAttribution::Unknown);
    }

    #[test]
    fn attribute_cross_layer() {
        let attr = FaultAttributor::attribute_cross(42, 0xFFFF_FFFF_8001_8000);
        assert_eq!(attr, FaultAttribution::CrossLayer { caller: 42, callee: TcbModule::Credo });
    }

    #[test]
    fn module_from_name_basic() {
        assert_eq!(TcbModule::from_name(b"framework::barrier"), TcbModule::Barrier);
        assert_eq!(TcbModule::from_name(b"credo_xyz"), TcbModule::Credo);
        assert_eq!(TcbModule::from_name(b"framework::mm"), TcbModule::Memory);
        assert_eq!(TcbModule::from_name(b"pmm_alloc"), TcbModule::Memory);
        assert_eq!(TcbModule::from_name(b"sync_lock"), TcbModule::Sync);
        assert_eq!(TcbModule::from_name(b"idt_handler"), TcbModule::Idt);
        assert_eq!(TcbModule::from_name(b"other"), TcbModule::Other);
    }

    #[test]
    fn module_short_name_unchanged() {
        assert_eq!(TcbModule::from_name(b"b"), TcbModule::Other);
        assert_eq!(TcbModule::from_name(b""), TcbModule::Other);
    }

    #[test]
    fn failure_record_tier0() {
        let mut r = DomainFailureRecord::new(1);
        assert_eq!(r.record_failure(100), 0);
        assert_eq!(r.record_failure(101), 0);
        assert_eq!(r.current_tier, 0);
        assert_eq!(r.downgraded, 0);
    }

    #[test]
    fn failure_record_tier1_after_3() {
        let mut r = DomainFailureRecord::new(1);
        r.record_failure(100);
        r.record_failure(101);
        let t = r.record_failure(102);
        assert_eq!(t, 1);
        assert_eq!(r.current_tier, 1);
        assert_eq!(r.downgraded, 1);
    }

    #[test]
    fn failure_record_tier2_after_5() {
        let mut r = DomainFailureRecord::new(1);
        for _ in 0..5 {
            r.record_failure(100);
        }
        assert_eq!(r.current_tier, 2);
    }

    #[test]
    fn failure_record_recovery_demote() {
        let mut r = DomainFailureRecord::new(1);
        for _ in 0..3 {
            r.record_failure(100);
        }
        assert_eq!(r.current_tier, 1);
        r.record_success();
        assert_eq!(r.current_tier, 0);
        assert_eq!(r.consecutive_failures, 0);
    }

    #[test]
    fn addr_range_edge_cases() {
        let r = AddrRange { start: 100, end: 200, name: b"test" };
        assert!(r.contains(100));
        assert!(r.contains(150));
        assert!(r.contains(199));
        assert!(!r.contains(200));
        assert!(!r.contains(99));
        assert!(!r.contains(0));
    }
}
