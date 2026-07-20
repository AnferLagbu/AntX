#![deny(unsafe_code)]
//! WASI FD 管理与 I/O: fd_close, fd_seek, fd_tell, fd_sync,
//! fd_prestat_get, fd_prestat_dir_name, fd_stat_get,
//! fd_read, fd_write, fd_pread, fd_pwrite, fd_allocate, fd_advise,
//! fd_renumber, fd_dup, fd_readdir

use crate::kernel::services::wasm::types::{Value, WasmError};
use crate::kernel::services::wasm::interpreter::Interpreter;
use super::{WasiContext, wasi_success, wasi_errno, WasiErrno};
use super::fd_table::{WasiFileType, WasiRights, read_iovec_from_memory, write_u32_to_memory, write_bytes_to_memory};

// ============================================================================
// G4: FD 管理
// ============================================================================

/// WASI fd_close: 关闭文件描述符
pub fn wasi_fd_close(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    match ctx.fd_table.close(fd) {
        Ok(_entry) => {
            // TODO: 关闭底层 inner_fd (VFS close)
            interp.stack.push(Value::I32(wasi_success()))?;
        }
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
        }
    }
    Ok(())
}

/// WASI fd_seek: 移动文件指针
pub fn wasi_fd_seek(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let offset = interp.stack.pop_i64()?;
    let whence = interp.stack.pop_i32()? as u32;
    let new_offset_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    // WASI whence: 0=SET, 1=CUR, 2=END
    // 映射到 VFS seek
    let _ = (offset, whence);

    // 简化实现: 写回 new_offset = 0 (完整实现需调用 VFS seek)
    write_u32_to_memory(interp, new_offset_ptr, 0);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_tell: 获取文件指针位置
pub fn wasi_fd_tell(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let offset_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    // 简化实现: 写回 offset = 0
    write_u32_to_memory(interp, offset_ptr, 0);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_sync: 同步文件数据到存储
pub fn wasi_fd_sync(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;

    match ctx.fd_table.get(fd) {
        Ok(_entry) => {
            // TODO: 调用 VFS fsync
            interp.stack.push(Value::I32(wasi_success()))?;
        }
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
        }
    }
    Ok(())
}

/// WASI fd_prestat_get: 获取 preopen fd 信息
pub fn wasi_fd_prestat_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let buf_ptr = interp.stack.pop_i32()? as u32;

    let entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    if entry.path.is_none() {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Badf)))?;
        return Ok(());
    }

    let path_len = entry.path.as_ref().unwrap().len() as u32;

    // 写入 prestat 结构: { pr_name_len: u32 }
    write_u32_to_memory(interp, buf_ptr, path_len);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_prestat_dir_name: 获取 preopen 目录名称
pub fn wasi_fd_prestat_dir_name(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;

    let entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    let path = match &entry.path {
        Some(p) => p.as_bytes(),
        None => {
            interp.stack.push(Value::I32(wasi_errno(WasiErrno::Badf)))?;
            return Ok(());
        }
    };

    let copy_len = core::cmp::min(path.len() as u32, path_len) as usize;
    write_bytes_to_memory(interp, path_ptr, &path[..copy_len]);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_stat_get: 获取 fd 状态信息
pub fn wasi_fd_stat_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let _buf_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    // TODO: 写入 fdstat 结构到 buf_ptr
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

// ============================================================================
// G5: FD I/O
// ============================================================================

/// WASI fd_read: 从 fd 读取数据到 iovec 数组
pub fn wasi_fd_read(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let iovs_ptr = interp.stack.pop_i32()? as u32;
    let iovs_len = interp.stack.pop_i32()? as u32;
    let nread_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    let iovecs = read_iovec_from_memory(interp, iovs_ptr, iovs_len)?;
    let mut total = 0u32;

    for iov in &iovecs {
        if iov.len == 0 {
            continue;
        }
        // TODO: 从 inner_fd 读取数据到 WASM 线性内存
        // 当前简化: 全部填充零
        for i in 0..iov.len {
            let _ = interp.memory.as_mut()
                .ok_or(WasmError::MemoryOutOfBounds)?
                .write_u8(iov.buf + i, 0);
        }
        total += iov.len;
    }

    write_u32_to_memory(interp, nread_ptr, total);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_write: 将 iovec 数组中的数据写入 fd
pub fn wasi_fd_write(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let iovs_ptr = interp.stack.pop_i32()? as u32;
    let iovs_len = interp.stack.pop_i32()? as u32;
    let nwritten_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    let iovecs = read_iovec_from_memory(interp, iovs_ptr, iovs_len)?;
    let mut total = 0u32;

    for iov in &iovecs {
        if iov.len == 0 {
            continue;
        }
        // TODO: 将 WASM 线性内存数据写入 inner_fd
        // 当前简化: 计数但不实际写入
        total += iov.len;
    }

    write_u32_to_memory(interp, nwritten_ptr, total);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_pread: 从 fd 指定偏移读取
pub fn wasi_fd_pread(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let iovs_ptr = interp.stack.pop_i32()? as u32;
    let iovs_len = interp.stack.pop_i32()? as u32;
    let _offset = interp.stack.pop_i64()? as u64;
    let nread_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    let iovecs = read_iovec_from_memory(interp, iovs_ptr, iovs_len)?;
    let mut total = 0u32;
    for iov in &iovecs {
        total += iov.len;
    }

    write_u32_to_memory(interp, nread_ptr, total);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_pwrite: 向 fd 指定偏移写入
pub fn wasi_fd_pwrite(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let iovs_ptr = interp.stack.pop_i32()? as u32;
    let iovs_len = interp.stack.pop_i32()? as u32;
    let _offset = interp.stack.pop_i64()? as u64;
    let nwritten_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    let iovecs = read_iovec_from_memory(interp, iovs_ptr, iovs_len)?;
    let mut total = 0u32;
    for iov in &iovecs {
        total += iov.len;
    }

    write_u32_to_memory(interp, nwritten_ptr, total);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI fd_allocate: 预分配文件空间
pub fn wasi_fd_allocate(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let _offset = interp.stack.pop_i64()?;
    let _len = interp.stack.pop_i64()?;

    match ctx.fd_table.get(fd) {
        Ok(_entry) => {
            // TODO: 调用 VFS fallocate
            interp.stack.push(Value::I32(wasi_success()))?;
        }
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
        }
    }
    Ok(())
}

/// WASI fd_advise: 提示文件访问模式
pub fn wasi_fd_advise(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let _offset = interp.stack.pop_i64()?;
    let _len = interp.stack.pop_i64()?;
    let _advice = interp.stack.pop_i32()?;

    match ctx.fd_table.get(fd) {
        Ok(_entry) => {
            // WASI advise 是提示性的，可忽略
            interp.stack.push(Value::I32(wasi_success()))?;
        }
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
        }
    }
    Ok(())
}

// ============================================================================
// G7: 高级 FD
// ============================================================================

/// WASI fd_renumber: 重编号 fd
pub fn wasi_fd_renumber(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let from = interp.stack.pop_i32()? as u32;
    let to = interp.stack.pop_i32()? as u32;

    match ctx.fd_table.renumber(from, to) {
        Ok(()) => {
            interp.stack.push(Value::I32(wasi_success()))?;
        }
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
        }
    }
    Ok(())
}

/// WASI fd_dup: 复制 fd
pub fn wasi_fd_dup(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;

    let entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    let new_entry = super::fd_table::WasiFdEntry {
        file_type: entry.file_type,
        rights: entry.rights,
        inner_fd: entry.inner_fd,
        path: None, // dup 的 fd 不是 preopen
    };

    match ctx.fd_table.alloc(new_entry) {
        Ok(new_fd) => {
            interp.stack.push(Value::I32(new_fd as i32))?;
        }
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
        }
    }
    Ok(())
}

/// WASI fd_readdir: 读取目录内容
pub fn wasi_fd_readdir(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let fd = interp.stack.pop_i32()? as u32;
    let _buf_ptr = interp.stack.pop_i32()? as u32;
    let _buf_len = interp.stack.pop_i32()? as u32;
    let _cookie = interp.stack.pop_i64()? as u64;
    let _buf_used_ptr = interp.stack.pop_i32()? as u32;

    let _entry = match ctx.fd_table.get(fd) {
        Ok(e) => e,
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
            return Ok(());
        }
    };

    // TODO: 实现目录遍历
    write_u32_to_memory(interp, _buf_used_ptr, 0);
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}
