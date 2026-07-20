#![deny(unsafe_code)]
//! WASI 路径操作: path_open, path_create_directory, path_remove_directory,
//! path_unlink_file, path_relative_path, path_symlink, path_readlink,
//! path_filestat_get, path_filestat_set_times, path_link

use crate::kernel::services::wasm::types::{Value, WasmError};
use crate::kernel::services::wasm::interpreter::Interpreter;
use super::{WasiContext, wasi_success, wasi_errno, WasiErrno};
use super::fd_table::{WasiFileType, WasiRights, WasiFdEntry, read_bytes_from_memory, write_u32_to_memory};
use alloc::string::String;

/// 从 WASM 线性内存读取路径字符串 (NUL 终止)
fn read_path(interp: &Interpreter, ptr: u32, len: u32) -> Result<String, WasiErrno> {
    let bytes = read_bytes_from_memory(interp, ptr, len)?;
    // 去除 NUL 终止符
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..end])
        .map(|s| String::from(s))
        .map_err(|_| WasiErrno::Inval)
}

/// 解析路径: 将 dirfd + relative path 组合为绝对路径
///
/// 简化实现: 直接返回 relative path (完整实现需要解析 dirfd 的 preopen 路径)
fn resolve_path(_ctx: &WasiContext, dirfd: u32, path: &str) -> Result<String, WasiErrno> {
    if dirfd == 0xffffff9c { // AT_FDCWD
        return Ok(String::from(path));
    }
    // 完整实现需要从 fd_table 获取 preopen 路径并拼接
    // 简化: 直接返回路径
    let _ = _ctx;
    Ok(String::from(path))
}

/// WASI path_open: 打开路径上的文件/目录
///
/// 参数: (dirfd, dirflags, path_ptr, path_len, o_flags, fs_rights_base, fs_rights_inheriting, fd_flags, fd_ptr)
pub fn wasi_path_open(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let _dirflags = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let _o_flags = interp.stack.pop_i32()? as u16;
    let _fs_rights_base = interp.stack.pop_i64()? as u64;
    let _fs_rights_inheriting = interp.stack.pop_i64()? as u64;
    let _fd_flags = interp.stack.pop_i32()? as u16;
    let fd_ptr = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &path)?;

    // TODO: 调用 VFS open → 获取 inner_fd → 创建 WasiFdEntry → alloc
    // 当前简化: 返回一个伪 fd
    let entry = WasiFdEntry {
        file_type: WasiFileType::RegularFile,
        rights: WasiRights::FILE,
        inner_fd: -1, // TODO: 实际 VFS fd
        path: Some(path),
    };

    match ctx.fd_table.alloc(entry) {
        Ok(new_fd) => {
            write_u32_to_memory(interp, fd_ptr, new_fd);
            interp.stack.push(Value::I32(wasi_success()))?;
        }
        Err(e) => {
            interp.stack.push(Value::I32(wasi_errno(e)))?;
        }
    }
    Ok(())
}

/// WASI path_create_directory: 创建目录
pub fn wasi_path_create_directory(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &path)?;

    // TODO: 调用 VFS mkdir
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_remove_directory: 删除目录
pub fn wasi_path_remove_directory(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &path)?;

    // TODO: 调用 VFS rmdir
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_unlink_file: 删除文件
pub fn wasi_path_unlink_file(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &path)?;

    // TODO: 调用 VFS unlink
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_symlink: 创建符号链接
pub fn wasi_path_symlink(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let old_path_ptr = interp.stack.pop_i32()? as u32;
    let old_path_len = interp.stack.pop_i32()? as u32;
    let dirfd = interp.stack.pop_i32()? as u32;
    let new_path_ptr = interp.stack.pop_i32()? as u32;
    let new_path_len = interp.stack.pop_i32()? as u32;

    let _old_path = read_path(interp, old_path_ptr, old_path_len)?;
    let new_path = read_path(interp, new_path_ptr, new_path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &new_path)?;

    // TODO: 调用 VFS symlink
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_readlink: 读取符号链接目标
pub fn wasi_path_readlink(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let _buf_ptr = interp.stack.pop_i32()? as u32;
    let _buf_len = interp.stack.pop_i32()? as u32;
    let _buf_used_ptr = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &path)?;

    // TODO: 调用 VFS readlink
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_rename: 重命名文件/目录
pub fn wasi_path_rename(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let old_dirfd = interp.stack.pop_i32()? as u32;
    let old_path_ptr = interp.stack.pop_i32()? as u32;
    let old_path_len = interp.stack.pop_i32()? as u32;
    let new_dirfd = interp.stack.pop_i32()? as u32;
    let new_path_ptr = interp.stack.pop_i32()? as u32;
    let new_path_len = interp.stack.pop_i32()? as u32;

    let _old_path = read_path(interp, old_path_ptr, old_path_len)?;
    let new_path = read_path(interp, new_path_ptr, new_path_len)?;
    let _old_abs = resolve_path(ctx, old_dirfd, &_old_path)?;
    let _new_abs = resolve_path(ctx, new_dirfd, &new_path)?;

    // TODO: 调用 VFS rename
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_filestat_get: 获取文件/目录状态
pub fn wasi_path_filestat_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let _flags = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let _buf_ptr = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &path)?;

    // TODO: 调用 VFS stat 并写入 filestat 结构
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_filestat_set_times: 设置文件/目录时间戳
pub fn wasi_path_filestat_set_times(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let _flags = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let _atim = interp.stack.pop_i64()?;
    let _mtim = interp.stack.pop_i64()?;

    let path = read_path(interp, path_ptr, path_len)?;
    let _abs_path = resolve_path(ctx, dirfd, &path)?;

    // TODO: 调用 VFS utimensat
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_link: 创建硬链接
pub fn wasi_path_link(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let old_dirfd = interp.stack.pop_i32()? as u32;
    let _old_flags = interp.stack.pop_i32()? as u32;
    let old_path_ptr = interp.stack.pop_i32()? as u32;
    let old_path_len = interp.stack.pop_i32()? as u32;
    let new_dirfd = interp.stack.pop_i32()? as u32;
    let new_path_ptr = interp.stack.pop_i32()? as u32;
    let new_path_len = interp.stack.pop_i32()? as u32;

    let _old_path = read_path(interp, old_path_ptr, old_path_len)?;
    let new_path = read_path(interp, new_path_ptr, new_path_len)?;
    let _old_abs = resolve_path(ctx, old_dirfd, &_old_path)?;
    let _new_abs = resolve_path(ctx, new_dirfd, &new_path)?;

    // TODO: 调用 VFS link
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}
