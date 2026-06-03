//! Credo / PWID 能力委托 (services 层算法等价)
//!
//! 镜像 `src/kernel/services/credo/grants.rs` 的算法, 在 Miri 下验证 UB.
//!
//! ## 关键不变量
//!
//! 1. **时间有效**: expires_tick > current_tick (永久时 expires=0)
//! 2. **同源禁止**: from_pwm != to_pwm
//! 3. **Floor 保护**: 撤销时不能动 viable_floor
//! 4. **槽位回收**: revoked 后 generation=0 表示空

#![allow(dead_code, unused_imports)]  // CapMatrix 仅测试用, 库自身无需

use crate::credo_policy::{CapBits, CapDomain, CapMatrix, CapabilityMatrix, InMemoryMatrix};

// ============================================================
// 委托记录
// ============================================================

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct GrantFlags: u32 {
        const NONE = 0;
        const NON_DELEGATABLE = 1 << 0;
        const REVOKED = 1 << 1;
        const AUDITED = 1 << 2;
    }
}

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

pub const MAX_GRANT_RECORDS: usize = 256;

#[derive(Debug)]
pub struct GrantTable {
    pub records: [Option<GrantRecord>; MAX_GRANT_RECORDS],
    pub next_gen: u32,
    pub used: usize,
}

impl Default for GrantTable {
    fn default() -> Self { Self::new() }
}

impl GrantTable {
    pub const fn new() -> Self {
        Self {
            records: [None; MAX_GRANT_RECORDS],
            next_gen: 1,
            used: 0,
        }
    }

    /// 添加委托记录
    pub fn add(&mut self, mut record: GrantRecord) -> Option<u32> {
        if self.used >= MAX_GRANT_RECORDS {
            return None;
        }
        let gen = self.next_gen;
        self.next_gen = self.next_gen.wrapping_add(1);
        if self.next_gen == 0 { self.next_gen = 1; } // 跳过 0
        record.generation = gen;
        for i in 0..MAX_GRANT_RECORDS {
            if self.records[i].is_none() {
                self.records[i] = Some(record);
                self.used += 1;
                return Some(gen);
            }
        }
        None
    }

    pub fn get(&self, gen: u32) -> Option<&GrantRecord> {
        if gen == 0 { return None; }
        self.records.iter().flatten().find(|r| r.generation == gen)
    }

    pub fn mark_revoked(&mut self, gen: u32) -> bool {
        for slot in &mut self.records {
            if let Some(s) = slot {
                if s.generation == gen {
                    if s.flags.contains(GrantFlags::REVOKED) {
                        return false;
                    }
                    s.flags |= GrantFlags::REVOKED;
                    *slot = None;
                    self.used -= 1;
                    return true;
                }
            }
        }
        false
    }

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

    pub fn revoke_all_from(&mut self, pwm: u64) -> usize {
        let mut count = 0;
        // 先收集 gens
        let mut gens: [u32; MAX_GRANT_RECORDS] = [0; MAX_GRANT_RECORDS];
        let mut idx = 0;
        for r in self.records.iter().flatten() {
            if r.from_pwm == pwm && idx < MAX_GRANT_RECORDS {
                gens[idx] = r.generation;
                idx += 1;
            }
        }
        for &gen in &gens {
            if gen == 0 { break; }
            if self.mark_revoked(gen) {
                count += 1;
            }
        }
        count
    }

    pub fn len(&self) -> usize { self.used }
    pub fn is_empty(&self) -> bool { self.used == 0 }
}

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
    PolicyDenied,
}

/// 委托操作: 验证 + 添加记录
#[allow(clippy::too_many_arguments)]
pub fn try_delegate(
    table: &mut GrantTable,
    from: &InMemoryMatrix,
    to: &mut InMemoryMatrix,
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
    // 实际授权
    let owned = from.get(domain);
    if owned.is_none() || !owned.unwrap().contains(bits) {
        return DelegationResult::Denied(DelegationDeny::PolicyDenied);
    }
    // CAS 模拟: 读-改-写 (单线程, 不会失败)
    let to_current = to.get(domain).unwrap_or(CapBits::NONE);
    let to_new = to_current | bits;
    to.0.set(domain, to_new);

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
    match table.add(rec) {
        Some(gen) => DelegationResult::Granted { gen },
        None => DelegationResult::Denied(DelegationDeny::TableFull),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table() -> GrantTable { GrantTable::new() }
    fn make_matrix() -> InMemoryMatrix {
        let mut m = InMemoryMatrix(CapMatrix::empty());
        m.0.set(CapDomain::FS, CapBits(0xFF));
        m
    }

    #[test]
    fn table_add_get() {
        let mut t = make_table();
        let rec = GrantRecord {
            from_pwm: 1, to_pwm: 2, domain: CapDomain::FS, bits: CapBits(0xFF),
            created_tick: 100, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        };
        let gen = t.add(rec).unwrap();
        assert!(gen > 0);
        assert_eq!(t.len(), 1);
        let got = t.get(gen).unwrap();
        assert_eq!(got.from_pwm, 1);
    }

    #[test]
    fn table_full() {
        let mut t = make_table();
        for i in 0..MAX_GRANT_RECORDS {
            t.add(GrantRecord {
                from_pwm: 1, to_pwm: i as u64, domain: CapDomain::FS, bits: CapBits(0x01),
                created_tick: 0, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
            }).unwrap();
        }
        let r = t.add(GrantRecord {
            from_pwm: 1, to_pwm: 999, domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        });
        assert!(r.is_none());
    }

    #[test]
    fn revoke_frees_slot() {
        let mut t = make_table();
        let gen = t.add(GrantRecord {
            from_pwm: 1, to_pwm: 2, domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        }).unwrap();
        assert!(t.mark_revoked(gen));
        assert_eq!(t.len(), 0);
        // 可再添加
        t.add(GrantRecord {
            from_pwm: 1, to_pwm: 3, domain: CapDomain::FS, bits: CapBits(0x02),
            created_tick: 0, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        }).unwrap();
    }

    #[test]
    fn expiry_check() {
        let mut t = make_table();
        let gen = t.add(GrantRecord {
            from_pwm: 1, to_pwm: 2, domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 100, expires_tick: 200, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        }).unwrap();
        assert!(t.is_valid(gen, 150));
        assert!(!t.is_valid(gen, 200));
    }

    #[test]
    fn permanent_never_expires() {
        let mut t = make_table();
        let gen = t.add(GrantRecord {
            from_pwm: 1, to_pwm: 2, domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        }).unwrap();
        assert!(t.is_valid(gen, u64::MAX));
    }

    #[test]
    fn revoke_all_from() {
        let mut t = make_table();
        for i in 0..3 {
            t.add(GrantRecord {
                from_pwm: 1, to_pwm: 10 + i as u64, domain: CapDomain::FS, bits: CapBits(0x01),
                created_tick: 0, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
            }).unwrap();
        }
        t.add(GrantRecord {
            from_pwm: 2, to_pwm: 100, domain: CapDomain::FS, bits: CapBits(0x01),
            created_tick: 0, expires_tick: 0, generation: 0, parent_gen: 0, flags: GrantFlags::NONE,
        }).unwrap();
        assert_eq!(t.len(), 4);
        let n = t.revoke_all_from(1);
        assert_eq!(n, 3);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn delegate_basic() {
        let mut table = make_table();
        let from = make_matrix();
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = try_delegate(&mut table, &from, &mut to, 1, 2,
            CapDomain::FS, CapBits(0b1000), 100, 0, false);
        assert!(matches!(r, DelegationResult::Granted { .. }));
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b1000)));
    }

    #[test]
    fn delegate_same_pwm() {
        let mut table = make_table();
        let from = make_matrix();
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = try_delegate(&mut table, &from, &mut to, 1, 1,
            CapDomain::FS, CapBits(0b1000), 100, 0, false);
        assert_eq!(r, DelegationResult::Denied(DelegationDeny::SamePwm));
    }

    #[test]
    fn delegate_invalid_expiry() {
        let mut table = make_table();
        let from = make_matrix();
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = try_delegate(&mut table, &from, &mut to, 1, 2,
            CapDomain::FS, CapBits(0b1000), 100, 50, false);
        assert_eq!(r, DelegationResult::Denied(DelegationDeny::InvalidExpiry));
    }

    #[test]
    fn delegate_no_authority() {
        let mut table = make_table();
        let from = InMemoryMatrix(CapMatrix::empty());
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = try_delegate(&mut table, &from, &mut to, 1, 2,
            CapDomain::FS, CapBits(0b1000), 100, 0, false);
        assert_eq!(r, DelegationResult::Denied(DelegationDeny::PolicyDenied));
    }

    /// 委托 + 撤销 单调性
    #[test]
    fn delegate_then_revoke_idempotent() {
        let mut table = make_table();
        let from = make_matrix();
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = try_delegate(&mut table, &from, &mut to, 1, 2,
            CapDomain::FS, CapBits(0b1000), 100, 0, false);
        let gen = match r {
            DelegationResult::Granted { gen } => gen,
            _ => panic!("expected Granted"),
        };
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b1000)));
        assert!(table.mark_revoked(gen));
        // 撤销仅释放记录, 不动 matrix (matrix 由调用方撤销)
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b1000)));
    }

    /// 链式: a→b→c, c 只能拿到 b 授予的子集
    #[test]
    fn chain_delegation() {
        let mut table = make_table();
        let mut a = InMemoryMatrix(CapMatrix::empty());
        a.0.set(CapDomain::FS, CapBits(0xFF));
        let mut b = InMemoryMatrix(CapMatrix::empty());

        // a → b: 授予 0b1100
        let r1 = try_delegate(&mut table, &a, &mut b, 1, 2,
            CapDomain::FS, CapBits(0b1100), 100, 0, false);
        assert!(matches!(r1, DelegationResult::Granted { .. }));
        // b 现在的 cap 是 0b1100
        assert_eq!(b.get(CapDomain::FS), Some(CapBits(0b1100)));

        // b → c: 尝试授予 0b1110 (含 b 没有的 0b0010)
        let mut c = InMemoryMatrix(CapMatrix::empty());
        let r2 = try_delegate(&mut table, &b, &mut c, 2, 3,
            CapDomain::FS, CapBits(0b1110), 100, 0, false);
        // b 不拥有 0b0010, 应被拒
        assert_eq!(r2, DelegationResult::Denied(DelegationDeny::PolicyDenied));
        assert_eq!(c.get(CapDomain::FS), Some(CapBits::NONE));
    }
}
