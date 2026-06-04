//! 消息队列 (Message Queue) 实现
//!
//! 提供结构化的进程间消息传递能力
//! 功能等价于 System V 消息队列

use super::types::*;
use crate::kernel::framework::userptr::{UserReadPtr, UserRefMut, UserWritePtr};
use crate::kernel::framework::proc::api::process_get_current_pid;

/// === 消息原始指针特权封装 (Framekernel 模式) ===
///
/// `NonNull<Message>` 是侵入式链表的关键句柄, 所有 unsafe 访问
/// (Box::from_raw/NonNull::new_unchecked/裸字段读) 都集中在 `MessageRef` 内部,
/// 业务逻辑 (`send`/`receive`/`free`) 通过安全方法操作。
pub(crate) mod raw {
    use super::Message;
    use alloc::boxed::Box;
    use core::ptr::NonNull;

    /// `NonNull<Message>` 的安全 newtype 封装
    #[derive(Clone, Copy)]
    pub struct MessageRef(NonNull<Message>);

    impl MessageRef {
        /// 构造一个 `MessageRef` (内部 unsafe 边界)
        ///
        /// # Safety (内部)
        /// - `nn` 必须为 `allocate_message` 返回的有效 NonNull。
        pub(crate) unsafe fn from_non_null(nn: NonNull<Message>) -> Self {
            Self(nn)
        }

        /// 从 `Option<NonNull<Message>>` 安全提升为 `MessageRef`
        ///
        /// - 内部 unsafe: 在 `raw` 子模块内已声明 `from_non_null` 边界
        /// - 调用方只需保证 `nn` 是 Some (从 mq.head/mq.tail 取出)
        pub(crate) fn from_some(nn: NonNull<Message>) -> Self {
            // SAFETY: nn is from Option::unwrap on a queue field that was
            // populated by allocate_message (intrusive list invariant).
            unsafe { Self::from_non_null(nn) }
        }

        /// 获取底层 NonNull
        #[inline(always)]
        pub fn as_non_null(self) -> NonNull<Message> {
            self.0
        }

        /// 读 `next` 字段 (侵入式链表)
        ///
        /// # Safety (内部)
        /// - `self` 必须是有效的 Message。
        #[inline(always)]
        pub fn next(&self) -> Option<NonNull<Message>> {
            // SAFETY: 调用方保证 self 指向有效 Message。
            unsafe { (*self.0.as_ptr()).next }
        }

        /// 写 `next` 字段
        #[inline(always)]
        pub fn set_next(&self, next: Option<NonNull<Message>>) {
            // SAFETY: 同上, self 必须是有效 Message。
            unsafe { (*self.0.as_ptr()).next = next }
        }

        /// 获取 &Message 引用
        #[inline(always)]
        pub fn as_ref(&self) -> &Message {
            // SAFETY: self 指向有效 Message。
            unsafe { &*self.0.as_ptr() }
        }

        /// 获取 &mut Message 引用
        #[inline(always)]
        pub fn as_mut(&self) -> &mut Message {
            // SAFETY: self 指向有效 Message, &mut 保证独占。
            unsafe { &mut *self.0.as_ptr() }
        }

        /// 释放消息 (Box::from_raw + drop)
        pub fn free(self) {
            // SAFETY: self 来自 allocate_message, 由 Box::into_raw 创建。
            let _ = unsafe { Box::from_raw(self.0.as_ptr()) };
        }
    }

    /// 分配一个新 `Message` 并包装为 `MessageRef` (集中 unsafe 入口)
    pub fn allocate() -> Option<MessageRef> {
        let msg = Box::new(Message::new());
        // Box::into_raw never returns null; if it somehow did, treat as OOM.
        let ptr = NonNull::new(Box::into_raw(msg))?;
        // SAFETY: ptr is non-null and was just produced by Box::into_raw.
        Some(unsafe { MessageRef::from_non_null(ptr) })
    }
}

use raw::MessageRef;

/// 查找空闲消息队列槽位
fn msgq_find_free(namespace: &mut IpcNamespace) -> Option<&mut MsgQueue> {
    namespace.msg_queues.iter_mut().find(|q| q.id == 0)
}

/// 根据 ID 查找消息队列
fn msgq_find_by_id(namespace: &mut IpcNamespace, id: IpcId) -> Option<&mut MsgQueue> {
    namespace.msg_queues.iter_mut().find(|q| q.id == id)
}

/// 分配消息结构体 (委托给 `raw::allocate`)
fn allocate_message() -> Option<MessageRef> {
    raw::allocate()
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

    let mq = match msgq_find_by_id(namespace, id) {
        Some(q) => q,
        None => return Err(-1),
    };

    // 检查队列是否已满
    if mq.count >= mq.max_msgs {
        return Err(-3);
    }

    // 分配消息结构体
    let msg_nn = match allocate_message() {
        Some(m) => m,
        None => return Err(-4),
    };

    // SAFETY: msg was just allocated by allocate_message and is non-null;
    // it will be freed by msgq_recv_safe or msgq_destroy_safe.
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
        // SAFETY: mq.tail is Some, pointing to a valid allocated Message
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

// ============================================================================
// FFI 导出函数
// ============================================================================

/// FFI: 创建消息队列
#[no_mangle]
pub fn ipc_msgq_create(perm: i32) -> IpcId {
    let ns = super::IPC_NAMESPACE.get_mut();
    let next_id = super::NEXT_IPC_ID.get_mut();
    let pid = process_get_current_pid();
    match msgq_create_safe(ns, next_id, perm, pid) {
        Ok(id) => id,
        Err(_) => 0,
    }
}

/// FFI: 发送消息。
///
/// # Safety
/// `data` 必须是有效可读指针, 至少 `size` 字节, 内存必须在调用期间保持有效。
/// 由 `sys_msgsnd` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe fn ipc_msgq_send(id: IpcId, type_: u64, data: *const u8, size: u64) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    let pid = process_get_current_pid();

    let slice = if data.is_null() || size == 0 {
        None
    } else {
        // SAFETY: caller guarantees data is valid for size bytes in user memory.
        let user_data = unsafe { UserReadPtr::new(data, size as usize) };
        Some(user_data)
    };

    // Convert UserReadPtr to Option<&[u8]> for the safe API
    let data_slice = slice.as_ref().map(|u| u.as_slice());

    match msgq_send_safe(ns, id, type_, data_slice, size as usize, pid) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// FFI: 接收消息。
///
/// # Safety
/// `data` 必须是有效可写指针, 至少 `size` 字节; `type_out` 用于返回消息类型。
/// 由 `sys_msgrcv` 分发, cred 校验已通过。
#[no_mangle]
pub unsafe fn ipc_msgq_recv(
    id: IpcId,
    type_out: *mut u64,
    data: *mut u8,
    size_out: *mut u64,
) -> i64 {
    let ns = super::IPC_NAMESPACE.get_mut();

    let mut type_opt = if type_out.is_null() {
        None
    } else {
        // SAFETY: caller guarantees type_out is a valid pointer to u64 in user memory.
        let out = unsafe { UserRefMut::<u64>::new(type_out) };
        Some(out)
    };

    let mut data_opt = if data.is_null() {
        None
    } else {
        // SAFETY: caller guarantees data is valid for MSG_MAX_SIZE bytes in user memory.
        let buf = unsafe { UserWritePtr::new(data, MSG_MAX_SIZE) };
        Some(buf)
    };

    let mut size_opt = if size_out.is_null() {
        None
    } else {
        // SAFETY: caller guarantees size_out is a valid pointer to u64 in user memory.
        let out = unsafe { UserRefMut::<u64>::new(size_out) };
        Some(out)
    };

    // Convert framework wrappers to safe Rust types
    let type_ref = type_opt.as_mut().map(|u| u.as_mut());
    let data_ref = data_opt.as_mut().map(|u| u.as_mut_slice());
    let size_ref = size_opt.as_mut().map(|u| u.as_mut());

    match msgq_recv_safe(ns, id, type_ref, data_ref, size_ref) {
        Ok(n) => n as i64,
        Err(_) => -1,
    }
}

/// FFI: 销毁消息队列
#[no_mangle]
pub fn ipc_msgq_destroy(id: IpcId) -> i32 {
    let ns = super::IPC_NAMESPACE.get_mut();
    match msgq_destroy_safe(ns, id) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}