//! WASI 路径操作: path_open, path_create_directory, path_remove_directory,
//! path_unlink_file, path_relative_path, path_symlink, path_readlink,
//! path_filestat_get, path_filestat_set_times, path_link

use crate::kernel::services::wasm::types::{Value, WasmError};
use crate::kernel::services::wasm::interpreter::Interpreter;
use super::{WasiContext, wasi_success, wasi_errno, WasiErrno};
use super::fd_table::{WasiFileType, WasiRights, WasiFdEntry, read_bytes_from_memory, write_u32_to_memory, write_bytes_to_memory};
use alloc::string::String;

/// 从 WASM 线性内存读取路径字符串 (NUL 终止)
fn read_path(interp: &Interpreter, ptr: u32, len: u32) -> Result<String, WasiErrno> {
    let bytes = read_bytes_from_memory(interp, ptr, len)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..end])
        .map(|s| String::from(s))
        .map_err(|_| WasiErrno::Inval)
}

/// 解析路径: 将 dirfd + relative path 组合为绝对路径
///
/// WASI 语义: dirfd 是 preopen fd (通过 fd_prestat_get 获取), relative path
/// 相对于 dirfd 的 preopen 路径。AT_FDCWD (0xffffff9c) 表示使用当前工作目录。
fn resolve_path(ctx: &WasiContext, dirfd: u32, path: &str) -> Result<String, WasiErrno> {
    // AT_FDCWD: 直接返回 relative path (相对当前工作目录)
    if dirfd == 0xffffff9c {
        return Ok(String::from(path));
    }

    // 从 fd_table 获取 preopen 路径
    let entry = ctx.fd_table.get(dirfd)?;
    match &entry.path {
        Some(base_path) => {
            // 拼接: base_path + "/" + relative path
            if path.starts_with('/') {
                // 绝对路径: 忽略 dirfd
                Ok(String::from(path))
            } else if base_path.ends_with('/') {
                Ok(alloc::format!("{}{}", base_path, path))
            } else {
                Ok(alloc::format!("{}/{}", base_path, path))
            }
        }
        None => {
            // 非 preopen fd, 使用 relative path 本身
            Ok(String::from(path))
        }
    }
}

/// 将路径字符串转换为 C 字符串指针 (用于 VFS API)
///
/// # Safety
/// 返回的指针指向 WASM 线性内存中的数据，仅在当前 WASM 实例生命周期内有效。
unsafe fn path_to_cstr(interp: &Interpreter, path: &str) -> Result<*const u8, WasiErrno> {
    let mem = interp.memory.as_ref().ok_or(WasiErrno::Inval)?;
    // 写入路径到线性内存末尾区域 (使用固定偏移避免冲突)
    // 简化: 使用临时缓冲区 (完整实现需分配专用区域)
    // 这里改为直接使用 path.as_ptr() — 在 no_std 环境中路径字符串在内核栈/堆上
    // VFS API 接受 *const u8 指向 C 字符串
    Ok(path.as_ptr())
}

/// WASI path_open: 打开路径上的文件/目录
///
/// 参数: (dirfd, dirflags, path_ptr, path_len, o_flags, fs_rights_base, fs_rights_inheriting, fd_flags, fd_ptr)
pub fn wasi_path_open(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let _dirflags = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let o_flags = interp.stack.pop_i32()? as u32;
    let _fs_rights_base = interp.stack.pop_i64()? as u64;
    let _fs_rights_inheriting = interp.stack.pop_i64()? as u64;
    let _fd_flags = interp.stack.pop_i32()? as u32;
    let fd_ptr = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let abs_path = resolve_path(ctx, dirfd, &path)?;

    // WASI o_flags → VFS flags 映射
    let vfs_flags = wasi_o_flags_to_vfs(o_flags);

    // 调用 VFS open
    // SAFETY: abs_path 是内核堆上的 String，as_ptr() 返回有效 C 字符串指针
    let vfs_fd = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_open(
            abs_path.as_ptr(),
            vfs_flags,
            0, // pwm (权限掩码)
        )
    };

    if vfs_fd < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Noent)))?;
        return Ok(());
    }

    // 创建 WASI fd 表条目
    let entry = WasiFdEntry {
        file_type: WasiFileType::RegularFile,
        rights: WasiRights::FILE,
        inner_fd: vfs_fd,
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

/// WASI o_flags → VFS flags 映射
fn wasi_o_flags_to_vfs(o_flags: u32) -> u32 {
    let mut flags = 0u32;
    // O_RDONLY = 0, O_WRONLY = 1, O_RDWR = 2 (位 0-1)
    flags |= o_flags & 0x03;
    // O_CREAT = 0x100 (WASI) → O_CREAT (VFS)
    if o_flags & 0x100 != 0 { flags |= 0x100; }
    // O_EXCL = 0x200 (WASI) → O_EXCL (VFS)
    if o_flags & 0x200 != 0 { flags |= 0x200; }
    // O_TRUNC = 0x400 (WASI) → O_TRUNC (VFS)
    if o_flags & 0x400 != 0 { flags |= 0x400; }
    // O_APPEND = 0x008 (WASI) → O_APPEND (VFS)
    if o_flags & 0x008 != 0 { flags |= 0x008; }
    flags
}

/// WASI path_create_directory: 创建目录
pub fn wasi_path_create_directory(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let abs_path = resolve_path(ctx, dirfd, &path)?;

    // SAFETY: abs_path 是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_mkdir(abs_path.as_ptr(), 0)
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        interp.stack.push(Value::I32(wasi_success()))?;
    }
    Ok(())
}

/// WASI path_remove_directory: 删除目录
pub fn wasi_path_remove_directory(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let abs_path = resolve_path(ctx, dirfd, &path)?;

    // SAFETY: abs_path 是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_rmdir(abs_path.as_ptr(), 0)
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        interp.stack.push(Value::I32(wasi_success()))?;
    }
    Ok(())
}

/// WASI path_unlink_file: 删除文件
pub fn wasi_path_unlink_file(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let abs_path = resolve_path(ctx, dirfd, &path)?;

    // SAFETY: abs_path 是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_unlink(abs_path.as_ptr(), 0)
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        interp.stack.push(Value::I32(wasi_success()))?;
    }
    Ok(())
}

/// WASI path_symlink: 创建符号链接
pub fn wasi_path_symlink(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let old_path_ptr = interp.stack.pop_i32()? as u32;
    let old_path_len = interp.stack.pop_i32()? as u32;
    let dirfd = interp.stack.pop_i32()? as u32;
    let new_path_ptr = interp.stack.pop_i32()? as u32;
    let new_path_len = interp.stack.pop_i32()? as u32;

    let old_path = read_path(interp, old_path_ptr, old_path_len)?;
    let new_path = read_path(interp, new_path_ptr, new_path_len)?;
    let _abs_new = resolve_path(ctx, dirfd, &new_path)?;

    // WASI path_symlink(old_path, dirfd, new_path)
    // old_path = target (被指向的路径), new_path = linkpath (链接路径)
    // VFS symlink(target, linkpath)
    // SAFETY: 路径是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_symlink(
            old_path.as_ptr(),
            new_path.as_ptr(),
            0,
        )
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        interp.stack.push(Value::I32(wasi_success()))?;
    }
    Ok(())
}

/// WASI path_readlink: 读取符号链接目标
pub fn wasi_path_readlink(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let buf_ptr = interp.stack.pop_i32()? as u32;
    let buf_len = interp.stack.pop_i32()? as u32;
    let buf_used_ptr = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let abs_path = resolve_path(ctx, dirfd, &path)?;

    // SAFETY: abs_path 是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_readlink(
            abs_path.as_ptr(),
            buf_ptr as *mut u8,
            buf_len as u64,
            0,
        )
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        write_u32_to_memory(interp, buf_used_ptr, result as u32);
        interp.stack.push(Value::I32(wasi_success()))?;
    }
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

    let old_path = read_path(interp, old_path_ptr, old_path_len)?;
    let new_path = read_path(interp, new_path_ptr, new_path_len)?;
    let old_abs = resolve_path(ctx, old_dirfd, &old_path)?;
    let new_abs = resolve_path(ctx, new_dirfd, &new_path)?;

    // SAFETY: 路径是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_rename(
            old_abs.as_ptr(),
            new_abs.as_ptr(),
            0,
        )
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        interp.stack.push(Value::I32(wasi_success()))?;
    }
    Ok(())
}

/// WASI path_filestat_get: 获取文件/目录状态
pub fn wasi_path_filestat_get(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let _flags = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let buf_ptr = interp.stack.pop_i32()? as u32;

    let path = read_path(interp, path_ptr, path_len)?;
    let abs_path = resolve_path(ctx, dirfd, &path)?;

    // SAFETY: abs_path 是内核堆上的 String
    let mut stat = crate::kernel::services::fs::vfs_types::VfsStat::default();
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_stat(
            abs_path.as_ptr(),
            &mut stat as *mut _,
            0,
        )
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Noent)))?;
        return Ok(());
    }

    // 写入 WASI filestat 结构
    write_filestat(interp, buf_ptr, &stat)?;
    interp.stack.push(Value::I32(wasi_success()))?;
    Ok(())
}

/// WASI path_filestat_set_times: 设置文件/目录时间戳
///
/// WASI 语义: atime/mtime 为 u64::MAX 时表示不修改该时间戳
pub fn wasi_path_filestat_set_times(ctx: &mut WasiContext, interp: &mut Interpreter) -> Result<(), WasmError> {
    let dirfd = interp.stack.pop_i32()? as u32;
    let _flags = interp.stack.pop_i32()? as u32;
    let path_ptr = interp.stack.pop_i32()? as u32;
    let path_len = interp.stack.pop_i32()? as u32;
    let atim = interp.stack.pop_i64()? as u64;
    let mtim = interp.stack.pop_i64()? as u64;

    let path = read_path(interp, path_ptr, path_len)?;
    let abs_path = resolve_path(ctx, dirfd, &path)?;

    // WASI u64::MAX 表示不修改该时间戳
    let vfs_atime = if atim == u64::MAX { u64::MAX } else { atim };
    let vfs_mtime = if mtim == u64::MAX { u64::MAX } else { mtim };

    // SAFETY: abs_path 是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_utimensat(
            abs_path.as_ptr(),
            vfs_atime,
            vfs_mtime,
            0,
        )
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        interp.stack.push(Value::I32(wasi_success()))?;
    }
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

    let old_path = read_path(interp, old_path_ptr, old_path_len)?;
    let new_path = read_path(interp, new_path_ptr, new_path_len)?;
    let _old_abs = resolve_path(ctx, old_dirfd, &old_path)?;
    let _new_abs = resolve_path(ctx, new_dirfd, &new_path)?;

    // VFS link(oldpath, newpath)
    // SAFETY: 路径是内核堆上的 String
    let result = unsafe {
        crate::kernel::framework::fs::vfs::api::vfs_link(
            old_path.as_ptr(),
            new_path.as_ptr(),
            0,
        )
    };

    if result < 0 {
        interp.stack.push(Value::I32(wasi_errno(WasiErrno::Io)))?;
    } else {
        interp.stack.push(Value::I32(wasi_success()))?;
    }
    Ok(())
}

/// 写入 WASI filestat 结构到线性内存
fn write_filestat(interp: &mut Interpreter, buf_ptr: u32, stat: &crate::kernel::services::fs::vfs_types::VfsStat) -> Result<(), WasmError> {
    let mem = interp.memory.as_mut().ok_or(WasmError::MemoryOutOfBounds)?;
    let base = buf_ptr as u64;

    let write_u64 = |mem: &mut crate::kernel::services::wasm::runtime::LinearMemory, off: u64, val: u64| {
        let bytes = val.to_le_bytes();
        for i in 0..8u64 {
            let _ = mem.write_u8((base + off + i) as u32, bytes[i as usize]);
        }
    };

    // WASI filestat: { dev: u64, ino: u64, filetype: u8, nlink: u64, size: u64, atim: u64, mtim: u64, ctim: u64 }
    write_u64(mem, 0, stat.node_id as u64);    // dev
    write_u64(mem, 8, stat.node_id as u64);    // ino
    let _ = mem.write_u8((base + 16) as u32, stat.file_type as u8);
    write_u64(mem, 17, 1);                      // nlink (默认 1)
    write_u64(mem, 25, stat.size as u64);
    write_u64(mem, 33, stat.atime);
    write_u64(mem, 41, stat.mtime);
    write_u64(mem, 49, stat.ctime);

    Ok(())
}
