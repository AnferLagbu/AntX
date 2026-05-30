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
        Self { kind, value: [0; 4] }
    }

    pub fn compute(kind: HvCksumType, data: &[u8]) -> Self {
        let mut ck = Self::new(kind);
        match kind {
            HvCksumType::Off => {},
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
            let w = u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]);
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
            let w = u64::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7]]);
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
        let hash = crate::kernel::credo::sha256::sha256(data);
        self.value[0] = u64::from_be_bytes(hash[0..8].try_into().unwrap());
        self.value[1] = u64::from_be_bytes(hash[8..16].try_into().unwrap());
        self.value[2] = u64::from_be_bytes(hash[16..24].try_into().unwrap());
        self.value[3] = u64::from_be_bytes(hash[24..32].try_into().unwrap());
    }
}
