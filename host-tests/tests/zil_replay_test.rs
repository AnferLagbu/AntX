//! HvFS ZIL 损坏记录跳过策略集成测试
//!
//! 验证 P0-I-15 修复后的回放行为:
//! 1. 单条 record CRC 损坏 → 回放继续, 返回其余完好 record
//! 2. 多条 record 部分损坏 → 仅跳过损坏的, 不 panic 不整体失败
//! 3. HvZilPersistError 各变体可识别
//!
//! 本文件以自包含的 mini-persist 镜像内核 `try_deserialize_record` /
//! `deserialize_zil_from_block` 行为, 不依赖内核 crate, 便于 host 端快速验证.
//! 内核源码侧的 `services/fs/hvfs/zil_persist.rs` 是该契约的权威实现.

const ZIL_BLOCK_SIZE: usize = 4096;
const ZIL_HEADER_SIZE: usize = 64;
const ZIL_TRAILER_SIZE: usize = 8;
const ZIL_RECORD_DISK_SIZE: usize = 256;
const ZIL_MAGIC: u32 = 0x5A494C31u32; // "ZIL1"
const ZIL_TAIL_MAGIC: u32 = 0x454E4400u32; // "END\0"

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HvZilPersistError {
    BufferTooShort { need: usize, got: usize },
    CrcMismatch { expected: u32, computed: u32 },
    UnknownRecordType(u8),
    InvalidBlock,
}

impl HvZilPersistError {
    pub fn as_static_str(&self) -> &'static str {
        match self {
            Self::BufferTooShort { .. } => "buffer_too_short",
            Self::CrcMismatch { .. } => "crc_mismatch",
            Self::UnknownRecordType(_) => "unknown_record_type",
            Self::InvalidBlock => "invalid_block",
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

fn serialize_record(rec_type: u8, txg: u64, obj_id: u64, offset: u64, size: u32, seq: u64, buf: &mut [u8]) {
    if buf.len() < ZIL_RECORD_DISK_SIZE {
        return;
    }
    buf[0] = rec_type;
    buf[1..9].copy_from_slice(&txg.to_le_bytes());
    buf[9..17].copy_from_slice(&obj_id.to_le_bytes());
    buf[17..25].copy_from_slice(&0u64.to_le_bytes());
    buf[25..33].copy_from_slice(&offset.to_le_bytes());
    buf[33..37].copy_from_slice(&size.to_le_bytes());
    buf[37..45].copy_from_slice(&seq.to_le_bytes());
    // name 128B 留零
    // data_hash 32B 留零
    let payload_end = 173 + 32;
    let rec_crc = crc32_checksum(&buf[..payload_end]);
    buf[payload_end..payload_end + 4].copy_from_slice(&rec_crc.to_le_bytes());
}

fn try_deserialize_record(buf: &[u8]) -> Result<(u8, u64, u64, u64, u32, u64), HvZilPersistError> {
    let actual_size = 173 + 32 + 4;
    if buf.len() < actual_size {
        return Err(HvZilPersistError::BufferTooShort { need: actual_size, got: buf.len() });
    }
    let rec_crc_arr: [u8; 4] = buf[actual_size - 4..actual_size]
        .try_into()
        .map_err(|_| HvZilPersistError::BufferTooShort { need: 4, got: actual_size - 4 })?;
    let rec_crc = u32::from_le_bytes(rec_crc_arr);
    let computed = crc32_checksum(&buf[..actual_size - 4]);
    if rec_crc != computed {
        return Err(HvZilPersistError::CrcMismatch { expected: rec_crc, computed });
    }
    let rec_type = buf[0];
    let txg_arr: [u8; 8] = buf[1..9].try_into().map_err(|_| HvZilPersistError::BufferTooShort { need: 8, got: 8 })?;
    let obj_arr: [u8; 8] = buf[9..17].try_into().map_err(|_| HvZilPersistError::BufferTooShort { need: 8, got: 8 })?;
    let off_arr: [u8; 8] = buf[25..33].try_into().map_err(|_| HvZilPersistError::BufferTooShort { need: 8, got: 8 })?;
    let size_arr: [u8; 4] = buf[33..37].try_into().map_err(|_| HvZilPersistError::BufferTooShort { need: 4, got: 4 })?;
    let seq_arr: [u8; 8] = buf[37..45].try_into().map_err(|_| HvZilPersistError::BufferTooShort { need: 8, got: 8 })?;
    let txg = u64::from_le_bytes(txg_arr);
    let obj_id = u64::from_le_bytes(obj_arr);
    let offset = u64::from_le_bytes(off_arr);
    let size = u32::from_le_bytes(size_arr);
    let seq = u64::from_le_bytes(seq_arr);
    Ok((rec_type, txg, obj_id, offset, size, seq))
}

/// 镜像内核 `deserialize_zil_from_block`: 损坏 record 跳过而非整体失败.
fn replay_with_skip_on_corrupt(block: &[u8]) -> Vec<(u8, u64, u64, u64, u32, u64)> {
    let mut out = Vec::new();
    if block.len() < ZIL_BLOCK_SIZE {
        return out;
    }
    let header_magic_arr: [u8; 4] = block[0..4].try_into().unwrap();
    let header_magic = u32::from_le_bytes(header_magic_arr);
    if header_magic != ZIL_MAGIC {
        return out;
    }
    let record_count_arr: [u8; 2] = block[24..26].try_into().unwrap();
    let record_count = u16::from_le_bytes(record_count_arr) as usize;

    let record_area = ZIL_HEADER_SIZE;
    for i in 0..record_count {
        let offset = record_area + i * ZIL_RECORD_DISK_SIZE;
        match try_deserialize_record(&block[offset..offset + ZIL_RECORD_DISK_SIZE]) {
            Ok(rec) => out.push(rec),
            Err(_e) => {
                // P0-I-15 契约: 损坏 record 跳过, 不 panic
            }
        }
    }
    out
}

fn build_valid_block(records: &[(u8, u64, u64, u64, u32, u64)]) -> Vec<u8> {
    let mut block = vec![0u8; ZIL_BLOCK_SIZE];
    // header magic
    block[0..4].copy_from_slice(&ZIL_MAGIC.to_le_bytes());
    // version
    block[4..6].copy_from_slice(&1u16.to_le_bytes());
    // record_count
    block[24..26].copy_from_slice(&(records.len() as u16).to_le_bytes());
    // records
    for (i, rec) in records.iter().enumerate() {
        let offset = ZIL_HEADER_SIZE + i * ZIL_RECORD_DISK_SIZE;
        serialize_record(rec.0, rec.1, rec.2, rec.3, rec.4, rec.5, &mut block[offset..offset + ZIL_RECORD_DISK_SIZE]);
    }
    // tail magic
    let trailer_offset = ZIL_BLOCK_SIZE - ZIL_TRAILER_SIZE;
    block[trailer_offset..trailer_offset + 4].copy_from_slice(&ZIL_TAIL_MAGIC.to_le_bytes());
    block
}

#[test]
fn single_corrupt_record_is_skipped_during_replay() {
    // P0-I-15 验收: 单条 CRC 损坏 → 回放返回剩余完好 record, 不 panic
    let valid_rec = (5u8, 1u64, 100u64, 4096u64, 512u32, 1u64);
    let mut block = build_valid_block(&[valid_rec, valid_rec]);
    // 破坏第二条 record 的 payload (offset 10, 在 CRC 之前)
    let corrupt_offset = ZIL_HEADER_SIZE + ZIL_RECORD_DISK_SIZE + 10;
    block[corrupt_offset] ^= 0xFF;

    let result = replay_with_skip_on_corrupt(&block);
    assert_eq!(result.len(), 1, "P0-I-15: 1 个损坏 record 应被跳过, 剩 1 个完好");
    assert_eq!(result[0].1, 1, "应保留 txg=1 的 record");
}

#[test]
fn multiple_records_partial_corruption_continues_replay() {
    // P0-I-15 验收: 5 条 record 损坏 2 条 → 回放返回 3 条
    let rec = (5u8, 1u64, 100u64, 4096u64, 512u32, 1u64);
    let records = vec![rec; 5];
    let mut block = build_valid_block(&records);
    // 损坏第 1 条和第 3 条的 payload
    for i in [0usize, 2] {
        let c = ZIL_HEADER_SIZE + i * ZIL_RECORD_DISK_SIZE + 10;
        block[c] ^= 0xFF;
    }

    let result = replay_with_skip_on_corrupt(&block);
    assert_eq!(result.len(), 3, "P0-I-15: 损坏 2/5 应回放出 3 条");
}

#[test]
fn all_records_corrupted_returns_empty_without_panic() {
    // P0-I-15 验收: 全损坏 → 返回空 Vec, 不 panic
    let rec = (5u8, 1u64, 100u64, 4096u64, 512u32, 1u64);
    let records = vec![rec; 4];
    let mut block = build_valid_block(&records);
    for i in 0..4 {
        let c = ZIL_HEADER_SIZE + i * ZIL_RECORD_DISK_SIZE + 10;
        block[c] ^= 0xFF;
    }

    let result = replay_with_skip_on_corrupt(&block);
    assert!(result.is_empty(), "P0-I-15: 全损坏应返回空, 不 panic");
}

#[test]
fn healthy_block_replays_all_records() {
    // P0-I-15 回归: 完好 block 应回放出全部 record
    let records = vec![
        (5u8, 1u64, 100u64, 4096u64, 512u32, 1u64),
        (5u8, 1u64, 101u64, 8192u64, 1024u32, 2u64),
        (5u8, 1u64, 102u64, 0u64, 0u32, 3u64),
    ];
    let block = build_valid_block(&records);
    let result = replay_with_skip_on_corrupt(&block);
    assert_eq!(result.len(), 3, "P0-I-15 回归: 完好 block 应回放 3 条");
}

#[test]
fn crc_mismatch_error_is_distinguishable() {
    // P0-I-15 验收: 错误类型语义化, 可识别 CrcMismatch
    let mut buf = [0u8; ZIL_RECORD_DISK_SIZE];
    serialize_record(5, 1, 100, 4096, 512, 1, &mut buf);
    buf[10] ^= 0xFF; // corrupt
    let err = try_deserialize_record(&buf).expect_err("损坏 record 应返回 Err");
    assert!(matches!(err, HvZilPersistError::CrcMismatch { .. }), "P0-I-15: 错误必须是 CrcMismatch");
    assert_eq!(err.as_static_str(), "crc_mismatch");
}

#[test]
fn buffer_too_short_error_is_distinguishable() {
    let buf = [0u8; 10]; // 远小于 ZIL_RECORD_DISK_SIZE
    let err = try_deserialize_record(&buf).expect_err("短缓冲应返回 Err");
    assert!(matches!(err, HvZilPersistError::BufferTooShort { .. }), "P0-I-15: 短缓冲必须 BufferTooShort");
    assert_eq!(err.as_static_str(), "buffer_too_short");
}

#[test]
fn truncated_block_is_rejected_silently() {
    // P0-I-15 验收: block 长度不足应返回空, 不 panic
    let block = vec![0u8; 100];
    let result = replay_with_skip_on_corrupt(&block);
    assert!(result.is_empty(), "P0-I-15: 截断 block 应返回空, 不 panic");
}

#[test]
fn bad_magic_block_is_rejected_silently() {
    // P0-I-15 验收: 错误 magic 应返回空, 不 panic
    let mut block = vec![0u8; ZIL_BLOCK_SIZE];
    block[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    let result = replay_with_skip_on_corrupt(&block);
    assert!(result.is_empty(), "P0-I-15: 错误 magic 应返回空, 不 panic");
}
