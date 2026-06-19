#![deny(unsafe_code)]
//! 消息队列策略 — T6-1 从 framework/ipc/msgq.rs 提取
//!
//! 纯策略逻辑: 参数校验、资源查找、状态管理、链表操作.
//! 所有 unsafe 操作通过 framework::ipc::msgq::raw (MessageRef) 安全方法完成.

use crate::kernel::framework::ipc::types::*;
use crate::kernel::framework::ipc::msgq::raw::{self, MessageRef};

/// 查找空闲消息队列槽位
pub fn msgq_find_free(namespace: &mut IpcNamespace) -> Option<&mut MsgQueue> {
    namespace.msg_queues.iter_mut().find(|q| q.id == 0)
}

/// 根据 ID 查找消息队列
pub fn msgq_find_by_id(namespace: &mut IpcNamespace, id: IpcId) -> Option<&mut MsgQueue> {
    namespace.msg_queues.iter_mut().find(|q| q.id == id)
}

/// 创建消息队列 (策略: 槽位分配 + 初始化)
pub fn msgq_create_safe(
    namespace: &mut IpcNamespace,
    next_id: &mut IpcId,
    perm: i32,
    current_pid: u32,
) -> Result<IpcId, i32> {
    let mq = match msgq_find_free(namespace) {
        Some(q) => q,
        None => return Err(-1),
    };

    // 初始化消息队列
    mq.id = *next_id;
    *next_id += 1;

    mq.owner = current_pid;
    mq.head = None;
    mq.tail = None;
    mq.count = 0;
    mq.max_msgs = MSG_QUEUE_MAX_MSGS;
    mq.max_size = MSG_MAX_SIZE as u32;
    mq.flags = 0;
    mq.perm = perm;

    mq.send_wait.init();
    mq.recv_wait.init();

    Ok(mq.id)
}

/// 向消息队列发送消息 (策略: 参数校验 + 容量检查 + 入队 + 唤醒)
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

    let mq = match msgq_find_by_id(namespace, id) {
        Some(q) => q,
        None => return Err(-1),
    };

    // 检查队列是否已满
    if mq.count >= mq.max_msgs {
        return Err(-3);
    }

    // 分配消息结构体 (委托 framework 机制)
    let msg_nn = match raw::allocate() {
        Some(m) => m,
        None => return Err(-4),
    };

    // SAFETY: msg 刚由 `allocate` 分配, 非空, 后续由
    // `msgq_recv_safe` 或 `msgq_destroy_safe` 释放.
    let msg_ref = msg_nn.as_mut();
    msg_ref.type_ = type_;
    msg_ref.sender = current_pid as u64;
    msg_ref.size = size as u64;
    msg_ref.next = None;

    // 复制数据 (如果有)
    if let Some(src) = data {
        if size > 0 && !src.is_empty() {
            msg_ref.data[..size].copy_from_slice(&src[..size]);
        }
    }

    // 入队 (尾插法)
    if mq.tail.is_none() {
        mq.head = Some(msg_nn.as_non_null());
        mq.tail = Some(msg_nn.as_non_null());
    } else {
        let tail_nn = mq.tail.unwrap();
        let tail_ref = MessageRef::from_some(tail_nn);
        tail_ref.set_next(Some(msg_nn.as_non_null()));
        mq.tail = Some(msg_nn.as_non_null());
    }
    mq.count += 1;

    // 唤醒等待接收的线程
    if mq.recv_wait.count() > 0 {
        mq.recv_wait.wake_one();
    }

    Ok(())
}

/// 从消息队列接收消息 (策略: 出队 + 数据拷贝 + 释放 + 唤醒)
pub fn msgq_recv_safe(
    namespace: &mut IpcNamespace,
    id: IpcId,
    type_out: Option<&mut u64>,
    data_out: Option<&mut [u8]>,
    size_out: Option<&mut u64>,
) -> Result<usize, i32> {
    let mq = match msgq_find_by_id(namespace, id) {
        Some(q) => q,
        None => return Err(-1),
    };

    // 检查队列是否为空
    if mq.head.is_none() {
        return Err(-2);
    }

    // 出队 (头删法)
    let msg_nn = mq.head.unwrap();
    let msg_ref = MessageRef::from_some(msg_nn);
    mq.head = msg_ref.next();

    if mq.head.is_none() {
        mq.tail = None;
    }
    mq.count -= 1;

    // 通过 MessageRef 安全读取字段
    let read_size = msg_ref.as_ref().size as usize;
    let msg_type = msg_ref.as_ref().type_;
    let msg_data = msg_ref.as_ref().data;
    let msg_size = msg_ref.as_ref().size;

    if let Some(t) = type_out {
        *t = msg_type;
    }

    if let Some(buf) = data_out {
        let copy_len = read_size.min(buf.len());
        if read_size > 0 {
            buf[..copy_len].copy_from_slice(&msg_data[..copy_len]);
        }
    }

    if let Some(s) = size_out {
        *s = msg_size;
    }

    // 通过 MessageRef 释放内存
    msg_ref.free();

    // 唤醒等待发送的线程
    if mq.send_wait.count() > 0 {
        mq.send_wait.wake_one();
    }

    Ok(read_size)
}

/// 销毁消息队列 (策略: 释放所有消息 + 清理结构体)
pub fn msgq_destroy_safe(namespace: &mut IpcNamespace, id: IpcId) -> Result<(), i32> {
    let mq = match msgq_find_by_id(namespace, id) {
        Some(q) => q,
        None => return Err(-1),
    };

    // 释放所有剩余消息
    while let Some(msg_nn) = mq.head {
        let msg_ref = MessageRef::from_some(msg_nn);
        mq.head = msg_ref.next();
        msg_ref.free();
    }

    // 清理结构体
    mq.id = 0;

    Ok(())
}
