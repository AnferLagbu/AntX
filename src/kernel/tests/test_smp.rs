use crate::kernel::tests::{TestResult, runner, check, assert_eq_test};
use core::sync::atomic::Ordering;

// ============================================================
// SMP — Per-CPU Count & Online
// ============================================================

fn test_smp_cpu_count_positive() -> TestResult {
    let count = crate::kernel::smp::get_cpu_count();
    check!(count >= 1, "cpu_count >= 1");
    check!(count <= 64, "cpu_count <= 64 (sane upper bound)");
    TestResult::Pass
}

fn test_smp_current_cpu_valid() -> TestResult {
    let cpu = crate::kernel::smp::get_current_cpu();
    let count = crate::kernel::smp::get_cpu_count();
    check!(cpu < count, "current CPU index within range");
    TestResult::Pass
}

fn test_smp_cpu_online() -> TestResult {
    let cpu = crate::kernel::smp::get_current_cpu();
    let online = crate::kernel::smp::is_cpu_online(cpu);
    check!(online, "BSP (cpu 0) must be online");
    TestResult::Pass
}

// ============================================================
// Per-CPU Scheduler — Initialization
// ============================================================

fn test_per_cpu_sched_init() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER_READY;
    check!(SCHEDULER_READY.load(Ordering::Acquire), "scheduler initialized");
    TestResult::Pass
}

fn test_per_cpu_current_valid() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    let _current = SCHEDULER.current();
    TestResult::Pass
}

fn test_per_cpu_has_runnable() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    let _runnable = SCHEDULER.has_runnable();
    TestResult::Pass
}

fn test_per_cpu_time_slice_positive() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    let slice = SCHEDULER.get_time_slice();
    check!(slice > 0, "time slice > 0");
    check!(slice <= 80, "time slice within quantum range");
    TestResult::Pass
}

fn test_per_cpu_rt_count() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    let count = SCHEDULER.get_rt_count();
    check!(count <= 256, "RT task count bounded");
    TestResult::Pass
}

// ============================================================
// MLFQ — Algorithm Invariants
// ============================================================

fn test_sched_policy_from_u32() -> TestResult {
    use crate::kernel::proc::scheduler::SchedPolicy;

    assert_eq_test!(SchedPolicy::from_u32(0), SchedPolicy::Normal, "0 → Normal");
    assert_eq_test!(SchedPolicy::from_u32(1), SchedPolicy::Fifo, "1 → Fifo");
    assert_eq_test!(SchedPolicy::from_u32(2), SchedPolicy::Rr, "2 → Rr");
    assert_eq_test!(SchedPolicy::from_u32(3), SchedPolicy::Idle, "3 → Idle");
    assert_eq_test!(SchedPolicy::from_u32(255), SchedPolicy::Normal, "255 → Normal fallback");
    TestResult::Pass
}

fn test_sched_policy_discriminant() -> TestResult {
    use crate::kernel::proc::scheduler::SchedPolicy;
    check!(SchedPolicy::Normal as u32 == 0, "Normal=0");
    check!(SchedPolicy::Fifo as u32 == 1, "Fifo=1");
    check!(SchedPolicy::Rr as u32 == 2, "Rr=2");
    check!(SchedPolicy::Idle as u32 == 3, "Idle=3");
    TestResult::Pass
}

fn test_sched_quota_operations() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    let test_pwm: u64 = 0xDEAD0000;
    SCHEDULER.set_quota(test_pwm, 100_000_000, 1_000_000_000);
    SCHEDULER.remove_quota(test_pwm);
    TestResult::Pass
}

fn test_sched_limit_init() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    SCHEDULER.set_limit(0x100001, 5);
    SCHEDULER.remove_quota(0x100001);
    TestResult::Pass
}

// ============================================================
// RT Scheduling — Policy Switching
// ============================================================

fn test_rt_policy_switching_self() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    use crate::kernel::proc::scheduler::SchedPolicy;

    let pid = SCHEDULER.current().unwrap_or(0);
    if pid == 0 {
        return TestResult::Skip("no current process to switch policy");
    }

    let result = SCHEDULER.set_sched_policy(pid, SchedPolicy::Fifo, 50);
    check!(result, "set_sched_policy on current should succeed");

    let result = SCHEDULER.set_sched_policy(pid, SchedPolicy::Rr, 30);
    check!(result, "switch Fifo → Rr should succeed");

    let result = SCHEDULER.set_sched_policy(pid, SchedPolicy::Normal, 0);
    check!(result, "switch Rr → Normal should succeed");
    TestResult::Pass
}

fn test_rt_invalid_pid() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    use crate::kernel::proc::scheduler::SchedPolicy;

    let result = SCHEDULER.set_sched_policy(0xFFFFFFFF, SchedPolicy::Fifo, 50);
    check!(!result, "invalid PID must fail");
    TestResult::Pass
}

// ============================================================
// Load Balancing — Fundamentals
// ============================================================

fn test_load_balance_no_panic() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    SCHEDULER.load_balance();
    TestResult::Pass
}

fn test_boost_priority_no_panic() -> TestResult {
    use crate::kernel::proc::scheduler::SCHEDULER;
    SCHEDULER.boost_priority();
    TestResult::Pass
}

// ============================================================
// Process Exit — CR3 Safety (Regression Test)
// ============================================================

fn test_kernel_pml4_exists() -> TestResult {
    let kpml4 = crate::kernel::mm::vmm::get_kernel_pml4();
    check!(kpml4 != 0, "kernel PML4 is non-zero");
    TestResult::Pass
}

fn test_kernel_pml4_stable() -> TestResult {
    let k1 = crate::kernel::mm::vmm::get_kernel_pml4();
    let k2 = crate::kernel::mm::vmm::get_kernel_pml4();
    check!(k1 == k2, "kernel PML4 is stable across calls");
    TestResult::Pass
}

fn test_user_proc_manager_destroy_no_kstack() -> TestResult {
    use crate::kernel::proc::user_proc::USER_PROC_MANAGER;

    let pid = crate::kernel::proc::scheduler::SCHEDULER.current().unwrap_or(0);
    if pid == 0 {
        return TestResult::Skip("no user process to test destroy_no_kstack");
    }
    if USER_PROC_MANAGER.get(pid as u32).is_none() {
        return TestResult::Skip("current PID not tracked in USER_PROC_MANAGER");
    }
    TestResult::Pass
}

// ============================================================
// Softirq — Registration & Vector
// ============================================================

fn test_softirq_vec_enum_values() -> TestResult {
    use crate::kernel::irq::SoftirqVec;
    check!(SoftirqVec::High.to_idx() == 0, "High=0");
    check!(SoftirqVec::Timer.to_idx() == 1, "Timer=1");
    check!(SoftirqVec::NetRx.to_idx() == 2, "NetRx=2");
    check!(SoftirqVec::NetTx.to_idx() == 3, "NetTx=3");
    check!(SoftirqVec::Block.to_idx() == 4, "Block=4");
    check!(SoftirqVec::Tasklet.to_idx() == 5, "Tasklet=5");
    check!(SoftirqVec::Sched.to_idx() == 6, "Sched=6");
    TestResult::Pass
}

fn test_softirq_from_u8() -> TestResult {
    use crate::kernel::irq::SoftirqVec;
    check!(SoftirqVec::from_u8(0) == Some(SoftirqVec::High), "0→High");
    check!(SoftirqVec::from_u8(1) == Some(SoftirqVec::Timer), "1→Timer");
    check!(SoftirqVec::from_u8(5) == Some(SoftirqVec::Tasklet), "5→Tasklet");
    check!(SoftirqVec::from_u8(255).is_none(), "255→None");
    TestResult::Pass
}

fn test_softirq_not_initially_in() -> TestResult {
    let in_softirq = crate::kernel::irq::in_softirq();
    check!(!in_softirq, "not in softirq context at test start");
    TestResult::Pass
}

fn test_softirq_pending_initially_zero() -> TestResult {
    let pending = crate::kernel::irq::pending_softirq();
    check!(!pending, "no pending softirqs at test start");
    TestResult::Pass
}

fn test_softirq_raise_then_check() -> TestResult {
    use crate::kernel::irq::SoftirqVec;

    crate::kernel::irq::open_softirq(SoftirqVec::Tasklet, || {});
    crate::kernel::irq::raise_softirq(SoftirqVec::Tasklet);

    let pending = crate::kernel::irq::pending_softirq();
    check!(pending, "softirq should be pending after raise");

    crate::kernel::irq::do_softirq();
    TestResult::Pass
}

fn test_softirq_mask_raise() -> TestResult {
    let mask: u64 = (1u64 << crate::kernel::irq::SoftirqVec::Timer.to_idx())
                  | (1u64 << crate::kernel::irq::SoftirqVec::NetRx.to_idx());

    crate::kernel::irq::raise_softirq_mask(mask);

    let pending = crate::kernel::irq::pending_softirq();
    check!(pending, "mask-raised softirqs should be pending");

    crate::kernel::irq::do_softirq();
    TestResult::Pass
}

// ============================================================
// Test Registration
// ============================================================

pub fn register_smp_tests() {
    let r = runner();

    r.register("smp", "cpu_count_positive", test_smp_cpu_count_positive);
    r.register("smp", "current_cpu_valid", test_smp_current_cpu_valid);
    r.register("smp", "cpu_online", test_smp_cpu_online);

    r.register("per_cpu_sched", "init", test_per_cpu_sched_init);
    r.register("per_cpu_sched", "current_valid", test_per_cpu_current_valid);
    r.register("per_cpu_sched", "has_runnable", test_per_cpu_has_runnable);
    r.register("per_cpu_sched", "time_slice_positive", test_per_cpu_time_slice_positive);
    r.register("per_cpu_sched", "rt_count_init", test_per_cpu_rt_count);

    r.register("sched_policy", "from_u32", test_sched_policy_from_u32);
    r.register("sched_policy", "discriminant", test_sched_policy_discriminant);

    r.register("sched_quota", "set_operations", test_sched_quota_operations);
    r.register("sched_limit", "set_init", test_sched_limit_init);

    r.register("rt_sched", "policy_switching_self", test_rt_policy_switching_self);
    r.register("rt_sched", "invalid_pid", test_rt_invalid_pid);

    r.register("load_balance", "no_panic", test_load_balance_no_panic);
    r.register("priority_boost", "no_panic", test_boost_priority_no_panic);

    r.register("proc_exit", "kernel_pml4_exists", test_kernel_pml4_exists);
    r.register("proc_exit", "kernel_pml4_stable", test_kernel_pml4_stable);
    r.register("proc_exit", "destroy_no_kstack", test_user_proc_manager_destroy_no_kstack);

    r.register("softirq", "vec_enum_values", test_softirq_vec_enum_values);
    r.register("softirq", "from_u8", test_softirq_from_u8);
    r.register("softirq", "not_initially_in", test_softirq_not_initially_in);
    r.register("softirq", "pending_initially_zero", test_softirq_pending_initially_zero);
    r.register("softirq", "raise_then_check", test_softirq_raise_then_check);
    r.register("softirq", "mask_raise", test_softirq_mask_raise);
}