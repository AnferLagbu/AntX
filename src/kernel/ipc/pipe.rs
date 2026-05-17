//! 管道 (Pipe) 实现
//!
//! 提供进程间字节流通信能力，支持阻塞式读写
//! 功能等价于 Linux 的 pipe() 系统调用

use super::types::*;

/// 查找空闲管道槽位
///
/// # Returns
/// * Some(&mut Pipe) - 找到空闲槽位
/// * None - 无可用槽位
unsafe fn pipe_find_free(namespace: &mut IpcNamespace) -> Option<&'static mut Pipe> {
    for i in 0..IPC_MAX_PIPES {
        if namespace.pipes[i].id == 0 {
            return Some(&mut *(&mut namespace.pipes[i] as *mut Pipe));
        }
    }
    None
}

/// 根据文件描述符查找管道
///
/// # Arguments
/// * `fd` - 文件描述符 (read_fd 或 write_fd)
///
/// # Returns
/// * Some(&mut Pipe) - 找到匹配的管道
/// * None - 未找到
unsafe fn pipe_find_by_fd(namespace: &mut IpcNamespace, fd: i32) -> Option<&'static mut Pipe> {
    for i in 0..IPC_MAX_PIPES {
        let pipe = &mut namespace.pipes[i];
        if pipe.id != 0 && (pipe.read_fd == fd || pipe.write_fd == fd) {
            return Some(&mut *(pipe as *mut Pipe));
        }
    }
    None
}

/// 创建管道 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `current_pid` - 当前进程 PID
///
/// # Returns
/// * Ok((i32, i32)) - 成功，返回 (read_fd, write_fd)
/// * Err(i32) - 失败，返回错误码 (-1 表示无可用槽位)
pub fn pipe_create_safe(
    namespace: &mut IpcNamespace,
    next_id: &mut IpcId,
    current_pid: u32,
) -> Result<(i32, i32), i32> {
    unsafe {
        let pipe = match pipe_find_free(namespace) {
            Some(p) => p,
            None => return Err(-1),
        };

        // 初始化管道
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

        pipe.read_wait.init();
        pipe.write_wait.init();

        Ok((pipe.read_fd, pipe.write_fd))
    }
}

/// 从管道读取数据 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `fd` - 文件描述符 (必须是 read_fd)
/// * `buf` - 目标缓冲区
/// * `count` - 最大读取字节数
///
/// # Returns
/// * Ok(u32) - 实际读取的字节数
/// * Err(i32) - 错误码 (-1: 无效 fd, -2: 非读端)
pub fn pipe_read_safe(
    namespace: &mut IpcNamespace,
    fd: i32,
    buf: &mut [u8],
    count: u32,
) -> Result<u32, i32> {
    if count == 0 {
        return Ok(0);
    }

    unsafe {
        let pipe = match pipe_find_by_fd(namespace, fd) {
            Some(p) => p,
            None => return Err(-1),
        };

        // 检查是否是读端
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

            // 读取一个字节
            buf[read_count as usize] = pipe.buffer[pipe.read_pos as usize];
            pipe.read_pos = (pipe.read_pos + 1) % PIPE_BUFFER_SIZE as u32;
            pipe.count -= 1;
            read_count += 1;

            // 唤醒写者
            if pipe.write_wait.count() > 0 {
                pipe.write_wait.wake_one();
            }
        }

        Ok(read_count)
    }
}

/// 向管道写入数据 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `fd` - 文件描述符 (必须是 write_fd)
/// * `buf` - 源数据缓冲区
/// * `count` - 要写入的字节数
///
/// # Returns
/// * Ok(u32) - 实际写入的字节数
/// * Err(i32) - 错误码
pub fn pipe_write_safe(
    namespace: &mut IpcNamespace,
    fd: i32,
    buf: &[u8],
    count: u32,
) -> Result<u32, i32> {
    if count == 0 {
        return Ok(0);
    }

    unsafe {
        let pipe = match pipe_find_by_fd(namespace, fd) {
            Some(p) => p,
            None => return Err(-1),
        };

        // 检查是否是写端
        if fd != pipe.write_fd {
            return Err(-2);
        }

        // 检查是否有读者
        if pipe.readers == 0 {
            return Err(-3);  // SIGPIPE
        }

        let mut written: u32 = 0;

        while written < count {
            if pipe.count >= PIPE_BUFFER_SIZE as u32 {
                return Err(-4);
            }

            // 写入一个字节
            pipe.buffer[pipe.write_pos as usize] = buf[written as usize];
            pipe.write_pos = (pipe.write_pos + 1) % PIPE_BUFFER_SIZE as u32;
            pipe.count += 1;
            written += 1;

            // 唤醒读者
            if pipe.read_wait.count() > 0 {
                pipe.read_wait.wake_one();
            }
        }

        Ok(written)
    }
}

/// 关闭管道的一端 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `fd` - 要关闭的文件描述符
///
/// # Returns
/// * Ok(()) - 成功
/// * Err(i32) - 错误码 (-1: 无效 fd)
pub fn pipe_close_safe(namespace: &mut IpcNamespace, fd: i32) -> Result<(), i32> {
    unsafe {
        let pipe = match pipe_find_by_fd(namespace, fd) {
            Some(p) => p,
            None => return Err(-1),
        };

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

        // 如果两端都关闭，释放管道资源
        if pipe.readers == 0 && pipe.writers == 0 {
            pipe.id = 0;
            pipe.read_fd = 0;
            pipe.write_fd = 0;
        }

        Ok(())
    }
}

// ============================================================================
// FFI 导出函数 (C 兼容接口)
// ============================================================================

/// FFI: 创建管道
#[no_mangle]
pub extern "C" fn ipc_pipe_create(pipefd: *mut i32) -> i32 {
    if pipefd.is_null() {
        return -1;
    }

    unsafe {
        use crate::kernel::ipc::{IPC_NAMESPACE, NEXT_IPC_ID};
        
        extern "C" { fn process_get_current_pid() -> u32; }
        let current_pid = process_get_current_pid();

        match pipe_create_safe(&mut IPC_NAMESPACE, &mut NEXT_IPC_ID, current_pid) {
            Ok((rfd, wfd)) => {
                *pipefd.add(0) = rfd;
                *pipefd.add(1) = wfd;
                0
            },
            Err(_) => -1,
        }
    }
}

/// FFI: 从管道读取
#[no_mangle]
pub extern "C" fn ipc_pipe_read(fd: i32, buf: *mut u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;
        
        let slice = core::slice::from_raw_parts_mut(buf, count as usize);
        match pipe_read_safe(&mut IPC_NAMESPACE, fd, slice, count) {
            Ok(n) => n as i32,
            Err(_) => -1,
        }
    }
}

/// FFI: 向管道写入
#[no_mangle]
pub extern "C" fn ipc_pipe_write(fd: i32, buf: *const u8, count: u32) -> i32 {
    if buf.is_null() || count == 0 {
        return -1;
    }

    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;
        
        let slice = core::slice::from_raw_parts(buf, count as usize);
        match pipe_write_safe(&mut IPC_NAMESPACE, fd, slice, count) {
            Ok(n) => n as i32,
            Err(_) => -1,
        }
    }
}

/// FFI: 关闭管道
#[no_mangle]
pub extern "C" fn ipc_pipe_close(fd: i32) -> i32 {
    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;
        
        match pipe_close_safe(&mut IPC_NAMESPACE, fd) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}
