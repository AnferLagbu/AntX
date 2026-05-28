use crate::register_tests_inner;
use crate::kernel::tests::{TestResult, runner, check, assert_eq_test};
use crate::kernel::net::utils::{
    atoi, strtol, inet_checksum, htons, ntohs, htonl, ntohl, format_mac,
};
use crate::kernel::net::driver::e1000::{
    E1000Device, E1000_TX_RING_SIZE, E1000_RX_RING_SIZE, E1000_RX_BUFFER_SIZE,
    E1000TxDesc, E1000RxDesc, virt_to_phys,
};
use crate::kernel::net::apps::{PingStats, NetAppError, internet_checksum as apps_checksum};
use crate::kernel::driver::{DeviceType, Driver};

fn net_atoi() -> TestResult {
    unsafe {
        assert_eq_test!(atoi(b"123\0".as_ptr() as *const i8), 123, "atoi 123");
        assert_eq_test!(atoi(b"-456\0".as_ptr() as *const i8), -456, "atoi -456");
        assert_eq_test!(atoi(b"0\0".as_ptr() as *const i8), 0, "atoi 0");
        assert_eq_test!(atoi(b"  789  \0".as_ptr() as *const i8), 789, "atoi spaces");
    }
    TestResult::Pass
}

fn net_strtol() -> TestResult {
    unsafe {
        let mut endptr: *mut i8 = core::ptr::null_mut();
        let val = strtol(b"12345\0".as_ptr() as *const i8, &mut endptr, 0);
        assert_eq_test!(val, 12345, "strtol decimal");
        let val = strtol(b"0xFF\0".as_ptr() as *const i8, &mut endptr, 0);
        assert_eq_test!(val, 255, "strtol hex");
        let val = strtol(b"0777\0".as_ptr() as *const i8, &mut endptr, 0);
        assert_eq_test!(val, 511, "strtol octal");
        let val = strtol(b"-100\0".as_ptr() as *const i8, &mut endptr, 0);
        assert_eq_test!(val, -100, "strtol negative");
    }
    TestResult::Pass
}

fn net_inet_checksum() -> TestResult {
    let data = b"Hello World";
    let cksum = inet_checksum(data);
    assert_eq_test!(cksum, inet_checksum(data), "checksum idempotent");
    let data2 = b"Hello World!";
    check!(cksum != inet_checksum(data2), "different data different checksum");
    TestResult::Pass
}

fn net_byteorder() -> TestResult {
    assert_eq_test!(htons(0x1234), 0x1234_u16.to_be(), "htons");
    assert_eq_test!(ntohs(0x3412), u16::from_be(0x3412), "ntohs");
    assert_eq_test!(htonl(0x12345678), 0x12345678_u32.to_be(), "htonl");
    assert_eq_test!(ntohl(0x78563412), u32::from_be(0x78563412), "ntohl");
    TestResult::Pass
}

fn net_mac_formatting() -> TestResult {
    let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
    let mut buf = [0u8; 18];
    let len = format_mac(&mac, &mut buf);
    assert_eq_test!(len, 17, "MAC len");
    assert_eq_test!(&buf[..17], b"00:11:22:33:44:55", "MAC format");
    TestResult::Pass
}

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
    let high_addr: u64 = 0xFFFF800000000000;
    assert_eq_test!(virt_to_phys(high_addr), 0, "high addr maps to 0");
    assert_eq_test!(virt_to_phys(0x12345678), 0x12345678, "low addr identity");
    TestResult::Pass
}

fn net_ping_stats() -> TestResult {
    let stats = PingStats::new();
    assert_eq_test!(stats.get_stats(), (0, 0, false), "initial stats");
    for _ in 0..3 { stats.increment_sent(); }
    assert_eq_test!(stats.get_stats().0, 3, "sent count");
    stats.increment_received();
    let (_, received, has_reply) = stats.get_stats();
    assert_eq_test!(received, 1, "received count");
    check!(has_reply, "has reply");
    TestResult::Pass
}

fn net_internet_checksum() -> TestResult {
    let data = [0x45, 0x00];
    let checksum = apps_checksum(&data);
    check!(checksum != 0, "checksum non-zero");
    TestResult::Pass
}

fn net_netapp_error_codes() -> TestResult {
    assert_eq_test!(NetAppError::Ok.as_i32(), 0, "Ok");
    assert_eq_test!(NetAppError::OutOfMemory.as_i32(), -1, "OOM");
    assert_eq_test!(NetAppError::InvalidArg.as_i32(), -2, "InvalidArg");
    assert_eq_test!(NetAppError::Timeout.as_i32(), -3, "Timeout");
    TestResult::Pass
}

pub fn register_tests() {
    let r = runner();
    register_tests_inner!{ r:
        "net::utils": {
            "atoi": net_atoi,
            "strtol": net_strtol,
            "inet_checksum": net_inet_checksum,
            "byteorder": net_byteorder,
            "mac_formatting": net_mac_formatting,
        },
        "net::e1000": {
            "device_creation": net_e1000_device_creation,
            "constants": net_e1000_constants,
            "descriptor_sizes": net_e1000_descriptor_sizes,
            "virt_to_phys": net_virt_to_phys,
        },
        "net::apps": {
            "ping_stats": net_ping_stats,
            "internet_checksum": net_internet_checksum,
            "error_codes": net_netapp_error_codes,
        },
    }
}
