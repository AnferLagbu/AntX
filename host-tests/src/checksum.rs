// B08-12 (DECISION-052 路线 C): 校验和消除平行实现.
//
// `Checksum`/`CksumKind` 不再本地实现 (本地 SHA-256 复刻已删除),
// 直接别名内核 `services::fs::hvfs::checksum::HvChecksum` 与
// `services::fs::hvfs::bp::HvCksumType` 规范实现 (host-test feature 暴露).
// 两者 API 逐项同构 (new/compute/verify + kind/value 字段), 测试无需改动.
// 原 `#![allow(dead_code)]` (F9 违规) 随实现删除而消失.

// 别名仅被本文件 tests 使用, 置于 cfg(test) 内避免非 test 构建的 unused import 警告.
#[cfg(test)]
mod tests {
    use queenx::kernel::services::fs::hvfs::checksum::HvChecksum as Checksum;
    use queenx::kernel::services::fs::hvfs::bp::HvCksumType as CksumKind;

    #[test]
    fn fletcher2_empty() {
        let ck = Checksum::compute(CksumKind::Fletcher2, b"");
        assert_eq!(ck.value[0], 0);
        assert_eq!(ck.value[1], 0);
    }

    #[test]
    fn fletcher2_deterministic() {
        let data = b"hello world";
        let ck1 = Checksum::compute(CksumKind::Fletcher2, data);
        let ck2 = Checksum::compute(CksumKind::Fletcher2, data);
        assert_eq!(ck1.value, ck2.value);
    }

    #[test]
    fn fletcher4_empty() {
        let ck = Checksum::compute(CksumKind::Fletcher4, b"");
        assert_eq!(ck.value[0], 0);
        assert_eq!(ck.value[1], 0);
        assert_eq!(ck.value[2], 0);
        assert_eq!(ck.value[3], 0);
    }

    #[test]
    fn fletcher4_deterministic() {
        let data = b"test data for fletcher4";
        let ck1 = Checksum::compute(CksumKind::Fletcher4, data);
        let ck2 = Checksum::compute(CksumKind::Fletcher4, data);
        assert_eq!(ck1.value, ck2.value);
    }

    #[test]
    fn fletcher4_different_data() {
        let ck1 = Checksum::compute(CksumKind::Fletcher4, b"hello");
        let ck2 = Checksum::compute(CksumKind::Fletcher4, b"world");
        assert_ne!(ck1.value, ck2.value);
    }

    #[test]
    fn verify_roundtrip_fletcher2() {
        let data = b"some test data for verification";
        let ck = Checksum::compute(CksumKind::Fletcher2, data);
        assert!(ck.verify(data));
    }

    #[test]
    fn verify_roundtrip_fletcher4() {
        let data = b"some test data for verification";
        let ck = Checksum::compute(CksumKind::Fletcher4, data);
        assert!(ck.verify(data));
    }

    #[test]
    fn verify_detects_corruption() {
        let data = b"original data";
        let ck = Checksum::compute(CksumKind::Fletcher4, data);
        let corrupted = b"corrupted data";
        assert!(!ck.verify(corrupted));
    }

    #[test]
    fn off_checksum_always_zero() {
        let ck = Checksum::compute(CksumKind::Off, b"any data");
        assert_eq!(ck.value, [0u64; 4]);
    }

    #[test]
    fn edonr_uses_fletcher4() {
        let data = b"test data";
        let ck_edonr = Checksum::compute(CksumKind::EdonR, data);
        let ck_f4 = Checksum::compute(CksumKind::Fletcher4, data);
        assert_eq!(ck_edonr.value, ck_f4.value);
    }

    #[test]
    fn fletcher2_single_byte() {
        let ck = Checksum::compute(CksumKind::Fletcher2, b"A");
        assert_ne!(
            ck.value[0], 0,
            "single byte should produce non-zero checksum"
        );
    }

    #[test]
    fn fletcher4_single_byte() {
        let ck = Checksum::compute(CksumKind::Fletcher4, b"A");
        assert_ne!(
            ck.value[0], 0,
            "single byte should produce non-zero checksum"
        );
    }

    #[test]
    fn fletcher2_odd_length() {
        let data = b"hello";
        assert_eq!(data.len() % 8, 5);
        let ck = Checksum::compute(CksumKind::Fletcher2, data);
        assert!(ck.verify(data), "odd-length data should verify");
    }

    #[test]
    fn fletcher4_odd_length() {
        let data = b"odd data length test";
        let ck = Checksum::compute(CksumKind::Fletcher4, data);
        assert!(ck.verify(data), "odd-length data should verify");
    }

    #[test]
    fn fletcher2_exact_8_bytes() {
        let data = b"12345678";
        let ck = Checksum::compute(CksumKind::Fletcher2, data);
        assert!(ck.verify(data));
    }

    #[test]
    fn fletcher4_exact_8_bytes() {
        let data = b"abcdefgh";
        let ck = Checksum::compute(CksumKind::Fletcher4, data);
        assert!(ck.verify(data));
    }

    #[test]
    fn fletcher2_large_data() {
        let data: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        let ck = Checksum::compute(CksumKind::Fletcher2, &data);
        assert!(ck.verify(&data), "large data should verify");
    }

    #[test]
    fn fletcher4_large_data() {
        let data: Vec<u8> = (0..8192).map(|i| (i % 256) as u8).collect();
        let ck = Checksum::compute(CksumKind::Fletcher4, &data);
        assert!(ck.verify(&data), "large data should verify");
    }

    #[test]
    fn fletcher2_different_lengths_same_prefix() {
        let ck1 = Checksum::compute(CksumKind::Fletcher2, b"hello");
        let ck2 = Checksum::compute(CksumKind::Fletcher2, b"hello world");
        assert_ne!(
            ck1.value, ck2.value,
            "different lengths should produce different checksums"
        );
    }

    #[test]
    fn fletcher4_different_lengths_same_prefix() {
        let ck1 = Checksum::compute(CksumKind::Fletcher4, b"hello");
        let ck2 = Checksum::compute(CksumKind::Fletcher4, b"hello world");
        assert_ne!(
            ck1.value, ck2.value,
            "different lengths should produce different checksums"
        );
    }

    #[test]
    fn verify_detects_single_bit_flip() {
        let data = b"some test data for verification";
        let ck = Checksum::compute(CksumKind::Fletcher4, data);
        let mut corrupted = data.to_vec();
        corrupted[5] ^= 0x01;
        assert!(!ck.verify(&corrupted), "single bit flip should be detected");
    }

    #[test]
    fn checksum_kind_off_verify_always_true() {
        let ck = Checksum::compute(CksumKind::Off, b"any data");
        assert!(
            ck.verify(b"different data"),
            "Off checksum should verify any data"
        );
    }

    #[test]
    fn sha256_checksum_short_data() {
        let data = b"ab";
        let ck1 = Checksum::compute(CksumKind::SHA256, data);
        let ck2 = Checksum::compute(CksumKind::SHA256, data);
        assert_eq!(
            ck1.value, ck2.value,
            "SHA256 checksum should be deterministic for short data"
        );
    }

    #[test]
    fn sha256_checksum_known_vector() {
        let data = b"abc";
        let ck = Checksum::compute(CksumKind::SHA256, data);
        let expected: [u64; 4] = [
            0xba7816bf8f01cfea,
            0x414140de5dae2223,
            0xb00361a396177a9c,
            0xb410ff61f20015ad,
        ];
        assert_eq!(
            ck.value, expected,
            "SHA256('abc') should match FIPS 180-4 test vector"
        );
    }

    #[test]
    fn sha256_checksum_empty() {
        let data = b"";
        let ck = Checksum::compute(CksumKind::SHA256, data);
        let expected: [u64; 4] = [
            0xe3b0c44298fc1c14,
            0x9afbf4c8996fb924,
            0x27ae41e4649b934c,
            0xa495991b7852b855,
        ];
        assert_eq!(
            ck.value, expected,
            "SHA256('') should match FIPS 180-4 test vector"
        );
    }

    #[test]
    fn sha256_checksum_long_data() {
        let data =
            b"sha256 via checksum module - this is a longer string that spans multiple blocks";
        let ck1 = Checksum::compute(CksumKind::SHA256, data);
        let ck2 = Checksum::compute(CksumKind::SHA256, data);
        assert_eq!(
            ck1.value, ck2.value,
            "SHA256 should be deterministic for long data"
        );
    }
}
