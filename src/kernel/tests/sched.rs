use crate::kernel::tests::{TestResult, TestFn, runner, check, assert_eq_test};
use crate::kernel::proc::scheduler_ex::{
    ThreadState, ThreadNode, ThreadPriority, SchedulerEx,
    SCHED_LEVELS, SCHED_LEVEL_0_QUANTUM, SCHED_LEVEL_1_QUANTUM,
    SCHED_LEVEL_2_QUANTUM, SCHED_LEVEL_3_QUANTUM,
};
use core::sync::atomic::Ordering;

fn thread_state_from_u8_valid() -> TestResult {
    assert_eq_test!(ThreadState::from_u8(0), ThreadState::Created, "state 0");
    assert_eq_test!(ThreadState::from_u8(1), ThreadState::Ready, "state 1");
    assert_eq_test!(ThreadState::from_u8(2), ThreadState::Running, "state 2");
    assert_eq_test!(ThreadState::from_u8(3), ThreadState::Blocked, "state 3");
    assert_eq_test!(ThreadState::from_u8(4), ThreadState::Zombie, "state 4");
    assert_eq_test!(ThreadState::from_u8(5), ThreadState::Terminated, "state 5");
    assert_eq_test!(ThreadState::from_u8(6), ThreadState::Frozen, "state 6");
    TestResult::Pass
}

fn thread_state_from_u8_invalid() -> TestResult {
    assert_eq_test!(ThreadState::from_u8(255), ThreadState::Created, "invalid 255");
    assert_eq_test!(ThreadState::from_u8(99), ThreadState::Created, "invalid 99");
    TestResult::Pass
}

fn thread_state_name() -> TestResult {
    assert_eq_test!(ThreadState::Created.name(), "Created", "Created name");
    assert_eq_test!(ThreadState::Ready.name(), "Ready", "Ready name");
    assert_eq_test!(ThreadState::Running.name(), "Running", "Running name");
    assert_eq_test!(ThreadState::Blocked.name(), "Blocked", "Blocked name");
    assert_eq_test!(ThreadState::Zombie.name(), "Zombie", "Zombie name");
    assert_eq_test!(ThreadState::Terminated.name(), "Terminated", "Terminated name");
    assert_eq_test!(ThreadState::Frozen.name(), "Frozen", "Frozen name");
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

fn thread_node_normal_lifecycle() -> TestResult {
    let node = ThreadNode::new();
    assert_eq_test!(node.get_state(), ThreadState::Created, "initial state");
    node.set_state_safe(ThreadState::Ready).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Ready, "after Ready");
    node.set_state_safe(ThreadState::Running).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Running, "after Running");
    node.set_state_safe(ThreadState::Ready).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Ready, "back to Ready");
    node.set_state_safe(ThreadState::Running).unwrap();
    node.set_state_safe(ThreadState::Blocked).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Blocked, "after Blocked");
    node.set_state_safe(ThreadState::Ready).unwrap();
    node.set_state_safe(ThreadState::Running).unwrap();
    node.set_state_safe(ThreadState::Zombie).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Zombie, "after Zombie");
    node.set_state_safe(ThreadState::Terminated).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Terminated, "after Terminated");
    TestResult::Pass
}

fn thread_node_freeze_thaw() -> TestResult {
    let node = ThreadNode::new();
    node.set_state_safe(ThreadState::Ready).unwrap();
    node.set_state_safe(ThreadState::Running).unwrap();
    node.set_state_safe(ThreadState::Frozen).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Frozen, "frozen");
    node.set_state_safe(ThreadState::Ready).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Ready, "thawed to Ready");
    node.set_state_safe(ThreadState::Running).unwrap();
    node.set_state_safe(ThreadState::Blocked).unwrap();
    node.set_state_safe(ThreadState::Frozen).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Frozen, "frozen from Blocked");
    node.set_state_safe(ThreadState::Blocked).unwrap();
    assert_eq_test!(node.get_state(), ThreadState::Blocked, "thawed to Blocked");
    TestResult::Pass
}

fn thread_node_illegal_transitions() -> TestResult {
    let node = ThreadNode::new();
    let result = node.set_state_safe(ThreadState::Running);
    check!(result.is_err(), "Created->Running should fail");
    assert_eq_test!(node.get_state(), ThreadState::Created, "state unchanged");

    node.state.store(ThreadState::Terminated as u32, Ordering::SeqCst);
    let result = node.set_state_safe(ThreadState::Running);
    check!(result.is_err(), "Terminated->Running should fail");
    assert_eq_test!(node.get_state(), ThreadState::Terminated, "state unchanged");

    node.state.store(ThreadState::Zombie as u32, Ordering::SeqCst);
    let result = node.set_state_safe(ThreadState::Ready);
    check!(result.is_err(), "Zombie->Ready should fail");
    assert_eq_test!(node.get_state(), ThreadState::Zombie, "state unchanged");
    TestResult::Pass
}

fn thread_node_state_change_count() -> TestResult {
    let node = ThreadNode::new();
    assert_eq_test!(node.state_change_count.load(Ordering::Relaxed), 0, "initial count");
    let _ = node.set_state_safe(ThreadState::Ready);
    assert_eq_test!(node.state_change_count.load(Ordering::Relaxed), 1, "after 1 transition");
    let _ = node.set_state_safe(ThreadState::Running);
    assert_eq_test!(node.state_change_count.load(Ordering::Relaxed), 2, "after 2 transitions");
    let _ = node.set_state_safe(ThreadState::Created);
    assert_eq_test!(node.state_change_count.load(Ordering::Relaxed), 2, "failed transition no count");
    TestResult::Pass
}

fn thread_node_exit_code_preserved() -> TestResult {
    let node = ThreadNode::new();
    node.exit_code.store(42, Ordering::SeqCst);
    node.set_state_safe(ThreadState::Ready).unwrap();
    node.set_state_safe(ThreadState::Running).unwrap();
    node.set_state_safe(ThreadState::Zombie).unwrap();
    assert_eq_test!(node.exit_code.load(Ordering::Relaxed), 42, "exit code after Zombie");
    node.set_state_safe(ThreadState::Terminated).unwrap();
    assert_eq_test!(node.exit_code.load(Ordering::Relaxed), 42, "exit code after Terminated");
    TestResult::Pass
}

fn scheduler_ex_initialization() -> TestResult {
    let sched = SchedulerEx::new();
    assert_eq_test!(sched.current.load(Ordering::SeqCst), 0, "current init");
    assert_eq_test!(sched.idle_thread.load(Ordering::SeqCst), 0, "idle init");
    assert_eq_test!(sched.tick_count.load(Ordering::SeqCst), 0, "tick init");
    assert_eq_test!(sched.need_reschedule.load(Ordering::SeqCst), 0, "reschedule init");
    assert_eq_test!(sched.runq.total.load(Ordering::SeqCst), 0, "runq total init");
    for level in 0..SCHED_LEVELS {
        assert_eq_test!(sched.runq.queues[level].load(Ordering::SeqCst), 0, "queue init");
        assert_eq_test!(sched.runq.counts[level].load(Ordering::SeqCst), 0, "count init");
    }
    TestResult::Pass
}

fn scheduler_ex_priority_mapping() -> TestResult {
    assert_eq_test!(SchedulerEx::priority_to_level(ThreadPriority::Realtime), 0, "Realtime level");
    assert_eq_test!(SchedulerEx::priority_to_level(ThreadPriority::High), 1, "High level");
    assert_eq_test!(SchedulerEx::priority_to_level(ThreadPriority::Normal), 2, "Normal level");
    assert_eq_test!(SchedulerEx::priority_to_level(ThreadPriority::Low), 3, "Low level");
    assert_eq_test!(SchedulerEx::priority_to_level(ThreadPriority::Idle), 3, "Idle level");
    TestResult::Pass
}

fn scheduler_ex_level_to_quantum() -> TestResult {
    assert_eq_test!(SchedulerEx::level_to_quantum(0), SCHED_LEVEL_0_QUANTUM, "level 0 quantum");
    assert_eq_test!(SchedulerEx::level_to_quantum(1), SCHED_LEVEL_1_QUANTUM, "level 1 quantum");
    assert_eq_test!(SchedulerEx::level_to_quantum(2), SCHED_LEVEL_2_QUANTUM, "level 2 quantum");
    assert_eq_test!(SchedulerEx::level_to_quantum(3), SCHED_LEVEL_3_QUANTUM, "level 3 quantum");
    assert_eq_test!(SchedulerEx::level_to_quantum(99), SCHED_LEVEL_3_QUANTUM, "fallback quantum");
    TestResult::Pass
}

fn null_pointer_safety() -> TestResult {
    let sched = SchedulerEx::new();
    sched.add_thread(core::ptr::null_mut());
    assert_eq_test!(sched.runq.total.load(Ordering::SeqCst), 0, "null thread rejected");
    TestResult::Pass
}

fn empty_queue_pop() -> TestResult {
    let sched = SchedulerEx::new();
    for level in 0..SCHED_LEVELS {
        check!(sched.run_queue_pop(level).is_none(), "empty queue pop");
    }
    TestResult::Pass
}

pub fn register_sched_ex_tests() {
    let r = runner();
    r.register("sched::thread_state", "from_u8_valid", thread_state_from_u8_valid as TestFn);
    r.register("sched::thread_state", "from_u8_invalid", thread_state_from_u8_invalid as TestFn);
    r.register("sched::thread_state", "name", thread_state_name as TestFn);
    r.register("sched::thread_state", "is_runnable", thread_state_is_runnable as TestFn);
    r.register("sched::thread_state", "is_alive", thread_state_is_alive as TestFn);
    r.register("sched::thread_state", "can_freeze", thread_state_can_freeze as TestFn);
    r.register("sched::thread_node", "normal_lifecycle", thread_node_normal_lifecycle as TestFn);
    r.register("sched::thread_node", "freeze_thaw", thread_node_freeze_thaw as TestFn);
    r.register("sched::thread_node", "illegal_transitions", thread_node_illegal_transitions as TestFn);
    r.register("sched::thread_node", "state_change_count", thread_node_state_change_count as TestFn);
    r.register("sched::thread_node", "exit_code_preserved", thread_node_exit_code_preserved as TestFn);
    r.register("sched::ex", "initialization", scheduler_ex_initialization as TestFn);
    r.register("sched::ex", "priority_mapping", scheduler_ex_priority_mapping as TestFn);
    r.register("sched::ex", "level_to_quantum", scheduler_ex_level_to_quantum as TestFn);
    r.register("sched::ex", "null_pointer_safety", null_pointer_safety as TestFn);
    r.register("sched::ex", "empty_queue_pop", empty_queue_pop as TestFn);
}

pub fn register_tests() {
    register_sched_ex_tests();
}
