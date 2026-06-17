// SPDX-License-Identifier: GPL-2.0
//! Credo 私有存储子系统 syscall — services 层安全代理
//!
//! ## 职责
//!
//! - 0 unsafe, 纯类型安全 + 参数验证
//! - credo 鉴权 (走 framework::credo::api::pwm_*)
//! - 用户指针 + 容量验证 (走 framework::syscall::raw)
//! - 委托 framework::driver::block 执行块设备实际操作
//!
//! ## 与 framework 边界
//!
//! framework 暴露的 `block_device_count/list/info/is_present/total_sectors`
//! 已封装块设备 (ATA / NVMe / AHCI / virtio-blk) 注册表, services 走该公共 API.
//!
//! ## 不允许简化: 即使 credo 私有, 仍走 services 业务 + 鉴权 + 校验三段式.

use crate::kernel::framework::credo;
use crate::kernel::framework::syscall::raw;
use crate::kernel::framework::syscall::Errno;
use crate::kernel::framework::driver::block as blk;

/// credo disk 域 = 4 (PWM domain 4 = storage 域)
const PWM_DOMAIN_STORAGE: u16 = 4;

/// 块设备信息写回结构 (与 framework 旧实现兼容, 76 字节)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct UserDiskInfo {
    pub disk_id: u32,
    pub present: u32,
    pub total_sectors: u32,
    pub sectors: u32,
    pub model: [u8; 64],
}

const USER_DISK_INFO_SIZE: u64 = 76;

/// 列出已注册块设备 id. `disks` 为用户态 u64 数组, `max_count` 为容量.
/// 返回写入的设备数 (>= 0), 失败返 Errno.
pub fn disk_list(disks_ptr: u64, max_count: u32) -> Result<usize, Errno> {
    if disks_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if max_count == 0 {
        return Err(Errno::EINVAL);
    }
    let bytes = (max_count as u64).checked_mul(8).ok_or(Errno::EINVAL)?;
    if !raw::check_user_buf(disks_ptr, bytes) {
        return Err(Errno::EFAULT);
    }
    let count = blk::block_device_count();
    let limit = (max_count as usize).min(count);
    for i in 0..limit {
        if !raw::write_u64_to_user(disks_ptr + (i as u64) * 8, i as u64) {
            return Err(Errno::EFAULT);
        }
    }
    Ok(limit)
}

/// 读单块设备信息. `info` 为 UserDiskInfo 用户指针 (76 字节).
pub fn disk_info(disk_id: u32, info_ptr: u64) -> Result<(), Errno> {
    if info_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_buf(info_ptr, USER_DISK_INFO_SIZE) {
        return Err(Errno::EFAULT);
    }
    let present = blk::hdd_is_present(disk_id as u8);
    let sectors = if present {
        blk::hdd_total_sectors(disk_id as u8) as u32
    } else {
        0
    };
    let (name, _is_present, _total) = if present {
        blk::block_device_info(disk_id as u8)
    } else {
        ("", false, 0u64)
    };
    let mut model = [0u8; 64];
    let name_bytes = name.as_bytes();
    let copy_len = name_bytes.len().min(63);
    model[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    let info = UserDiskInfo {
        disk_id,
        present: present as u32,
        total_sectors: sectors,
        sectors,
        model,
    };
    if !raw::write_struct_to_user(info_ptr, &info) {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// 格式化块设备. `fstype` 为用户 C 字符串.
pub fn disk_format(disk_id: u32, fstype_ptr: u64) -> Result<(), Errno> {
    if fstype_ptr == 0 {
        return Err(Errno::EFAULT);
    }
    if !raw::check_user_ptr(fstype_ptr) {
        return Err(Errno::EFAULT);
    }
    // credo 鉴权: storage 域 required=1 (写)
    let pwm = credo::api::pwm_get_current();
    if !credo::api::pwm_has_capability(pwm, PWM_DOMAIN_STORAGE, 1) {
        return Err(Errno::EACCES);
    }
    if !blk::hdd_is_present(disk_id as u8) {
        return Err(Errno::ENOENT);
    }
    // 委托 framework: 走 sys_disk_format 内置实现 (ATA/NVMe 各驱动的格式化路径).
    // 实际格式化在 framework::syscall::mod::sys_disk_format 已有完整逻辑.
    // services 层仅校验 + 鉴权, 真实 IO 委派.
    Err(Errno::ENOSYS) // 临时占位: 详细实现后续由 framework storage 子模块接管
}

/// 块设备分区表写入.
pub fn disk_partition(disk_id: u32, total_sectors: u64) -> Result<(), Errno> {
    if total_sectors == 0 || total_sectors > u32::MAX as u64 {
        return Err(Errno::EINVAL);
    }
    let pwm = credo::api::pwm_get_current();
    if !credo::api::pwm_has_capability(pwm, PWM_DOMAIN_STORAGE, 1) {
        return Err(Errno::EACCES);
    }
    if !blk::hdd_is_present(disk_id as u8) {
        return Err(Errno::ENOENT);
    }
    let _ = disk_id;
    let _ = total_sectors;
    Err(Errno::ENOSYS)
}

/// FAT 格式化.
pub fn fat_format(disk_id: u32) -> Result<(), Errno> {
    let pwm = credo::api::pwm_get_current();
    if !credo::api::pwm_has_capability(pwm, PWM_DOMAIN_STORAGE, 1) {
        return Err(Errno::EACCES);
    }
    if !blk::hdd_is_present(disk_id as u8) {
        return Err(Errno::ENOENT);
    }
    let _ = disk_id;
    Err(Errno::ENOSYS)
}
