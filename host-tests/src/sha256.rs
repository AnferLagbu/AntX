// B08-12 (DECISION-052 路线 C): SHA-256 消除平行实现.
//
// 本模块不再本地实现 SHA-256 (K 常量表/填充/轮函数已删除),
// 被测对象改为内核 `services::credo::sha256` 规范实现 (host-test feature 暴露).
// 本文件仅承载回归测试, 经 `queenx` path 依赖直接验证内核真实源码.
// 原 `#![allow(dead_code)]` (F9 违规) 随实现删除而消失.

#[cfg(test)]
mod tests {
    use queenx::kernel::framework::credo::sha256::sha256;

    #[test]
    fn sha256_empty() {
        let expected: [u8; 32] = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(sha256(b""), expected);
    }

    #[test]
    fn sha256_abc() {
        let expected: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(sha256(b"abc"), expected);
    }

    #[test]
    fn sha256_long_message() {
        let expected: [u8; 32] = [
            0x24, 0x8d, 0x6a, 0x61, 0xd2, 0x06, 0x38, 0xb8, 0xe5, 0xc0, 0x26, 0x93, 0x0c, 0x3e,
            0x60, 0x39, 0xa3, 0x3c, 0xe4, 0x59, 0x64, 0xff, 0x21, 0x67, 0xf6, 0xec, 0xed, 0xd4,
            0x19, 0xdb, 0x06, 0xc1,
        ];
        assert_eq!(
            sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            expected
        );
    }

    #[test]
    fn sha256_deterministic() {
        let h1 = sha256(b"hello world");
        let h2 = sha256(b"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn sha256_different_inputs() {
        let h1 = sha256(b"hello");
        let h2 = sha256(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn sha256_single_byte() {
        let h = sha256(b"a");
        assert_ne!(h, [0u8; 32], "single byte should produce non-zero hash");
    }

    #[test]
    fn sha256_exactly_55_bytes() {
        let data: Vec<u8> = (0..55).map(|i| i as u8).collect();
        let h = sha256(&data);
        assert_ne!(h, [0u8; 32]);
    }

    #[test]
    fn sha256_exactly_56_bytes() {
        let data: Vec<u8> = (0..56).map(|i| i as u8).collect();
        let h = sha256(&data);
        assert_ne!(h, [0u8; 32]);
    }

    #[test]
    fn sha256_exactly_64_bytes() {
        let data: Vec<u8> = (0..64).map(|i| i as u8).collect();
        let h = sha256(&data);
        assert_ne!(h, [0u8; 32]);
    }

    #[test]
    fn sha256_exactly_63_bytes() {
        let data: Vec<u8> = (0..63).map(|i| i as u8).collect();
        let h = sha256(&data);
        assert_ne!(h, [0u8; 32]);
    }

    #[test]
    fn sha256_multi_block() {
        let data: Vec<u8> = (0..200).map(|i| (i % 256) as u8).collect();
        let h1 = sha256(&data);
        let h2 = sha256(&data);
        assert_eq!(h1, h2, "multi-block hash should be deterministic");
    }

    #[test]
    fn sha256_all_zeros() {
        let data = [0u8; 64];
        let h = sha256(&data);
        assert_ne!(h, [0u8; 32]);
    }

    #[test]
    fn sha256_all_ones() {
        let data = [0xFFu8; 64];
        let h = sha256(&data);
        assert_ne!(h, [0u8; 32]);
    }

    #[test]
    fn sha256_avalanche_effect() {
        let h1 = sha256(b"hello");
        let h2 = sha256(b"hellp");
        let diff_bits: u32 = h1
            .iter()
            .zip(h2.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum();
        assert!(
            diff_bits > 32,
            "single char change should flip many bits, got {} bits",
            diff_bits
        );
    }

    #[test]
    fn sha256_large_input() {
        let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let h = sha256(&data);
        assert_ne!(h, [0u8; 32]);
    }
}
