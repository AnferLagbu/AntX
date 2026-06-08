#![deny(unsafe_code)]
//! mmap — services 层安全代理
//!
//! @SAFE: 本文件不含 unsafe 代码。
//! 所有 unsafe 操作已委托至 framework::syscall::mmap。
//!
//! ## 职责
//!
//! - 提供类型安全的 mmap/munmap API
//! - VFS 交互: fd → inode_id 解析 (属于 services 层职责)
//! - 参数验证与类型转换

use crate::kernel::framework::syscall::mmap as fw_mmap;
use crate::kernel::framework::syscall::types::Errno;
use crate::kernel::framework::mm::vma::MmStruct;

// ============================================================================
// mmap 标志位 re-export
// ============================================================================

/// MAP_SHARED: 写入回写文件
pub const MAP_SHARED: i32 = 0x01;
/// MAP_PRIVATE: 写入触发 COW, 不回写文件
pub const MAP_PRIVATE: i32 = 0x02;
/// MAP_ANONYMOUS: 匿名映射 (无文件后端)
pub const MAP_ANONYMOUS: i32 = 0x20;
/// MAP_FIXED: 强制使用指定地址
pub const MAP_FIXED: i32 = 0x10;

// ============================================================================
// VFS 交互 (services 层职责)
// ============================================================================

/// 从 fd 获取 inode_id
///
/// 通过进程文件描述符表查找对应的 inode 编号.
/// 此函数属于 services 层, 因为它涉及 VFS fdtable 查找,
/// 而 VFS 是 services 层管理的资源.
///
/// 当前简化实现: fd 直接映射为 inode_id + 1 (避免 0).
/// 后续集成完整 VFS fdtable 后替换.
pub fn fd_to_inode_id(fd: i32) -> u32 {
    if fd < 0 {
        return 0;
    }
    // TODO(TRACK-5B3EBC): 从当前进程的 fdtable 获取 inode_id
    // 当前简化: fd + 1 作为 inode_id (0 表示无效)
    (fd as u32).wrapping_add(1)
}

// ============================================================================
// mmap 安全 API
// ============================================================================

/// mmap 系统调用安全代理
///
/// 参数验证 + VFS 交互 + 委托 framework 层.
///
/// ## pwm 桥接
///
/// `pwm` 为创建该映射的进程凭证. framework 层在 #PF 同步填 pcache 时
/// 通过 `vfs_pread_inode(.., pwm)` 校验文件访问权限. 调用方应传入
/// 进程当前凭证 (例如 `credo::pwm_get_current()`), 不传时退化为
/// resolve_pwm(0) → TEST_PWM (仅 initramfs 场景安全).
pub fn mmap_syscall(
    mm: &MmStruct,
    addr_hint: u64,
    length: u64,
    prot: i32,
    flags: i32,
    fd: i32,
    offset: u64,
    pwm: u64,
) -> Result<usize, Errno> {
    // 参数验证
    if length == 0 {
        return Err(Errno::EINVAL);
    }

    let map_anonymous = (flags & MAP_ANONYMOUS) != 0;

    // 文件映射: 在 services 层解析 fd → inode_id
    if !map_anonymous && fd >= 0 {
        let inode_id = fd_to_inode_id(fd);
        if inode_id == 0 {
            return Err(Errno::EBADF);
        }
        // inode_id 已在 services 层解析, framework 层直接使用
    }

    // 委托 framework 层执行底层映射, 透传 pwm
    fw_mmap::mmap_syscall(mm, addr_hint, length, prot, flags, fd, offset, pwm)
}

/// munmap 系统调用安全代理
pub fn munmap_syscall(mm: &MmStruct, addr: u64, length: u64) -> Result<(), Errno> {
    if addr == 0 || length == 0 {
        return Err(Errno::EINVAL);
    }
    fw_mmap::munmap_syscall(mm, addr, length)
}

/// mprotect 系统调用安全代理
pub fn mprotect_syscall(mm: &MmStruct, addr: u64, length: u64, prot: i32) -> Result<(), Errno> {
    if addr == 0 || length == 0 {
        return Err(Errno::EINVAL);
    }
    fw_mmap::mprotect_syscall(mm, addr, length, prot)
}
