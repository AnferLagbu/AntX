use crate::kernel::fs::hvfs::bp::*;
use crate::kernel::fs::hvfs::checksum::HvChecksum;
use crate::kernel::fs::hvfs::spa::*;
use crate::kernel::fs::hvfs::dmu::*;
use crate::kernel::fs::hvfs::arc::*;
use crate::kernel::fs::hvfs::zap::*;
use crate::kernel::fs::hvfs::txg::*;
use crate::kernel::fs::hvfs::zil::*;
use crate::kernel::tests::runner;
use crate::{check, assert_eq_test};

fn test_bp_null() -> Result<(), &'static str> {
    let bp = HvBlockPointer::null();
    check!(bp.is_null(), "null bp should be null");
    check!(bp.get_dva(0).is_none(), "null bp dva should be None");
    Ok(())
}

fn test_bp_dva_set_get() -> Result<(), &'static str> {
    let mut bp = HvBlockPointer::null();
    let dva = HvDva::new(0, 4096, 8192);
    bp.set_dva(0, dva);
    let got = bp.get_dva(0).ok_or("dva not set")?;
    check!(got.vdev_id == 0, "vdev_id mismatch");
    check!(got.offset == 4096, "offset mismatch");
    check!(got.asize == 8192, "asize mismatch");
    Ok(())
}

fn test_bp_birth_txg() -> Result<(), &'static str> {
    let mut bp = HvBlockPointer::null();
    bp.set_birth(42);
    check!(bp.birth_txg == 42, "birth txg mismatch");
    Ok(())
}

fn test_checksum_fletcher4_basic() -> Result<(), &'static str> {
    let data = b"hello world test data for checksum verification";
    let ck_a = HvChecksum::compute(HvCksumType::Fletcher4, data);
    let ck_b = HvChecksum::compute(HvCksumType::Fletcher4, data);
    check!(ck_a.value == ck_b.value, "same data should produce same checksum");
    Ok(())
}

fn test_checksum_different_data() -> Result<(), &'static str> {
    let ck_a = HvChecksum::compute(HvCksumType::Fletcher4, b"hello");
    let ck_b = HvChecksum::compute(HvCksumType::Fletcher4, b"world");
    check!(ck_a.value != ck_b.value, "different data should differ");
    Ok(())
}

fn test_spa_config_name() -> Result<(), &'static str> {
    let cfg = HvSpaConfig::new("test-pool");
    let name = core::str::from_utf8(&cfg.name).unwrap_or("").trim_end_matches('\0');
    check!(name.starts_with("test-pool"), "expected test-pool prefix");
    Ok(())
}

fn test_spa_uberblock_null() -> Result<(), &'static str> {
    let ub = HvUberblock::null();
    check!(!ub.is_valid(), "null uberblock should be invalid");
    Ok(())
}

fn test_spa_uberblock_checksum() -> Result<(), &'static str> {
    let mut ub = HvUberblock {
        magic: HV_SPA_MAGIC,
        version: HV_SPA_VERSION,
        txg: 1,
        root_bp: HvBlockPointer::null(),
        timestamp: 100,
        root_dataset_obj: 0,
        pool_guid: 0xABCD,
        checkpoint_txg: 0,
        pwid_domain_id: 0,
        _pad: [0; 6],
        checksum: [0; 4],
    };
    ub.compute_checksum();
    check!(ub.verify_checksum(), "checksum should verify");
    Ok(())
}

fn test_spa_uberblock_invalid_magic() -> Result<(), &'static str> {
    let mut ub = HvUberblock::null();
    ub.magic = 0xDEADBEEF;
    check!(!ub.is_valid(), "wrong magic should be invalid");
    Ok(())
}

fn test_dmu_object_default() -> Result<(), &'static str> {
    let obj = HvDmuObject::new_file(1, 0);
    check!(obj.obj_id == 1, "obj_id mismatch");
    check!(obj.obj_type == HvObjType::File, "obj_type should be File");
    check!(obj.size == 0, "new object size should be 0");
    Ok(())
}

fn test_dmu_object_cow() -> Result<(), &'static str> {
    let mut obj = HvDmuObject::new_file(2, 0);
    let new_bp = HvBlockPointer::null();
    obj.cow_bp(new_bp, 5);
    check!(obj.birth_txg == 5, "birth txg should be 5");
    Ok(())
}

fn test_dmu_object_dir_type() -> Result<(), &'static str> {
    let obj = HvDmuObject::new_dir(3, 0);
    check!(obj.is_dir(), "Dir should report as dir");
    Ok(())
}

fn test_zap_insert_lookup() -> Result<(), &'static str> {
    let mut zap = HvZap::new();
    zap.insert_u64("key1", 42);
    let val = zap.lookup_u64("key1").ok_or("key1 not found")?;
    check!(val == 42, "value mismatch");
    Ok(())
}

fn test_zap_overwrite() -> Result<(), &'static str> {
    let mut zap = HvZap::new();
    zap.insert_u64("key1", 10);
    zap.insert_u64("key1", 99);
    let val = zap.lookup_u64("key1").ok_or("key1 not found after overwrite")?;
    check!(val == 99, "overwrite should set 99");
    Ok(())
}

fn test_zap_nonexistent() -> Result<(), &'static str> {
    let zap = HvZap::new();
    check!(zap.lookup_u64("no_such_key").is_none(), "nonexistent should be None");
    Ok(())
}

fn test_zap_remove() -> Result<(), &'static str> {
    let mut zap = HvZap::new();
    zap.insert_u64("rm_me", 7);
    check!(zap.lookup_u64("rm_me").is_some(), "should exist before remove");
    zap.remove("rm_me");
    check!(zap.lookup_u64("rm_me").is_none(), "should not exist after remove");
    Ok(())
}

fn test_txg_group_init() -> Result<(), &'static str> {
    let mut tg = HvTxgGroup::new();
    tg.init(1);
    check!(tg.current_txg() >= 1, "txg current should be at least 1");
    Ok(())
}

fn test_txg_group_transition() -> Result<(), &'static str> {
    let mut tg = HvTxgGroup::new();
    tg.init(1);
    let new_txg = tg.transition();
    check!(new_txg >= 2, "txg should advance");
    Ok(())
}

fn test_zil_record_create() -> Result<(), &'static str> {
    let rec = HvZilRecord::new_create(1, 0, "test_file");
    check!(rec.txg == 1, "txg mismatch");
    check!(rec.obj_id == 0, "obj_id should be 0");
    Ok(())
}

fn test_zil_record_write() -> Result<(), &'static str> {
    let rec = HvZilRecord::new_write(2, 10, 0, 1024);
    check!(rec.txg == 2, "txg mismatch");
    check!(rec.obj_id == 10, "obj_id mismatch");
    Ok(())
}

fn test_zil_add_and_sync() -> Result<(), &'static str> {
    let mut zil = HvZil::new();
    zil.init();
    zil.add_record(HvZilRecord::new_write(1, 5, 0, 512));
    zil.sync(1);
    check!(zil.committed_seq.load(core::sync::atomic::Ordering::SeqCst) >= 1, "committed_seq should advance");
    Ok(())
}

fn test_arc_init() -> Result<(), &'static str> {
    let arc = HvArc::new();
    arc.init(128);
    check!(arc.is_initialized(), "arc should be initialized");
    Ok(())
}

fn test_arc_lookup_miss() -> Result<(), &'static str> {
    let arc = HvArc::new();
    arc.init(128);
    let key = HvArcKey::new(0, 0, 0);
    check!(arc.lookup(&key).is_none(), "empty arc should return None");
    Ok(())
}

fn test_arc_insert_lookup() -> Result<(), &'static str> {
    let arc = HvArc::new();
    arc.init(128);
    let key = HvArcKey::new(0, 4096, 1);
    let data: [u8; 16] = [0xAA; 16];
    arc.insert(key, &data, HvArcBufType::Data);
    let ptr = arc.lookup(&key).ok_or("should find inserted entry")?;
    let found = unsafe { core::slice::from_raw_parts(ptr, 16) };
    check!(found[0] == 0xAA, "data mismatch");
    Ok(())
}

pub fn register_hvfs_tests() {
    let r = runner();
    r.register("hvfs::bp", "null", test_bp_null);
    r.register("hvfs::bp", "dva_set_get", test_bp_dva_set_get);
    r.register("hvfs::bp", "birth_txg", test_bp_birth_txg);
    r.register("hvfs::checksum", "fletcher4_basic", test_checksum_fletcher4_basic);
    r.register("hvfs::checksum", "different_data", test_checksum_different_data);
    r.register("hvfs::spa", "config_name", test_spa_config_name);
    r.register("hvfs::spa", "uberblock_null", test_spa_uberblock_null);
    r.register("hvfs::spa", "uberblock_checksum", test_spa_uberblock_checksum);
    r.register("hvfs::spa", "uberblock_invalid", test_spa_uberblock_invalid_magic);
    r.register("hvfs::dmu", "default", test_dmu_object_default);
    r.register("hvfs::dmu", "cow", test_dmu_object_cow);
    r.register("hvfs::dmu", "dir_type", test_dmu_object_dir_type);
    r.register("hvfs::zap", "insert_lookup", test_zap_insert_lookup);
    r.register("hvfs::zap", "overwrite", test_zap_overwrite);
    r.register("hvfs::zap", "nonexistent", test_zap_nonexistent);
    r.register("hvfs::zap", "remove", test_zap_remove);
    r.register("hvfs::txg", "group_init", test_txg_group_init);
    r.register("hvfs::txg", "transition", test_txg_group_transition);
    r.register("hvfs::zil", "record_create", test_zil_record_create);
    r.register("hvfs::zil", "record_write", test_zil_record_write);
    r.register("hvfs::zil", "add_and_sync", test_zil_add_and_sync);
    r.register("hvfs::arc", "init", test_arc_init);
    r.register("hvfs::arc", "lookup_miss", test_arc_lookup_miss);
    r.register("hvfs::arc", "insert_lookup", test_arc_insert_lookup);
}
