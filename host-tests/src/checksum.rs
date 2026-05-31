#![allow(dead_code)] // 测试基础设施模块 (Checksum 由 #[cfg(test)] 测试使用)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CksumKind {
    Off,
    Fletcher2,
    Fletcher4,
    SHA256,
    EdonR,
}

#[derive(Debug, Clone, Copy)]
pub struct Checksum {
    pub kind: CksumKind,
    pub value: [u64; 4],
}

impl Checksum {
    pub fn new(kind: CksumKind) -> Self {
        Self {
            kind,
            value: [0; 4],
        }
    }

    pub fn compute(kind: CksumKind, data: &[u8]) -> Self {
        let mut ck = Self::new(kind);
        match kind {
            CksumKind::Off => {}
            CksumKind::Fletcher2 => ck.fletcher2(data),
            CksumKind::Fletcher4 => ck.fletcher4(data),
            CksumKind::SHA256 => ck.sha256(data),
            CksumKind::EdonR => ck.fletcher4(data),
        }
        ck
    }

    pub fn verify(&self, data: &[u8]) -> bool {
        let computed = Self::compute(self.kind, data);
        self.value == computed.value
    }

    fn fletcher2(&mut self, data: &[u8]) {
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        for chunk in data.chunks_exact(8) {
            let w = u64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
        }
        let rem = data.len() % 8;
        if rem > 0 {
            let start = data.len() - rem;
            let mut last = [0u8; 8];
            last[..rem].copy_from_slice(&data[start..]);
            let w = u64::from_le_bytes(last);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
        }
        self.value[0] = a;
        self.value[1] = b;
    }

    fn fletcher4(&mut self, data: &[u8]) {
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        let mut c: u64 = 0;
        let mut d: u64 = 0;
        for chunk in data.chunks_exact(8) {
            let w = u64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
            c = c.wrapping_add(b);
            d = d.wrapping_add(c);
        }
        let rem = data.len() % 8;
        if rem > 0 {
            let start = data.len() - rem;
            let mut last = [0u8; 8];
            last[..rem].copy_from_slice(&data[start..]);
            let w = u64::from_le_bytes(last);
            a = a.wrapping_add(w);
            b = b.wrapping_add(a);
            c = c.wrapping_add(b);
            d = d.wrapping_add(c);
        }
        self.value[0] = a;
        self.value[1] = b;
        self.value[2] = c;
        self.value[3] = d;
    }

    fn sha256(&mut self, data: &[u8]) {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
            0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
            0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
            0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
            0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
            0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
            0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
            0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
            0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
            0xc67178f2,
        ];

        #[inline(always)]
        fn rotr(x: u32, n: u32) -> u32 {
            x.rotate_right(n)
        }

        fn sha256_transform(state: &mut [u32; 8], block: &[u8; 64]) {
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = ((block[i * 4] as u32) << 24)
                    | ((block[i * 4 + 1] as u32) << 16)
                    | ((block[i * 4 + 2] as u32) << 8)
                    | (block[i * 4 + 3] as u32);
            }
            for i in 16..64 {
                let s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
                let s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16]
                    .wrapping_add(s0)
                    .wrapping_add(w[i - 7])
                    .wrapping_add(s1);
            }
            let mut a = state[0];
            let mut b = state[1];
            let mut c = state[2];
            let mut d = state[3];
            let mut e = state[4];
            let mut f = state[5];
            let mut g = state[6];
            let mut h = state[7];
            for i in 0..64 {
                let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
                let ch = (e & f) ^ ((!e) & g);
                let t1 = h
                    .wrapping_add(s1)
                    .wrapping_add(ch)
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
                let maj = (a & b) ^ (a & c) ^ (b & c);
                let t2 = s0.wrapping_add(maj);
                h = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }
            state[0] = state[0].wrapping_add(a);
            state[1] = state[1].wrapping_add(b);
            state[2] = state[2].wrapping_add(c);
            state[3] = state[3].wrapping_add(d);
            state[4] = state[4].wrapping_add(e);
            state[5] = state[5].wrapping_add(f);
            state[6] = state[6].wrapping_add(g);
            state[7] = state[7].wrapping_add(h);
        }

        let mut state: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];
        let len = data.len();
        let mut i = 0;
        while i + 64 <= len {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[i..i + 64]);
            sha256_transform(&mut state, &block);
            i += 64;
        }
        let remaining = len - i;
        let mut block = [0u8; 64];
        if remaining > 0 {
            block[..remaining].copy_from_slice(&data[i..i + remaining]);
        }
        block[remaining] = 0x80;
        if remaining >= 56 {
            sha256_transform(&mut state, &block);
            block = [0u8; 64];
        }
        let bit_len = (len as u64) * 8;
        block[56..64].copy_from_slice(&bit_len.to_be_bytes());
        sha256_transform(&mut state, &block);

        self.value[0] = ((state[0] as u64) << 32) | (state[1] as u64);
        self.value[1] = ((state[2] as u64) << 32) | (state[3] as u64);
        self.value[2] = ((state[4] as u64) << 32) | (state[5] as u64);
        self.value[3] = ((state[6] as u64) << 32) | (state[7] as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
