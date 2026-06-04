//! 故障恢复 API (Recovery Barrier 对外接口)
//!
//! ## 调用方契约
//! - 各可恢复子系统注册: `hvfs`、`fs`、`net`、`proc` 等
//! - `proc::scheduler::tick` —— 周期 tick 推进恢复
//! - `idt::on_panic` —— panic 路径触发 IDT 级别恢复
//! - 启动流程: `boot` 阶段调用 `recovery_barrier_maintenance` 启动后台 tick
//!
//! ## 内部接口
//! - `RecoveryDomain` trait/struct 在 `barrier::domain`,本文件是 C ABI
//!   友好的入口层。`#[no_mangle]` 函数供 asm stub / FFI 边界调用。
//!
//! ## 安全约束
//! - `recovery_*` 函数内部使用 `RECOVERY_MANAGER.lock()`,**不**可在
//!   中断上下文调用除 `recovery_panic_flag_*` 外的函数。
//! - `recovery_panic_flag_*` 是无锁 atomic,可在任何上下文使用。
//!
//! ## 性能特征
//! - `recovery_*` 路径: spinlock + 数组线性扫描 O(N),N ≤ 16 域,常数时间
//! - `recovery_panic_flag_*`: atomic load/store,无锁
use core::sync::atomic::{AtomicBool, Ordering};

use super::domain::RecoveryDomain;
use super::types::*;

static RECOVERY_ATTEMPTED: AtomicBool = AtomicBool::new(false);
static BOOT_FINGERPRINTS_CHECKED: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub fn recovery_barrier_maintenance() {
    use crate::kernel::framework::proc_tcb_legacy::scheduler::TICK_COUNT;
    let tick = TICK_COUNT.load(Ordering::SeqCst);

    let mgr = super::RECOVERY_MANAGER.lock();
    mgr.tick(tick);

    if !BOOT_FINGERPRINTS_CHECKED.swap(true, Ordering::SeqCst) {
        mgr.check_boot_fingerprints();
    }
}

#[no_mangle]
pub fn recovery_domain_register(domain_id: u64) -> i32 {
    let domain: &'static RecoveryDomain = {
        let bx = alloc::boxed::Box::new(RecoveryDomain::new(domain_id));
        alloc::boxed::Box::leak(bx)
    };
    match super::RECOVERY_MANAGER.lock().register(domain) {
        Some(_) => 0,
        None => -1,
    }
}

#[no_mangle]
pub fn recovery_domain_unregister(domain_id: u64) -> i32 {
    let mut mgr = super::RECOVERY_MANAGER.lock();
    let count = mgr.count.load(Ordering::SeqCst) as usize;
    for i in 0..count {
        if let Some(dom) = mgr.domains[i] {
            if dom.id == domain_id {
                mgr.domains[i] = None;
                let id_idx = domain_id as usize;
                if id_idx < DIRECT_MAP_SIZE {
                    mgr.direct_map[id_idx] = None;
                }
                return 0;
            }
        }
    }
    -1
}

#[cfg(feature = "kernel_test")]
#[no_mangle]
pub fn recovery_test_rollback(domain_id: u64, crash_fingerprint: u64) -> i32 {
    use crate::kernel::framework::proc_tcb_legacy::scheduler::TICK_COUNT;
    let tick = TICK_COUNT.load(Ordering::SeqCst);
    let mgr = super::RECOVERY_MANAGER.lock();
    let rollbacks = mgr.cascade_rollback(domain_id, tick, crash_fingerprint);
    if rollbacks > 0 {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_panic_flag_is_set() -> bool {
    super::PANIC_FLAG.load(Ordering::SeqCst)
}

#[no_mangle]
pub fn recovery_panic_flag_clear() {
    super::PANIC_FLAG.store(false, Ordering::SeqCst)
}

#[no_mangle]
pub fn recovery_try_recover_from_idt() -> i32 {
    use crate::kernel::framework::proc_tcb_legacy::scheduler::TICK_COUNT;
    let tick = TICK_COUNT.load(Ordering::SeqCst);

    if RECOVERY_ATTEMPTED.swap(true, Ordering::SeqCst) {
        return -2;
    }

    let Some(mgr) = super::RECOVERY_MANAGER.try_lock() else {
        RECOVERY_ATTEMPTED.store(false, Ordering::SeqCst);
        return -3;
    };

    let count = mgr.count.load(Ordering::SeqCst) as usize;
    if count == 0 {
        return -1;
    }

    let fingerprint = {
        let mut h: u64 = 5381;
        let msg = super::PANIC_MSG.lock();
        for &b in msg.iter() {
            if b == 0 {
                break;
            }
            h = h.wrapping_mul(33).wrapping_add(b as u64);
        }
        h
    };

    let fault_rip: u64 = super::CRASH_RIP.load(Ordering::SeqCst);
    if let Some(target_id) = mgr
        .locate_domain_by_addr(fault_rip)
        .or_else(|| mgr.locate_domain_by_panic_msg())
    {
        let rollbacks = mgr.cascade_rollback(target_id, tick, fingerprint);
        if rollbacks > 0 {
            if let Some(dom) = mgr.find(target_id) {
                dom.persist_crash_fingerprint(fingerprint);
            }
            recovery_panic_flag_clear();
            RECOVERY_ATTEMPTED.store(false, Ordering::SeqCst);
            return 0;
        }
    }

    for i in 0..count {
        if let Some(dom) = mgr.domains[i] {
            let did = dom.id;
            let rollbacks = mgr.cascade_rollback(did, tick, fingerprint);
            if rollbacks > 0 {
                dom.persist_crash_fingerprint(fingerprint);
                recovery_panic_flag_clear();
                RECOVERY_ATTEMPTED.store(false, Ordering::SeqCst);
                return 0;
            }
        }
    }
    -1
}

#[no_mangle]
pub fn recovery_trigger_panic() -> ! {
    super::PANIC_FLAG.store(true, Ordering::SeqCst);
    panic!("[RECOVERY-TEST] Deliberate panic for barrier-stack E2E test");
}

#[no_mangle]
pub fn recovery_was_attempted() -> i32 {
    if RECOVERY_ATTEMPTED.load(Ordering::SeqCst) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub fn recovery_domain_set_cbs(
    domain_id: u64,
    capture_fn: Option<unsafe fn()>,
    rollback_fn: Option<unsafe fn() -> bool>,
) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        *dom.capture_cb.lock() = capture_fn;
        *dom.rollback_cb.lock() = rollback_fn;
        0
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_undo_record(domain_id: u64, field_ptr: *mut u8, old_val: u64) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        let mut undo = dom.undo.lock();
        undo.record(field_ptr as *mut u64, old_val);
        0
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_undo_count(domain_id: u64) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        dom.undo.lock().count as i32
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_domain_add_dep(domain_id: u64, dep_id: u64) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        if dom.add_dependency(dep_id) {
            if let Some(dep_dom) = mgr.find(dep_id) {
                dep_dom.add_depended_by(domain_id);
            }
            0
        } else {
            -1
        }
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_domain_dep_count(domain_id: u64) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        dom.dependency_count() as i32
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_domain_add_addr_range(domain_id: u64, start: u64, end: u64) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        if dom.add_addr_range(start, end) {
            0
        } else {
            -1
        }
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_rollback_log_count() -> i32 {
    let log = super::manager::ROLLBACK_LOG.lock();
    log.iter().filter(|e| e.is_some()).count() as i32
}

#[no_mangle]
pub fn recovery_domain_get_state(domain_id: u64) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        dom.get_state() as i32
    } else {
        -1
    }
}

#[no_mangle]
pub fn recovery_domain_get_failures(domain_id: u64) -> i32 {
    let mgr = super::RECOVERY_MANAGER.lock();
    if let Some(dom) = mgr.find(domain_id) {
        dom.consecutive_failures.load(Ordering::SeqCst) as i32
    } else {
        -1
    }
}

#[cfg(feature = "fault_injection")]
#[no_mangle]
pub fn recovery_set_fault_rate(rate: u32) {
    super::fault_inject::FAULT_INJECTION_RATE.store(rate, Ordering::SeqCst);
}

#[cfg(feature = "fault_injection")]
#[no_mangle]
pub fn recovery_get_fault_rate() -> u32 {
    super::fault_inject::FAULT_INJECTION_RATE.load(Ordering::SeqCst)
}
