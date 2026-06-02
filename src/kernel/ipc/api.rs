//! IPC 子系统 API 层
//!
//! 管道 / 共享内存 / 消息队列 / 信号量 / 信号的统一入口,
//! 等价于 POSIX IPC 函数族 + System V 信号量/消息队列。
//!
//! ## 调用方契约
//! - `syscall::mod` —— `sys_pipe` / `sys_shmget` / `sys_msgget` / `sys_semget` / `sys_kill`
//! - `proc::api` —— 进程 fork/exec 时继承/清理 IPC 资源
//! - `ipc::scheduler_integration` —— 阻塞/唤醒与调度器交互
//!
//! ## 内部接口
//! - `pipe.rs` —— `pipe_create_safe` / `pipe_close_safe` / `pipe_read_safe` / `pipe_write_safe`
//! - `shm.rs` —— `shm_create_safe` / `shm_attach_safe` / `shm_detach_safe` / `shm_destroy_safe`
//! - `msgq.rs` —— `msgq_create_safe` / `msgq_send_safe` / `msgq_recv_safe`
//! - `sem.rs` —— `sem_create_safe` / `sem_wait_safe` / `sem_post_safe`
//! - `signal.rs` —— `signal_send_safe` / `signal_register_safe`
//! - `types.rs` —— `Pipe`, `ShmSegment`, `MsgQueue`, `Semaphore`, `SignalAction`
//! - `scheduler_integration.rs` —— `block_current_thread` / `wake_one_thread` / `wake_all_threads`
//!
//! ## 安全约束
//! - 所有 `_safe` 函数接收 `&mut IpcNamespace`, 调用方负责命名空间生命周期
//! - 全局 `IPC_NAMESPACE` 在 `ipc_init()` 中初始化, 必须单线程调用
//! - 管道/信号量等待队列依赖调度器集成, 不可在中断上下文操作
//! - `unsafe` 仅存在于 FFI 边界和静态可变变量访问 (均在 `mod.rs` 和具体实现中)
//!
//! ## 性能特征
//! - 管道读写: O(1) 环形缓冲区, 零拷贝指针
//! - 共享内存: O(1) 页表映射, 512 条目 VMA 哈希
//! - 消息队列: O(1) 固定槽位, 非链表
//! - 资源上限: pipes[64] / shm[16] / msgq[32] / sem[64]

pub use super::types::{
    IpcId, IpcType, Message, MsgQueue, Pipe, Semaphore, ShmSegment, SignalAction, SignalNum,
    IPC_MAX_PIPES, IPC_MAX_SHM_SEGS, IPC_MAX_MSG_QUEUES, IPC_MAX_SEMAPHORES, MSG_MAX_SIZE,
};

pub use super::scheduler_integration::{
    block_current_thread, block_with_timeout, wake_all_threads, wake_one_thread,
};

#[cfg(feature = "async")]
pub use super::async_ipc::{
    wait_for_condition, AsyncMsgReceiver, AsyncMsgSender, AsyncPipeReader, AsyncPipeWriter,
};
