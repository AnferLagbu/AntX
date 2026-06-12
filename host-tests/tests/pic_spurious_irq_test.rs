//! Legacy 8259A PIC 假性 IRQ 检测契约测试 (I-25)
//!
//! 验证 `detect_spurious_8259_irq(irq, master_isr, slave_isr) -> Option<bool>` 判定逻辑:
//! 1. 非 IRQ7/IRQ15 候选 → 返回 `None` (跳过检测, 进入正常路径)
//! 2. IRQ7: master ISR bit7 == 0 → 假性
//! 3. IRQ7: master ISR bit7 == 1 → 真实
//! 4. IRQ15: slave ISR bit7 == 0 → 假性
//! 5. IRQ15: slave ISR bit7 == 1 → 真实
//!
//! 主机端镜像内核判定函数 (`is_spurious_8259_irq`), 验证 (irq, master_isr, slave_isr)
//! 三元组的真值表. 内核 `src/kernel/framework/idt/idt.rs::detect_spurious_8259_irq`
//! 是该契约权威实现; 本测试通过相同的判定公式独立验证真值表覆盖.

/// 镜像内核判定 (无 I/O 副作用, 仅做纯位运算)
fn is_spurious_8259_irq(irq: u8, master_isr: u8, slave_isr: u8) -> Option<bool> {
    if irq != 7 && irq != 15 {
        return None;
    }
    let isr = if irq >= 8 { slave_isr } else { master_isr };
    let bit = 1u8 << 7; // IRQ7 / IRQ15 都是 bit 7
    Some(isr & bit == 0)
}

#[test]
fn test_non_candidate_irq_returns_none() {
    // IRQ0-6, IRQ8-14 → None (直接进入正常路径, 不读 ISR)
    for irq in 0u8..16 {
        if irq == 7 || irq == 15 {
            continue;
        }
        assert_eq!(
            is_spurious_8259_irq(irq, 0xFF, 0xFF),
            None,
            "irq={} should return None",
            irq
        );
    }
}

#[test]
fn test_irq7_master_spurious_when_isr_bit7_clear() {
    // master ISR 全 0 → bit7 clear → 假性
    assert_eq!(is_spurious_8259_irq(7, 0x00, 0xFF), Some(true));
    // bit7 单独 clear
    assert_eq!(is_spurious_8259_irq(7, 0b0111_1111, 0xFF), Some(true));
}

#[test]
fn test_irq7_master_real_when_isr_bit7_set() {
    // bit7 set → 真实
    assert_eq!(is_spurious_8259_irq(7, 0x80, 0xFF), Some(false));
    // 其他 bit 不影响
    assert_eq!(is_spurious_8259_irq(7, 0xFF, 0xFF), Some(false));
    assert_eq!(is_spurious_8259_irq(7, 0b1010_1010, 0xFF), Some(false));
}

#[test]
fn test_irq15_slave_spurious_when_isr_bit7_clear() {
    // slave ISR 全 0 → bit7 clear → 假性 (master 状态不影响)
    assert_eq!(is_spurious_8259_irq(15, 0xFF, 0x00), Some(true));
    assert_eq!(is_spurious_8259_irq(15, 0x00, 0b0111_1111), Some(true));
}

#[test]
fn test_irq15_slave_real_when_isr_bit7_set() {
    // slave bit7 set → 真实
    assert_eq!(is_spurious_8259_irq(15, 0x00, 0x80), Some(false));
    assert_eq!(is_spurious_8259_irq(15, 0xFF, 0xFF), Some(false));
    assert_eq!(is_spurious_8259_irq(15, 0x00, 0b1010_1010), Some(false));
}

#[test]
fn test_irq_selection_isolation() {
    // master ISR 与 slave ISR 互不影响: 同一 ISR 字节在两个 IRQ 上结果不同
    // 因为 IRQ7 读 master_isr, IRQ15 读 slave_isr
    let master = 0x80; // bit7 set
    let slave = 0x00;  // bit7 clear
    assert_eq!(is_spurious_8259_irq(7, master, slave), Some(false));
    assert_eq!(is_spurious_8259_irq(15, master, slave), Some(true));
}

#[test]
fn test_eoi_strategy_distinction() {
    // 验证 EOI 策略区分:
    // - master 假性 (IRQ7): 不发 EOI 到 0x20
    // - slave 假性 (IRQ15): 发 EOI 到 0x20 (master), 不发到 0xA0 (slave)
    // 该测试用纯函数模拟 EOI 决策 (返回是否需要发 EOI 到对应端口)
    fn needs_master_eoi(irq: u8) -> bool {
        irq >= 8 // 仅 slave 假性需要 EOI master
    }
    fn needs_slave_eoi(_irq: u8) -> bool {
        false // 假性 IRQ 永不 EOI slave (避免误清真实在服务 IRQ)
    }
    assert!(!needs_master_eoi(7));
    assert!(needs_master_eoi(15));
    assert!(!needs_slave_eoi(7));
    assert!(!needs_slave_eoi(15));
}
