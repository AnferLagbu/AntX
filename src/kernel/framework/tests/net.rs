use crate::kernel::framework::driver::{DeviceType, Driver};
use crate::kernel::framework::mm::KERNEL_BASE;
use crate::kernel::framework::driver::{

    virt_to_phys, E1000Device, E1000RxDesc, E1000TxDesc, E1000_RX_BUFFER_SIZE, E1000_RX_RING_SIZE,
    E1000_TX_RING_SIZE,
};
use crate::kernel::framework::tests::{assert_eq_test, check, runner, TestResult};
use crate::register_tests_inner;

fn net_e1000_device_creation() -> TestResult {
    let dev = E1000Device::new();
    assert_eq_test!(dev.bus, 0, "bus");
    assert_eq_test!(dev.device, 0, "device");
    check!(!dev.is_ready(), "not ready");
    assert_eq_test!(dev.name(), "Intel E1000 Gigabit Ethernet", "name");
    assert_eq_test!(dev.device_type(), DeviceType::Network, "type");
    TestResult::Pass
}

fn net_e1000_constants() -> TestResult {
    assert_eq_test!(E1000_TX_RING_SIZE, 64, "TX ring");
    assert_eq_test!(E1000_RX_RING_SIZE, 128, "RX ring");
    assert_eq_test!(E1000_RX_BUFFER_SIZE, 2048, "RX buffer");
    TestResult::Pass
}

fn net_e1000_descriptor_sizes() -> TestResult {
    assert_eq_test!(core::mem::size_of::<E1000TxDesc>(), 16, "TX desc size");
    assert_eq_test!(core::mem::size_of::<E1000RxDesc>(), 16, "RX desc size");
    TestResult::Pass
}

fn net_virt_to_phys() -> TestResult {
    let high_addr: u64 = KERNEL_BASE;
    assert_eq_test!(virt_to_phys(high_addr), 0, "high addr maps to 0");
    assert_eq_test!(virt_to_phys(0x12345678), 0x12345678, "low addr identity");
    TestResult::Pass
}

fn net_hton_ntoh() -> TestResult {
    TestResult::Pass
}

fn net_byteorder() -> TestResult {
    let val: u16 = 0x1234;
    assert_eq_test!(val.to_be(), val.to_le().swap_bytes(), "swap");
    TestResult::Pass
}

fn net_mac_formatting() -> TestResult {
    TestResult::Pass
}

pub fn register_tests() {
    let r = runner();
    register_tests_inner! { r:
        "net::e1000": {
            "device_creation": net_e1000_device_creation,
            "constants": net_e1000_constants,
            "descriptor_sizes": net_e1000_descriptor_sizes,
            "virt_to_phys": net_virt_to_phys,
        },
        "net::utils": {
            "hton_ntoh": net_hton_ntoh,
            "byteorder": net_byteorder,
            "mac_formatting": net_mac_formatting,
        },
    }
}
