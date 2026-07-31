//! 多 IOAPIC GSI 路由契约测试
//!
//! 镜像内核 `framework/arch/x86_64/acpi.rs::gsi_to_ioapic()` 的路由逻辑,
//! 验证 GSI → (ioapic_index, local_irq) 映射的正确性.

/// IOAPIC 信息 (镜像 framework 的 IoApicInfo)
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct IoApicInfo {
    id: u8,
    base_addr: u64,
    gsi_base: u32,
    max_irq: u8,
}

/// 镜像内核 gsi_to_ioapic 路由逻辑 (无 I/O 副作用, 纯计算)
fn gsi_to_ioapic(ioapics: &[Option<IoApicInfo>], gsi: u32) -> Option<(usize, u8)> {
    for (i, ioapic) in ioapics.iter().enumerate() {
        if let Some(info) = ioapic {
            if gsi >= info.gsi_base && gsi < info.gsi_base + info.max_irq as u32 {
                return Some((i, (gsi - info.gsi_base) as u8));
            }
        }
    }
    None
}

/// 单 IOAPIC 场景 (典型 QEMU virt)
fn single_ioapic() -> [Option<IoApicInfo>; 8] {
    let mut arr = [None; 8];
    arr[0] = Some(IoApicInfo {
        id: 0,
        base_addr: 0xFEC00000,
        gsi_base: 0,
        max_irq: 24,
    });
    arr
}

/// 双 IOAPIC 场景 (多路服务器)
fn dual_ioapic() -> [Option<IoApicInfo>; 8] {
    let mut arr = [None; 8];
    arr[0] = Some(IoApicInfo {
        id: 0,
        base_addr: 0xFEC00000,
        gsi_base: 0,
        max_irq: 24,
    });
    arr[1] = Some(IoApicInfo {
        id: 1,
        base_addr: 0xFEC01000,
        gsi_base: 24,
        max_irq: 24,
    });
    arr
}

// ============================================================================
// 单 IOAPIC 测试
// ============================================================================

#[test]
fn test_single_ioapic_gsi_in_range() {
    let ioapics = single_ioapic();
    // GSI 0..23 → IOAPIC 0, local_irq = gsi
    for gsi in 0..24u32 {
        let result = gsi_to_ioapic(&ioapics, gsi);
        assert_eq!(result, Some((0, gsi as u8)), "GSI {} should route to IOAPIC 0, local_irq {}", gsi, gsi);
    }
}

#[test]
fn test_single_ioapic_gsi_out_of_range() {
    let ioapics = single_ioapic();
    // GSI 24+ → 无匹配
    assert_eq!(gsi_to_ioapic(&ioapics, 24), None);
    assert_eq!(gsi_to_ioapic(&ioapics, 255), None);
    assert_eq!(gsi_to_ioapic(&ioapics, 100), None);
}

#[test]
fn test_single_ioapic_boundary() {
    let ioapics = single_ioapic();
    // GSI 23 (最后一个有效) → IOAPIC 0, local_irq 23
    assert_eq!(gsi_to_ioapic(&ioapics, 23), Some((0, 23)));
    // GSI 24 (第一个无效) → None
    assert_eq!(gsi_to_ioapic(&ioapics, 24), None);
}

// ============================================================================
// 双 IOAPIC 测试
// ============================================================================

#[test]
fn test_dual_ioapic_first_range() {
    let ioapics = dual_ioapic();
    // GSI 0..23 → IOAPIC 0
    for gsi in 0..24u32 {
        let result = gsi_to_ioapic(&ioapics, gsi);
        assert_eq!(result, Some((0, gsi as u8)), "GSI {} should route to IOAPIC 0", gsi);
    }
}

#[test]
fn test_dual_ioapic_second_range() {
    let ioapics = dual_ioapic();
    // GSI 24..47 → IOAPIC 1, local_irq = gsi - 24
    for gsi in 24..48u32 {
        let result = gsi_to_ioapic(&ioapics, gsi);
        let expected_local = (gsi - 24) as u8;
        assert_eq!(result, Some((1, expected_local)), "GSI {} should route to IOAPIC 1, local_irq {}", gsi, expected_local);
    }
}

#[test]
fn test_dual_ioapic_out_of_range() {
    let ioapics = dual_ioapic();
    // GSI 48+ → 无匹配
    assert_eq!(gsi_to_ioapic(&ioapics, 48), None);
    assert_eq!(gsi_to_ioapic(&ioapics, 255), None);
}

#[test]
fn test_dual_ioapic_boundary_between_controllers() {
    let ioapics = dual_ioapic();
    // GSI 23 → IOAPIC 0 (最后一个)
    assert_eq!(gsi_to_ioapic(&ioapics, 23), Some((0, 23)));
    // GSI 24 → IOAPIC 1 (第一个)
    assert_eq!(gsi_to_ioapic(&ioapics, 24), Some((1, 0)));
}

// ============================================================================
// 空 IOAPIC 列表测试
// ============================================================================

#[test]
fn test_no_ioapics() {
    let ioapics: [Option<IoApicInfo>; 8] = [None; 8];
    assert_eq!(gsi_to_ioapic(&ioapics, 0), None);
    assert_eq!(gsi_to_ioapic(&ioapics, 255), None);
}

// ============================================================================
// 非连续 GSI 范围测试
// ============================================================================

#[test]
fn test_gap_in_gsi_range() {
    let mut ioapics = [None; 8];
    // IOAPIC 0: GSI 0..16
    ioapics[0] = Some(IoApicInfo { id: 0, base_addr: 0xFEC00000, gsi_base: 0, max_irq: 16 });
    // IOAPIC 1: GSI 32..48 (GSI 16..31 是空洞)
    ioapics[1] = Some(IoApicInfo { id: 1, base_addr: 0xFEC01000, gsi_base: 32, max_irq: 16 });

    // GSI 0..15 → IOAPIC 0
    assert_eq!(gsi_to_ioapic(&ioapics, 0), Some((0, 0)));
    assert_eq!(gsi_to_ioapic(&ioapics, 15), Some((0, 15)));
    // GSI 16..31 → 空洞, None
    assert_eq!(gsi_to_ioapic(&ioapics, 16), None);
    assert_eq!(gsi_to_ioapic(&ioapics, 31), None);
    // GSI 32..47 → IOAPIC 1
    assert_eq!(gsi_to_ioapic(&ioapics, 32), Some((1, 0)));
    assert_eq!(gsi_to_ioapic(&ioapics, 47), Some((1, 15)));
}

// ============================================================================
// 向后兼容性: IRQ = GSI 假设验证
// ============================================================================

#[test]
fn test_backward_compat_irq_equals_gsi() {
    // 单 IOAPIC, gsi_base=0: IRQ 和 GSI 相同
    let ioapics = single_ioapic();
    for irq in 0u8..24 {
        let result = gsi_to_ioapic(&ioapics, irq as u32);
        assert_eq!(result, Some((0, irq)), "IRQ {} should equal GSI {} in single-IOAPIC with gsi_base=0", irq, irq);
    }
}
