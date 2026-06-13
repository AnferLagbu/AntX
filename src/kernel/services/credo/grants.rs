#![deny(unsafe_code)]
//! 能力委托 — 链式委托 / 时间约束 / 撤销传播 (services 层)
//!
//! ## 框内核中的表达
//!
//! 委托是 services 层的**策略**, TCB 只提供"原子地 grant/revoke"操作.
//! 链式委托、时间约束、撤销传播都是策略层关注点.
//!
//! ## 数据结构
//!
//! ```text
//! GrantRecord {
//!   from_pwm: u64,        // 委托者
//!   to_pwm: u64,          // 受托者
//!   domain: CapDomain,    // 哪个能力域
//!   bits: CapBits,        // 授予的位
//!   created_tick: u64,    // 创建时间
//!   expires_tick: u64,    // 过期时间 (0 = 永久)
//!   generation: u32,      // 委托代数
//!   parent_gen: u32,      // 父委托 (链式)
//!   flags: GrantFlags,    // NON_DELEGATABLE / REVOKED
//! }
//! ```
//!
//! ## @SAFE
//! 本文件不含 `unsafe`. 时间由 framework::time 提供.

#![allow(dead_code)]

use super::policy::{CapBits, CapDomain, CapabilityMatrix, GrantResult, PolicyEngine};
use core::sync::atomic::{AtomicU32, Ordering};

bitflags::bitflags! {
    /// 委托标志
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GrantFlags: u32 {
        const NONE       = 0;
        /// 不可再委托
        const NON_DELEGATABLE = 1 << 0;
        /// 已撤销
        const REVOKED    = 1 << 1;
        /// 审计已记录
        const AUDITED    = 1 << 2;
    }
}

/// 委托记录
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantRecord {
    pub from_pwm: u64,
    pub to_pwm: u64,
    pub domain: CapDomain,
    pub bits: CapBits,
    pub created_tick: u64,
    pub expires_tick: u64,
    pub generation: u32,
    pub parent_gen: u32,
    pub flags: GrantFlags,
}

/// 最大委托记录数
pub const MAX_GRANT_RECORDS: usize = 256;

/// 委托表 (固定大小, 避免动态分配)
pub struct GrantTable {
    records: [GrantRecord; MAX_GRANT_RECORDS],
    next_gen: AtomicU32,
    used: AtomicU32,
}

impl GrantTable {
    pub const fn new() -> Self {
        const EMPTY: GrantRecord = GrantRecord {
            from_pwm: 0,
            to_pwm: 0,
            domain: CapDomain(0),
            bits: CapBits(0),
            created_tick: 0,
            expires_tick: 0,
            generation: 0,
            parent_gen: 0,
            flags: GrantFlags::NONE,
        };
        Self {
            records: [EMPTY; MAX_GRANT_RECORDS],
            next_gen: AtomicU32::new(1),
            used: AtomicU32::new(0),
        }
    }

    /// 添加委托记录
    pub fn add(&mut self, mut record: GrantRecord) -> Option<u32> {
        if self.used.load(Ordering::Acquire) as usize >= MAX_GRANT_RECORDS {
            return None;
        }
        let gen = self.next_gen.fetch_add(1, Ordering::AcqRel);
        record.generation = gen;
        // 线性查找空槽 (generation == 0)
        for i in 0..MAX_GRANT_RECORDS {
            if self.records[i].generation == 0 {
                self.records[i] = record;
                self.used.fetch_add(1, Ordering::AcqRel);
                return Some(gen);
            }
        }
        None
    }

    /// 按 generation 查找
    pub fn get(&self, gen: u32) -> Option<&GrantRecord> {
        if gen == 0 { return None; }
        for r in &self.records {
            if r.generation == gen {
                return Some(r);
            }
        }
        None
    }

    /// 标记撤销 (释放槽)
    pub fn mark_revoked(&mut self, gen: u32) -> bool {
        for i in 0..MAX_GRANT_RECORDS {
            if self.records[i].generation == gen {
                if self.records[i].flags.contains(GrantFlags::REVOKED) {
                    return false;
                }
                self.records[i].flags |= GrantFlags::REVOKED;
                self.records[i].generation = 0; // 释放
                self.used.fetch_sub(1, Ordering::AcqRel);
                return true;
            }
        }
        false
    }

    /// 检查委托是否仍有效
    pub fn is_valid(&self, gen: u32, current_tick: u64) -> bool {
        match self.get(gen) {
            Some(r) => {
                if r.flags.contains(GrantFlags::REVOKED) {
                    return false;
                }
                if r.expires_tick != 0 && current_tick >= r.expires_tick {
                    return false;
                }
                true
            }
            None => false,
        }
    }

    /// 撤销某 PWM 发出的所有委托
    pub fn revoke_all_from(&mut self, pwm: u64) -> usize {
        let mut count = 0;
        let gens: [u32; MAX_GRANT_RECORDS] = {
            let mut g = [0u32; MAX_GRANT_RECORDS];
            let mut idx = 0;
            for r in &self.records {
                if r.generation != 0 && r.from_pwm == pwm && idx < MAX_GRANT_RECORDS {
                    g[idx] = r.generation;
                    idx += 1;
                }
            }
            g
        };
        for &gen in &gens {
            if gen == 0 { break; }
            if self.mark_revoked(gen) {
                count += 1;
            }
        }
        count
    }

    pub fn len(&self) -> usize {
        self.used.load(Ordering::Acquire) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 委托操作结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationResult {
    Granted { gen: u32 },
    Denied(DelegationDeny),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationDeny {
    NonDelegatable,
    TableFull,
    InvalidExpiry,
    SamePwm,
    Policy(GrantResult),
}

/// 委托引擎
pub struct DelegationEngine<'a> {
    pub table: &'a mut GrantTable,
    pub policy: &'a PolicyEngine,
}

impl<'a> DelegationEngine<'a> {
    pub fn new(table: &'a mut GrantTable, policy: &'a PolicyEngine) -> Self {
        Self { table, policy }
    }

    /// 委托
    pub fn delegate<M: CapabilityMatrix>(
        &mut self,
        from_matrix: &M,
        to_matrix: &mut M,
        from_pwm: u64,
        to_pwm: u64,
        domain: CapDomain,
        bits: CapBits,
        current_tick: u64,
        expires_tick: u64,
        non_delegatable: bool,
    ) -> DelegationResult {
        if from_pwm == to_pwm {
            return DelegationResult::Denied(DelegationDeny::SamePwm);
        }
        if expires_tick != 0 && expires_tick <= current_tick {
            return DelegationResult::Denied(DelegationDeny::InvalidExpiry);
        }
        let grant_res = self.policy.grant(from_matrix, to_matrix, domain, bits);
        match grant_res {
            GrantResult::Granted => {
                let flags = if non_delegatable {
                    GrantFlags::NON_DELEGATABLE
                } else {
                    GrantFlags::NONE
                };
                let rec = GrantRecord {
                    from_pwm,
                    to_pwm,
                    domain,
                    bits,
                    created_tick: current_tick,
                    expires_tick,
                    generation: 0,
                    parent_gen: 0,
                    flags,
                };
                match self.table.add(rec) {
                    Some(gen) => DelegationResult::Granted { gen },
                    None => DelegationResult::Denied(DelegationDeny::TableFull),
                }
            }
            other => DelegationResult::Denied(DelegationDeny::Policy(other)),
        }
    }

    /// 撤销指定 gen 的委托
    pub fn revoke<M: CapabilityMatrix>(
        &mut self,
        matrix: &mut M,
        gen: u32,
    ) -> bool {
        if let Some(rec) = self.table.get(gen).copied() {
            if rec.flags.contains(GrantFlags::REVOKED) {
                return false;
            }
            let _ = self.policy.revoke(matrix, rec.domain, rec.bits);
            self.table.mark_revoked(gen);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::policy::InMemoryMatrix;

    fn make_matrix() -> InMemoryMatrix {
        InMemoryMatrix::new()
    }

    #[test]
    fn grant_table_basic_add() {
        let mut t = GrantTable::new();
        let rec = GrantRecord {
            from_pwm: 1,
            to_pwm: 2,
            domain: CapDomain::FS,
            bits: CapBits(0xFF),
            created_tick: 100,
            expires_tick: 0,
            generation: 0,
            parent_gen: 0,
            flags: GrantFlags::NONE,
        };
        let gen = t.add(rec).unwrap();
        assert!(gen > 0);
        assert_eq!(t.len(), 1);
        let got = t.get(gen).unwrap();
        assert_eq!(got.from_pwm, 1);
        assert_eq!(got.to_pwm, 2);
    }

    #[test]
    fn grant_table_full() {
        let mut t = GrantTable::new();
        for i in 0..MAX_GRANT_RECORDS {
            let rec = GrantRecord {
                from_pwm: 1,
                to_pwm: 2 + i as u64,
                domain: CapDomain::FS,
                bits: CapBits(0xFF),
                created_tick: 0,
                expires_tick: 0,
                generation: 0,
                parent_gen: 0,
                flags: GrantFlags::NONE,
            };
            assert!(t.add(rec).is_some(), "should add {}", i);
        }
        let rec = GrantRecord {
            from_pwm: 1, to_pwm: 999,
            domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0,
            generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        };
        assert_eq!(t.add(rec), None);
    }

    #[test]
    fn grant_table_revoke_frees_slot() {
        let mut t = GrantTable::new();
        let rec = GrantRecord {
            from_pwm: 1, to_pwm: 2,
            domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0,
            generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        };
        let gen = t.add(rec).unwrap();
        assert_eq!(t.len(), 1);
        assert!(t.mark_revoked(gen));
        assert_eq!(t.len(), 0);

        let rec2 = GrantRecord {
            from_pwm: 1, to_pwm: 3,
            domain: CapDomain::FS, bits: CapBits(0x02),
            created_tick: 0, expires_tick: 0,
            generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        };
        assert!(t.add(rec2).is_some());
    }

    #[test]
    fn grant_expiry() {
        let mut t = GrantTable::new();
        let rec = GrantRecord {
            from_pwm: 1, to_pwm: 2,
            domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 100, expires_tick: 200,
            generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        };
        let gen = t.add(rec).unwrap();
        assert!(t.is_valid(gen, 150));
        assert!(!t.is_valid(gen, 200));
        assert!(!t.is_valid(gen, 300));
    }

    #[test]
    fn grant_permanent_never_expires() {
        let mut t = GrantTable::new();
        let rec = GrantRecord {
            from_pwm: 1, to_pwm: 2,
            domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0,
            generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        };
        let gen = t.add(rec).unwrap();
        assert!(t.is_valid(gen, u64::MAX));
    }

    #[test]
    fn revoke_all_from() {
        let mut t = GrantTable::new();
        // 来自 pwm=1 的 3 个委托
        for i in 0..3 {
            let rec = GrantRecord {
                from_pwm: 1, to_pwm: 10 + i,
                domain: CapDomain::FS, bits: CapBits(0x01),
                created_tick: 0, expires_tick: 0,
                generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
            };
            t.add(rec).unwrap();
        }
        // 来自 pwm=2 的 1 个委托
        let rec = GrantRecord {
            from_pwm: 2, to_pwm: 100,
            domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0,
            generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        };
        t.add(rec).unwrap();

        assert_eq!(t.len(), 4);
        let revoked = t.revoke_all_from(1);
        assert_eq!(revoked, 3);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn delegate_basic() {
        let mut table = GrantTable::new();
        let policy = PolicyEngine::new();
        let mut from = make_matrix();
        let mut to = make_matrix();
        from.set(CapDomain::FS, CapBits(0xFF)).unwrap();

        let mut eng = DelegationEngine::new(&mut table, &policy);
        let r = eng.delegate(&from, &mut to, 1, 2, CapDomain::FS, CapBits(0b1000), 100, 0, false);
        assert!(matches!(r, DelegationResult::Granted { .. }));
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b1000)));
    }

    #[test]
    fn delegate_same_pwm() {
        let mut table = GrantTable::new();
        let policy = PolicyEngine::new();
        let mut from = make_matrix();
        let mut to = make_matrix();
        from.set(CapDomain::FS, CapBits(0xFF)).unwrap();
        let mut eng = DelegationEngine::new(&mut table, &policy);
        let r = eng.delegate(&from, &mut to, 1, 1, CapDomain::FS, CapBits(0b1000), 100, 0, false);
        assert_eq!(r, DelegationResult::Denied(DelegationDeny::SamePwm));
    }

    #[test]
    fn delegate_invalid_expiry() {
        let mut table = GrantTable::new();
        let policy = PolicyEngine::new();
        let mut from = make_matrix();
        let mut to = make_matrix();
        from.set(CapDomain::FS, CapBits(0xFF)).unwrap();
        let mut eng = DelegationEngine::new(&mut table, &policy);
        // 过期刻度 (50) <= 当前刻度 (100)
        let r = eng.delegate(&from, &mut to, 1, 2, CapDomain::FS, CapBits(0b1000), 100, 50, false);
        assert_eq!(r, DelegationResult::Denied(DelegationDeny::InvalidExpiry));
    }

    #[test]
    fn delegate_no_authority() {
        let mut table = GrantTable::new();
        let policy = PolicyEngine::new();
        let from = make_matrix();
        let mut to = make_matrix();
        let mut eng = DelegationEngine::new(&mut table, &policy);
        let r = eng.delegate(&from, &mut to, 1, 2, CapDomain::FS, CapBits(0b1000), 100, 0, false);
        assert!(matches!(r, DelegationResult::Denied(DelegationDeny::Policy(_))));
    }

    #[test]
    fn delegate_revoke() {
        let mut table = GrantTable::new();
        let policy = PolicyEngine::new();
        let mut from = make_matrix();
        let mut to = make_matrix();
        from.set(CapDomain::FS, CapBits(0xFF)).unwrap();
        let mut eng = DelegationEngine::new(&mut table, &policy);
        let r = eng.delegate(&from, &mut to, 1, 2, CapDomain::FS, CapBits(0b1000), 100, 0, false);
        let gen = match r {
            DelegationResult::Granted { gen } => gen,
            _ => panic!("expected Granted"),
        };
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b1000)));
        assert!(eng.revoke(&mut to, gen));
        // 0b1000 非 floor (FS floor = 0b0101), 应被撤销
        assert_eq!(to.get(CapDomain::FS), Some(CapBits::NONE));
    }
}
