use super::check;
use crate::kernel::fs::devfs::devfs::{DEVFS_DATA, DEVFS_MAX_DEVICES};
use crate::kernel::tests::{runner, TestResult};
use crate::register_tests_inner;

fn test_devfs_mount() -> TestResult {
    let result = DEVFS_DATA.mount("/dev");
    check!(result == 0, "devfs mount failed");
    check!(
        DEVFS_DATA.device_count() == 4,
        "expected 4 default devices after mount"
    );
    TestResult::Pass
}

fn test_devfs_open_default_devices() -> TestResult {
    check!(DEVFS_DATA.open("null").is_some(), "should open null");
    check!(DEVFS_DATA.open("zero").is_some(), "should open zero");
    check!(DEVFS_DATA.open("console").is_some(), "should open console");
    check!(DEVFS_DATA.open("tty").is_some(), "should open tty");
    check!(
        DEVFS_DATA.open("nonexistent").is_none(),
        "should not open nonexistent device"
    );
    TestResult::Pass
}

fn test_devfs_read_null() -> TestResult {
    let mut buf = [0xAAu8; 16];
    let n = DEVFS_DATA.read(0, &mut buf);
    check!(n == 0, "null device should return 0 bytes");
    TestResult::Pass
}

fn test_devfs_read_zero() -> TestResult {
    let mut buf = [0xFFu8; 16];
    let n = DEVFS_DATA.read(1, &mut buf);
    check!(n == 16, "zero device should fill buffer");
    check!(buf == [0u8; 16], "zero device should fill with zeros");
    TestResult::Pass
}

fn test_devfs_register_device() -> TestResult {
    let count_before = DEVFS_DATA.device_count();
    let result = DEVFS_DATA.register_device("testdev", 10);
    check!(result == 0, "register_device should succeed");
    check!(
        DEVFS_DATA.device_count() == count_before + 1,
        "device count should increase"
    );
    check!(
        DEVFS_DATA.open("testdev").is_some(),
        "should open newly registered device"
    );
    TestResult::Pass
}

fn test_devfs_unregister_device() -> TestResult {
    DEVFS_DATA.register_device("tempdev", 20);
    let count_before = DEVFS_DATA.device_count();
    let result = DEVFS_DATA.unregister_device("tempdev");
    check!(result == 0, "unregister_device should succeed");
    check!(
        DEVFS_DATA.device_count() == count_before - 1,
        "device count should decrease"
    );
    check!(
        DEVFS_DATA.open("tempdev").is_none(),
        "should not open unregistered device"
    );
    TestResult::Pass
}

fn test_devfs_register_duplicate() -> TestResult {
    let result = DEVFS_DATA.register_device("null", 0);
    check!(result == -1, "registering duplicate should fail");
    TestResult::Pass
}

fn test_devfs_unregister_nonexistent() -> TestResult {
    let result = DEVFS_DATA.unregister_device("nonexistent_dev");
    check!(result == -1, "unregistering nonexistent should fail");
    TestResult::Pass
}

fn test_devfs_readdir() -> TestResult {
    let first = DEVFS_DATA.readdir(0);
    check!(first.is_some(), "readdir(0) should return a device");
    let beyond = DEVFS_DATA.readdir(DEVFS_MAX_DEVICES + 10);
    check!(beyond.is_none(), "readdir beyond count should return None");
    TestResult::Pass
}

pub fn register_devfs_tests() {
    let r = runner();
    register_tests_inner! { r:
        "DevFS": {
            "mount": test_devfs_mount,
            "open_default_devices": test_devfs_open_default_devices,
            "read_null": test_devfs_read_null,
            "read_zero": test_devfs_read_zero,
            "register_device": test_devfs_register_device,
            "unregister_device": test_devfs_unregister_device,
            "register_duplicate": test_devfs_register_duplicate,
            "unregister_nonexistent": test_devfs_unregister_nonexistent,
            "readdir": test_devfs_readdir,
        },
    }
}
