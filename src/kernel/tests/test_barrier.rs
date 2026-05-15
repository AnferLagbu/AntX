use crate::kernel::barrier::domain::RecoveryDomain;
use crate::kernel::tests::runner;
use crate::{check, assert_eq_test};

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
    r.register("barrier::domain", "create", test_domain_create);
    r.register("barrier::domain", "barrier_push", test_domain_barrier_push);
    r.register("barrier::domain", "dependency", test_domain_dependency);
}
