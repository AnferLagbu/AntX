//! IPC 子系统 (Inter-Process Communication)
//!
//! 提供进程间通信的核心机制，包括：
//! - **管道 (Pipe)**: 字节流通信，支持阻塞读写
//! - **信号 (Signal)**: 异步通知机制
//! - **共享内存 (SHM)**: 高效大数据传输
//! - **消息队列 (MsgQ)**: 结构化消息传递
//! - **信号量 (Semaphore)**: 同步原语
//!
//! ## 架构设计
//!
//! ```text
//! IPC Subsystem
//! ├── Namespace (全局资源容器)
//! │   ├── pipes[64]        // 管道数组
//! │   ├── shm_segs[16]     // 共享内存段数组
//! │   ├── msg_queues[32]   // 消息队列数组
//! │   └── semaphores[64]   // 信号量数组
//!
//! ├── Pipe Module         // 管道管理
//! ├── Shm Module          // 共享内存管理
//! ├── MsgQ Module         // 消息队列管理
//! ├── Semaphore Module    // 信号量管理
//! └── Signal Module       // 信号机制
//! ```
//!
//! ## 设计理念
//!
//! - **功能等价**: 与 C 版本 API 完全兼容
//! - **类型安全**: 利用 Rust 类型系统消除空指针和缓冲区溢出
//! - **零成本抽象**: 关键路径性能与 C 版本相当
//! - **FFI 兼容**: 同时提供 Rust 安全接口和 C FFI 接口

// ============================================================================
// 子模块声明
// ============================================================================

/// 数据类型定义
pub mod types;

/// 管道实现
pub mod pipe;

/// 共享内存实现
pub mod shm;

/// 消息队列实现
pub mod msgq;

/// 信号量实现
pub mod sem;

/// 信号机制实现
pub mod signal;

// ============================================================================
// 调度器集成 (阻塞/唤醒)
// ============================================================================

/// 调度器集成功能
pub mod scheduler_integration;

// ============================================================================
// 异步 IPC 基础设施
// ============================================================================

#[cfg(feature = "async")]
pub mod async_ipc;

// ============================================================================
// 全局状态
// ============================================================================

use types::*;

/// IPC 命名空间 (全局资源容器)
///
/// 存储所有 IPC 资源的静态数组。
/// 在内核初始化时通过 `ipc_init()` 初始化。
static mut IPC_NAMESPACE: IpcNamespace = IpcNamespace {
    pipes: [const { Pipe::new() }; IPC_MAX_PIPES],
    shm_segs: [const { ShmSegment::new() }; IPC_MAX_SHM_SEGS],
    msg_queues: [const { MsgQueue::new() }; IPC_MAX_MSG_QUEUES],
    semaphores: [const { Semaphore::new() }; IPC_MAX_SEMAPHORES],
};

/// 全局 ID 分配器
///
/// 确保每个 IPC 资源有唯一标识符。
/// 从 1 开始递增，0 表示"未使用/无效"。
static mut NEXT_IPC_ID: IpcId = 1;

// ============================================================================
// 初始化函数
// ============================================================================

/// 初始化 IPC 子系统
///
/// 必须在内核启动早期调用，初始化所有资源槽位。
///
/// # Safety
/// 此函数只能调用一次，且必须在多核启动前完成。
#[no_mangle]
pub unsafe extern "C" fn ipc_init() {
    // 重置 ID 分配器
    NEXT_IPC_ID = 1;

    // 初始化管道等待队列
    for i in 0..IPC_MAX_PIPES {
        IPC_NAMESPACE.pipes[i].id = 0;
        IPC_NAMESPACE.pipes[i].read_wait.init();
        IPC_NAMESPACE.pipes[i].write_wait.init();
    }

    // 初始化消息队列等待队列
    for i in 0..IPC_MAX_MSG_QUEUES {
        IPC_NAMESPACE.msg_queues[i].id = 0;
        IPC_NAMESPACE.msg_queues[i].send_wait.init();
        IPC_NAMESPACE.msg_queues[i].recv_wait.init();
    }

    // 初始化信号量等待队列
    for i in 0..IPC_MAX_SEMAPHORES {
        IPC_NAMESPACE.semaphores[i].id = 0;
        IPC_NAMESPACE.semaphores[i].wait.init();
    }
}

// ============================================================================
// 重新导出常用类型
// ============================================================================

pub use types::{
    IpcId, IpcType,
    SignalNum, SignalAction,
    Pipe, Message, MsgQueue,
    ShmSegment, Semaphore,
};

// 调度器集成功能导出
pub use scheduler_integration::{
    block_current_thread, wake_one_thread, wake_all_threads, block_with_timeout
};

// 异步 IPC 功能导出 (需要 async feature)
#[cfg(feature = "async")]
pub use async_ipc::{
    AsyncPipeWriter, AsyncPipeReader,
    AsyncMsgSender, AsyncMsgReceiver,
    wait_for_condition
};

// ============================================================================
// 压力测试与边界测试
// ============================================================================

#[cfg(test)]
mod stress_tests;

// ============================================================================
// 异步 IPC 示例 (文档用途)
// ============================================================================

#[cfg(any(doc, test))]
mod async_examples;

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe_create_and_close() {
        let mut ns = IpcNamespace {
            pipes: [const { Pipe::new() }; IPC_MAX_PIPES],
            shm_segs: [const { ShmSegment::new() }; IPC_MAX_SHM_SEGS],
            msg_queues: [const { MsgQueue::new() }; IPC_MAX_MSG_QUEUES],
            semaphores: [const { Semaphore::new() }; IPC_MAX_SEMAPHORES],
        };
        
        let mut next_id: IpcId = 1;
        let pid: u32 = 100;

        // 测试创建管道
        match pipe::pipe_create_safe(&mut ns, &mut next_id, pid) {
            Ok((rfd, wfd)) => {
                assert!(rfd > 0);
                assert!(wfd > 0);
                assert_eq!(wfd, rfd + 1);
                
                // 测试关闭读端
                assert!(pipe::pipe_close_safe(&mut ns, rfd).is_ok());
                
                // 测试关闭写端
                assert!(pipe::pipe_close_safe(&mut ns, wfd).is_ok());
            },
            Err(e) => panic!("Failed to create pipe: {}", e),
        }
    }

    #[test]
    fn test_shm_lifecycle() {
        let mut ns = IpcNamespace {
            pipes: [const { Pipe::new() }; IPC_MAX_PIPES],
            shm_segs: [const { ShmSegment::new() }; IPC_MAX_SHM_SEGS],
            msg_queues: [const { MsgQueue::new() }; IPC_MAX_MSG_QUEUES],
            semaphores: [const { Semaphore::new() }; IPC_MAX_SEMAPHORES],
        };
        
        let mut next_id: IpcId = 1;
        let pid: u32 = 200;

        // 测试创建共享内存
        let id = match shm::shm_create_safe(&mut ns, &mut next_id, 4096, 0o666, pid) {
            Ok(id) => id,
            Err(e) => panic!("Failed to create SHM: {}", e),
        };

        // 测试附加
        let addr = match shm::shm_attach_safe(&mut ns, id, pid) {
            Ok(addr) => addr,
            Err(e) => panic!("Failed to attach SHM: {}", e),
        };
        assert_ne!(addr, 0);

        // 测试分离
        assert!(shm::shm_detach_safe(&mut ns, id, pid).is_ok());

        // 测试销毁
        assert!(shm::shm_destroy_safe(&mut ns, id).is_ok());
    }

    #[test]
    fn test_msgq_send_recv() {
        let mut ns = IpcNamespace {
            pipes: [const { Pipe::new() }; IPC_MAX_PIPES],
            shm_segs: [const { ShmSegment::new() }; IPC_MAX_SHM_SEGS],
            msg_queues: [const { MsgQueue::new() }; IPC_MAX_MSG_QUEUES],
            semaphores: [const { Semaphore::new() }; IPC_MAX_SEMAPHORES],
        };
        
        let mut next_id: IpcId = 1;
        let pid: u32 = 300;

        // 创建消息队列
        let id = match msgq::msgq_create_safe(&mut ns, &mut next_id, 0o666, pid) {
            Ok(id) => id,
            Err(e) => panic!("Failed to create MsgQ: {}", e),
        };

        // 发送消息
        let data = b"Hello, IPC!";
        assert!(msgq::msgq_send_safe(
            &mut ns, id, 42, Some(data), data.len(), pid
        ).is_ok());

        // 接收消息
        let mut type_out: u64 = 0;
        let mut buf = [0u8; MSG_MAX_SIZE];
        let mut size_out: u64 = 0;

        let read_size = match msgq::msgq_recv_safe(
            &mut ns, id, Some(&mut type_out), Some(&mut buf), Some(&mut size_out)
        ) {
            Ok(n) => n,
            Err(e) => panic!("Failed to receive message: {}", e),
        };

        assert_eq!(type_out, 42);
        assert_eq!(read_size, data.len());
        assert_eq!(&buf[..data.len()], data);

        // 销毁队列
        assert!(msgq::msgq_destroy_safe(&mut ns, id).is_ok());
    }

    #[test]
    fn test_semaphore_operations() {
        let mut ns = IpcNamespace {
            pipes: [const { Pipe::new() }; IPC_MAX_PIPES],
            shm_segs: [const { ShmSegment::new() }; IPC_MAX_SHM_SEGS],
            msg_queues: [const { MsgQueue::new() }; IPC_MAX_MSG_QUEUES],
            semaphores: [const { Semaphore::new() }; IPC_MAX_SEMAPHORES],
        };
        
        let mut next_id: IpcId = 1;
        let pid: u32 = 400;

        // 创建计数为 1 的信号量 (互斥锁)
        let id = match sem::sem_create_safe(&mut ns, &mut next_id, 1, 10, pid) {
            Ok(id) => id,
            Err(e) => panic!("Failed to create semaphore: {}", e),
        };

        // P 操作 (应该成功)
        assert!(sem::sem_wait_safe(&mut ns, id).is_ok());

        // V 操作
        assert!(sem::sem_post_safe(&mut ns, id).is_ok());

        // 销毁信号量
        assert!(sem::sem_destroy_safe(&mut ns, id).is_ok());
    }

    #[test]
    fn test_signal_validation() {
        // 测试有效信号
        assert!(signal::signal_send_safe(1, 100).is_ok());      // SIGINT
        assert!(signal::signal_register_safe(1, None, 0).is_ok());
        assert!(signal::signal_block_safe(1).is_ok());
        assert!(signal::signal_unblock_safe(1).is_ok());

        // 测试无效信号 (超出范围)
        assert!(signal::signal_send_safe(0, 100).is_err());     // 信号 0 无效
        assert!(signal::signal_send_safe(33, 100).is_err());    // 超出最大值
        assert!(signal::signal_register_safe(0, None, 0).is_err());
        assert!(signal::signal_block_safe(0).is_err());
        assert!(signal::signal_unblock_safe(34).is_err());
    }
}
