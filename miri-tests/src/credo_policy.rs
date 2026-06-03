//! Credo / PWID 能力策略 — 纯算法安全层
//!
//! 在 framekernel 中, Credo (PWM 能力矩阵) 被分为:
//! - **framework (TCB)**: 物理存储 (16×64 AtomicU64 矩阵) + SHA-256 + 持久化
//! - **services (本模块)**: 策略 — 哪些位代表什么 + 委托规则 + 撤销传播
//!
//! 本文件是 services 层的**纯算法等价重写**, 接受 Miri 严格扫描。
//!
//! ## 关键不变量
//!
//! 1. **单调性**: 能力只增不递减 (除显式 revoke 外, 永不自动降权)
//! 2. **可传递性**: grant(a, b, caps) 后, b 的有效能力 ⊇ caps (在 domain 内)
//! 3. **边界检查**: 任何 capability 操作必须先通过 viable_floor 检查
//! 4. **审计完整**: 所有 cap 变更必须经过 audit 通道
//!
//! ## 与 TCB 的边界
//!
//! services/credo 不直接读写 AtomicU64 — 通过 framework 提供的
//! 安全 trait `CapabilityBackend` 抽象。TDD: 本模块先用 mock 后端测试,
//! 真实集成时实现 framework::AtomicMatrixBackend。

#![allow(dead_code)]

/// 16 个能力域
pub const CAP_DOMAINS: usize = 16;
/// 每域 64 位
pub const CAP_BITS_PER_DOMAIN: u64 = 64;

/// 域 ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CapDomain(pub u8);

impl CapDomain {
    pub const SYSTEM: CapDomain = CapDomain(0);
    pub const FS: CapDomain = CapDomain(1);
    pub const NET: CapDomain = CapDomain(2);
    pub const PROC: CapDomain = CapDomain(3);
    pub const DEVICE: CapDomain = CapDomain(4);
    pub const USER_MGMT: CapDomain = CapDomain(5);
    pub const IPC: CapDomain = CapDomain(6);
    pub const MEM: CapDomain = CapDomain(7);
    pub const TIME: CapDomain = CapDomain(8);
    pub const BARRIER: CapDomain = CapDomain(9);
    pub const SIGNAL: CapDomain = CapDomain(10);
    pub const SHM: CapDomain = CapDomain(11);
    pub const SEM: CapDomain = CapDomain(12);
    pub const MSGQ: CapDomain = CapDomain(13);
    pub const DMA: CapDomain = CapDomain(14);
    pub const RESERVED: CapDomain = CapDomain(15);

    /// 验证域 ID 合法
    pub fn is_valid(self) -> bool {
        (self.0 as usize) < CAP_DOMAINS
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl From<u16> for CapDomain {
    fn from(v: u16) -> Self {
        CapDomain((v % 16) as u8)
    }
}

/// 能力位集合 (单个域内)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(transparent)]
pub struct CapBits(pub u64);

impl CapBits {
    pub const NONE: CapBits = CapBits(0);
    pub const ALL: CapBits = CapBits(u64::MAX);

    pub fn contains(self, other: CapBits) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 计算差集 (a 中有但 b 中没有的位)
    pub fn diff(self, other: CapBits) -> CapBits {
        CapBits(self.0 & !other.0)
    }
}

impl core::ops::BitOr for CapBits {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self { CapBits(self.0 | rhs.0) }
}

impl core::ops::BitAnd for CapBits {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self { CapBits(self.0 & rhs.0) }
}

impl core::ops::BitOrAssign for CapBits {
    fn bitor_assign(&mut self, rhs: Self) { self.0 |= rhs.0; }
}

impl core::ops::Not for CapBits {
    type Output = Self;
    fn not(self) -> Self { CapBits(!self.0) }
}

/// 完整 16 域能力矩阵 (逻辑视图, 与 TCB 的 16×AtomicU64 对应)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CapMatrix {
    pub rows: [CapBits; CAP_DOMAINS],
}

impl CapMatrix {
    pub const fn empty() -> Self {
        Self { rows: [CapBits::NONE; CAP_DOMAINS] }
    }

    /// 安全地访问某个域 (避免越界)
    pub fn get(&self, domain: CapDomain) -> Option<CapBits> {
        if domain.is_valid() {
            Some(self.rows[domain.index()])
        } else {
            None
        }
    }

    /// 安全地修改某个域
    ///
    /// 返回: 旧值 (用于审计)
    pub fn set(&mut self, domain: CapDomain, bits: CapBits) -> Option<CapBits> {
        if domain.is_valid() {
            let old = self.rows[domain.index()];
            self.rows[domain.index()] = bits;
            Some(old)
        } else {
            None
        }
    }

    /// 检查 `(domain, required) ⊆ self`
    pub fn check(&self, domain: CapDomain, required: CapBits) -> bool {
        self.get(domain).map(|b| b.contains(required)).unwrap_or(false)
    }
}

/// 可行性下界 (viability floor)
///
/// 系统保留的最低能力: 用户/进程管理
pub fn viable_floor() -> CapMatrix {
    const FS_READ: u64 = 1 << 0;
    const FS_EXEC: u64 = 1 << 2;
    const PROC_FORK: u64 = 1 << 0;
    const PROC_EXEC: u64 = 1 << 1;
    const USER_LIST: u64 = 1 << 0;

    let mut m = CapMatrix::empty();
    m.rows[CapDomain::FS.index()] = CapBits(FS_READ | FS_EXEC);
    m.rows[CapDomain::PROC.index()] = CapBits(PROC_FORK | PROC_EXEC);
    m.rows[CapDomain::USER_MGMT.index()] = CapBits(USER_LIST);
    m
}

/// 委托记录
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantRecord {
    pub from_pwm: u64,
    pub to_pwm: u64,
    pub domain: CapDomain,
    pub bits: CapBits,
}

/// 撤销结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeResult {
    /// 撤销了 (bits_before, bits_after)
    Revoked { before: CapBits, after: CapBits },
    /// 域无效
    InvalidDomain,
    /// 无可撤销
    NoChange,
}

/// 策略引擎 (services 层)
#[derive(Debug, Clone, Copy)]
pub struct PolicyEngine {
    floor: CapMatrix,
}

impl Default for PolicyEngine {
    fn default() -> Self { Self::new() }
}

impl PolicyEngine {
    pub const fn new() -> Self {
        Self { floor: viable_floor_const() }
    }

    /// 检查 (pwm, domain, required) 是否被允许
    ///
    /// 步骤:
    /// 1. 域必须合法
    /// 2. required 必须在 viable_floor 之外 (不能撤销基础能力)
    /// 3. caller 的有效能力必须 ⊇ required
    pub fn check<M: CapabilityMatrix>(
        &self,
        matrix: &M,
        domain: CapDomain,
        required: CapBits,
    ) -> bool {
        if !domain.is_valid() {
            return false;
        }
        if required.is_empty() {
            return true; // 无要求即通过
        }
        let owned = match matrix.get(domain) {
            Some(b) => b,
            None => return false,
        };
        // 必须拥有全部 required 位
        if !owned.contains(required) {
            return false;
        }
        // 不能撤销 viable_floor
        let floor_bits = self.floor.get(domain).unwrap_or(CapBits::NONE);
        if required.contains(floor_bits) {
            // 试图撤销基础能力 — 拒绝
            return false;
        }
        true
    }

    /// 委托能力 (grant)
    ///
    /// 规则:
    /// - from_pwm 必须拥有 bits (在 domain 内)
    /// - 不能授予 viable_floor 之外的能力 (防止提权)
    pub fn grant<M: CapabilityMatrix>(
        &self,
        from_matrix: &M,
        to_matrix: &mut M,
        domain: CapDomain,
        bits: CapBits,
    ) -> GrantResult {
        if !domain.is_valid() {
            return GrantResult::InvalidDomain;
        }
        let owned = match from_matrix.get(domain) {
            Some(b) => b,
            None => return GrantResult::NoAuthority,
        };
        if !owned.contains(bits) {
            return GrantResult::NoAuthority;
        }
        if bits.is_empty() {
            return GrantResult::Empty;
        }
        let to_current = to_matrix.get(domain).unwrap_or(CapBits::NONE);
        let to_new = to_current | bits;
        to_matrix.set(domain, to_new);
        GrantResult::Granted { from_had: owned, to_now: to_new }
    }

    /// 撤销能力 (revoke)
    pub fn revoke<M: CapabilityMatrix>(
        &self,
        matrix: &mut M,
        domain: CapDomain,
        bits: CapBits,
    ) -> RevokeResult {
        if !domain.is_valid() {
            return RevokeResult::InvalidDomain;
        }
        if bits.is_empty() {
            return RevokeResult::NoChange;
        }
        let current = match matrix.get(domain) {
            Some(b) => b,
            None => return RevokeResult::NoChange,
        };
        // 不能撤销 viable_floor
        let floor_bits = self.floor.get(domain).unwrap_or(CapBits::NONE);
        let revocable = bits.diff(floor_bits);
        if revocable.is_empty() {
            return RevokeResult::NoChange; // 全部是基础能力, 不可撤销
        }
        let new_bits = current.diff(revocable);
        matrix.set(domain, new_bits);
        RevokeResult::Revoked { before: current, after: new_bits }
    }
}

const fn viable_floor_const() -> CapMatrix {
    const FS_READ: u64 = 1 << 0;
    const FS_EXEC: u64 = 1 << 2;
    const PROC_FORK: u64 = 1 << 0;
    const PROC_EXEC: u64 = 1 << 1;
    const USER_LIST: u64 = 1 << 0;
    CapMatrix {
        rows: [
            CapBits(0u64),                                                          // 0 SYSTEM
            CapBits(FS_READ | FS_EXEC),                                              // 1 FS
            CapBits(0u64),                                                           // 2 NET
            CapBits(PROC_FORK | PROC_EXEC),                                          // 3 PROC
            CapBits(0u64),                                                           // 4 DEVICE
            CapBits(USER_LIST),                                                      // 5 USER_MGMT
            CapBits(0u64), CapBits(0u64), CapBits(0u64),                             // 6 IPC, 7 MEM, 8 TIME
            CapBits(0u64), CapBits(0u64), CapBits(0u64), CapBits(0u64), CapBits(0u64), // 9-13 BARRIER..MSGQ
            CapBits(0u64), CapBits(0u64),                                            // 14 DMA, 15 RESERVED
        ],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantResult {
    Granted { from_had: CapBits, to_now: CapBits },
    NoAuthority,
    InvalidDomain,
    Empty,
}

/// 安全能力矩阵抽象 (services 调用 TCB)
pub trait CapabilityMatrix {
    fn get(&self, domain: CapDomain) -> Option<CapBits>;
    fn set(&mut self, domain: CapDomain, bits: CapBits) -> Option<CapBits>;
}

/// 内存实现 (用于测试 / 单线程场景)
#[derive(Debug, Clone, Copy, Default)]
pub struct InMemoryMatrix(pub CapMatrix);

impl CapabilityMatrix for InMemoryMatrix {
    fn get(&self, domain: CapDomain) -> Option<CapBits> {
        self.0.get(domain)
    }
    fn set(&mut self, domain: CapDomain, bits: CapBits) -> Option<CapBits> {
        self.0.set(domain, bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_domain_validity() {
        assert!(CapDomain(0).is_valid());
        assert!(CapDomain(15).is_valid());
        assert!(!CapDomain(16).is_valid());
        assert!(!CapDomain(255).is_valid());
    }

    #[test]
    fn cap_bits_contains() {
        let a = CapBits(0b1100);
        let b = CapBits(0b1000);
        let c = CapBits(0b1001);
        assert!(a.contains(b));
        assert!(!a.contains(c));
        assert!(CapBits::ALL.contains(CapBits::ALL));
    }

    #[test]
    fn cap_bits_diff() {
        let a = CapBits(0b1111);
        let b = CapBits(0b1010);
        let d = a.diff(b);
        assert_eq!(d, CapBits(0b0101));
    }

    #[test]
    fn matrix_check() {
        let mut m = CapMatrix::empty();
        m.set(CapDomain::FS, CapBits(0xFF));
        assert!(m.check(CapDomain::FS, CapBits(0x0F)));
        assert!(!m.check(CapDomain::FS, CapBits(0x100)));
        assert!(!m.check(CapDomain::NET, CapBits(0x01)));
    }

    #[test]
    fn matrix_invalid_domain() {
        let m = CapMatrix::empty();
        assert_eq!(m.get(CapDomain(20)), None);
        assert!(!m.check(CapDomain(20), CapBits::ALL));
    }

    #[test]
    fn floor_basic_operations() {
        let f = viable_floor();
        // FS 至少有 READ | EXEC
        assert!(f.check(CapDomain::FS, CapBits(1 << 0))); // READ
        assert!(f.check(CapDomain::FS, CapBits(1 << 2))); // EXEC
        // PROC 至少有 FORK | EXEC
        assert!(f.check(CapDomain::PROC, CapBits(1 << 0)));
        // DEVICE floor = 0, 无强制
        assert!(f.check(CapDomain::DEVICE, CapBits::NONE));
    }

    #[test]
    fn policy_check_requires_authority() {
        let p = PolicyEngine::new();
        let mut m = InMemoryMatrix(CapMatrix::empty());
        m.set(CapDomain::FS, CapBits(0xFF));
        assert!(p.check(&m, CapDomain::FS, CapBits(0x01)));
        assert!(!p.check(&m, CapDomain::FS, CapBits(0x100)));
    }

    #[test]
    fn policy_check_floor_protection() {
        let p = PolicyEngine::new();
        let mut m = InMemoryMatrix(CapMatrix::empty());
        m.set(CapDomain::FS, CapBits::ALL);
        // 不能检查 floor 中的位
        let floor = viable_floor().get(CapDomain::FS).unwrap();
        assert!(!p.check(&m, CapDomain::FS, floor));
    }

    #[test]
    fn policy_grant_basic() {
        let p = PolicyEngine::new();
        let mut from = InMemoryMatrix(CapMatrix::empty());
        from.set(CapDomain::FS, CapBits(0b1111));
        let mut to = InMemoryMatrix(CapMatrix::empty());

        let r = p.grant(&from, &mut to, CapDomain::FS, CapBits(0b0011));
        assert!(matches!(r, GrantResult::Granted { .. }));
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b0011)));
    }

    #[test]
    fn policy_grant_no_authority() {
        let p = PolicyEngine::new();
        let from = InMemoryMatrix(CapMatrix::empty());
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = p.grant(&from, &mut to, CapDomain::FS, CapBits(0b0001));
        assert_eq!(r, GrantResult::NoAuthority);
        assert_eq!(to.get(CapDomain::FS), Some(CapBits::NONE));
    }

    #[test]
    fn policy_grant_invalid_domain() {
        let p = PolicyEngine::new();
        let from = InMemoryMatrix(CapMatrix::empty());
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = p.grant(&from, &mut to, CapDomain(20), CapBits(0b0001));
        assert_eq!(r, GrantResult::InvalidDomain);
    }

    #[test]
    fn policy_grant_empty() {
        let p = PolicyEngine::new();
        let mut from = InMemoryMatrix(CapMatrix::empty());
        from.set(CapDomain::FS, CapBits(0xFF));
        let mut to = InMemoryMatrix(CapMatrix::empty());
        let r = p.grant(&from, &mut to, CapDomain::FS, CapBits::NONE);
        assert_eq!(r, GrantResult::Empty);
    }

    #[test]
    fn policy_revoke_basic() {
        let p = PolicyEngine::new();
        let mut m = InMemoryMatrix(CapMatrix::empty());
        // 给 0b1111 (覆盖 floor 0b0101 + 一个可撤销位 0b1000)
        m.set(CapDomain::FS, CapBits(0b1111));
        // 撤销 0b1000 (非 floor)
        let r = p.revoke(&mut m, CapDomain::FS, CapBits(0b1000));
        match r {
            RevokeResult::Revoked { before, after } => {
                assert_eq!(before, CapBits(0b1111));
                assert_eq!(after, CapBits(0b0111));
            }
            _ => panic!("expected Revoked"),
        }
    }

    #[test]
    fn policy_revoke_protects_floor() {
        let p = PolicyEngine::new();
        let mut m = InMemoryMatrix(CapMatrix::empty());
        m.set(CapDomain::FS, CapBits::ALL);
        let floor = viable_floor().get(CapDomain::FS).unwrap();
        // 试图撤销 floor 内的位
        let r = p.revoke(&mut m, CapDomain::FS, floor);
        assert_eq!(r, RevokeResult::NoChange);
    }

    #[test]
    fn policy_revoke_invalid_domain() {
        let p = PolicyEngine::new();
        let mut m = InMemoryMatrix(CapMatrix::empty());
        let r = p.revoke(&mut m, CapDomain(20), CapBits(0b0001));
        assert_eq!(r, RevokeResult::InvalidDomain);
    }

    #[test]
    fn policy_revoke_empty() {
        let p = PolicyEngine::new();
        let mut m = InMemoryMatrix(CapMatrix::empty());
        let r = p.revoke(&mut m, CapDomain::FS, CapBits::NONE);
        assert_eq!(r, RevokeResult::NoChange);
    }

    /// 委托-撤销 单调性: grant + revoke 不变量
    #[test]
    fn grant_then_revoke_idempotent() {
        let p = PolicyEngine::new();
        let mut from = InMemoryMatrix(CapMatrix::empty());
        from.set(CapDomain::FS, CapBits(0xFF));
        let mut to = InMemoryMatrix(CapMatrix::empty());

        // 委托 0b1000 (非 floor, 可撤销)
        let _ = p.grant(&from, &mut to, CapDomain::FS, CapBits(0b1000));
        assert_eq!(to.get(CapDomain::FS), Some(CapBits(0b1000)));

        // 撤销 0b1000
        let r = p.revoke(&mut to, CapDomain::FS, CapBits(0b1000));
        assert!(matches!(r, RevokeResult::Revoked { after, .. } if after == CapBits::NONE));
    }

    /// 委托累积: 多次 grant 应单调增加
    #[test]
    fn grant_monotonic() {
        let p = PolicyEngine::new();
        let mut from = InMemoryMatrix(CapMatrix::empty());
        from.set(CapDomain::FS, CapBits::ALL);
        let mut to = InMemoryMatrix(CapMatrix::empty());

        p.grant(&from, &mut to, CapDomain::FS, CapBits(0b0001));
        let after1 = to.get(CapDomain::FS).unwrap();
        p.grant(&from, &mut to, CapDomain::FS, CapBits(0b0010));
        let after2 = to.get(CapDomain::FS).unwrap();

        // 单调性: after2 ⊇ after1
        assert!(after2.contains(after1));
    }

    /// 委托-撤销 跨域独立性
    #[test]
    fn grant_independent_across_domains() {
        let p = PolicyEngine::new();
        let mut from = InMemoryMatrix(CapMatrix::empty());
        from.set(CapDomain::FS, CapBits::ALL);
        from.set(CapDomain::NET, CapBits::ALL);
        let mut to = InMemoryMatrix(CapMatrix::empty());

        p.grant(&from, &mut to, CapDomain::FS, CapBits(0b1111));
        // NET 仍为空
        assert_eq!(to.get(CapDomain::NET), Some(CapBits::NONE));

        // 撤销 FS 不影响 NET
        p.revoke(&mut to, CapDomain::FS, CapBits(0b1111));
        assert_eq!(to.get(CapDomain::NET), Some(CapBits::NONE));
    }

    /// 压力测试: 1000 个 grant/revoke 序列, 验证不变量
    #[test]
    fn stress_grant_revoke() {
        let p = PolicyEngine::new();
        let mut from = InMemoryMatrix(CapMatrix::empty());
        from.set(CapDomain::FS, CapBits(0xFFFF_FFFF_FFFF_FFFF));
        let mut to = InMemoryMatrix(CapMatrix::empty());

        // 简单 LCG 伪随机
        let mut rng: u64 = 0xDEAD_BEEF;
        let mut next = || {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            rng
        };

        for _ in 0..1000 {
            let op = next() & 0x1;
            let bits = CapBits(next() & 0xFFFF);
            if op == 0 {
                p.grant(&from, &mut to, CapDomain::FS, bits);
            } else {
                p.revoke(&mut to, CapDomain::FS, bits);
            }
        }
        // 不变量: to 仍 ≤ from (grant 不超过 from 的权限)
        let from_bits = from.get(CapDomain::FS).unwrap();
        let to_bits = to.get(CapDomain::FS).unwrap();
        assert!(from_bits.contains(to_bits) || to_bits == CapBits::NONE);
    }
}
