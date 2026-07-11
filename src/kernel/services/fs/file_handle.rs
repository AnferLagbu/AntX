#![deny(unsafe_code)]
//! 文件句柄系统 — name_to_handle_at / open_by_handle_at
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::fs::vfs::api。
//!
//! ## 职责
//!
//! - 实现 Linux 风格的文件句柄导出/导入
//! - 支持文件描述符跨进程传递
//! - 支持文件系统快照/备份
//!
//! ## 参考
//!
//! - Linux name_to_handle_at(2) 手册页
//! - Linux open_by_handle_at(2) 手册页

use crate::kernel::framework::syscall::Errno;
use crate::kernel::framework::mm::copy_user::{copy_to_user, copy_from_user};

/// 文件句柄类型 (与 Linux 兼容)
pub const FILE_HANDLE_GHEST_ID: i32 = 0x01; // 通用句柄类型

/// 文件句柄大小
pub const FILE_HANDLE_SIZE: usize = 144; // 8 + 128 + 8 (对齐)

/// name_to_handle_at — 导出文件句柄
///
/// 将文件路径导出为可序列化的文件句柄, 用于跨进程传递或持久化.
///
/// # 参数
/// - `dirfd`: 目录文件描述符 (AT_FDCWD = -100)
/// - `path`: 文件路径
/// - `handle_type`: 句柄类型 (仅支持 FILE_HANDLE_GHEST_ID)
/// - `handle_buf`: 用户空间缓冲区 (输出)
/// - `mnt_id`: 挂载点 ID (输出)
/// - `flags`: 标志位 (AT_EMPTY_PATH = 0x1000)
///
/// # 返回
/// - 成功: 写入句柄到 handle_buf, 返回 0
/// - 失败: 返回 -errno
pub fn name_to_handle_at_syscall(
    _dirfd: i32,
    _path_ptr: u64,
    handle_type: i32,
    handle_buf: u64,
    mnt_id: u64,
    _flags: u32,
) -> Result<i64, Errno> {
    // 参数验证
    if handle_buf == 0 {
        return Err(Errno::EFAULT);
    }

    // 仅支持通用句柄类型
    if handle_type != FILE_HANDLE_GHEST_ID {
        return Err(Errno::ENOTSUP);
    }

    // 通过 VFS 获取文件信息
    // 注意: 当前实现简化为获取当前目录的 inode 信息
    // 完整实现需要路径解析
    let inode_id = 1u32; // 临时: 使用根目录 inode
    let mount_idx = 0u32;

    // 构建句柄数据 (inode_id + mount_idx)
    let mut handle_data = [0u8; FILE_HANDLE_SIZE];
    handle_data[0..4].copy_from_slice(&inode_id.to_le_bytes());
    handle_data[4..8].copy_from_slice(&mount_idx.to_le_bytes());
    handle_data[8] = FILE_HANDLE_GHEST_ID as u8; // handle_type
    handle_data[12..16].copy_from_slice(&8u32.to_le_bytes()); // handle_bytes

    // 写入用户空间
    copy_to_user(handle_buf, &handle_data[..16], 16)
        .map_err(|_| Errno::EFAULT)?;

    // 写入 mnt_id
    if mnt_id != 0 {
        let mnt_data = mount_idx.to_le_bytes();
        copy_to_user(mnt_id, &mnt_data, 4)
            .map_err(|_| Errno::EFAULT)?;
    }

    Ok(0)
}

/// open_by_handle_at — 通过句柄打开文件
///
/// 使用之前导出的文件句柄打开文件.
///
/// # 参数
/// - `mount_fd`: 挂载点文件描述符
/// - `handle_ptr`: 用户空间句柄缓冲区
/// - `handle_type`: 句柄类型
/// - `flags`: 打开标志 (O_RDONLY, O_WRONLY 等)
///
/// # 返回
/// - 成功: 返回新的文件描述符
/// - 失败: 返回 -errno
pub fn open_by_handle_at_syscall(
    _mount_fd: i32,
    handle_ptr: u64,
    handle_type: i32,
    flags: u32,
) -> Result<i64, Errno> {
    // 参数验证
    if handle_ptr == 0 {
        return Err(Errno::EFAULT);
    }

    // 仅支持通用句柄类型
    if handle_type != FILE_HANDLE_GHEST_ID {
        return Err(Errno::ENOTSUP);
    }

    // 从用户空间读取句柄
    let mut handle_data = [0u8; 16];
    copy_from_user(&mut handle_data, handle_ptr, 16)
        .map_err(|_| Errno::EFAULT)?;

    // 提取 inode_id 和 mount_idx
    let inode_id = u32::from_le_bytes(handle_data[0..4].try_into().unwrap_or([0; 4]));
    let mount_idx = u32::from_le_bytes(handle_data[4..8].try_into().unwrap_or([0; 4]));

    // 获取当前进程的 PWM
    let pwm = crate::kernel::framework::credo::session::get_current_pwm();

    // 通过 VFS 打开文件
    // 注意: 当前实现简化为打开一个虚拟文件
    // 完整实现需要根据 inode_id 和 mount_idx 打开实际文件
    let open_file = crate::kernel::services::fs::vfs_types::OpenFile::new(
        inode_id,
        mount_idx,
        flags,
        pwm,
        0, // File type
    );

    // 插入全局 OpenFile 表
    let handle_id = crate::kernel::services::fs::open_file_table::OPEN_FILE_TABLE
        .alloc(open_file)
        .ok_or(Errno::ENOMEM)?;

    // 在当前进程 fd 表中分配 fd
    // 注意: 当前实现简化为返回 handle_id 作为 fd
    // 完整实现需要在进程 fd 表中分配
    Ok(handle_id as i64)
}
