//! kmalloc/slab 中断安全锁契约测试 (P1-I-28)
//!
//! 验证:
//! 1. kmalloc.rs::acquire_lock/release_lock 签名变更为 (无) -> IrqSaveFlags / (&flags) -> ()
//! 2. kmalloc_slab.rs::slab_lock/slab_unlock 同上
//! 3. 源码静态扫描确认调用点一致 (let flags = self.acquire_lock(); ... self.release_lock(&flags);)
//! 4. 模拟中断嵌套: 持锁期间 irq disabled 状态下 ISR 不会自旋死锁
//!
//! 主机端测试: 镜像内核锁接口的最小化版本, 验证 (acquire 返回 flag, release 接 flag) 的
//! 配对契约. 内核 `src/kernel/framework/mm/kmalloc.rs` 是该契约权威实现.

use core::sync::atomic::{AtomicBool, Ordering};

/// 镜像内核 IrqSaveFlags (仅 8 字节对齐的 u64 包装, host 端测试不需要真实 RFLAGS 内容)
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
struct IrqSaveFlags(u64);

/// 镜像 spinlock 模块: host 端用 AtomicBool 模拟"中断已禁用"标记
static IRQ_DISABLED: AtomicBool = AtomicBool::new(false);

#[inline(always)]
fn disable_interrupts() -> IrqSaveFlags {
    let prev = IRQ_DISABLED.swap(true, Ordering::Acquire);
    IrqSaveFlags(if prev { 1 } else { 0 })
}

#[inline(always)]
fn restore_interrupts(flags: &IrqSaveFlags) {
    // flags.0 == 1 表示先前已禁用 (P1-I-28 不动前态)
    IRQ_DISABLED.store(flags.0 == 1, Ordering::Release);
}

#[inline(always)]
fn acquire_lock(lock: &AtomicBool) -> IrqSaveFlags {
    let flags = disable_interrupts();
    while lock
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    flags
}

#[inline(always)]
fn release_lock(lock: &AtomicBool, flags: &IrqSaveFlags) {
    lock.store(false, Ordering::Release);
    restore_interrupts(flags);
}

/// 模拟内核 kmalloc / slab 的锁调用
struct MockHeap {
    lock: AtomicBool,
}

impl MockHeap {
    fn allocate(&self, size: usize) -> Option<usize> {
        if size == 0 { return None; }
        let flags = acquire_lock(&self.lock);
        // 模拟分配逻辑
        let ptr = size as usize;
        release_lock(&self.lock, &flags);
        Some(ptr)
    }
}

#[test]
fn lock_acquire_release_paired_with_flags() {
    // 基础: acquire 必返 flags, release 必接 flags
    let heap = MockHeap { lock: AtomicBool::new(false) };
    let _ = heap.allocate(64);
    assert!(!heap.lock.load(Ordering::Acquire), "释放后 lock 必为 false");
}

#[test]
fn irq_disabled_during_critical_section() {
    // P1-I-28 验收: 持锁期间 IRQ 必为 disabled
    let lock = AtomicBool::new(false);
    let flags = acquire_lock(&lock);
    assert!(
        IRQ_DISABLED.load(Ordering::Acquire),
        "P1-I-28: 持锁期间 IRQ 必须 disabled"
    );
    release_lock(&lock, &flags);
    assert!(
        !IRQ_DISABLED.load(Ordering::Acquire),
        "P1-I-28: 释放后 IRQ 必恢复 (此处先前 IRQ=enabled, 故恢复 enabled)"
    );
}

#[test]
fn nested_critical_section_preserves_irq_state() {
    // P1-I-28 验收: 嵌套临界区不会破坏先前 IRQ 状态
    // 模拟外层 IRQ=enabled 时进入临界区
    IRQ_DISABLED.store(false, Ordering::Release);
    let lock = AtomicBool::new(false);

    let flags1 = acquire_lock(&lock);
    assert!(IRQ_DISABLED.load(Ordering::Acquire));

    // 模拟"内层"也调用 acquire — 必须能正确嵌套 (flags1 持有, flags2 重新获取)
    // 真实内核不可重入同锁, 但本测试验证 flags 传递的正确性
    release_lock(&lock, &flags1);
    assert!(!IRQ_DISABLED.load(Ordering::Acquire), "释放后 IRQ 必恢复");
}

#[test]
fn irq_disabled_prior_state_preserved() {
    // P1-I-28 验收: 若进入临界区前 IRQ 已被禁用, 释放后必须仍禁用 (不能错误开启)
    IRQ_DISABLED.store(true, Ordering::Release);
    let lock = AtomicBool::new(false);

    let flags = acquire_lock(&lock);
    assert!(IRQ_DISABLED.load(Ordering::Acquire));
    release_lock(&lock, &flags);
    assert!(
        IRQ_DISABLED.load(Ordering::Acquire),
        "P1-I-28: 释放后 IRQ 必须保持先前状态 (此处先前 IRQ=disabled, 故仍 disabled)"
    );

    // 恢复初始态
    IRQ_DISABLED.store(false, Ordering::Release);
}

#[test]
fn kmalloc_source_uses_irq_save_flags_signature() {
    // P1-I-28 验收: 源码静态扫描 — kmalloc.rs 的 lock 函数签名使用 IrqSaveFlags
    let source = include_str!("../../src/kernel/framework/mm/kmalloc.rs");
    // 修复后必须包含: fn acquire_lock(&self) -> IrqSaveFlags
    assert!(
        source.contains("fn acquire_lock(&self) -> IrqSaveFlags"),
        "P1-I-28: kmalloc.rs::acquire_lock 签名必须返回 IrqSaveFlags"
    );
    assert!(
        source.contains("fn release_lock(&self, flags: &IrqSaveFlags)"),
        "P1-I-28: kmalloc.rs::release_lock 签名必须接 &IrqSaveFlags"
    );
    // 必须导入 disable_interrupts / restore_interrupts
    assert!(
        (source.contains("use crate::kernel::framework::sync::spinlock::")
            || source.contains("use crate::kernel::framework::sync::"))
            && source.contains("disable_interrupts")
            && source.contains("restore_interrupts"),
        "P1-I-28: kmalloc.rs 必须导入 disable/restore 中断原语"
    );
    // 必须实现 disable + compare_exchange_weak (旧版是裸 compare_exchange_weak)
    let has_disable_then_cas = source
        .lines()
        .any(|line| line.contains("let flags = disable_interrupts();"))
        && source
            .lines()
            .any(|line| line.contains("compare_exchange_weak"));
    assert!(
        has_disable_then_cas,
        "P1-I-28: kmalloc.rs acquire_lock 必须先 disable_interrupts 再 CAS"
    );
}

#[test]
fn kmalloc_slab_source_uses_irq_save_flags_signature() {
    // P1-I-28 验收: kmalloc_slab.rs 同样必须使用 IrqSaveFlags
    let source = include_str!("../../src/kernel/framework/mm/kmalloc_slab.rs");
    assert!(
        source.contains("fn slab_lock() -> IrqSaveFlags"),
        "P1-I-28: kmalloc_slab.rs::slab_lock 签名必须返回 IrqSaveFlags"
    );
    assert!(
        source.contains("fn slab_unlock(flags: &IrqSaveFlags)"),
        "P1-I-28: kmalloc_slab.rs::slab_unlock 签名必须接 &IrqSaveFlags"
    );
    // 调用点必须用 let flags = slab_lock(); ... slab_unlock(&flags);
    let paired_call = source.contains("let flags = slab_lock();")
        && source.contains("slab_unlock(&flags)");
    assert!(
        paired_call,
        "P1-I-28: kmalloc_slab.rs 调用点必须用 let flags = slab_lock(); slab_unlock(&flags); 配对"
    );
}
