//! 管道 (Pipe) 实现
//!
//! 提供进程间字节流通信能力，支持阻塞式读写
//! 功能等价于 Linux 的 pipe() 系统调用
//!
//! ## SAFETY
//!
//! - `pipe_find_free_index` / `pipe_find_by_fd_index` 是安全函数，
//!   仅返回数组索引，不涉及裸指针转换。
//! - 所有对 `namespace.pipes[idx]` 的访问在 `PIPE_LOCK` 保护下进行，
//!   防止并发竞争。
//! - FFI 函数通过 `RacyCell::get_mut()` 安全访问全局 IPC_NAMESPACE。

use super::types::*;
use super::IPC_NAMESPACE;
use crate::kernel::framework::userptr::{UserReadPtr, UserRefMut, UserWritePtr};
use crate::kernel::framework::proc::process_get_current_pid;


use crate::kernel::framework::sync::IrqSpinLock;
static PIPE_LOCK: IrqSpinLock<()> = IrqSpinLock::new(());

fn pipe_find_free_index(namespace: &IpcNamespace) -> Option<usize> {
    for i in 0..IPC_MAX_PIPES {
        if namespace.pipes[i].id == 0 {
            return Some(i);
        }
    }
    None
}

fn pipe_find_by_fd_index(namespace: &IpcNamespace, fd: i32) -> Option<usize> {
    for i in 0..IPC_MAX_PIPES {
        let pipe = &namespace.pipes[i];
        if pipe.id != 0 && (pipe.read_fd == fd || pipe.write_fd == fd) {
            return Some(i);
        }
    }
    None
}

/// 判断 fd 是否为 pipe fd (公开接口, 供 sendfile/splice 使用)
pub fn is_pipe_fd(fd: i32) -> bool {
    let ns = IPC_NAMESPACE.get_mut();
    pipe_find_by_fd_index(ns, fd).is_some()
}

pub fn pipe_create_safe(
    namespace: &mut IpcNamespace,
    next_id: &mut IpcId,
    current_pid: u32,
) -> Result<(i32, i32), i32> {
    let _lock = PIPE_LOCK.lock();

    let idx = match pipe_find_free_index(namespace) {
        Some(i) => i,
        None => return Err(-1),
    };

    let pipe = &mut namespace.pipes[idx];

    pipe.id = *next_id;
    *next_id += 1;

    pipe.buffer = [0u8; PIPE_BUFFER_SIZE];
    pipe.read_pos = 0;
    pipe.write_pos = 0;
    pipe.count = 0;

    pipe.read_pid = current_pid;
    pipe.write_pid = current_pid;

    pipe.read_fd = (pipe.id * 2) as i32;
    pipe.write_fd = (pipe.id * 2 + 1) as i32;

    pipe.readers = 1;
    pipe.writers = 1;
    pipe.flags = 0;

    // SAFETY: WaitQueue::init 只在内核初始化阶段或锁保护下调用，
    // 此时 PIPE_LOCK 已持有，无并发访问。
    pipe.read_wait.init();
    pipe.write_wait.init();

    Ok((pipe.read_fd, pipe.write_fd))
}

pub fn pipe_read_safe(
    namespace: &mut IpcNamespace,
    fd: i32,
    buf: &mut [u8],
    count: u32,
) -> Result<u32, i32> {
    if count == 0 {
        return Ok(0);
    }

    let _lock = PIPE_LOCK.lock();

    let idx = match pipe_find_by_fd_index(namespace, fd) {
        Some(i) => i,
        None => return Err(-1),
    };

    let pipe = &mut namespace.pipes[idx];

    if fd != pipe.read_fd {
        return Err(-2);
    }

    let mut read_count: u32 = 0;

    while read_count < count {
        if pipe.count == 0 {
            if pipe.writers == 0 {
                break;
            }
            if read_count > 0 {
                break;
            }
            return Err(-4);
        }

        buf[read_count as usize] = pipe.buffer[pipe.read_pos as usize];
        pipe.read_pos = (pipe.read_pos + 1) % PIPE_BUFFER_SIZE as u32;
        pipe.count -= 1;
        read_count += 1;

        if pipe.write_wait.count() > 0 {
            pipe.write_wait.wake_one();
        }
    }

    Ok(read_count)
}

pub fn pipe_write_safe(
    namespace: &mut IpcNamespace,
    fd: i32,
    buf: &[u8],
    count: u32,
) -> Result<u32, i32> {
    if count == 0 {
        return Ok(0);
    }

    let _lock = PIPE_LOCK.lock();

    let idx = match pipe_find_by_fd_index(namespace, fd) {
        Some(i) => i,
        None => return Err(-1),
    };

    let pipe = &mut namespace.pipes[idx];

    if fd != pipe.write_fd {
        return Err(-2);
    }

    if pipe.readers == 0 {
        return Err(-3);
    }

    let mut written: u32 = 0;

    while written < count {
        if pipe.count >= PIPE_BUFFER_SIZE as u32 {
            return Err(-4);
        }

        pipe.buffer[pipe.write_pos as usize] = buf[written as usize];
        pipe.write_pos = (pipe.write_pos + 1) % PIPE_BUFFER_SIZE as u32;
        pipe.count += 1;
        written += 1;

        if pipe.read_wait.count() > 0 {
            pipe.read_wait.wake_one();
        }
    }

    Ok(written)
}

pub fn pipe_close_safe(namespace: &mut IpcNamespace, fd: i32) -> Result<(), i32> {
    let _lock = PIPE_LOCK.lock();

    let idx = match pipe_find_by_fd_index(namespace, fd) {
        Some(i) => i,
        None => return Err(-1),
    };

    let pipe = &mut namespace.pipes[idx];

    if fd == pipe.read_fd {
        pipe.readers -= 1;
        pipe.read_fd = 0;
        if pipe.readers == 0 {
            pipe.write_wait.wake_all();
        }
    } else if fd == pipe.write_fd {
        pipe.writers -= 1;
        pipe.write_fd = 0;
        if pipe.writers == 0 {
            pipe.read_wait.wake_all();
        }
    }

    if pipe.readers == 0 && pipe.writers == 0 {
        pipe.id = 0;
        pipe.read_fd = 0;
        pipe.write_fd = 0;
    }

    Ok(())
}

/// POSIX `pipe(pipefd)` 内核实现。
///
/// # Safety
/// `pipefd` 必须是可写指针, 含至少 2 个 `i32` 空间 (用于返回 [read_fd, write_fd])。
/// 由 `sys_pipe` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe fn ipc_pipe_create(pipefd: *mut i32) -> i32 {
    if pipefd.is_null() {
        return -1;
    }

    let ns = super::IPC_NAMESPACE.get_mut();
    let next_id = super::NEXT_IPC_ID.get_mut();
    let current_pid = process_get_current_pid();

    match pipe_create_safe(ns, next_id, current_pid) {
        Ok((rfd, wfd)) => {
            // SAFETY: pipefd 已校验非空; 调用方保证其指向用户态内存中
            // 至少 2 个有效的 i32 值.
            let mut fds = unsafe { UserRefMut::<[i32; 2]>::new(pipefd as *mut [i32; 2]) };
            let arr = fds.as_mut();
            arr[0] = rfd;
            arr[1] = wfd;
            0
        }
        Err(_) => -1,
    }
}

/// POSIX `read(fd, buf, count)` 内核实现 (仅 pipe fd)。
///
/// # Safety
/// `buf` 必须是有效可写指针, 至少 `count` 字节, 内存必须在调用期间保持有效。
/// 由 `sys_read` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe fn ipc_pipe_read(fd: i32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    let ns = super::IPC_NAMESPACE.get_mut();
    // SAFETY: buf 已校验非空; 调用方保证其指向用户态内存中
    // 至少 `count` 个有效字节.
    let mut user_buf = unsafe { UserWritePtr::new(buf, count as usize) };
    match pipe_read_safe(ns, fd, user_buf.as_mut_slice(), count) {
        Ok(n) => n as i32,
        Err(_) => -1,
    }
}

/// POSIX `write(fd, buf, count)` 内核实现 (仅 pipe fd)。
///
/// # Safety
/// `buf` 必须是有效可读指针, 至少 `count` 字节, 内存必须在调用期间保持有效。
/// 由 `sys_write` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe fn ipc_pipe_write(fd: i32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    let ns = super::IPC_NAMESPACE.get_mut();
    // SAFETY: buf 已校验非空; 调用方保证其指向用户态内存中
    // 至少 `count` 个有效字节.
    let user_buf = unsafe { UserReadPtr::new(buf, count as usize) };
    match pipe_write_safe(ns, fd, user_buf.as_slice(), count) {
        Ok(n) => n as i32,
        Err(_) => -1,
    }
}

#[no_mangle]
pub fn ipc_pipe_close(fd: i32) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    match pipe_close_safe(ns, fd) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}