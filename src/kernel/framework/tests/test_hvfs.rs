#![cfg(target_arch = "x86_64")]
use crate::register_tests_inner;

use super::check;
use crate::kernel::framework::fs::hvfs::arc::{HvArc, HvArcKey, HvArcBufType};
use crate::kernel::framework::fs::hvfs::bp::{HvBlockPointer, HvDva, HvCksumType};
use crate::kernel::framework::fs::hvfs::checksum::HvChecksum;
use crate::kernel::framework::fs::hvfs::dmu::{HvDmuObject, HvObjType};
use crate::kernel::framework::fs::hvfs::spa::{HvSpaConfig, HvUberblock, HV_SPA_MAGIC};
use crate::kernel::framework::fs::hvfs::txg::HvTxgGroup;
use crate::kernel::framework::fs::hvfs::zap::HvZap;
use crate::kernel::framework::fs::hvfs::zil::{HvZilRecord, HvZil};
use crate::kernel::framework::tests::{runner, TestResult};

fn test_bp_null() -> TestResult {
    let bp = HvBlockPointer::null();
    check!(bp.is_null(), "null bp should be null");
    check!(bp.get_dva(0).is_none(), "null bp dva should be None");
    TestResult::Pass
}

#[expect(clippy::manual_let_else, reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底")]
fn test_bp_dva_set_get() -> TestResult {
    let mut bp = HvBlockPointer::null();
    let dva = HvDva::new(0, 4096, 8192);
    bp.set_dva(0, dva);
    let got = match bp.get_dva(0) {
        Some(v) => v,
        None => return TestResult::Fail("dva not set"),
    };
    check!(got.vdev_id == 0, "vdev_id mismatch");
    check!(got.offset == 4096, "offset mismatch");
    check!(got.asize == 8192, "asize mismatch");
    TestResult::Pass
}

fn test_bp_birth_txg() -> TestResult {
    let mut bp = HvBlockPointer::null();
    bp.set_birth(42);
    check!(bp.birth_txg == 42, "birth txg mismatch");
    TestResult::Pass
}

fn test_checksum_fletcher4_basic() -> TestResult {
    let data = b"hello world test data for checksum verification";
    let ck_a = HvChecksum::compute(HvCksumType::Fletcher4, data);
    let ck_b = HvChecksum::compute(HvCksumType::Fletcher4, data);
    check!(
        ck_a.value == ck_b.value,
        "same data should produce same checksum"
    );
    TestResult::Pass
}

fn test_checksum_different_data() -> TestResult {
    let ck_a = HvChecksum::compute(HvCksumType::Fletcher4, b"hello");
    let ck_b = HvChecksum::compute(HvCksumType::Fletcher4, b"world");
    check!(ck_a.value != ck_b.value, "different data should differ");
    TestResult::Pass
}

fn test_spa_config_name() -> TestResult {
    let cfg = HvSpaConfig::new("test-pool");
    let name = core::str::from_utf8(&cfg.name)
        .unwrap_or("")
        .trim_end_matches('\0');
    check!(name.starts_with("test-pool"), "expected test-pool prefix");
    TestResult::Pass
}

fn test_spa_uberblock_null() -> TestResult {
    let ub = HvUberblock::null();
    check!(!ub.is_valid(), "null uberblock should be invalid");
    TestResult::Pass
}

fn test_spa_uberblock_checksum() -> TestResult {
    let mut ub = HvUberblock {
        txg: 1,
        root_bp: HvBlockPointer::null(),
        timestamp: 100,
        root_dataset_obj: 0,
        pool_guid: 0xABCD,
        checkpoint_txg: 0,
        checksum: [0; 4],
        magic: HV_SPA_MAGIC,
        pwm_domain_id: 0,
        _pad: [0; 2],
    };
    ub.compute_checksum();
    check!(ub.verify_checksum(), "checksum should verify");
    TestResult::Pass
}

fn test_spa_uberblock_invalid_magic() -> TestResult {
    let mut ub = HvUberblock::null();
    ub.magic = 0xDEADBEEF;
    check!(!ub.is_valid(), "wrong magic should be invalid");
    TestResult::Pass
}

fn test_dmu_object_default() -> TestResult {
    let obj = HvDmuObject::new_file(1, 0);
    check!(obj.obj_id == 1, "obj_id mismatch");
    check!(obj.obj_type == HvObjType::File, "obj_type should be File");
    check!(obj.size == 0, "new object size should be 0");
    TestResult::Pass
}

fn test_dmu_object_cow() -> TestResult {
    let mut obj = HvDmuObject::new_file(2, 0);
    let new_bp = HvBlockPointer::null();
    obj.cow_bp(new_bp, 5);
    check!(obj.birth_txg == 5, "birth txg should be 5");
    TestResult::Pass
}

fn test_dmu_object_dir_type() -> TestResult {
    let obj = HvDmuObject::new_dir(3, 0);
    check!(obj.is_dir(), "Dir should report as dir");
    TestResult::Pass
}

#[expect(clippy::manual_let_else, reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底")]
fn test_zap_insert_lookup() -> TestResult {
    let zap = HvZap::new();
    zap.insert_u64("key1", 42);
    let val = match zap.lookup_u64("key1") {
        Some(v) => v,
        None => return TestResult::Fail("key1 not found"),
    };
    check!(val == 42, "value mismatch");
    TestResult::Pass
}

#[expect(clippy::manual_let_else, reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底")]
fn test_zap_overwrite() -> TestResult {
    let zap = HvZap::new();
    zap.insert_u64("key1", 10);
    zap.insert_u64("key1", 99);
    let val = match zap.lookup_u64("key1") {
        Some(v) => v,
        None => return TestResult::Fail("key1 not found after overwrite"),
    };
    check!(val == 99, "overwrite should set 99");
    TestResult::Pass
}

fn test_zap_nonexistent() -> TestResult {
    let zap = HvZap::new();
    check!(
        zap.lookup_u64("no_such_key").is_none(),
        "nonexistent should be None"
    );
    TestResult::Pass
}

fn test_zap_remove() -> TestResult {
    let zap = HvZap::new();
    zap.insert_u64("rm_me", 7);
    check!(
        zap.lookup_u64("rm_me").is_some(),
        "should exist before remove"
    );
    zap.remove("rm_me");
    check!(
        zap.lookup_u64("rm_me").is_none(),
        "should not exist after remove"
    );
    TestResult::Pass
}

fn test_txg_group_init() -> TestResult {
    let mut tg = HvTxgGroup::new();
    tg.init(1);
    check!(tg.current_txg() >= 1, "txg current should be at least 1");
    TestResult::Pass
}

fn test_txg_group_transition() -> TestResult {
    let mut tg = HvTxgGroup::new();
    tg.init(1);
    let new_txg = tg.transition();
    check!(new_txg >= 2, "txg should advance");
    TestResult::Pass
}

fn test_zil_record_create() -> TestResult {
    let rec = HvZilRecord::new_create(1, 0, "test_file");
    check!(rec.txg == 1, "txg mismatch");
    check!(rec.obj_id == 0, "obj_id should be 0");
    TestResult::Pass
}

fn test_zil_record_write() -> TestResult {
    let rec = HvZilRecord::new_write(2, 10, 0, 1024);
    check!(rec.txg == 2, "txg mismatch");
    check!(rec.obj_id == 10, "obj_id mismatch");
    TestResult::Pass
}

fn test_zil_add_and_sync() -> TestResult {
    let zil = HvZil::new();
    zil.init();
    zil.add_record(HvZilRecord::new_write(1, 5, 0, 512));
    zil.sync(1);
    check!(
        zil.committed_seq.load(core::sync::atomic::Ordering::SeqCst) >= 1,
        "committed_seq should advance"
    );
    TestResult::Pass
}

fn test_arc_init() -> TestResult {
    let arc = HvArc::new();
    arc.init(128);
    check!(arc.is_initialized(), "arc should be initialized");
    TestResult::Pass
}

fn test_arc_lookup_miss() -> TestResult {
    let arc = HvArc::new();
    arc.init(128);
    let key = HvArcKey::new(0, 0, 0);
    check!(arc.lookup(&key).is_none(), "empty arc should return None");
    TestResult::Pass
}

#[expect(clippy::manual_let_else, reason = "manual_let_else: if-let + unwrap 模式改 let-else 语法; 部分场景有 return value 需改 match, 当前优先 expect 兑底")]
fn test_arc_insert_lookup() -> TestResult {
    let arc = HvArc::new();
    arc.init(128);
    let key = HvArcKey::new(0, 4096, 1);
    let data: [u8; 16] = [0xAA; 16];
    arc.insert(key, &data, HvArcBufType::Data);
    let ptr = match arc.lookup(&key) {
        Some(p) => p,
        None => return TestResult::Fail("should find inserted entry"),
    };
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let found = unsafe { core::slice::from_raw_parts(ptr, 16) };
    check!(found[0] == 0xAA, "data mismatch");
    TestResult::Pass
}

pub fn register_hvfs_tests() {
    let r = runner();
    register_tests_inner! { r:
        "hvfs::bp": {
            "null": test_bp_null,
            "dva_set_get": test_bp_dva_set_get,
            "birth_txg": test_bp_birth_txg,
        },
        "hvfs::checksum": {
            "fletcher4_basic": test_checksum_fletcher4_basic,
            "different_data": test_checksum_different_data,
        },
        "hvfs::spa": {
            "config_name": test_spa_config_name,
            "uberblock_null": test_spa_uberblock_null,
            "uberblock_checksum": test_spa_uberblock_checksum,
            "uberblock_invalid": test_spa_uberblock_invalid_magic,
        },
        "hvfs::dmu": {
            "default": test_dmu_object_default,
            "cow": test_dmu_object_cow,
            "dir_type": test_dmu_object_dir_type,
        },
        "hvfs::zap": {
            "insert_lookup": test_zap_insert_lookup,
            "overwrite": test_zap_overwrite,
            "nonexistent": test_zap_nonexistent,
            "remove": test_zap_remove,
        },
        "hvfs::txg": {
            "group_init": test_txg_group_init,
            "transition": test_txg_group_transition,
        },
        "hvfs::zil": {
            "record_create": test_zil_record_create,
            "record_write": test_zil_record_write,
            "add_and_sync": test_zil_add_and_sync,
        },
        "hvfs::arc": {
            "init": test_arc_init,
            "lookup_miss": test_arc_lookup_miss,
            "insert_lookup": test_arc_insert_lookup,
        },
    }
}
