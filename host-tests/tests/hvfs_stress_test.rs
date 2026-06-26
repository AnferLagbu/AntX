//! hvfs: 压力集成测试
//!
//! 验证 HvFS 高频路径 (CAS 去重表 / ZAP 哈希表 / ZIL 事务日志) 在
//! 100~256 次循环下的正确性与性能特征. 集成测试视角, 不依赖真实硬件.
//!
//! 追踪: I-05
//! SPDX-License-Identifier: Apache-2.0
//!
//! ## 与单元测试的分工
//! - 单元测试 (`src/hvfs/*`) 验证各模块基本行为
//! - 本文件验证整体压力下的稳定性
//!
//! ## 与性能基准的分工
//! - `framekernel_bench` 测量 ns_per_op 微基准
//! - 本文件验证 `assert!(rc >= 1)` 等业务正确性约束

use queenx_host_tests::hvfs::{bp, dedup, zap, zil};

#[test]
fn stress_cas_insert_lookup_100() {
    let cas = dedup::get_cas();
    for i in 0..100u64 {
        let data = i.to_le_bytes();
        let hash = dedup::sha256(&data);
        let mut bp = bp::HvBlockPointer::null();
        bp.set_birth(i);
        cas.insert(hash, bp);
        let found = cas.lookup(&hash);
        assert!(found.is_some(), "CAS insert+lookup failed at {}", i);
        let rc = cas.ref_count(&hash);
        assert!(rc >= 1, "ref_count >= 1 at {}", i);
    }
    let (_h, _m, s) = cas.get_stats();
    assert!(s >= 100, "synced >= 100: got {}", s);
}

#[test]
fn stress_cas_dedup_50_ref_inc_dec() {
    let cas = dedup::get_cas();
    let data = b"hello-world-identical-block";
    let hash = dedup::sha256(data);
    let bp = bp::HvBlockPointer::null();
    cas.insert(hash, bp);
    for _ in 0..50 {
        cas.ref_inc(&hash);
    }
    assert_eq!(cas.ref_count(&hash), 51);
    for _ in 0..50 {
        cas.ref_dec(&hash);
    }
    assert_eq!(cas.ref_count(&hash), 1);
    cas.ref_dec(&hash);
    assert_eq!(cas.ref_count(&hash), 0);
    assert!(!cas.is_known(&hash));
}

#[test]
fn stress_zap_hash_collision_256() {
    let z = zap::HvZap::with_capacity(256);
    for i in 0..256 {
        let name = format!("entry_{:04}", i);
        let value = (i * 9973) ^ 0xDEADBEEF;
        assert!(z.insert_u64(&name, value), "ZAP insert #{} failed", i);
    }
    assert_eq!(z.len(), 256);
    for i in 0..256 {
        let name = format!("entry_{:04}", i);
        let expected = (i * 9973) ^ 0xDEADBEEF;
        assert_eq!(
            z.lookup_u64(&name),
            Some(expected),
            "ZAP lookup mismatch at {}",
            i
        );
    }
    z.remove("entry_0000");
    assert_eq!(z.len(), 255);
}

#[test]
fn stress_zap_clear_reuse_10_rounds() {
    let z = zap::HvZap::new();
    for round in 0..10 {
        for i in 0..50 {
            z.insert_u64(&format!("k{}", i), i as u64);
        }
        assert_eq!(z.len(), 50, "round {}: len != 50", round);
        z.clear();
        assert_eq!(z.len(), 0, "round {}: not empty after clear", round);
    }
}

#[test]
fn stress_sha256_deterministic_100() {
    let mut prev = [0u8; 32];
    for i in 0..100u64 {
        let data = i.to_le_bytes();
        let h1 = dedup::sha256(&data);
        let h2 = dedup::sha256(&data);
        assert_eq!(h1, h2, "sha256 non-deterministic at {}", i);
        if i > 0 {
            assert_ne!(h1, prev, "sha256 collision at {}", i);
        }
        prev = h1;
    }
}

#[test]
fn stress_zil_roundtrip_100_records() {
    let zil = zil::HvZil::new();
    let names: [&str; 5] = ["alpha", "beta", "gamma", "delta", "epsilon"];
    for i in 0..100u64 {
        let mut rec = zil::HvZilRecord::new_create(i, 0, names[(i % 5) as usize]);
        rec.seq = i + 1;
        zil.records.lock().push(rec);
    }
    assert_eq!(zil.records.lock().len(), 100);
    zil.sync(101);
    let seq = zil
        .committed_seq
        .load(core::sync::atomic::Ordering::Relaxed);
    assert!(seq > 0, "committed_seq should advance");
}
