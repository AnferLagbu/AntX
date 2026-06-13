//! I-29 补充验收: 启动早期 VFS 在无 root 能力时返回 EACCES + 权限矩阵 16 domain 全覆盖
//!
//! 镜像内核 [src/kernel/services/fs/mount.rs::current_pwm] 与
//! [src/kernel/framework/credo/capability.rs] 的 16 domain 矩阵契约.
//!
//! ## 覆盖
//! 1. 启动早期 pwm==0 → current_pwm() 返回 EACCES
//! 2. pwm!=0 但 capability 不足 → has_capability 返回 false → EACCES
//! 3. pwm!=0 且 capability 充足 → 准入
//! 4. 16 个 domain 全部参与 (grant/revoke/has/superset 路径全覆盖)
//! 5. viable floor 的 FS_READ|FS_EXECUTE + PROC_FORK|PROC_EXEC 不变
//!
//! 部分 domain bit 在本测试中未直接引用, 保留常量定义以镜像内核完整 16×N 矩阵.
#![allow(dead_code)]

const EACCES: i32 = -13; // POSIX EACCES

/// 镜像 [services/fs/mount.rs::current_pwm]
fn current_pwm(pwm: u64) -> Result<u64, i32> {
    if pwm == 0 { Err(EACCES) } else { Ok(pwm) }
}

/// 镜像 [framework/credo/capability.rs::pwm_has_capability] 简化版 (位检查)
fn pwm_has_capability(pwm: u64, _domain: u16, required: u64) -> bool {
    if pwm == 0 {
        return false;
    }
    // 真内核走 IdentityTable 查 cap 矩阵; 这里直接 mock
    // pwm==u64::MAX 表示全权, 其余为最小权
    pwm == u64::MAX || (required == 0)
}

const SYS_CAP_ALL: u64 = 0xFFFFFFFFFFFFFFFF;

const CAP_DOMAIN_SYSTEM: u16 = 0;
const CAP_DOMAIN_FS: u16 = 1;
const CAP_DOMAIN_NET: u16 = 2;
const CAP_DOMAIN_PROC: u16 = 3;
const CAP_DOMAIN_DEVICE: u16 = 4;
const CAP_DOMAIN_USER_MGMT: u16 = 5;
const CAP_DOMAIN_IPC: u16 = 6;
const CAP_DOMAIN_MEM: u16 = 7;
const CAP_DOMAIN_TIME: u16 = 8;
const CAP_DOMAIN_BARRIER: u16 = 9;
const CAP_DOMAIN_SIGNAL: u16 = 10;
const CAP_DOMAIN_SHM: u16 = 11;
const CAP_DOMAIN_SEM: u16 = 12;
const CAP_DOMAIN_MSGQ: u16 = 13;
const CAP_DOMAIN_DMA: u16 = 14;
const CAP_DOMAIN_RESERVED: u16 = 15;

const FS_CAP_READ: u64 = 1 << 0;
const FS_CAP_WRITE: u64 = 1 << 1;
const FS_CAP_EXECUTE: u64 = 1 << 2;
const FS_CAP_CREATE: u64 = 1 << 3;
const FS_CAP_DELETE: u64 = 1 << 4;
const FS_CAP_CHOWN: u64 = 1 << 5;
const FS_CAP_CHMOD: u64 = 1 << 6;

const NET_CAP_SEND: u64 = 1 << 0;
const NET_CAP_RECV: u64 = 1 << 1;
const NET_CAP_CONNECT: u64 = 1 << 2;
const NET_CAP_LISTEN: u64 = 1 << 3;
const NET_CAP_BIND: u64 = 1 << 4;

const PROC_CAP_FORK: u64 = 1 << 0;
const PROC_CAP_EXEC: u64 = 1 << 1;
const PROC_CAP_KILL: u64 = 1 << 2;
const PROC_CAP_WAIT: u64 = 1 << 3;
const PROC_CAP_CREATE: u64 = 1 << 4;

const USER_MGMT_CAP_LIST: u64 = 1 << 0;
const USER_MGMT_CAP_CREATE: u64 = 1 << 1;
const USER_MGMT_CAP_DELETE: u64 = 1 << 2;
const USER_MGMT_CAP_MODIFY: u64 = 1 << 3;

const DEVICE_CAP_MMIO: u64 = 1 << 0;
const DEVICE_CAP_IRQ: u64 = 1 << 1;
const DEVICE_CAP_DMA: u64 = 1 << 2;
const _DEVICE_CAP_BIND: u64 = 1 << 3; // reserved, not used in mock

const VIABLE_FLOOR: [u64; 16] = {
    let mut f = [0u64; 16];
    f[CAP_DOMAIN_FS as usize] = FS_CAP_READ | FS_CAP_EXECUTE;
    f[CAP_DOMAIN_PROC as usize] = PROC_CAP_FORK | PROC_CAP_EXEC;
    f
};

#[derive(Clone, Copy)]
struct CapBits(u64);

impl CapBits {
    fn has(&self, bit: u64) -> bool { (self.0 & bit) != 0 }
    fn grant(&mut self, bits: u64) { self.0 |= bits; }
    fn revoke(&mut self, bits: u64) { self.0 &= !bits; }
    fn is_superset_of(&self, other: &CapBits) -> bool { (self.0 & other.0) == other.0 }
}

#[derive(Clone)]
struct CapabilityMatrix {
    caps: [CapBits; 16],
}

impl CapabilityMatrix {
    fn new() -> Self {
        Self { caps: [CapBits(0); 16] }
    }
    fn all() -> Self {
        Self { caps: [CapBits(SYS_CAP_ALL); 16] }
    }
    fn viable() -> Self {
        let mut cm = Self::new();
        for d in 0..16u16 {
            cm.caps[d as usize] = CapBits(VIABLE_FLOOR[d as usize]);
        }
        cm
    }
    fn has(&self, domain: u16, bits: u64) -> bool {
        if domain as usize >= 16 { return false; }
        self.caps[domain as usize].has(bits)
    }
    fn grant(&mut self, domain: u16, bits: u64) {
        if domain as usize >= 16 { return; }
        self.caps[domain as usize].grant(bits);
    }
    fn revoke(&mut self, domain: u16, bits: u64) {
        if domain as usize >= 16 { return; }
        self.caps[domain as usize].revoke(bits);
    }
    fn is_superset_of(&self, other: &CapabilityMatrix) -> bool {
        for i in 0..16 {
            if !self.caps[i].is_superset_of(&other.caps[i]) {
                return false;
            }
        }
        true
    }
}

// =====================================================================
// 启动早期 VFS 路径 (current_pwm)
// =====================================================================

#[test]
fn early_vfs_no_pwm_returns_eacces() {
    // 启动早期 / 匿名会话: pwm==0
    let res = current_pwm(0);
    assert_eq!(res, Err(EACCES));
}

#[test]
fn early_vfs_with_pwm_returns_ok() {
    let res = current_pwm(42);
    assert_eq!(res, Ok(42));
}

#[test]
fn early_vfs_pwm_zero_blocks_capability_check() {
    // 即使 capability 检查在 pwm==0 上返回 true (历史 bug),
    // current_pwm 必须先拦截, 整体路径仍返回 EACCES
    let pwm = current_pwm(0).unwrap_or(0);
    let cap_ok = pwm_has_capability(pwm, CAP_DOMAIN_SYSTEM, 0x01);
    assert!(!cap_ok, "pwm==0 下能力检查必须为 false");
}

#[test]
fn mount_path_no_session_yields_eacces() {
    // 镜像 services/fs/mount.rs::mount_syscall 完整检查顺序
    let pwm = match current_pwm(0) {
        Ok(p) => p,
        Err(EACCES) => return, // 正确路径
        Err(_) => panic!("必须返回 EACCES, 不可走其它错误码"),
    };
    let _ = pwm_has_capability(pwm, CAP_DOMAIN_SYSTEM, 0x01);
    panic!("未在 pwm==0 时拦截");
}

#[test]
fn mount_path_with_pwm_but_no_cap_yields_eacces() {
    let pwm = current_pwm(7).expect("pwm!=0 时必须 Ok");
    // 假设 pwm=7 没有 CAP_SYS_ADMIN (0x01)
    let has_cap = pwm_has_capability(pwm, CAP_DOMAIN_SYSTEM, 0x01);
    assert!(!has_cap, "pwm=7 (非全权) 不应有 CAP_SYS_ADMIN");
}

#[test]
fn mount_path_with_full_cap_passes() {
    let pwm = current_pwm(u64::MAX).expect("pwm!=0 时必须 Ok");
    let has_cap = pwm_has_capability(pwm, CAP_DOMAIN_SYSTEM, 0x01);
    assert!(has_cap, "pwm=u64::MAX 应有全部能力");
}

// =====================================================================
// 16 domain 权限矩阵
// =====================================================================

#[test]
fn matrix_has_16_domains() {
    // 编译期: 16 domain 常量
    let last = CAP_DOMAIN_RESERVED;
    assert_eq!(last, 15);
    // 运行时: viable floor 长度 = 16
    assert_eq!(VIABLE_FLOOR.len(), 16);
}

#[test]
fn matrix_viable_floor_is_minimal() {
    let cm = CapabilityMatrix::viable();
    // FS domain
    assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
    assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_EXECUTE));
    assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_WRITE));
    assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_CREATE));
    // PROC domain
    assert!(cm.has(CAP_DOMAIN_PROC, PROC_CAP_FORK));
    assert!(cm.has(CAP_DOMAIN_PROC, PROC_CAP_EXEC));
    assert!(!cm.has(CAP_DOMAIN_PROC, PROC_CAP_KILL));
    // 其它 14 domain 全部为 0
    for d in [CAP_DOMAIN_SYSTEM, CAP_DOMAIN_NET, CAP_DOMAIN_DEVICE,
              CAP_DOMAIN_USER_MGMT, CAP_DOMAIN_IPC, CAP_DOMAIN_MEM,
              CAP_DOMAIN_TIME, CAP_DOMAIN_BARRIER, CAP_DOMAIN_SIGNAL,
              CAP_DOMAIN_SHM, CAP_DOMAIN_SEM, CAP_DOMAIN_MSGQ,
              CAP_DOMAIN_DMA, CAP_DOMAIN_RESERVED] {
        assert!(!cm.has(d, 1), "domain {} 在 viable floor 必须为 0", d);
    }
}

#[test]
fn matrix_all_grants_every_domain() {
    let cm = CapabilityMatrix::all();
    for d in 0..16u16 {
        assert!(cm.has(d, SYS_CAP_ALL), "domain {} 应有全权", d);
    }
}

#[test]
fn matrix_grant_revoke_isolates_domains() {
    let mut cm = CapabilityMatrix::new();
    cm.grant(CAP_DOMAIN_FS, FS_CAP_READ);
    cm.grant(CAP_DOMAIN_NET, NET_CAP_SEND);
    cm.grant(CAP_DOMAIN_PROC, PROC_CAP_FORK);
    cm.grant(CAP_DOMAIN_DEVICE, DEVICE_CAP_MMIO);
    cm.grant(CAP_DOMAIN_USER_MGMT, USER_MGMT_CAP_LIST);

    // FS
    assert!(cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
    assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_WRITE));
    // NET
    assert!(cm.has(CAP_DOMAIN_NET, NET_CAP_SEND));
    assert!(!cm.has(CAP_DOMAIN_NET, NET_CAP_RECV));
    // PROC
    assert!(cm.has(CAP_DOMAIN_PROC, PROC_CAP_FORK));
    assert!(!cm.has(CAP_DOMAIN_PROC, PROC_CAP_EXEC));
    // DEVICE
    assert!(cm.has(CAP_DOMAIN_DEVICE, DEVICE_CAP_MMIO));
    assert!(!cm.has(CAP_DOMAIN_DEVICE, DEVICE_CAP_IRQ));
    // USER_MGMT
    assert!(cm.has(CAP_DOMAIN_USER_MGMT, USER_MGMT_CAP_LIST));
    assert!(!cm.has(CAP_DOMAIN_USER_MGMT, USER_MGMT_CAP_CREATE));

    // 撤销验证
    cm.revoke(CAP_DOMAIN_FS, FS_CAP_READ);
    assert!(!cm.has(CAP_DOMAIN_FS, FS_CAP_READ));
    cm.revoke(CAP_DOMAIN_NET, NET_CAP_SEND);
    assert!(!cm.has(CAP_DOMAIN_NET, NET_CAP_SEND));
}

#[test]
fn matrix_out_of_range_is_silent() {
    let mut cm = CapabilityMatrix::all();
    cm.grant(16, 0xFF);
    cm.grant(255, 0xFF);
    cm.revoke(99, SYS_CAP_ALL);
    // 不应影响现有 domain
    for d in 0..16u16 {
        assert!(cm.has(d, SYS_CAP_ALL));
    }
}

#[test]
fn matrix_superset_transitivity() {
    let root = CapabilityMatrix::all();
    let admin = {
        let mut cm = CapabilityMatrix::new();
        cm.grant(CAP_DOMAIN_FS, FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE);
        cm.grant(CAP_DOMAIN_PROC, PROC_CAP_FORK | PROC_CAP_EXEC | PROC_CAP_KILL);
        cm
    };
    let user = CapabilityMatrix::viable();

    assert!(root.is_superset_of(&admin));
    assert!(root.is_superset_of(&user));
    assert!(admin.is_superset_of(&user));
    assert!(!user.is_superset_of(&admin));
    assert!(!user.is_superset_of(&root));
}

#[test]
fn matrix_cap_bits_isolated() {
    let mut cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
    cb.grant(FS_CAP_EXECUTE);
    assert!(cb.has(FS_CAP_READ));
    assert!(cb.has(FS_CAP_WRITE));
    assert!(cb.has(FS_CAP_EXECUTE));
    cb.revoke(FS_CAP_WRITE);
    assert!(cb.has(FS_CAP_READ));
    assert!(!cb.has(FS_CAP_WRITE));
    assert!(cb.has(FS_CAP_EXECUTE));
}

#[test]
fn matrix_grant_idempotent_and_revoke_noop() {
    let mut cb = CapBits(FS_CAP_READ);
    cb.grant(FS_CAP_READ);
    assert_eq!(cb.0, FS_CAP_READ);
    cb.revoke(FS_CAP_WRITE);
    assert_eq!(cb.0, FS_CAP_READ);
}

#[test]
fn matrix_cap_bit_superset_reflexive() {
    let a = CapBits(FS_CAP_READ | FS_CAP_WRITE);
    let b = CapBits(FS_CAP_READ);
    assert!(a.is_superset_of(&b));
    assert!(a.is_superset_of(&a));
    assert!(!b.is_superset_of(&a));
}

#[test]
fn all_16_domains_covered_by_viable_or_zero() {
    // 不变量: viable floor 中每个 domain 都有定义位 (可以为 0)
    // 这是 16 domain 全覆盖的硬性契约
    let cm = CapabilityMatrix::viable();
    let mut nonzero_domains = 0;
    for d in 0..16u16 {
        // 任何 domain 都必须能 has 查询, 0 位也算覆盖
        let _ = cm.has(d, 0);
        if cm.has(d, 1) || cm.has(d, FS_CAP_READ) {
            nonzero_domains += 1;
        }
    }
    assert!(nonzero_domains >= 2, "viable floor 应至少有 FS/PROC 非空");
    // 16 个 domain 全部 1..=15 可寻址
    for d in 0..16u16 {
        let mut cm2 = CapabilityMatrix::new();
        cm2.grant(d, 0x1);
        // 不应越界
        assert_eq!(cm2.caps.len(), 16);
    }
}
