//! 引导映像编码 / 校验 (Miri 验证版)
//!
//! 与内核 `kernel/config/boot_image.rs` 等价, 验证:
//! - pack/unpack 字节对齐正确
//! - 不变式: 编码后再解码 == 原始值
//! - 边界: 大端/小端处理不会越界

pub const HEADER_MAGIC: u32 = 0x51584558; // "QXRX" in ASCII
pub const ENCODED_LEN: usize = 256;

/// 大端 pack u32
pub fn pack_u32_be(buf: &mut [u8; ENCODED_LEN], offset: usize, val: u32) {
    assert!(offset + 4 <= ENCODED_LEN, "pack offset out of bounds");
    buf[offset] = ((val >> 24) & 0xff) as u8;
    buf[offset + 1] = ((val >> 16) & 0xff) as u8;
    buf[offset + 2] = ((val >> 8) & 0xff) as u8;
    buf[offset + 3] = (val & 0xff) as u8;
}

/// 大端 unpack u32
pub fn unpack_u32_be(buf: &[u8; ENCODED_LEN], offset: usize) -> u32 {
    assert!(offset + 4 <= ENCODED_LEN, "unpack offset out of bounds");
    ((buf[offset] as u32) << 24)
        | ((buf[offset + 1] as u32) << 16)
        | ((buf[offset + 2] as u32) << 8)
        | (buf[offset + 3] as u32)
}

/// CRC-32 (IEEE 802.3) 纯软件实现, 用于引导映像完整性校验
const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in data {
        let idx = ((crc ^ b as u32) & 0xff) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[idx];
    }
    crc ^ 0xffff_ffff
}

/// 引导映像封装
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootImageHeader {
    pub magic: u32,
    pub version: u32,
    pub flags: u32,
    pub total_size: u32,
    pub capabilities: u64,
    pub crc: u32,
}

impl BootImageHeader {
    pub fn encode(&self) -> [u8; ENCODED_LEN] {
        let mut buf = [0u8; ENCODED_LEN];
        pack_u32_be(&mut buf, 0, self.magic);
        pack_u32_be(&mut buf, 4, self.version);
        pack_u32_be(&mut buf, 8, self.flags);
        pack_u32_be(&mut buf, 12, self.total_size);
        // capabilities: 8 字节, 大端
        for i in 0..8 {
            buf[16 + i] = ((self.capabilities >> (56 - 8 * i)) & 0xff) as u8;
        }
        // CRC 在最后 4 字节
        let crc = crc32(&buf[..252]);
        pack_u32_be(&mut buf, 252, crc);
        buf
    }

    pub fn decode(buf: &[u8; ENCODED_LEN]) -> Option<Self> {
        let magic = unpack_u32_be(buf, 0);
        if magic != HEADER_MAGIC {
            return None;
        }
        let version = unpack_u32_be(buf, 4);
        let flags = unpack_u32_be(buf, 8);
        let total_size = unpack_u32_be(buf, 12);
        let mut capabilities: u64 = 0;
        for i in 0..8 {
            capabilities |= (buf[16 + i] as u64) << (56 - 8 * i);
        }
        let crc = unpack_u32_be(buf, 252);
        let computed = crc32(&buf[..252]);
        if crc != computed {
            return None;
        }
        Some(Self {
            magic,
            version,
            flags,
            total_size,
            capabilities,
            crc,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_roundtrip() {
        let mut buf = [0u8; ENCODED_LEN];
        let val = 0xdead_beef_u32;
        pack_u32_be(&mut buf, 100, val);
        let got = unpack_u32_be(&buf, 100);
        assert_eq!(val, got);
    }

    #[test]
    fn pack_alignment() {
        // 任意 4 字节对齐 offset 都应工作
        for offset in (0..ENCODED_LEN - 4).step_by(4) {
            let mut buf = [0u8; ENCODED_LEN];
            let val = (offset as u32) * 7 + 1;
            pack_u32_be(&mut buf, offset, val);
            assert_eq!(unpack_u32_be(&buf, offset), val);
        }
    }

    #[test]
    fn header_encode_decode_roundtrip() {
        let h = BootImageHeader {
            magic: HEADER_MAGIC,
            version: 0x0001_0000,
            flags: 0xabcd_1234,
            total_size: 8192,
            capabilities: 0x1234_5678_9abc_def0,
            crc: 0,
        };
        let buf = h.encode();
        let decoded = BootImageHeader::decode(&buf).expect("decode failed");
        assert_eq!(h.magic, decoded.magic);
        assert_eq!(h.version, decoded.version);
        assert_eq!(h.flags, decoded.flags);
        assert_eq!(h.total_size, decoded.total_size);
        assert_eq!(h.capabilities, decoded.capabilities);
    }

    #[test]
    fn header_decode_invalid_magic() {
        let mut buf = [0u8; ENCODED_LEN];
        pack_u32_be(&mut buf, 0, 0xdead_beef); // 错误 magic
        let crc = crc32(&buf[..252]);
        pack_u32_be(&mut buf, 252, crc);
        assert!(BootImageHeader::decode(&buf).is_none());
    }

    #[test]
    fn header_decode_invalid_crc() {
        let h = BootImageHeader {
            magic: HEADER_MAGIC,
            version: 1,
            flags: 0,
            total_size: 0,
            capabilities: 0,
            crc: 0,
        };
        let buf = h.encode();
        // 故意翻转一个 bit
        let mut corrupted = buf;
        corrupted[100] ^= 0x01;
        assert!(BootImageHeader::decode(&corrupted).is_none());
    }

    #[test]
    fn crc32_known_value() {
        // "123456789" 的 CRC-32 IEEE 应为 0xCBF43926
        let data = b"123456789";
        assert_eq!(crc32(data), 0xcbf4_3926);
    }

    #[test]
    fn crc32_empty() {
        assert_eq!(crc32(&[]), 0);
    }
}
