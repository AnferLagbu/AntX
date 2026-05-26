use crate::kernel::tests::{TestResult, TestFn, runner, check, assert_eq_test};
use crate::kernel::proc::scheduler_ex::{
    ThreadState, SchedulerEx,
};
use crate::kernel::proc::thread::Thread;
use crate::kernel::proc::types::{
    SCHED_LEVEL_0_QUANTUM, SCHED_LEVEL_1_QUANTUM,
    SCHED_LEVEL_2_QUANTUM, SCHED_LEVEL_3_QUANTUM,
};
use core::sync::atomic::Ordering;
use alloc::boxed::Box;

fn thread_state_from_u32_valid() -> TestResult {
    assert_eq_test!(ThreadState::from_u32(0), ThreadState::Created, "state 0");
    assert_eq_test!(ThreadState::from_u32(1), ThreadState::Ready, "state 1");
    assert_eq_test!(ThreadState::from_u32(2), ThreadState::Running, "state 2");
    assert_eq_test!(ThreadState::from_u32(3), ThreadState::Blocked, "state 3");
    assert_eq_test!(ThreadState::from_u32(4), ThreadState::Zombie, "state 4");
    assert_eq_test!(ThreadState::from_u32(5), ThreadState::Terminated, "state 5");
    assert_eq_test!(ThreadState::from_u32(6), ThreadState::Frozen, "state 6");
    TestResult::Pass
}

fn thread_state_from_u32_invalid() -> TestResult {
    assert_eq_test!(ThreadState::from_u32(255), ThreadState::Created, "invalid 255");
    assert_eq_test!(ThreadState::from_u32(99), ThreadState::Created, "invalid 99");
    TestResult::Pass
}

fn thread_state_is_runnable() -> TestResult {
    check!(ThreadState::Ready.is_runnable(), "Ready is runnable");
    check!(ThreadState::Running.is_runnable(), "Running is runnable");
    check!(!ThreadState::Created.is_runnable(), "Created not runnable");
    check!(!ThreadState::Blocked.is_runnable(), "Blocked not runnable");
    check!(!ThreadState::Zombie.is_runnable(), "Zombie not runnable");
    check!(!ThreadState::Terminated.is_runnable(), "Terminated not runnable");
    check!(!ThreadState::Frozen.is_runnable(), "Frozen not runnable");
    TestResult::Pass
}

fn thread_state_is_alive() -> TestResult {
    check!(ThreadState::Created.is_alive(), "Created is alive");
    check!(ThreadState::Ready.is_alive(), "Ready is alive");
    check!(ThreadState::Running.is_alive(), "Running is alive");
    check!(ThreadState::Blocked.is_alive(), "Blocked is alive");
    check!(ThreadState::Frozen.is_alive(), "Frozen is alive");
    check!(!ThreadState::Zombie.is_alive(), "Zombie not alive");
    check!(!ThreadState::Terminated.is_alive(), "Terminated not alive");
    TestResult::Pass
}

fn thread_state_can_freeze() -> TestResult {
    check!(ThreadState::Running.can_freeze(), "Running can freeze");
    check!(ThreadState::Ready.can_freeze(), "Ready can freeze");
    check!(ThreadState::Blocked.can_freeze(), "Blocked can freeze");
    check!(!ThreadState::Created.can_freeze(), "Created cannot freeze");
    check!(!ThreadState::Zombie.can_freeze(), "Zombie cannot freeze");
    check!(!ThreadState::Terminated.can_freeze(), "Terminated cannot freeze");
    check!(!ThreadState::Frozen.can_freeze(), "Frozen cannot freeze");
    TestResult::Pass
}

/// ✅ 使用统一的 Thread 结构体测试 (替代 ThreadNode)
fn thread_normal_lifecycle() -> TestResult {
    let t = Box::new(Thread::new(1, 1));
    assert_eq_test!(t.get_state(), ThreadState::Created, "initial state");
    assert!(t.set_state_safe(ThreadState::Ready).is_ok(), "Created->Ready");
    assert_eq_test!(t.get_state(), ThreadState::Ready, "after Ready");
    assert!(t.set_state_safe(ThreadState::Running).is_ok(), "Ready->Running");
    assert_eq_test!(t.get_state(), ThreadState::Running, "after Running");
    assert!(t.set_state_safe(ThreadState::Ready).is_ok(), "Running->Ready");
    assert_eq_test!(t.get_state(), ThreadState::Ready, "back to Ready");
    assert!(t.set_state_safe(ThreadState::Running).is_ok(), "Ready->Running");
    assert!(t.set_state_safe(ThreadState::Blocked).is_ok(), "Running->Blocked");
    assert_eq_test!(t.get_state(), ThreadState::Blocked, "after Blocked");
    assert!(t.set_state_safe(ThreadState::Ready).is_ok(), "Blocked->Ready");
    assert!(t.set_state_safe(ThreadState::Running).is_ok(), "Ready->Running");
    assert!(t.set_state_safe(ThreadState::Zombie).is_ok(), "Running->Zombie");
    assert_eq_test!(t.get_state(), ThreadState::Zombie, "after Zombie");
    assert!(t.set_state_safe(ThreadState::Terminated).is_ok(), "Zombie->Terminated");
    assert_eq_test!(t.get_state(), ThreadState::Terminated, "after Terminated");
    TestResult::Pass
}

fn thread_freeze_thaw() -> TestResult {
    let t = Box::new(Thread::new(2, 1));
    assert!(t.set_state_safe(ThreadState::Ready).is_ok());
    assert!(t.set_state_safe(ThreadState::Running).is_ok());
    assert!(t.set_state_safe(ThreadState::Frozen).is_ok());
    assert_eq_test!(t.get_state(), ThreadState::Frozen, "frozen");
    assert!(t.set_state_safe(ThreadState::Ready).is_ok());
    assert_eq_test!(t.get_state(), ThreadState::Ready, "thawed to Ready");
    assert!(t.set_state_safe(ThreadState::Running).is_ok());
    assert!(t.set_state_safe(ThreadState::Blocked).is_ok());
    assert!(t.set_state_safe(ThreadState::Frozen).is_ok());
    assert_eq_test!(t.get_state(), ThreadState::Frozen, "frozen from Blocked");
    assert!(t.set_state_safe(ThreadState::Blocked).is_ok());
    assert_eq_test!(t.get_state(), ThreadState::Blocked, "thawed to Blocked");
    TestResult::Pass
}

fn thread_illegal_transitions() -> TestResult {
    let t = Box::new(Thread::new(3, 1));
    let result = t.set_state_safe(ThreadState::Running);
    check!(result.is_err(), "Created->Running should fail");
    assert_eq_test!(t.get_state(), ThreadState::Created, "state unchanged");

    t.state.store(ThreadState::Terminated as u32, Ordering::SeqCst);
    let result = t.set_state_safe(ThreadState::Running);
    check!(result.is_err(), "Terminated->Running should fail");
    assert_eq_test!(t.get_state(), ThreadState::Terminated, "state unchanged");

    t.state.store(ThreadState::Zombie as u32, Ordering::SeqCst);
    let result = t.set_state_safe(ThreadState::Ready);
    check!(result.is_err(), "Zombie->Ready should fail");
    assert_eq_test!(t.get_state(), ThreadState::Zombie, "state unchanged");
    TestResult::Pass
}

fn thread_state_change_count() -> TestResult {
    let t = Box::new(Thread::new(4, 1));
    assert_eq_test!(t.state_change_count.load(Ordering::Relaxed), 0, "initial count");
    let _ = t.set_state_safe(ThreadState::Ready);
    assert_eq_test!(t.state_change_count.load(Ordering::Relaxed), 1, "after 1 transition");
    let _ = t.set_state_safe(ThreadState::Running);
    assert_eq_test!(t.state_change_count.load(Ordering::Relaxed), 2, "after 2 transitions");
    let _ = t.set_state_safe(ThreadState::Created);  // illegal, still Running→Created
    let _ = t.set_state_safe(ThreadState::Created);  // still illegal
    assert_eq_test!(t.state_change_count.load(Ordering::Relaxed), 2, "failed transition no count");
    TestResult::Pass
}

fn thread_exit_code_preserved() -> TestResult {
    let t = Box::new(Thread::new(5, 1));
    t.exit_code.store(42, Ordering::SeqCst);
    assert!(t.set_state_safe(ThreadState::Ready).is_ok());
    assert!(t.set_state_safe(ThreadState::Running).is_ok());
    assert!(t.set_state_safe(ThreadState::Zombie).is_ok());
    assert_eq_test!(t.exit_code.load(Ordering::Relaxed), 42, "exit code after Zombie");
    assert!(t.set_state_safe(ThreadState::Terminated).is_ok());
    assert_eq_test!(t.exit_code.load(Ordering::Relaxed), 42, "exit code after Terminated");
    TestResult::Pass
}

fn scheduler_initialization() -> TestResult {
    let sched = SchedulerEx::new();
    assert_eq_test!(sched.current.load(Ordering::SeqCst), 0, "current init");
    assert_eq_test!(sched.idle_thread.load(Ordering::SeqCst), 0, "idle init");
    assert_eq_test!(sched.tick_count.load(Ordering::SeqCst), 0, "tick init");
    assert_eq_test!(sched.need_reschedule.load(Ordering::SeqCst), 0, "reschedule init");
    assert_eq_test!(sched.stats.total_switches.load(Ordering::SeqCst), 0, "switches init");
    TestResult::Pass
}

fn scheduler_run_queue_empty() -> TestResult {
    let sched = SchedulerEx::new();
    assert!(sched.run_queue.is_empty(), "empty run queue");
    TestResult::Pass
}

fn null_pointer_safety() -> TestResult {
    let sched = SchedulerEx::new();
    sched.add_thread(core::ptr::null_mut());
    assert!(sched.run_queue.is_empty(), "null thread rejected");
    TestResult::Pass
}

fn quantum_constants() -> TestResult {
    assert_eq_test!(SCHED_LEVEL_0_QUANTUM, 80, "level 0 quantum");
    assert_eq_test!(SCHED_LEVEL_1_QUANTUM, 60, "level 1 quantum");
    assert_eq_test!(SCHED_LEVEL_2_QUANTUM, 40, "level 2 quantum");
    assert_eq_test!(SCHED_LEVEL_3_QUANTUM, 20, "level 3 quantum");
    TestResult::Pass
}

fn thread_type_safety() -> TestResult {
    // ✅ 验证 Thread 可以直接传给 SchedulerEx (类型安全, 无需强转)
    let sched = SchedulerEx::new();
    let t = Box::into_raw(Box::new(Thread::new(10, 1)));
    unsafe {
        (*t).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
    }
    sched.add_thread(t);
    assert_eq!(sched.run_queue.len(), 1);
    // Cleanup
    let popped = sched.run_queue.pop_front();
    assert!(popped.is_some());
    unsafe {
        if let Some(p) = popped { drop(Box::from_raw(p)); }
    }
    TestResult::Pass
}

pub fn register_sched_ex_tests() {
    let r = runner();
    r.register("sched::thread_state", "from_u32_valid", thread_state_from_u32_valid as TestFn);
    r.register("sched::thread_state", "from_u32_invalid", thread_state_from_u32_invalid as TestFn);
    r.register("sched::thread_state", "is_runnable", thread_state_is_runnable as TestFn);
    r.register("sched::thread_state", "is_alive", thread_state_is_alive as TestFn);
    r.register("sched::thread_state", "can_freeze", thread_state_can_freeze as TestFn);
    r.register("sched::thread", "normal_lifecycle", thread_normal_lifecycle as TestFn);
    r.register("sched::thread", "freeze_thaw", thread_freeze_thaw as TestFn);
    r.register("sched::thread", "illegal_transitions", thread_illegal_transitions as TestFn);
    r.register("sched::thread", "state_change_count", thread_state_change_count as TestFn);
    r.register("sched::thread", "exit_code_preserved", thread_exit_code_preserved as TestFn);
    r.register("sched::ex", "initialization", scheduler_initialization as TestFn);
    r.register("sched::ex", "run_queue_empty", scheduler_run_queue_empty as TestFn);
    r.register("sched::ex", "null_pointer_safety", null_pointer_safety as TestFn);
    r.register("sched::ex", "quantum_constants", quantum_constants as TestFn);
    r.register("sched::ex", "thread_type_safety", thread_type_safety as TestFn);
}

pub fn register_tests() {
    register_sched_ex_tests();
}