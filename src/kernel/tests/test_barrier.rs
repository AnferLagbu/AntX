use alloc::boxed::Box;
use crate::kernel::barrier::undo_log::UndoLog;
use crate::kernel::barrier::domain::RecoveryDomain;
use crate::kernel::tests::runner;
use crate::check;

fn test_undo_log_basic() -> Result<(), &'static str> {
    let mut undo = Box::new(UndoLog::new());
    check!(undo.count == 0, "expected empty undo log");
    Ok(())
}

fn test_undo_log_record() -> Result<(), &'static str> {
    let mut undo = Box::new(UndoLog::new());
    undo.current_generation = 1;

    let mut value: u64 = 42;
    let old = value;
    value = 100;
    undo.record(&mut value as *mut u64, old);

    check!(undo.count == 1, "expected 1 entry after record");
    check!(undo.entries[0].generation == 1, "generation mismatch");
    Ok(())
}

fn test_undo_log_dedup() -> Result<(), &'static str> {
    let mut undo = Box::new(UndoLog::new());
    undo.current_generation = 1;

    let mut v: u64 = 1;
    undo.record(&mut v as *mut u64, v);
    v = 2;
    undo.record(&mut v as *mut u64, v);
    v = 3;
    undo.record(&mut v as *mut u64, v);

    check!(undo.count <= 3, "dedup should limit entries");
    Ok(())
}

fn test_undo_log_rollback() -> Result<(), &'static str> {
    let mut undo = Box::new(UndoLog::new());
    undo.current_generation = 1;

    let mut v: u64 = 42;
    undo.record(&mut v as *mut u64, v);
    v = 99;

    check!(undo.count == 1, "should have 1 entry");

    let rolled = undo.rollback_to(0);
    check!(rolled == 1, "should have rolled back 1 entry");
    check!(undo.count == 0, "undo log should be empty after full rollback");
    Ok(())
}

fn test_domain_create() -> Result<(), &'static str> {
    let dom = RecoveryDomain::new(6);
    check!(dom.id == 6, "domain id mismatch");
    Ok(())
}

fn test_domain_barrier_push() -> Result<(), &'static str> {
    let dom = RecoveryDomain::new(7);
    dom.barrier_generation.store(3, core::sync::atomic::Ordering::SeqCst);
    dom.push_barrier_snapshot(100);

    let top = dom.barrier_stack_top.load(core::sync::atomic::Ordering::SeqCst) as usize;
    check!(top == 1, "barrier stack top should be 1");
    Ok(())
}

fn test_domain_dependency() -> Result<(), &'static str> {
    let dom = RecoveryDomain::new(9);
    check!(dom.add_dependency(2), "add_dependency failed");
    check!(dom.depends_on_id(2), "depends_on_id should return true");
    check!(dom.dependency_count() == 1, "dependency_count should be 1");
    check!(!dom.depends_on_id(99), "should not depend on 99");
    Ok(())
}

pub fn register_barrier_tests() {
    let r = runner();
    r.register("barrier::undo_log", "basic", test_undo_log_basic);
    r.register("barrier::undo_log", "record", test_undo_log_record);
    r.register("barrier::undo_log", "dedup", test_undo_log_dedup);
    r.register("barrier::undo_log", "rollback", test_undo_log_rollback);
    r.register("barrier::domain", "create", test_domain_create);
    r.register("barrier::domain", "barrier_push", test_domain_barrier_push);
    r.register("barrier::domain", "dependency", test_domain_dependency);
}
