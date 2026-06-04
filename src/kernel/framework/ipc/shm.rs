//! 共享内存 (Shared Memory) 实现
//!
//! 提供高效的进程间大数据传输能力
//! 功能等价于 POSIX shmget/shmat/shmdt

use super::types::*;
use crate::kernel::framework::userptr::UserRefMut;
use crate::kernel::framework::proc::api::process_get_current_pid;

/// 查找空闲共享内存段槽位
fn shm_find_free(namespace: &mut IpcNamespace) -> Option<&mut ShmSegment> {
    namespace.shm_segs.iter_mut().find(|s| s.id == 0)
}

/// 根据 ID 查找共享内存段
fn shm_find_by_id(namespace: &mut IpcNamespace, id: IpcId) -> Option<&mut ShmSegment> {
    namespace.shm_segs.iter_mut().find(|s| s.id == id)
}

/// 创建共享内存段 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `next_id` - 全局 ID 分配器
/// * `size` - 段大小 (字节)
/// * `perm` - 权限标志
/// * `current_pid` - 当前进程 PID
///
/// # Returns
/// * Ok(IpcId) - 成功，返回共享内存 ID
/// * Err(i32) - 失败 (-1: 参数无效, -2: 无可用槽位, -3: 内存分配失败)
pub fn shm_create_safe(
    namespace: &mut IpcNamespace,
    next_id: &mut IpcId,
    size: u64,
    perm: i32,
    current_pid: u32,
) -> Result<IpcId, i32> {
    // 参数校验
    if size == 0 || size > SHM_MAX_SIZE {
        return Err(-1);
    }

    // 查找空闲槽位
    let shm = match shm_find_free(namespace) {
        Some(s) => s,
        None => return Err(-2),
    };

    // 计算需要的页数并分配物理内存
    let pages = size.div_ceil(4096);
    let phys = crate::kernel::framework::mm::api::pmm_alloc_pages(pages as usize);
    if phys.is_null() {
        return Err(-3);
    }

    // 初始化共享内存段
    shm.id = *next_id;
    *next_id += 1;

    shm.phys_addr = phys as u64;
    shm.size = size;
    shm.creator = current_pid;
    shm.ref_count = 0;
    shm.attach_count = 0;
    shm.flags = 0;
    shm.perm = perm;
    shm.attached_pids = [0u32; 16];

    Ok(shm.id)
}

/// 将共享内存段附加到当前进程地址空间 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `id` - 共享内存 ID
/// * `current_pid` - 当前进程 PID
///
/// # Returns
/// * Ok(u64) - 成功，返回物理地址
/// * Err(i32) - 错误码
pub fn shm_attach_safe(
    namespace: &mut IpcNamespace,
    id: IpcId,
    current_pid: u32,
) -> Result<u64, i32> {
    let shm = match shm_find_by_id(namespace, id) {
        Some(s) => s,
        None => return Err(-1), // 无效 ID
    };

    // 检查是否已经附加过
    for i in 0..shm.attach_count as usize {
        if shm.attached_pids[i] == current_pid {
            return Ok(shm.phys_addr); // 已附加，直接返回地址
        }
    }

    // 检查是否超过最大附加进程数
    if shm.attach_count >= 16 {
        return Err(-2); // 超出限制
    }

    // 记录附加信息
    shm.attached_pids[shm.attach_count as usize] = current_pid;
    shm.attach_count += 1;
    shm.ref_count += 1;

    Ok(shm.phys_addr)
}

/// 从当前进程分离共享内存段 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `id` - 共享内存 ID
/// * `current_pid` - 当前进程 PID
///
/// # Returns
/// * Ok(()) - 成功
/// * Err(i32) - 错误码
pub fn shm_detach_safe(
    namespace: &mut IpcNamespace,
    id: IpcId,
    current_pid: u32,
) -> Result<(), i32> {
    let shm = match shm_find_by_id(namespace, id) {
        Some(s) => s,
        None => return Err(-1),
    };

    // 查找并移除当前进程的附加记录
    for i in 0..shm.attach_count as usize {
        if shm.attached_pids[i] == current_pid {
            shm.attached_pids[i] = shm.attached_pids[shm.attach_count as usize - 1];
            shm.attached_pids[shm.attach_count as usize - 1] = 0;
            shm.attach_count -= 1;
            shm.ref_count -= 1;
            return Ok(());
        }
    }

    Err(-2) // 进程未附加此段
}

/// 销毁共享内存段 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `id` - 共享内存 ID
///
/// # Returns
/// * Ok(()) - 成功
/// * Err(i32) - 错误码 (-1: 无效 ID, -2: 仍有进程附加)
pub fn shm_destroy_safe(namespace: &mut IpcNamespace, id: IpcId) -> Result<(), i32> {
    let shm = match shm_find_by_id(namespace, id) {
        Some(s) => s,
        None => return Err(-1),
    };

    // 检查是否有进程仍附加
    if shm.ref_count > 0 {
        return Err(-2);
    }

    // 释放物理页
    let pages = shm.size.div_ceil(4096);
    crate::kernel::framework::mm::api::pmm_free_pages(shm.phys_addr as *mut u8, pages as usize);

    // 清理结构体
    shm.id = 0;
    shm.phys_addr = 0;
    shm.size = 0;

    Ok(())
}

// ============================================================================
// FFI 导出函数
// ============================================================================

/// FFI: 创建共享内存段
#[no_mangle]
pub fn ipc_shm_create(size: u64, perm: i32) -> IpcId {
    let ns = super::IPC_NAMESPACE.get_mut();
    let next_id = super::NEXT_IPC_ID.get_mut();
    let pid = process_get_current_pid();
    match shm_create_safe(ns, next_id, size, perm, pid) {
        Ok(id) => id,
        Err(_) => 0,
    }
}

/// FFI: 附加共享内存段。
///
/// # Safety
/// `addr` 必须是有效可写指针, 用于返回映射的虚拟地址。
/// 由 `sys_shmat` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe fn ipc_shm_attach(id: IpcId, addr: *mut *mut u8) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    let pid = process_get_current_pid();
    match shm_attach_safe(ns, id, pid) {
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
#[no_mangle]
pub fn ipc_shm_detach(id: IpcId) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    let pid = process_get_current_pid();
    match shm_detach_safe(ns, id, pid) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// FFI: 销毁共享内存段
#[no_mangle]
pub fn ipc_shm_destroy(id: IpcId) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    match shm_destroy_safe(ns, id) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}