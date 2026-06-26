//! mm: IoMem 别名检测集成测试
//!
//! 追踪: I-25
//! SPDX-License-Identifier: Apache-2.0
//!
//! 复刻 `src/kernel/framework/iomem.rs` 中的 `AliasRegistry` 逻辑,
//! 在 host 端进行集成测试, 验证:
//! 1. 区间重叠检测的边界条件 (前/后/包含/完全相同)
//! 2. 大量非冲突区间注册的性能和正确性
//! 3. 满 (64 项) 时返回错误
//! 4. unregister 后的碎片化注册
//! 5. saturating_add 溢出场景的健壮性
//!
//! ## 与单元测试的分工
//! 原 `src/iomem_alias.rs` 内联 `#[cfg(test)] mod tests` 共 16 个测试用例,
//! 合并入本文件作为集成测试, 避免双重编译.
//!
//! 注: 完整验证还需 QEMU 端运行, 见 `scripts/qemu_boot_test.sh`。
//! 这里主要覆盖 AliasRegistry 算法的集成场景.

#![allow(dead_code)]

const MAX_MMIO_MAPPINGS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Entry {
    phys: u64,
    len: usize,
    name: &'static str,
}

struct AliasRegistry {
    entries: Vec<Entry>,
    capacity: usize,
}

impl AliasRegistry {
    fn new() -> Self {
        Self { entries: Vec::with_capacity(MAX_MMIO_MAPPINGS), capacity: MAX_MMIO_MAPPINGS }
    }

    fn check_conflict(&self, phys: u64, len: usize) -> Option<&'static str> {
        let end = phys.saturating_add(len as u64);
        for e in &self.entries {
            let existing_end = e.phys.saturating_add(e.len as u64);
            // 标准区间重叠判定: [a, b) ∩ [c, d) ≠ ∅ ⇔ a < d && c < b
            if phys < existing_end && end > e.phys {
                return Some(e.name);
            }
        }
        None
    }

    fn register(&mut self, phys: u64, len: usize, name: &'static str) -> Result<(), &'static str> {
        if self.entries.len() >= self.capacity {
            return Err("MMIO alias registry full");
        }
        if phys & 0x3 != 0 {
            return Err("phys not 4-byte aligned");
        }
        if len == 0 {
            return Err("zero-length MMIO region");
        }
        if let Some(_conflict) = self.check_conflict(phys, len) {
            return Err("MMIO region overlaps existing region");
        }
        self.entries.push(Entry { phys, len, name });
        Ok(())
    }

    fn unregister(&mut self, phys: u64) -> Result<(), &'static str> {
        let pos = self.entries.iter().position(|e| e.phys == phys).ok_or("not found")?;
        self.entries.swap_remove(pos);
        Ok(())
    }

    fn count(&self) -> usize {
        self.entries.len()
    }

    fn capacity(&self) -> usize {
        self.capacity
    }
}

#[test]
fn test_register_basic() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x100, "dev1").is_ok());
    assert!(r.register(0x2000, 0x100, "dev2").is_ok());
    assert_eq!(r.count(), 2);
}

#[test]
fn test_alignment_check() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1001, 0x100, "dev").is_err());
}

#[test]
fn test_zero_length() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0, "dev").is_err());
}

#[test]
fn test_exact_overlap() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x100, "dev1").is_ok());
    assert!(r.register(0x1000, 0x100, "dev2").is_err());
}

#[test]
fn test_left_overlap() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x100, "dev1").is_ok());
    // 新区间左边界在 dev1 内部
    assert!(r.register(0x1080, 0x100, "dev2").is_err());
}

#[test]
fn test_right_overlap() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x100, "dev1").is_ok());
    // 新区间右边界覆盖 dev1 左边界
    assert!(r.register(0x0F80, 0x100, "dev2").is_err());
}

#[test]
fn test_complete_containment() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x1000, "dev1").is_ok());
    // 完全包含
    assert!(r.register(0x1100, 0x100, "dev2").is_err());
    // 从内部开头
    assert!(r.register(0x1000, 0x100, "dev3").is_err());
    // 到内部结尾
    assert!(r.register(0x1F00, 0x100, "dev4").is_err());
}

#[test]
fn test_touching_is_ok() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x100, "dev1").is_ok());
    // 紧邻但不重叠 (上一个 end == 下一个 phys)
    assert!(r.register(0x1100, 0x100, "dev2").is_ok());
    assert!(r.register(0x1200, 0x100, "dev3").is_ok());
    assert_eq!(r.count(), 3);
}

#[test]
fn test_unregister_and_reuse() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x100, "dev1").is_ok());
    assert!(r.unregister(0x1000).is_ok());
    // 释放后可以重新注册
    assert!(r.register(0x1000, 0x100, "dev1-revived").is_ok());
    assert_eq!(r.count(), 1);
}

#[test]
fn test_unregister_not_found() {
    let mut r = AliasRegistry::new();
    assert!(r.unregister(0x1000).is_err());
}

#[test]
fn test_unregister_then_register_overlap() {
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x200, "dev1").is_ok());
    assert!(r.register(0x1200, 0x200, "dev2").is_ok());
    // 释放 dev1 后, 0x1000-0x1200 不再被占用
    assert!(r.unregister(0x1000).is_ok());
    // 0x1100-0x1200 与 dev2 (0x1200-0x1400) 紧邻不重叠
    assert!(r.register(0x1100, 0x100, "dev3").is_ok());
    assert_eq!(r.count(), 2);
}

#[test]
fn test_capacity_full() {
    let mut r = AliasRegistry::new();
    // 注册 64 个不重叠的 4KB 区间
    for i in 0..MAX_MMIO_MAPPINGS {
        let phys = 0x100000 + (i as u64) * 0x1000;
        assert!(r.register(phys, 0x100, "dev").is_ok(), "i={}", i);
    }
    assert_eq!(r.count(), 64);
    // 65 个应该失败
    let phys = 0x100000 + 64 * 0x1000;
    assert!(r.register(phys, 0x100, "dev").is_err());
}

#[test]
fn test_pci_bar_simulation() {
    // 模拟真实 PCI BAR 场景: e1000 BAR0 (128KB) + ahci BAR5 (8KB) + xhci BAR0 (1MB)
    let mut r = AliasRegistry::new();
    assert!(r.register(0xFEB_C0000, 128 * 1024, "e1000-bar0").is_ok());
    assert!(r.register(0xFEB_E0000, 8 * 1024, "ahci-bar5").is_ok());
    assert!(r.register(0xFEB_F0000, 1024 * 1024, "xhci-bar0").is_ok());
    // 重叠尝试 (在 e1000 BAR0 内部)
    assert!(r.register(0xFEB_C1000, 0x100, "fake-e1000").is_err());
    // 在 ahci BAR5 内部 (offset 0x800)
    assert!(r.register(0xFEB_E0800, 0x100, "fake-ahci").is_err());
    // 在 xhci BAR0 内部
    assert!(r.register(0xFEB_F1000, 0x100, "fake-xhci").is_err());
    // 紧邻 e1000 BAR0 末尾 (e1000 ends at 0xFEBDFFFF, next starts at 0xFEBE0000)
    assert!(r.register(0xFEB_E0000, 0x100, "ahci-revived").is_err());
    assert_eq!(r.count(), 3);
}

#[test]
fn test_stress_non_overlapping() {
    // 压力测试: 1000 个不重叠区间 (受容量限制只能 64, 然后 unregister 一半再继续)
    let mut r = AliasRegistry::new();
    for i in 0..64 {
        let phys = 0x1_0000_0000 + (i as u64) * 0x10_0000;
        assert!(r.register(phys, 0x1000, "dev").is_ok());
    }
    // 容量满
    assert!(r.register(0xFFFF_FFFF_FFFF_FFFF, 0x1000, "dev").is_err());
    // unregister 奇数项
    for i in (1..64).step_by(2) {
        let phys = 0x1_0000_0000 + (i as u64) * 0x10_0000;
        assert!(r.unregister(phys).is_ok());
    }
    // 释放出 32 个空位, 重新注册 32 个新区间
    for i in 0..32 {
        let phys = 0x2_0000_0000 + (i as u64) * 0x10_0000;
        assert!(r.register(phys, 0x1000, "new-dev").is_ok());
    }
    assert_eq!(r.count(), 64);
}

#[test]
fn test_saturating_add_overflow() {
    // 极端 phys 接近 u64::MAX 时, 应使用 saturating_add 避免 panic
    let mut r = AliasRegistry::new();
    // phys = 0xFFFFFFFFFFFE0000 (4 字节对齐, 接近 u64::MAX), len = 0x20000
    // end = saturating_add = u64::MAX (saturate 不会 panic)
    let phys: u64 = 0xFFFF_FFFF_FFFE_0000;
    let res = r.register(phys, 0x20000, "dev");
    assert!(res.is_ok(), "register near u64::MAX must not panic: {:?}", res);
    // 区间结束地址溢出时, 任何 phys > e.phys 都被认为冲突 (因为 existing_end = u64::MAX)
    assert!(r.register(phys + 0x1000, 0x100, "after-overflow").is_err());
    // phys < e.phys 的区间仍然可以注册 (因为 end > e.phys 为 false)
    assert!(r.register(0x1000, 0x100, "before-overflow").is_ok());
}

#[test]
fn test_aligned_4byte_boundaries() {
    // PCI BAR 要求 4 字节对齐
    let mut r = AliasRegistry::new();
    assert!(r.register(0x1000, 0x100, "ok").is_ok());
    assert!(r.register(0x1100, 0x100, "ok").is_ok());
    assert!(r.register(0x1202, 0x100, "unaligned").is_err());
    assert!(r.register(0x1206, 0x100, "unaligned").is_err());
    assert!(r.register(0x120A, 0x100, "unaligned").is_err());
    // 4 字节对齐的应该 OK
    assert!(r.register(0x1204, 0x100, "aligned").is_ok());
}
