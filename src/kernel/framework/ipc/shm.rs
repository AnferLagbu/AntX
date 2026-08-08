//! 共享内存 (SHM) FFI 边界 — T6-1 策略已迁移至 services/ipc/shm.rs
//!
//! 本模块仅保留 FFI 函数 (用户空间指针转换, 委托 services 策略).
//!
//! ## SAFETY
//!
//! - FFI 函数通过 `RacyCell::get_mut()` 安全访问全局 IPC_NAMESPACE.
//! - 用户空间指针通过 `UserRefMut` 安全访问.

use super::types::IpcId;
use crate::kernel::framework::proc::process_get_current_pid;
use crate::kernel::framework::userptr::UserRefMut;

// ============================================================================
// FFI 导出函数
// ============================================================================

/// FFI: 创建共享内存段
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn ipc_shm_create(size: u64, perm: i32) -> IpcId {
    let ns = super::IPC_NAMESPACE.get_mut();
    let next_id = super::NEXT_IPC_ID.get_mut();
    let pid = process_get_current_pid();
    crate::kernel::services::ipc::shm::shm_create_safe(ns, next_id, size, perm, pid).unwrap_or(0)
}

/// FFI: 附加共享内存段。
///
/// # Safety
/// `addr` 必须是有效可写指针, 用于返回映射的虚拟地址。
/// 由 `sys_shmat` 分发, cred 校验已通过。
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ipc_shm_attach(id: IpcId, addr: *mut *mut u8) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    let pid = process_get_current_pid();
    #[expect(
        clippy::option_if_let_else,
        reason = "含 unsafe { UserRefMut::new(addr); *out.as_mut() = phys_addr } userptr 副作用, 改 map_or 触发冗余闭包, 保留 match 形式"
    )]
    match crate::kernel::services::ipc::shm::shm_attach_safe(ns, id, pid) {
        Ok(phys_addr) => {
            if !addr.is_null() {
                // SAFETY: caller guarantees addr is a valid pointer to
                // a *mut u8 in user memory.
                let mut out = unsafe { UserRefMut::<*mut u8>::new(addr) };
                *out.as_mut() = phys_addr as *mut u8;
            }
            0
        }
        Err(_) => -1,
    }
}

/// FFI: 分离共享内存段
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn ipc_shm_detach(id: IpcId) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    let pid = process_get_current_pid();
    crate::kernel::services::ipc::shm::shm_detach_safe(ns, id, pid).map_or(-1, |()| 0)
}

/// FFI: 销毁共享内存段
// SAFETY: FFI 导出函数，通过 C ABI 与外部代码互操作
#[unsafe(no_mangle)]
pub extern "C" fn ipc_shm_destroy(id: IpcId) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    match crate::kernel::services::ipc::shm::shm_destroy_safe(ns, id) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}
