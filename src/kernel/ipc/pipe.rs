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
//! - FFI 函数中的 `unsafe` 块仅用于：访问全局 `IPC_NAMESPACE` 静态可变变量、
//!   从 C 指针构造切片。

use super::types::*;
use crate::kernel::proc::api::process_get_current_pid;

static PIPE_LOCK: spin::Mutex<()> = spin::Mutex::new(());

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

#[no_mangle]
pub fn ipc_pipe_create(pipefd: *mut i32) -> i32 {
    if pipefd.is_null() {
        return -1;
    }

    // SAFETY: IPC_NAMESPACE is a static mut accessed only under PIPE_LOCK;
    // pipefd is validated non-null above; add(0)/add(1) stay within the
    // two-int array that the caller must provide.
    unsafe {
        use crate::kernel::ipc::{IPC_NAMESPACE, NEXT_IPC_ID};

        let current_pid = process_get_current_pid();

        match pipe_create_safe(&mut IPC_NAMESPACE, &mut NEXT_IPC_ID, current_pid) {
            Ok((rfd, wfd)) => {
                *pipefd.add(0) = rfd;
                *pipefd.add(1) = wfd;
                0
            }
            Err(_) => -1,
        }
    }
}

#[no_mangle]
pub fn ipc_pipe_read(fd: i32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    // SAFETY: buf is validated non-null; count matches the valid region;
    // IPC_NAMESPACE access is serialized by PIPE_LOCK internally.
    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;

        let slice = core::slice::from_raw_parts_mut(buf, count as usize);
        match pipe_read_safe(&mut IPC_NAMESPACE, fd, slice, count) {
            Ok(n) => n as i32,
            Err(_) => -1,
        }
    }
}

#[no_mangle]
pub fn ipc_pipe_write(fd: i32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    // SAFETY: buf is validated non-null; count matches the valid region;
    // IPC_NAMESPACE access is serialized by PIPE_LOCK internally.
    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;

        let slice = core::slice::from_raw_parts(buf, count as usize);
        match pipe_write_safe(&mut IPC_NAMESPACE, fd, slice, count) {
            Ok(n) => n as i32,
            Err(_) => -1,
        }
    }
}

#[no_mangle]
pub fn ipc_pipe_close(fd: i32) -> i32 {
    // SAFETY: IPC_NAMESPACE access is serialized by PIPE_LOCK internally.
    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;

        match pipe_close_safe(&mut IPC_NAMESPACE, fd) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}
