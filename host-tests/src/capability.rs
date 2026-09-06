// B08-12 (DECISION-052 路线 C): 能力矩阵消除平行实现.
//
// 本模块不再本地实现 CapBits/CapabilityMatrix (16 域位矩阵复刻已删除),
// 回归测试重写为直接验证内核 `services::credo::policy` 规范实现:
// - `CapBits`      — 位操作原语 (contains/diff/is_empty/BitOr)
// - `CapMatrix`    — 不可变能力位图快照 (empty/all/from_bits/get)
// - `InMemoryMatrix` — 可变能力矩阵 (get/set, CapabilityMatrix trait)
// - 域常量与能力位常量 — `services::credo::capability`
// 原 `#![allow(dead_code)]` (F9 违规) 随实现删除而消失.

#[cfg(test)]
mod tests {
    use queenx::kernel::services::credo::capability::{
        FS_CAP_CHOWN, FS_CAP_DELETE, FS_CAP_EXECUTE, FS_CAP_READ, FS_CAP_WRITE, PROC_CAP_EXEC,
        PROC_CAP_FORK, PROC_CAP_KILL, SYS_CAP_ALL,
    };
    use queenx::kernel::services::credo::policy::{
        CapBits, CapDomain, CapMatrix, InMemoryMatrix, CapabilityMatrix, VIABLE_FLOOR,
    };

    #[test]
    fn cap_bits_has() {
        let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
        assert!(cb.contains(CapBits(FS_CAP_READ)));
        assert!(cb.contains(CapBits(FS_CAP_WRITE)));
        assert!(!cb.contains(CapBits(FS_CAP_EXECUTE)));
    }

    #[test]
    fn cap_bits_grant() {
        let cb = CapBits(FS_CAP_READ) | CapBits(FS_CAP_WRITE);
        assert!(cb.contains(CapBits(FS_CAP_READ)));
        assert!(cb.contains(CapBits(FS_CAP_WRITE)));
    }

    #[test]
    fn cap_bits_revoke() {
        let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE).diff(CapBits(FS_CAP_READ));
        assert!(!cb.contains(CapBits(FS_CAP_READ)));
        assert!(cb.contains(CapBits(FS_CAP_WRITE)));
    }

    #[test]
    fn cap_bits_superset() {
        let full = CapBits(FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE);
        let partial = CapBits(FS_CAP_READ);
        assert!(full.contains(partial));
        assert!(!partial.contains(full));
    }

    #[test]
    fn cap_matrix_new_empty() {
        let cm = InMemoryMatrix::new();
        assert_eq!(cm.get(CapDomain::FS), Some(CapBits::NONE));
        assert_eq!(cm.get(CapDomain::PROC), Some(CapBits::NONE));
    }

    #[test]
    fn cap_matrix_grant_revoke() {
        let cm = InMemoryMatrix::new();
        cm.set(CapDomain::FS, CapBits(FS_CAP_READ | FS_CAP_WRITE)).unwrap();
        assert!(cm
            .get(CapDomain::FS)
            .unwrap()
            .contains(CapBits(FS_CAP_READ)));
        assert!(cm
            .get(CapDomain::FS)
            .unwrap()
            .contains(CapBits(FS_CAP_WRITE)));
        cm.set(CapDomain::FS, CapBits(FS_CAP_READ)).unwrap();
        assert!(cm
            .get(CapDomain::FS)
            .unwrap()
            .contains(CapBits(FS_CAP_READ)));
        assert!(!cm
            .get(CapDomain::FS)
            .unwrap()
            .contains(CapBits(FS_CAP_WRITE)));
    }

    #[test]
    fn cap_matrix_all() {
        let cm = CapMatrix::all();
        for d in 0..16u8 {
            assert_eq!(cm.get(CapDomain(d)), CapBits::ALL);
        }
    }

    #[test]
    fn cap_matrix_viable() {
        let cm = CapMatrix::from_bits(VIABLE_FLOOR);
        assert!(cm
            .get(CapDomain::FS)
            .contains(CapBits(FS_CAP_READ)));
        assert!(cm
            .get(CapDomain::FS)
            .contains(CapBits(FS_CAP_EXECUTE)));
        assert!(!cm
            .get(CapDomain::FS)
            .contains(CapBits(FS_CAP_WRITE)));
        assert!(cm
            .get(CapDomain::PROC)
            .contains(CapBits(PROC_CAP_FORK)));
        assert!(cm
            .get(CapDomain::PROC)
            .contains(CapBits(PROC_CAP_EXEC)));
    }

    #[test]
    fn cap_matrix_superset() {
        let parent = CapMatrix::all();
        let child = CapMatrix::from_bits(VIABLE_FLOOR);
        for d in 0..16u8 {
            assert!(parent.get(CapDomain(d)).contains(child.get(CapDomain(d))));
            assert!(!child.get(CapDomain(d)).contains(parent.get(CapDomain(d))));
        }
    }

    #[test]
    fn cap_matrix_out_of_range() {
        let cm = InMemoryMatrix::new();
        assert!(!CapDomain(16).is_valid());
        assert!(!CapDomain(255).is_valid());
        assert_eq!(cm.get(CapDomain(16)), None);
        assert_eq!(cm.get(CapDomain(255)), None);
    }

    #[test]
    fn cap_bits_empty_has_nothing() {
        let cb = CapBits::NONE;
        assert!(!cb.contains(CapBits(FS_CAP_READ)));
        assert!(!cb.contains(CapBits(FS_CAP_WRITE)));
        assert!(!cb.contains(CapBits(SYS_CAP_ALL)));
    }

    #[test]
    fn cap_bits_grant_all_then_revoke_one() {
        let cb = CapBits::ALL;
        assert!(cb.contains(CapBits(FS_CAP_READ)));
        assert!(cb.contains(CapBits(SYS_CAP_ALL)));
        let cb = cb.diff(CapBits(FS_CAP_READ));
        assert!(!cb.contains(CapBits(FS_CAP_READ)));
        assert!(cb.contains(CapBits(FS_CAP_WRITE)));
    }

    #[test]
    fn cap_bits_revoke_nonexistent_is_noop() {
        let cb = CapBits(FS_CAP_READ).diff(CapBits(FS_CAP_WRITE));
        assert!(cb.contains(CapBits(FS_CAP_READ)));
        assert!(!cb.contains(CapBits(FS_CAP_WRITE)));
    }

    #[test]
    fn cap_bits_grant_idempotent() {
        let cb = CapBits(FS_CAP_READ) | CapBits(FS_CAP_READ);
        assert!(cb.contains(CapBits(FS_CAP_READ)));
        assert_eq!(cb, CapBits(FS_CAP_READ));
    }

    #[test]
    fn cap_matrix_delegation_chain() {
        let root = CapMatrix::all();
        let admin = InMemoryMatrix::new();
        admin
            .set(
                CapDomain::FS,
                CapBits(FS_CAP_READ | FS_CAP_WRITE | FS_CAP_EXECUTE | (1 << 3) | (1 << 4)),
            )
            .unwrap();
        admin
            .set(
                CapDomain::PROC,
                CapBits(PROC_CAP_FORK | PROC_CAP_EXEC | PROC_CAP_KILL),
            )
            .unwrap();
        let user = InMemoryMatrix::new();
        user.set(CapDomain::FS, CapBits(FS_CAP_READ | FS_CAP_EXECUTE))
            .unwrap();
        user.set(CapDomain::PROC, CapBits(PROC_CAP_FORK | PROC_CAP_EXEC))
            .unwrap();
        assert!(root.get(CapDomain::FS).contains(admin.get(CapDomain::FS).unwrap()));
        assert!(admin
            .get(CapDomain::FS)
            .unwrap()
            .contains(user.get(CapDomain::FS).unwrap()));
        assert!(!user
            .get(CapDomain::FS)
            .unwrap()
            .contains(admin.get(CapDomain::FS).unwrap()));
    }

    #[test]
    fn cap_matrix_revocation_partial() {
        let all = CapMatrix::all();
        let fs_bits = all.get(CapDomain::FS).diff(CapBits(FS_CAP_DELETE | FS_CAP_CHOWN));
        assert!(fs_bits.contains(CapBits(FS_CAP_READ)));
        assert!(fs_bits.contains(CapBits(FS_CAP_WRITE)));
        assert!(!fs_bits.contains(CapBits(FS_CAP_DELETE)));
        assert!(!fs_bits.contains(CapBits(FS_CAP_CHOWN)));
    }

    #[test]
    fn cap_matrix_viable_is_not_all() {
        let viable = CapMatrix::from_bits(VIABLE_FLOOR);
        let all = CapMatrix::all();
        assert!(all.get(CapDomain::FS).contains(viable.get(CapDomain::FS)));
        assert!(!viable.get(CapDomain::FS).contains(all.get(CapDomain::FS)));
    }

    #[test]
    fn cap_matrix_grant_out_of_range_silent() {
        let cm = InMemoryMatrix::new();
        assert!(cm.set(CapDomain(16), CapBits(0xFF)).is_err());
        assert!(cm.set(CapDomain(255), CapBits(0xFF)).is_err());
        assert_eq!(cm.get(CapDomain(16)), None);
        assert_eq!(cm.get(CapDomain(255)), None);
    }

    #[test]
    fn cap_matrix_revoke_out_of_range_silent() {
        // 内核 set 对非法域返回 Err, 矩阵内容不变 (等价原 revoke 静默失败)
        let cm = InMemoryMatrix::new();
        assert!(cm.set(CapDomain(16), CapBits::ALL).is_err());
        assert!(cm.set(CapDomain(255), CapBits::ALL).is_err());
        for d in 0..16u8 {
            assert_eq!(cm.get(CapDomain(d)), Some(CapBits::NONE));
        }
    }

    #[test]
    fn cap_matrix_cross_domain_isolation() {
        let cm = InMemoryMatrix::new();
        cm.set(CapDomain::FS, CapBits(FS_CAP_READ)).unwrap();
        assert_eq!(cm.get(CapDomain::PROC), Some(CapBits::NONE));
        assert_eq!(cm.get(CapDomain::NET), Some(CapBits::NONE));
    }

    #[test]
    fn cap_matrix_empty_not_superset_of_viable() {
        let empty = CapMatrix::empty();
        let viable = CapMatrix::from_bits(VIABLE_FLOOR);
        assert!(!empty
            .get(CapDomain::FS)
            .contains(viable.get(CapDomain::FS)));
    }

    #[test]
    fn cap_bits_superset_reflexive() {
        let cb = CapBits(FS_CAP_READ | FS_CAP_WRITE);
        assert!(cb.contains(cb));
    }

    #[test]
    fn cap_matrix_superset_reflexive() {
        let cm = CapMatrix::from_bits(VIABLE_FLOOR);
        assert!(cm.get(CapDomain::FS).contains(cm.get(CapDomain::FS)));
    }
}
