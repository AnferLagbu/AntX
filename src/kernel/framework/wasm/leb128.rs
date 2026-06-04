//! LEB128 编解码器 — Little Endian Base 128 变长整数编码

use alloc::vec::Vec;

use super::types::WasmError;

pub fn read_leb128_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, WasmError> {
    let mut result: u32 = 0;
    let mut shift: u32 = 0;
    loop {
        if *pos >= bytes.len() {
            return Err(WasmError::Truncated);
        }
        let byte = bytes[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 32 {
            return Err(WasmError::Leb128Overflow);
        }
    }
    Ok(result)
}

pub fn read_leb128_i32(bytes: &[u8], pos: &mut usize) -> Result<i32, WasmError> {
    let mut result: i32 = 0;
    let mut shift: u32 = 0;
    let size: u32 = 32;
    let mut byte: u8;
    loop {
        if *pos >= bytes.len() {
            return Err(WasmError::Truncated);
        }
        byte = bytes[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as i32) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
        if shift >= 32 {
            return Err(WasmError::Leb128Overflow);
        }
    }
    if shift < size && (byte & 0x40) != 0 {
        result |= !0 << shift;
    }
    Ok(result)
}

pub fn read_leb128_i64(bytes: &[u8], pos: &mut usize) -> Result<i64, WasmError> {
    let mut result: i64 = 0;
    let mut shift: u32 = 0;
    let size: u32 = 64;
    let mut byte: u8;
    loop {
        if *pos >= bytes.len() {
            return Err(WasmError::Truncated);
        }
        byte = bytes[*pos];
        *pos += 1;
        result |= ((byte & 0x7F) as i64) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            break;
        }
        if shift >= 64 {
            return Err(WasmError::Leb128Overflow);
        }
    }
    if shift < size && (byte & 0x40) != 0 {
        result |= !0 << shift;
    }
    Ok(result)
}

pub fn read_name(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>, WasmError> {
    let len = read_leb128_u32(bytes, pos)? as usize;
    if *pos + len > bytes.len() {
        return Err(WasmError::Truncated);
    }
    let name = bytes[*pos..*pos + len].to_vec();
    *pos += len;
    Ok(name)
}
