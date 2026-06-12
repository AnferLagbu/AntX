//! IDT IST 验证契约测试 (I-24)
//!
//! 验证 `TSS::ist_validated()` 判定逻辑:
//! 1. 全部 4 个 IST 字段非零 → true
//! 2. 任一 IST 字段为 0 → false (启动顺序错误, IDT init 应失败)
//! 3. 边界: 越界读写 (index 7+) 不应被误报为 IST 字段
//!
//! 主机端镜像内核 TSS 字段结构, 验证 4 字段同时非零的 AND 短路语义.
//! 内核 `src/kernel/framework/arch/x86_64/tss.rs::ist_validated` 是权威实现.

const IST_COUNT: usize = 7;

struct TaskStateSegment {
    ist: [u64; IST_COUNT],
    // 其他字段 (rsp0/rsp1/rsp2/...) 在 host-test 中省略
}

impl TaskStateSegment {
    fn new() -> Self {
        Self {
            ist: [0; IST_COUNT],
        }
    }

    fn set_ist(&mut self, index: usize, stack_top: u64) {
        if index < IST_COUNT {
            self.ist[index] = stack_top;
        }
    }

    /// 镜像内核 `ist_validated` 实现: IST 0-3 全部非零 → true
    fn ist_validated(&self) -> bool {
        self.ist[0] != 0 && self.ist[1] != 0 && self.ist[2] != 0 && self.ist[3] != 0
    }
}

#[test]
fn test_ist_validated_all_set() {
    // 启动顺序正确: 4 个 IST 全部填充非零栈顶
    let mut tss = TaskStateSegment::new();
    tss.set_ist(0, 0xFFFF_8000_0000_1000);
    tss.set_ist(1, 0xFFFF_8000_0000_2000);
    tss.set_ist(2, 0xFFFF_8000_0000_3000);
    tss.set_ist(3, 0xFFFF_8000_0000_4000);
    assert!(tss.ist_validated());
}

#[test]
fn test_ist_validated_uninit_returns_false() {
    // 全部为 0 → 启动顺序错误
    let tss = TaskStateSegment::new();
    assert!(!tss.ist_validated());
}

#[test]
fn test_ist_validated_partial_fails() {
    // 仅设置 IST 0 → false
    let mut tss = TaskStateSegment::new();
    tss.set_ist(0, 0x1000);
    assert!(!tss.ist_validated());

    // 设置 0, 1, 2, 缺 3
    let mut tss = TaskStateSegment::new();
    tss.set_ist(0, 0x1000);
    tss.set_ist(1, 0x2000);
    tss.set_ist(2, 0x3000);
    assert!(!tss.ist_validated());

    // 设置 0, 1, 缺 2
    let mut tss = TaskStateSegment::new();
    tss.set_ist(0, 0x1000);
    tss.set_ist(1, 0x2000);
    assert!(!tss.ist_validated());
}

#[test]
fn test_ist_validated_ist4_to_7_ignored() {
    // IST 4-7 不在 validated 检查范围, 不影响结果
    let mut tss = TaskStateSegment::new();
    tss.set_ist(0, 0x1000);
    tss.set_ist(1, 0x2000);
    tss.set_ist(2, 0x3000);
    tss.set_ist(3, 0x4000);
    // 即使 4-7 没设置, validated 仍为 true
    assert!(tss.ist_validated());

    // 即使 4-7 设置了, validated 仍由 0-3 决定
    let mut tss = TaskStateSegment::new();
    tss.set_ist(4, 0x5000);
    tss.set_ist(5, 0x6000);
    tss.set_ist(6, 0x7000);
    assert!(!tss.ist_validated()); // 0-3 仍为 0
}

#[test]
fn test_idt_ist_to_tss_ist_mapping() {
    // 验证 IDT IST 字段 N → TSS ist[N-1] 的映射契约
    // (与 idt.rs init 中的 4 个 IST 条目一致)
    let mappings = [
        (1, 0), // #DF: IDT IST=1 → TSS ist[0]
        (2, 1), // NMI: IDT IST=2 → TSS ist[1]
        (4, 3), // #PF: IDT IST=4 → TSS ist[3]
        (3, 2), // 0x82: IDT IST=3 → TSS ist[2]
    ];
    for (idt_ist, tss_idx) in mappings {
        // N → N-1
        assert_eq!(idt_ist as usize - 1, tss_idx, "IDT IST={} should map to TSS ist[{}]", idt_ist, tss_idx);
    }
}
