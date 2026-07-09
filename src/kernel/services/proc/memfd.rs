//! memfd_create 系统调用实现
//!
//! 创建匿名内存文件，可用于 mmap 共享内存。
//! 简化实现: 在 tmpfs 中创建临时文件。

use crate::kernel::framework::syscall::types::Errno;

/// MFD_CLOEXEC 标志位
const MFD_CLOEXEC: u32 = 0x0001;
/// MFD_ALLOW_SEALING 标志位
const MFD_ALLOW_SEALING: u32 = 0x0002;
/// MFD_HUGE_16GB 标志位 (简化: 不支持大页)
const MFD_HUGE_MASK: u32 = 0x3F << 26;

/// memfd_create — 创建匿名内存文件
pub fn memfd_create_syscall(_name_ptr: u64, flags: u32) -> Result<usize, Errno> {
    // 检查 flags 有效性
    let supported_flags = MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_HUGE_MASK;
    if flags & !supported_flags != 0 {
        return Err(Errno::EINVAL);
    }

    // 检查是否支持大页 (暂不支持)
    if flags & MFD_HUGE_MASK != 0 {
        return Err(Errno::EINVAL);
    }

    // 构造路径: /dev/shm/memfd_<pid>
    let current_pid = crate::kernel::framework::proc::process_get_current_pid();
    let path = alloc::format!("/dev/shm/memfd_{}", current_pid);

    // 获取当前 PWM
    let pwm = crate::kernel::framework::credo::session::get_current_pwm();

    // 尝试在 tmpfs 中创建文件
    let fd = crate::kernel::framework::fs::api::vfs_open(
        path.as_ptr() as *const u8,
        0x241, // O_RDWR | O_CREAT | O_EXCL
        pwm,
    );

    if fd < 0 {
        return Err(Errno::ENOSYS);
    }

    let _ = flags & MFD_CLOEXEC;
    // TODO: 设置 fd 的 CLOEXEC 标记
    Ok(fd as usize)
}
