use crate::register_tests_inner;
use crate::kernel::tests::{TestResult, runner, check, assert_eq_test};
use crate::kernel::mm::slab::{
    KmemCache, SLAB_MAX_OBJECT_SIZE, SLAB_MIN_OBJECT_SIZE,
    GENERAL_CACHE_SIZES, find_general_cache_index,
};
use crate::kernel::syscall::types::Errno;
use crate::kernel::credo::sha256::sha256;
use crate::kernel::timer::pit::{
    PIT_BASE_FREQUENCY, DEFAULT_INTERRUPT_FREQ_HZ, PIT_MAX_COUNT, PIT_MIN_COUNT,
};

fn slab_cache_creation() -> TestResult {
    let cache = KmemCache::create("test_cache", 64);
    check!(cache.is_ok(), "cache creation should succeed");
    let cache = cache.unwrap();
    assert_eq_test!(cache.object_size, 64, "object size");
    check!(cache.objects_per_slab > 0, "objects per slab");
    assert_eq_test!(cache.slab_count, 0, "initial slab count");
    TestResult::Pass
}

fn slab_cache_invalid_size() -> TestResult {
    check!(KmemCache::create("zero", 0).is_err(), "size 0 rejected");
    check!(KmemCache::create("huge", SLAB_MAX_OBJECT_SIZE + 1).is_err(), "oversize rejected");
    TestResult::Pass
}

fn slab_cache_min_size() -> TestResult {
    let cache = KmemCache::create("tiny", 8).unwrap();
    assert_eq_test!(cache.object_size, SLAB_MIN_OBJECT_SIZE, "min size enforced");
    TestResult::Pass
}

fn slab_general_cache_sizes() -> TestResult {
    assert_eq_test!(GENERAL_CACHE_SIZES[0], 16, "size 0");
    assert_eq_test!(GENERAL_CACHE_SIZES[3], 128, "size 3");
    assert_eq_test!(GENERAL_CACHE_SIZES[7], 2048, "size 7");
    TestResult::Pass
}

fn slab_find_general_cache_index() -> TestResult {
    assert_eq_test!(find_general_cache_index(16), Some(0), "idx 16");
    assert_eq_test!(find_general_cache_index(32), Some(1), "idx 32");
    assert_eq_test!(find_general_cache_index(64), Some(2), "idx 64");
    assert_eq_test!(find_general_cache_index(2048), Some(7), "idx 2048");
    assert_eq_test!(find_general_cache_index(3000), None, "idx 3000");
    TestResult::Pass
}

fn syscall_error_conversion() -> TestResult {
    assert_eq_test!(Errno::E_PERM.as_i64(), -1, "E_PERM");
    assert_eq_test!(Errno::E_NOMEM.as_i64(), -12, "E_NOMEM");
    assert_eq_test!(Errno::E_INVAL.as_i64(), -22, "E_INVAL");
    TestResult::Pass
}

fn syscall_error_from_i64() -> TestResult {
    assert_eq_test!(Errno::from_i64(-1), Some(Errno::E_PERM), "from -1");
    assert_eq_test!(Errno::from_i64(-22), Some(Errno::E_INVAL), "from -22");
    assert_eq_test!(Errno::from_i64(-999), None, "from -999");
    TestResult::Pass
}

fn syscall_error_display() -> TestResult {
    let s = alloc::format!("{}", Errno::E_PERM);
    assert_eq_test!(s.as_str(), "Operation not permitted", "E_PERM display");
    let s = alloc::format!("{}", Errno::E_NOSYS);
    assert_eq_test!(s.as_str(), "Function not implemented", "E_NOSYS display");
    TestResult::Pass
}

fn sha256_empty() -> TestResult {
    let expected: [u8; 48] = [
        0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
        0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
        0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
        0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq_test!(sha256(b""), expected, "SHA256 empty");
    TestResult::Pass
}

fn sha256_abc() -> TestResult {
    let expected: [u8; 48] = [
        0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea,
        0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
        0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c,
        0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq_test!(sha256(b"abc"), expected, "SHA256 abc");
    TestResult::Pass
}

fn pit_constants() -> TestResult {
    assert_eq_test!(PIT_BASE_FREQUENCY, 1_193_182, "PIT base freq");
    check!(DEFAULT_INTERRUPT_FREQ_HZ > 0, "default freq positive");
    assert_eq_test!(PIT_MAX_COUNT, 65535, "max count");
    assert_eq_test!(PIT_MIN_COUNT, 1, "min count");
    TestResult::Pass
}

fn pit_divisor_calculation() -> TestResult {
    let divisor = PIT_BASE_FREQUENCY / 1000;
    assert_eq_test!(divisor, 1193, "1000Hz divisor");
    let actual_freq = PIT_BASE_FREQUENCY / divisor;
    check!((actual_freq as i64 - 1000).abs() < 5, "actual freq close to 1000");
    TestResult::Pass
}

fn pit_frequency_bounds() -> TestResult {
    check!(PIT_MIN_COUNT >= 1, "min count >= 1");
    assert_eq_test!(PIT_MAX_COUNT as u64, 65535, "max count value");
    let max_freq = PIT_BASE_FREQUENCY / PIT_MIN_COUNT as u64;
    check!(max_freq > 1_000_000, "max freq > 1MHz");
    let min_freq = PIT_BASE_FREQUENCY / PIT_MAX_COUNT as u64;
    check!(min_freq < 20, "min freq < 20Hz");
    TestResult::Pass
}

pub fn register_slab_tests() {
    let r = runner();
    register_tests_inner!{ r:
        "mm::slab": {
            "cache_creation": slab_cache_creation,
            "cache_invalid_size": slab_cache_invalid_size,
            "cache_min_size": slab_cache_min_size,
            "general_cache_sizes": slab_general_cache_sizes,
            "find_general_cache_index": slab_find_general_cache_index,
        },
    }
}

pub fn register_syscall_ffi_tests() {
    let r = runner();
    register_tests_inner!{ r:
        "syscall::ffi": {
            "error_conversion": syscall_error_conversion,
            "error_from_i64": syscall_error_from_i64,
            "error_display": syscall_error_display,
        },
    }
}

pub fn register_sha256_tests() {
    let r = runner();
    register_tests_inner!{ r:
        "pwm::sha256": {
            "empty": sha256_empty,
            "abc": sha256_abc,
        },
    }
}

pub fn register_pit_tests() {
    let r = runner();
    register_tests_inner!{ r:
        "timer::pit": {
            "constants": pit_constants,
            "divisor_calculation": pit_divisor_calculation,
            "frequency_bounds": pit_frequency_bounds,
        },
    }
}

pub fn register_tests() {
    register_slab_tests();
    register_syscall_ffi_tests();
    register_sha256_tests();
    register_pit_tests();
}
