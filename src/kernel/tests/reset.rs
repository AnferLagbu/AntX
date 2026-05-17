//! # BSR/BHR 单元测试
//!
//! 测试 Barrier Soft Reset 和 Barrier Hard Reset 功能

use crate::kernel::tests::{TestCase, TestResult, check, assert_eq_test};

fn snapshot_basic() -> TestResult {
    use crate::kernel::barrier::snapshot::tests;
    check!(tests::test_snapshot_basic(), "snapshot basic");
    TestResult::Pass
}

fn snapshot_registry() -> TestResult {
    use crate::kernel::barrier::snapshot::tests;
    check!(tests::test_registry_register(), "registry register");
    check!(tests::test_registry_priority_order(), "registry priority");
    TestResult::Pass
}

fn recovery_result_types() -> TestResult {
    use crate::kernel::barrier::reset::tests;
    check!(tests::test_recovery_result(), "recovery result");
    check!(tests::test_recovery_layer(), "recovery layer");
    TestResult::Pass
}

fn recovery_config() -> TestResult {
    use crate::kernel::barrier::reset::tests;
    check!(tests::test_config_default(), "config default");
    TestResult::Pass
}

fn audit_log() -> TestResult {
    use crate::kernel::barrier::reset::tests;
    check!(tests::test_audit_log(), "audit log");
    TestResult::Pass
}

fn bsr_freeze_unfreeze() -> TestResult {
    use crate::kernel::barrier::reset::tests;
    check!(tests::test_bsr_freeze_unfreeze(), "bsr freeze/unfreeze");
    TestResult::Pass
}

fn recovery_stats() -> TestResult {
    use crate::kernel::barrier::reset::tests;
    check!(tests::test_stats(), "recovery stats");
    TestResult::Pass
}

fn device_type_enum() -> TestResult {
    use crate::kernel::barrier::DeviceType;
    
    assert_eq_test!(DeviceType::Keyboard as u32, 1, "keyboard type");
    assert_eq_test!(DeviceType::Serial as u32, 2, "serial type");
    assert_eq_test!(DeviceType::Timer as u32, 3, "timer type");
    assert_eq_test!(DeviceType::Network as u32, 4, "network type");
    assert_eq_test!(DeviceType::Storage as u32, 5, "storage type");
    
    assert_eq_test!(DeviceType::from_u32(1), DeviceType::Keyboard, "from_u32 keyboard");
    assert_eq_test!(DeviceType::from_u32(99), DeviceType::Unknown, "from_u32 unknown");
    
    TestResult::Pass
}

fn recovery_layer_order() -> TestResult {
    use crate::kernel::barrier::RecoveryLayer;
    
    assert_eq_test!(RecoveryLayer::Layer1 as u32, 1, "layer1 value");
    assert_eq_test!(RecoveryLayer::Layer2 as u32, 2, "layer2 value");
    assert_eq_test!(RecoveryLayer::Layer3 as u32, 3, "layer3 value");
    
    TestResult::Pass
}

fn recovery_result_checks() -> TestResult {
    use crate::kernel::barrier::RecoveryResult;
    
    let success = RecoveryResult::Success;
    let failed = RecoveryResult::Failed;
    let escalate = RecoveryResult::Escalate;
    
    check!(success.is_success(), "success is_success");
    check!(!success.should_escalate(), "success not escalate");
    
    check!(!failed.is_success(), "failed not success");
    check!(!failed.should_escalate(), "failed not escalate");
    
    check!(!escalate.is_success(), "escalate not success");
    check!(escalate.should_escalate(), "escalate should_escalate");
    
    TestResult::Pass
}

fn snapshot_register_api() -> TestResult {
    use crate::kernel::barrier::{snapshot_register_device, snapshot_unregister_device, DeviceType};
    
    let registered = snapshot_register_device(999, DeviceType::Timer, "test_dev", 0xF000, 10);
    check!(registered, "register device");
    
    let unregistered = snapshot_unregister_device(999);
    check!(unregistered, "unregister device");
    
    TestResult::Pass
}

fn recovery_stats_api() -> TestResult {
    use crate::kernel::barrier::{recovery_get_stats, recovery_reset_stats};
    
    recovery_reset_stats();
    let (bsr, bhr, tick) = recovery_get_stats();
    assert_eq_test!(bsr, 0, "bsr count");
    assert_eq_test!(bhr, 0, "bhr count");
    assert_eq_test!(tick, 0, "last tick");
    
    TestResult::Pass
}

pub fn register_tests() {
    let runner = unsafe { crate::kernel::tests::TEST_RUNNER.get().unwrap() };
    runner.register("barrier::snapshot", "basic", snapshot_basic);
    runner.register("barrier::snapshot", "registry", snapshot_registry);
    runner.register("barrier::reset", "result_types", recovery_result_types);
    runner.register("barrier::reset", "config", recovery_config);
    runner.register("barrier::reset", "audit_log", audit_log);
    runner.register("barrier::bsr", "freeze_unfreeze", bsr_freeze_unfreeze);
    runner.register("barrier::reset", "stats", recovery_stats);
    runner.register("barrier::snapshot", "device_type", device_type_enum);
    runner.register("barrier::reset", "layer_order", recovery_layer_order);
    runner.register("barrier::reset", "result_checks", recovery_result_checks);
    runner.register("barrier::snapshot", "register_api", snapshot_register_api);
}
