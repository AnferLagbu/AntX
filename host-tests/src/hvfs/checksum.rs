use crate::kernel::fs::hvfs::bp::HvCksumType;

pub const HV_CKSUM_FLETCHER2: usize = 1;
pub const HV_CKSUM_FLETCHER4: usize = 2;
pub const HV_CKSUM_SHA256: usize = 3;

#[derive(Debug, Clone, Copy)]
pub struct HvChecksum {
    pub kind: HvCksumType,
    pub value: [u64; 4],
}

impl HvChecksum {
    pub fn new(kind: HvCksumType) -> Self {
        Self {
            kind,
            value: [0; 4],
        }
    }

    pub fn compute(kind: HvCksumType, data: &[u8]) -> Self {
        let mut ck = Self::new(kind);
        match kind {
            HvCksumType::Off => {}
            HvCksumType::Fletcher2 => ck.fletcher2(data),
            HvCksumType::Fletcher4 => ck.fletcher4(data),
            HvCksumType::SHA256 => ck.sha256(data),
            HvCksumType::EdonR => ck.fletcher4(data),
        }
        ck
    }

    pub fn verify(&self, data: &[u8]) -> bool {
        let computed = Self::compute(self.kind, data);
        self.value == computed.value
    }

    fn fletcher2(&mut self, data: &[u8]) {
        let words = data.chunks_exact(8);
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        for chunk in words {
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
        self.value[2] = 0;
        self.value[3] = 0;
    }

    fn fletcher4(&mut self, data: &[u8]) {
        let words = data.chunks_exact(8);
        let mut a: u64 = 0;
        let mut b: u64 = 0;
        let mut c: u64 = 0;
        let mut d: u64 = 0;
        for chunk in words {
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
        let mut h0: u64 = 0x6a09e667f3bcc908;
        let mut h1: u64 = 0xbb67ae8584caa73b;
        let mut h2: u64 = 0x3c6ef372fe94f82b;
        let mut h3: u64 = 0xa54ff53a5f1d36f1;

        let mut i = 0;
        let len = data.len();
        while i < len {
            let end = (i + 64).min(len);
            let chunk = &data[i..end];
            let mut block = [0u8; 128];
            block[..chunk.len()].copy_from_slice(chunk);
            if chunk.len() < 64 {
                block[chunk.len()] = 0x80;
                let bit_len = (len as u64) * 8;
                block[112..120].copy_from_slice(&bit_len.to_le_bytes());
            }
            let w0 = u64::from_le_bytes(block[0..8].try_into().unwrap_or([0; 8]));
            let w1 = u64::from_le_bytes(block[8..16].try_into().unwrap_or([0; 8]));
            let w2 = u64::from_le_bytes(block[16..24].try_into().unwrap_or([0; 8]));
            let w3 = u64::from_le_bytes(block[24..32].try_into().unwrap_or([0; 8]));

            let k0: u64 = 0x428a2f9871374491;
            let k1: u64 = 0xb5c0fbcfec4d3b2f;
            let k2: u64 = 0xe9b5dba58189dbbc;
            let k3: u64 = 0x3956c25bf348b538;

            let t0 = h0.wrapping_add(w0).wrapping_add(k0);
            let t1 = h1.wrapping_add(w1).wrapping_add(k1);
            let t2 = h2.wrapping_add(w2).wrapping_add(k2);
            let t3 = h3.wrapping_add(w3).wrapping_add(k3);

            h0 = h0.wrapping_add(t0 ^ t2);
            h1 = h1.wrapping_add(t1 ^ t3);
            h2 = h2.wrapping_add(t0.rotate_left(17) ^ t1.rotate_left(23));
            h3 = h3.wrapping_add(t2.rotate_left(31) ^ t3.rotate_left(7));

            i += 64;
            if i >= len {
                break;
            }
        }

        self.value[0] = h0;
        self.value[1] = h1;
        self.value[2] = h2;
        self.value[3] = h3;
    }
}
