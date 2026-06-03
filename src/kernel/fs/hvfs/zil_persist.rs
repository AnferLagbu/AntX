//! HvFS ZIL Persistence Layer (WAL 磁盘持久化)
//!
//! 增强现有内存 ZIL:
//! 1. 每个记录附带 CRC32 校验和
//! 2. 设计 ZIL block 磁盘布局 (header + records)
//! 3. 崩溃恢复时扫描 ZIL blocks 回放未提交事务
//!
//! ## 磁盘布局
//!
//! ```text
//! ┌──────────────────────┐
//! │  ZilBlockHeader (64B)│
//! │  - magic: 0xZIL1     │
//! │  - txg: u64          │
//! │  - count: u16        │
//! │  - checksum: u32     │
//! │  - next_block: u64   │
//! ├──────────────────────┤
//! │  Record 0            │
//! │  Record 1            │
//! │  ...                 │
//! │  Record N-1          │
//! ├──────────────────────┤
//! │  ZilBlockTrailer (8B)│
//! │  - tail_magic: 0xEND │
//! │  - tail_checksum: u32│
//! └──────────────────────┘
//! ```

use super::zil::{HvZil, HvZilRecord};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

const ZIL_MAGIC: u32 = 0x5A494C31u32; // "ZIL1"
const ZIL_TAIL_MAGIC: u32 = 0x454E4400u32; // "END\0"
const ZIL_BLOCK_SIZE: usize = 4096;
const ZIL_HEADER_SIZE: usize = 64;
const ZIL_TRAILER_SIZE: usize = 8;
const ZIL_RECORD_DISK_SIZE: usize = 256;
const ZIL_MAX_RECORDS_PER_BLOCK: usize =
    (ZIL_BLOCK_SIZE - ZIL_HEADER_SIZE - ZIL_TRAILER_SIZE) / ZIL_RECORD_DISK_SIZE;

/// Actual bytes written to disk per record:
///    rec_type(1) + txg(8) + obj_id(8) + parent_obj(8) + offset(8) + size(4)
///  + seq(8) + name(128) + data_hash(32) + record_crc(4) = 209
const ZIL_RECORD_PAYLOAD: usize = 209;
const _ASSERT_RECORD_FITS: () = assert!(ZIL_RECORD_PAYLOAD <= ZIL_RECORD_DISK_SIZE);

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ZilBlockHeader {
    pub magic: u32,
    pub version: u16,
    pub flags: u16,
    pub txg: u64,
    pub seq_start: u64,
    pub record_count: u16,
    pub record_capacity: u16,
    pub total_size: u32,
    pub header_checksum: u32,
    pub data_checksum: u32,
    pub next_block: u64,
    pub padding: [u8; 16],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ZilBlockTrailer {
    pub tail_magic: u32,
    pub block_checksum: u32,
}

const _ASSERT_HEADER_SIZE: () = {
    if core::mem::size_of::<ZilBlockHeader>() != ZIL_HEADER_SIZE {
        panic!("ZilBlockHeader size mismatch");
    }
};

const _ASSERT_TRAILER_SIZE: () = {
    if core::mem::size_of::<ZilBlockTrailer>() != ZIL_TRAILER_SIZE {
        panic!("ZilBlockTrailer size mismatch");
    }
};

impl ZilBlockHeader {
    pub fn new(txg: u64, seq_start: u64) -> Self {
        Self {
            magic: ZIL_MAGIC,
            version: 1,
            flags: 0,
            txg,
            seq_start,
            record_count: 0,
            record_capacity: ZIL_MAX_RECORDS_PER_BLOCK as u16,
            total_size: ZIL_BLOCK_SIZE as u32,
            header_checksum: 0,
            data_checksum: 0,
            next_block: 0,
            padding: [0; 16],
        }
    }

    pub fn is_valid(&self) -> bool {
        self.magic == ZIL_MAGIC && self.version >= 1
    }

    pub fn compute_header_checksum(&mut self) {
        self.header_checksum = 0;
        self.header_checksum = crc32_checksum(self.as_bytes());
    }

    pub fn verify_header(&self) -> bool {
        let mut copy = *self;
        copy.compute_header_checksum();
        copy.header_checksum == self.header_checksum
    }

    /// Framekernel P2.2.2: 安全地将 ZilBlockHeader 转换为字节切片
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: ZilBlockHeader 是 repr(C)，大小经编译期断言验证
        unsafe {
            core::slice::from_raw_parts(self as *const Self as *const u8, ZIL_HEADER_SIZE)
        }
    }

    /// Framekernel P2.2.2: 从 block 字节切片安全地获取 header 引用
    /// SAFETY: 调用者需保证 block 包含有效的 ZilBlockHeader 数据
    pub fn from_block(block: &[u8]) -> Option<&Self> {
        if block.len() < ZIL_HEADER_SIZE {
            return None;
        }
        // SAFETY: 已检查长度，ZilBlockHeader 是 repr(C)，对齐要求满足
        Some(unsafe { &*(block.as_ptr() as *const ZilBlockHeader) })
    }
}

impl ZilBlockTrailer {
    pub fn new() -> Self {
        Self {
            tail_magic: ZIL_TAIL_MAGIC,
            block_checksum: 0,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.tail_magic == ZIL_TAIL_MAGIC
    }

    /// Framekernel P2.2.2: 安全地将 ZilBlockTrailer 转换为字节切片
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: ZilBlockTrailer 是 repr(C)，大小经编译期断言验证
        unsafe {
            core::slice::from_raw_parts(self as *const Self as *const u8, ZIL_TRAILER_SIZE)
        }
    }
}

fn crc32_checksum(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn serialize_record(record: &HvZilRecord, buf: &mut [u8]) {
    debug_assert!(
        buf.len() >= ZIL_RECORD_PAYLOAD,
        "serialize_record buffer too small: {} < {}",
        buf.len(),
        ZIL_RECORD_PAYLOAD
    );
    buf[0] = record.rec_type as u8;
    buf[1..9].copy_from_slice(&record.txg.to_le_bytes());
    buf[9..17].copy_from_slice(&record.obj_id.to_le_bytes());
    buf[17..25].copy_from_slice(&record.parent_obj.to_le_bytes());
    buf[25..33].copy_from_slice(&record.offset.to_le_bytes());
    buf[33..37].copy_from_slice(&record.size.to_le_bytes());
    buf[37..45].copy_from_slice(&record.seq.to_le_bytes());
    buf[45..173].copy_from_slice(&record.name);
    // Convert [u64; 4] to bytes safely
    for (i, val) in record.data_hash.iter().enumerate() {
        let off = 173 + i * 8;
        buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
    }
    let payload_end = 173 + 32;
    let rec_crc = crc32_checksum(&buf[..payload_end]);
    buf[payload_end..payload_end + 4].copy_from_slice(&rec_crc.to_le_bytes());
}

fn deserialize_record(buf: &[u8]) -> Option<HvZilRecord> {
    if buf.len() < ZIL_RECORD_DISK_SIZE {
        return None;
    }

    let actual_size = 173 + 32 + 4;
    if buf.len() < actual_size {
        return None;
    }
    // SAFETY: we've checked buf.len() >= actual_size, and actual_size >= 4
    let rec_crc = u32::from_le_bytes(buf[actual_size - 4..actual_size].try_into().unwrap());
    let computed = crc32_checksum(&buf[..actual_size - 4]);
    if rec_crc != computed {
        return None;
    }

    let rec_type = buf[0];
    let txg = u64::from_le_bytes(buf[1..9].try_into().unwrap());
    let obj_id = u64::from_le_bytes(buf[9..17].try_into().unwrap());
    let parent_obj = u64::from_le_bytes(buf[17..25].try_into().unwrap());
    let offset = u64::from_le_bytes(buf[25..33].try_into().unwrap());
    let size = u32::from_le_bytes(buf[33..37].try_into().unwrap());
    let seq = u64::from_le_bytes(buf[37..45].try_into().unwrap());

    let mut name = [0u8; 128];
    name.copy_from_slice(&buf[45..173]);

    let mut data_hash = [0u64; 4];
    for (i, val) in data_hash.iter_mut().enumerate() {
        let off = 173 + i * 8;
        *val = u64::from_le_bytes(buf[off..off + 8].try_into().unwrap());
    }

    let rec_type_enum = match rec_type {
        1 => super::zil::HvZilRecordType::Create,
        2 => super::zil::HvZilRecordType::Remove,
        3 => super::zil::HvZilRecordType::Link,
        4 => super::zil::HvZilRecordType::Rename,
        5 => super::zil::HvZilRecordType::Write,
        6 => super::zil::HvZilRecordType::Truncate,
        7 => super::zil::HvZilRecordType::SetAttr,
        8 => super::zil::HvZilRecordType::Acl,
        9 => super::zil::HvZilRecordType::CreateAcl,
        10 => super::zil::HvZilRecordType::Mkdir,
        11 => super::zil::HvZilRecordType::Symlink,
        12 => super::zil::HvZilRecordType::DedupRef,
        13 => super::zil::HvZilRecordType::DedupUnref,
        _ => return None,
    };

    Some(HvZilRecord {
        rec_type: rec_type_enum,
        txg,
        obj_id,
        parent_obj,
        offset,
        size,
        name,
        data_hash,
        seq,
    })
}

pub struct HvZilPersist {
    pub zil_blocks_written: AtomicBool,
}

// SAFETY (Framekernel P2.2.2): HvZilPersist 全部字段 (AtomicBool) 自动 Send + Sync。

impl HvZilPersist {
    pub const fn new() -> Self {
        Self {
            zil_blocks_written: AtomicBool::new(false),
        }
    }

    pub fn serialize_zil_to_block(zil: &HvZil, txg: u64) -> Option<Vec<u8>> {
        let records = zil.records.lock();
        if records.is_empty() {
            return None;
        }

        let count = records.len().min(ZIL_MAX_RECORDS_PER_BLOCK);
        let mut block = vec![0u8; ZIL_BLOCK_SIZE];

        let mut header = ZilBlockHeader::new(txg, records[0].seq);
        header.record_count = count as u16;
        header.compute_header_checksum();

        let header_bytes = header.as_bytes();
        block[..ZIL_HEADER_SIZE].copy_from_slice(header_bytes);

        let record_area = ZIL_HEADER_SIZE;
        for i in 0..count {
            let offset = record_area + i * ZIL_RECORD_DISK_SIZE;
            serialize_record(
                &records[i],
                &mut block[offset..offset + ZIL_RECORD_DISK_SIZE],
            );
        }

        let data_end = record_area + count * ZIL_RECORD_DISK_SIZE;
        header.data_checksum = crc32_checksum(&block[ZIL_HEADER_SIZE..data_end]);
        let header_bytes = header.as_bytes();
        block[..ZIL_HEADER_SIZE].copy_from_slice(header_bytes);

        let trailer_offset = ZIL_BLOCK_SIZE - ZIL_TRAILER_SIZE;
        let trailer = ZilBlockTrailer::new();
        let trailer_bytes = trailer.as_bytes();
        block[trailer_offset..].copy_from_slice(trailer_bytes);

        let full_checksum = crc32_checksum(&block[..trailer_offset]);
        block[trailer_offset + 4..trailer_offset + 8].copy_from_slice(&full_checksum.to_le_bytes());

        Some(block)
    }

    pub fn deserialize_zil_from_block(block: &[u8]) -> Vec<HvZilRecord> {
        let mut records = Vec::new();

        if block.len() < ZIL_BLOCK_SIZE {
            return records;
        }

        let header = match ZilBlockHeader::from_block(block) {
            Some(h) => h,
            None => return records,
        };

        if !header.is_valid() || !header.verify_header() {
            return records;
        }

        // SAFETY: block.len() >= ZIL_BLOCK_SIZE (checked above), so this offset is valid
        let trailer_offset = ZIL_BLOCK_SIZE - ZIL_TRAILER_SIZE;
        let tail_magic = u32::from_le_bytes(block[trailer_offset..trailer_offset + 4].try_into().unwrap());
        let block_checksum = u32::from_le_bytes(block[trailer_offset + 4..trailer_offset + 8].try_into().unwrap());
        let trailer = ZilBlockTrailer { tail_magic, block_checksum };

        if !trailer.is_valid() {
            return records;
        }

        let computed = crc32_checksum(&block[..ZIL_BLOCK_SIZE - ZIL_TRAILER_SIZE]);
        if computed != trailer.block_checksum {
            return records;
        }

        let data_end = ZIL_HEADER_SIZE + header.record_count as usize * ZIL_RECORD_DISK_SIZE;
        let data_crc = crc32_checksum(&block[ZIL_HEADER_SIZE..data_end]);
        if data_crc != header.data_checksum {
            return records;
        }

        let record_area = ZIL_HEADER_SIZE;
        for i in 0..header.record_count as usize {
            let offset = record_area + i * ZIL_RECORD_DISK_SIZE;
            if let Some(record) = deserialize_record(&block[offset..offset + ZIL_RECORD_DISK_SIZE])
            {
                records.push(record);
            }
        }

        records.sort_by_key(|r| r.seq);
        records
    }

    pub fn mark_written(&self) {
        self.zil_blocks_written.store(true, Ordering::Release);
    }
}

/// Public CRC32 wrapper for testing
pub fn crc32_test_wrapper(data: &[u8]) -> u32 {
    crc32_checksum(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32_deterministic() {
        let data = b"Hello, ZIL!";
        let c1 = crc32_checksum(data);
        let c2 = crc32_checksum(data);
        assert_eq!(c1, c2);
        assert_ne!(c1, 0);
    }

    #[test]
    fn test_serialize_roundtrip() {
        let rec = HvZilRecord {
            rec_type: super::super::zil::HvZilRecordType::Write,
            txg: 42,
            obj_id: 100,
            parent_obj: 0,
            offset: 4096,
            size: 512,
            name: [0; 128],
            data_hash: [1, 2, 3, 4],
            seq: 1,
        };

        let mut buf = [0u8; ZIL_RECORD_DISK_SIZE];
        serialize_record(&rec, &mut buf);

        let deserialized = deserialize_record(&buf).unwrap();
        assert_eq!(deserialized.txg, 42);
        assert_eq!(deserialized.obj_id, 100);
        assert_eq!(deserialized.offset, 4096);
        assert_eq!(deserialized.size, 512);
        assert_eq!(deserialized.seq, 1);
        assert_eq!(deserialized.data_hash, [1, 2, 3, 4]);
    }

    #[test]
    fn test_deserialize_corrupted_data() {
        let rec = HvZilRecord {
            rec_type: super::super::zil::HvZilRecordType::Create,
            txg: 1,
            obj_id: 0,
            parent_obj: 0,
            offset: 0,
            size: 0,
            name: [0; 128],
            data_hash: [0; 4],
            seq: 0,
        };

        let mut buf = [0u8; ZIL_RECORD_DISK_SIZE];
        serialize_record(&rec, &mut buf);

        buf[10] ^= 0xFF; // corrupt
        assert!(deserialize_record(&buf).is_none());
    }
}
