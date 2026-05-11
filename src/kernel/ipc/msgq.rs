//! 消息队列 (Message Queue) 实现
//!
//! 提供结构化的进程间消息传递能力
//! 功能等价于 System V 消息队列

use super::types::*;
use alloc::alloc::{alloc, dealloc, Layout};

/// 查找空闲消息队列槽位
unsafe fn msgq_find_free(namespace: &mut IpcNamespace) -> Option<&'static mut MsgQueue> {
    for i in 0..IPC_MAX_MSG_QUEUES {
        if namespace.msg_queues[i].id == 0 {
            return Some(&mut *(&mut namespace.msg_queues[i] as *mut MsgQueue));
        }
    }
    None
}

/// 根据 ID 查找消息队列
unsafe fn msgq_find_by_id(namespace: &mut IpcNamespace, id: IpcId) -> Option<&'static mut MsgQueue> {
    for i in 0..IPC_MAX_MSG_QUEUES {
        if namespace.msg_queues[i].id == id {
            return Some(&mut *(&mut namespace.msg_queues[i] as *mut MsgQueue));
        }
    }
    None
}

/// 分配消息结构体 (使用内核堆)
///
/// # Returns
/// * Some(*mut Message) - 成功分配
/// * None - 分配失败
fn allocate_message() -> Option<*mut Message> {
    unsafe {
        let layout = Layout::new::<Message>();
        let ptr = alloc(layout);
        if ptr.is_null() {
            None
        } else {
            Some(ptr as *mut Message)
        }
    }
}

/// 释放消息结构体
unsafe fn free_message(msg: *mut Message) {
    let layout = Layout::new::<Message>();
    dealloc(msg as *mut u8, layout);
}

/// 创建消息队列 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `next_id` - 全局 ID 分配器
/// * `perm` - 权限标志
/// * `current_pid` - 当前进程 PID
///
/// # Returns
/// * Ok(IpcId) - 成功，返回消息队列 ID
/// * Err(i32) - 失败 (-1: 无可用槽位)
pub fn msgq_create_safe(
    namespace: &mut IpcNamespace,
    next_id: &mut IpcId,
    perm: i32,
    current_pid: u32,
) -> Result<IpcId, i32> {
    unsafe {
        let mq = match msgq_find_free(namespace) {
            Some(q) => q,
            None => return Err(-1),
        };

        // 初始化消息队列
        mq.id = *next_id;
        *next_id += 1;

        mq.owner = current_pid;
        mq.head = core::ptr::null_mut();
        mq.tail = core::ptr::null_mut();
        mq.count = 0;
        mq.max_msgs = MSG_QUEUE_MAX_MSGS;
        mq.max_size = MSG_MAX_SIZE as u32;
        mq.flags = 0;
        mq.perm = perm;

        mq.send_wait.init();
        mq.recv_wait.init();

        Ok(mq.id)
    }
}

/// 向消息队列发送消息 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `id` - 消息队列 ID
/// * `type_` - 消息类型 (用户自定义)
/// * `data` - 消息数据指针
/// * `size` - 数据长度
/// * `current_pid` - 发送者 PID
///
/// # Returns
/// * Ok(()) - 成功
/// * Err(i32) - 错误码 (-1: 无效 ID, -2: 消息过大, -3: 队列满, -4: 内存分配失败)
pub fn msgq_send_safe(
    namespace: &mut IpcNamespace,
    id: IpcId,
    type_: u64,
    data: Option<&[u8]>,
    size: usize,
    current_pid: u32,
) -> Result<(), i32> {
    // 参数校验
    if size > MSG_MAX_SIZE {
        return Err(-2);
    }

    unsafe {
        let mq = match msgq_find_by_id(namespace, id) {
            Some(q) => q,
            None => return Err(-1),
        };

        // 检查队列是否已满
        if mq.count >= mq.max_msgs {
            return Err(-3);
        }

        // 分配消息结构体
        let msg = match allocate_message() {
            Some(m) => m,
            None => return Err(-4),
        };

        // 初始化消息
        (*msg).type_ = type_;
        (*msg).sender = current_pid as u64;
        (*msg).size = size as u64;
        (*msg).next = core::ptr::null_mut();
        (*msg).data = [0u8; MSG_MAX_SIZE];

        // 复制数据 (如果有)
        if let Some(src) = data {
            if size > 0 && !src.is_empty() {
                (&mut (*msg).data)[..size].copy_from_slice(&src[..size]);
            }
        }

        // 入队 (尾插法)
        if mq.tail.is_null() {
            mq.head = msg;
            mq.tail = msg;
        } else {
            (*mq.tail).next = msg;
            mq.tail = msg;
        }
        mq.count += 1;

        // 唤醒等待接收的线程
        if mq.recv_wait.count() > 0 {
            mq.recv_wait.wake_one();
        }

        Ok(())
    }
}

/// 从消息队列接收消息 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `id` - 消息队列 ID
/// * `type_out` - 输出: 消息类型 (可为 NULL)
/// * `data_out` - 输出: 消息数据缓冲区 (可为 NULL)
/// * `size_out` - 输出: 实际数据长度 (可为 NULL)
/// * `max_size` - data_out 缓冲区大小
///
/// # Returns
/// * Ok(usize) - 成功，返回实际读取的字节数
/// * Err(i32) - 错误码 (-1: 无效 ID, -2: 队列为空)
pub fn msgq_recv_safe(
    namespace: &mut IpcNamespace,
    id: IpcId,
    type_out: Option<&mut u64>,
    data_out: Option<&mut [u8]>,
    size_out: Option<&mut u64>,
) -> Result<usize, i32> {
    unsafe {
        let mq = match msgq_find_by_id(namespace, id) {
            Some(q) => q,
            None => return Err(-1),
        };

        // 检查队列是否为空
        if mq.head.is_null() {
            return Err(-2);
        }

        // 出队 (头删法)
        let msg = mq.head;
        mq.head = (*msg).next;

        if mq.head.is_null() {
            mq.tail = core::ptr::null_mut();
        }
        mq.count -= 1;

        // 提取消息信息
        let read_size = (*msg).size as usize;

        if let Some(t) = type_out {
            *t = (*msg).type_;
        }

        if let Some(buf) = data_out {
            let copy_len = read_size.min(buf.len());
            if read_size > 0 {
                buf[..copy_len].copy_from_slice(&(&(*msg).data)[..copy_len]);
            }
        }

        if let Some(s) = size_out {
            *s = (*msg).size;
        }

        // 释放消息内存
        free_message(msg);

        // 唤醒等待发送的线程
        if mq.send_wait.count() > 0 {
            mq.send_wait.wake_one();
        }

        Ok(read_size)
    }
}

/// 销毁消息队列 (Rust 安全接口)
///
/// # Arguments
/// * `namespace` - IPC 命名空间引用
/// * `id` - 消息队列 ID
///
/// # Returns
/// * Ok(()) - 成功
/// * Err(i32) - 错误码 (-1: 无效 ID)
pub fn msgq_destroy_safe(namespace: &mut IpcNamespace, id: IpcId) -> Result<(), i32> {
    unsafe {
        let mq = match msgq_find_by_id(namespace, id) {
            Some(q) => q,
            None => return Err(-1),
        };

        // 释放所有剩余消息
        while !mq.head.is_null() {
            let msg = mq.head;
            mq.head = (*msg).next;
            free_message(msg);
        }

        // 清理结构体
        mq.id = 0;

        Ok(())
    }
}

// ============================================================================
// FFI 导出函数
// ============================================================================

/// FFI: 创建消息队列
#[no_mangle]
pub extern "C" fn ipc_msgq_create(perm: i32) -> IpcId {
    unsafe {
        use crate::kernel::ipc::{IPC_NAMESPACE, NEXT_IPC_ID};

        extern "C" { fn process_get_current_pid() -> u32; }
        let pid = process_get_current_pid();

        match msgq_create_safe(&mut IPC_NAMESPACE, &mut NEXT_IPC_ID, perm, pid) {
            Ok(id) => id,
            Err(_) => 0,
        }
    }
}

/// FFI: 发送消息
#[no_mangle]
pub extern "C" fn ipc_msgq_send(id: IpcId, type_: u64, data: *const u8, size: u64) -> i32 {
    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;

        extern "C" { fn process_get_current_pid() -> u32; }
        let pid = process_get_current_pid();

        let slice = if data.is_null() || size == 0 {
            None
        } else {
            Some(core::slice::from_raw_parts(data, size as usize))
        };

        match msgq_send_safe(&mut IPC_NAMESPACE, id, type_, slice, size as usize, pid) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}

/// FFI: 接收消息
#[no_mangle]
pub extern "C" fn ipc_msgq_recv(
    id: IpcId,
    type_out: *mut u64,
    data: *mut u8,
    size_out: *mut u64,
) -> i64 {
    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;

        let type_opt = if type_out.is_null() { None } else { Some(&mut *type_out) };

        let data_opt = if data.is_null() {
            None
        } else {
            Some(core::slice::from_raw_parts_mut(data, MSG_MAX_SIZE))
        };

        let size_opt = if size_out.is_null() { None } else { Some(&mut *size_out) };

        match msgq_recv_safe(&mut IPC_NAMESPACE, id, type_opt, data_opt, size_opt) {
            Ok(n) => n as i64,
            Err(_) => -1,
        }
    }
}

/// FFI: 销毁消息队列
#[no_mangle]
pub extern "C" fn ipc_msgq_destroy(id: IpcId) -> i32 {
    unsafe {
        use crate::kernel::ipc::IPC_NAMESPACE;

        match msgq_destroy_safe(&mut IPC_NAMESPACE, id) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}
