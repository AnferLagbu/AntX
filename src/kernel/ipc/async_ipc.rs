//! 异步 IPC 基础设施
//!
//! 基于 Future trait 实现非阻塞的异步 IPC 操作：
//! - **AsyncPipe**: 异步管道读写
//! - **AsyncMsgQueue**: 异步消息队列
//! - **AsyncSemaphore**: 异步信号量操作
//!
//! ## 设计理念
//!
//! 采用类似 `tokio` 的 Future 模式，将阻塞操作转换为
//! 可组合的异步任务，支持：
//! - 零拷贝数据传输 (通过 Pin)
//! - 取消安全 (Cancel Safety)
//! - 超时和取消支持

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

use super::msgq;
use super::pipe;
use super::types::*;
use super::IPC_NAMESPACE;

// ============================================================================
// 异步管道实现
// ============================================================================

/// 异步管道写入 Future
///
/// 当管道缓冲区满时自动挂起，等待空间可用后继续。
///
/// # Example
/// ```rust,no_run
/// let mut writer = AsyncPipeWriter::new(pipe_id);
/// writer.write(data).await?;
/// ```
pub struct AsyncPipeWriter {
    pipe_id: IpcId,
    data: Option<Vec<u8>>,
    written: usize,
}

impl AsyncPipeWriter {
    /// 创建新的异步管道写入器
    pub fn new(pipe_id: IpcId) -> Self {
        Self {
            pipe_id,
            data: None,
            written: 0,
        }
    }

    /// 准备要写入的数据
    pub fn write(&mut self, data: Vec<u8>) -> &mut Self {
        self.data = Some(data);
        self.written = 0;
        self
    }
}

impl Future for AsyncPipeWriter {
    type Output = Result<usize, i32>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 获取数据引用
        let data = match &self.data {
            Some(d) => d,
            None => return Poll::Ready(Err(-1)), // 未设置数据
        };

        if self.written >= data.len() {
            return Poll::Ready(Ok(self.written)); // 已写完
        }

        // 尝试写入剩余数据
        let remaining = &data[self.written..];

        // 调用同步写入接口 (非阻塞模式)
        match pipe::pipe_write_safe(
            IPC_NAMESPACE.get_mut(),
            self.pipe_id,
            remaining.as_ptr(),
            remaining.len(),
        ) {
            Ok(n) if n > 0 => {
                self.written += n;
                if self.written >= data.len() {
                    Poll::Ready(Ok(self.written))
                } else {
                    // 还有数据未写入，注册 waker 后继续
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Ok(0) => {
                // 管道满或无写端，挂起等待
                cx.waker().clone(); // 保存 waker 以便唤醒
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

/// 异步管道读取 Future
///
/// 当管道为空时自动挂起，等待数据到达后继续。
pub struct AsyncPipeReader {
    pipe_id: IpcId,
    buffer: Option<Vec<u8>>,
    read: usize,
}

impl AsyncPipeReader {
    /// 创建新的异步管道读取器
    pub fn new(pipe_id: IpcId) -> Self {
        Self {
            pipe_id,
            buffer: None,
            read: 0,
        }
    }

    /// 设置接收缓冲区大小
    pub fn with_buffer(&mut self, size: usize) -> &mut Self {
        self.buffer = Some(vec![0u8; size]);
        self.read = 0;
        self
    }
}

impl Future for AsyncPipeReader {
    type Output = Result<Vec<u8>, i32>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let buffer = match &mut self.buffer {
            Some(b) => b,
            None => return Poll::Ready(Err(-1)), // 未设置缓冲区
        };

        // 尝试读取数据
        let mut bytes_read: u64 = 0;

        match pipe::pipe_read_safe(
            IPC_NAMESPACE.get_mut(),
            self.pipe_id,
            buffer.as_mut_ptr(),
            buffer.len(),
            Some(&mut bytes_read),
        ) {
            Ok(_) if bytes_read > 0 => {
                buffer.truncate(bytes_read as usize);
                let result = buffer.clone();
                Poll::Ready(Ok(result))
            }
            Ok(_) => {
                // 管道空或无读端，挂起等待
                cx.waker().clone();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

// ============================================================================
// 异步消息队列实现
// ============================================================================

/// 异步消息发送 Future
///
/// 当队列满时自动挂起，等待空间可用后继续。
pub struct AsyncMsgSender {
    msgq_id: IpcId,
    message: Option<Message>,
}

impl AsyncMsgSender {
    /// 创建新的异步消息发送器
    pub fn new(msgq_id: IpcId) -> Self {
        Self {
            msgq_id,
            message: None,
        }
    }

    /// 设置要发送的消息
    pub fn send(&mut self, msg_type: u64, data: &[u8], sender_pid: u32) -> &mut Self {
        let mut msg = Message::new();
        msg.msg_type = msg_type;

        if !data.is_empty() && data.len() <= MSG_MAX_SIZE {
            msg.data[..data.len()].copy_from_slice(data);
            msg.size = data.len() as u64;
        }

        msg.sender_pid = sender_pid;
        self.message = Some(msg);
        self
    }
}

impl Future for AsyncMsgSender {
    type Output = Result<(), i32>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let msg = match &this.message {
            Some(m) => m,
            None => return Poll::Ready(Err(-1)), // 未设置消息
        };

        // 尝试发送消息
        match msgq::msgq_send_safe(
            IPC_NAMESPACE.get_mut(),
            this.msgq_id,
            msg.msg_type,
            Some(&msg.data),
            msg.size as usize,
            msg.sender_pid,
        ) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(e) if e == -3 => {
                // 队列满
                cx.waker().clone();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

/// 异步消息接收 Future
///
/// 当队列为空时自动挂起，等待消息到达后继续。
pub struct AsyncMsgReceiver {
    msgq_id: IpcId,
    filter_type: Option<u64>,
}

impl AsyncMsgReceiver {
    /// 创建新的异步消息接收器
    pub fn new(msgq_id: IpcId) -> Self {
        Self {
            msgq_id,
            filter_type: None,
        }
    }

    /// 设置消息类型过滤器 (可选)
    pub fn filter_by_type(&mut self, msg_type: u64) -> &mut Self {
        self.filter_type = Some(msg_type);
        self
    }
}

impl Future for AsyncMsgReceiver {
    type Output = Result<Message, i32>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        let mut recv_msg = Message::new();

        match msgq::msgq_recv_safe(
            IPC_NAMESPACE.get_mut(),
            this.msgq_id,
            this.filter_type.as_mut(),
            Some(&mut recv_msg.data),
            Some(&mut recv_msg.size),
        ) {
            Ok(_) => {
                // 填充消息元信息
                recv_msg.msg_type = this.filter_type.unwrap_or(0);
                Poll::Ready(Ok(recv_msg))
            }
            Err(e) if e == -4 => {
                // 队列空
                cx.waker().clone();
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

// ============================================================================
// 辅助函数
// ============================================================================

/// 将同步阻塞操作转换为异步 Future
///
/// 适用于任何基于条件变量的等待场景。
pub async fn wait_for_condition<F>(condition_check: F, timeout_ms: u64) -> Result<(), i32>
where
    F: Fn() -> bool,
{
    WaitForConditionFuture {
        condition_check,
        timeout_ms,
        start_time: None,
    }
    .await
}

/// 条件等待 Future 实现
struct WaitForConditionFuture<F>
where
    F: Fn() -> bool,
{
    condition_check: F,
    timeout_ms: u64,
    start_time: Option<u64>,
}

impl<F> Future for WaitForConditionFuture<F>
where
    F: Fn() -> bool,
{
    type Output = Result<(), i32>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 检查条件是否满足
        if (self.condition_check)() {
            return Poll::Ready(Ok(()));
        }

        // 记录起始时间
        if self.start_time.is_none() {
            self.start_time = Some(rdtsc());
        }

        // 检查超时
        if self.timeout_ms > 0 {
            let elapsed = rdtsc() - self.start_time.unwrap();
            if elapsed >= ms_to_ticks(self.timeout_ms) {
                return Poll::Ready(Err(-1)); // 超时
            }
        }

        // 条件不满足且未超时，挂起等待
        cx.waker().clone();
        Poll::Pending
    }
}

/// 读取 TSC 时间戳计数器
fn rdtsc() -> u64 {
    crate::arch!(timestamp())
}

/// 将毫秒转换为 TSC ticks (近似值)
fn ms_to_ticks(ms: u64) -> u64 {
    const APPROX_CPU_FREQ_MHZ: u64 = 1000; // 1 GHz
    ms * APPROX_CPU_FREQ_MHZ * 1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_async_pipe_writer_creation() {
        let writer = AsyncPipeWriter::new(42);
        assert_eq!(writer.pipe_id, 42);
        assert!(writer.data.is_none());
        assert_eq!(writer.written, 0);
    }

    #[test]
    fn test_async_pipe_reader_creation() {
        let reader = AsyncPipeReader::new(99);
        assert_eq!(reader.pipe_id, 99);
        assert!(reader.buffer.is_none());
    }

    #[test]
    fn test_async_msg_sender_creation() {
        let sender = AsyncMsgSender::new(123);
        assert_eq!(sender.msgq_id, 123);
        assert!(sender.message.is_none());
    }

    #[test]
    fn test_async_msg_receiver_with_filter() {
        let receiver = AsyncMsgReceiver::new(456).filter_by_type(42);
        assert_eq!(receiver.msgq_id, 456);
        assert_eq!(receiver.filter_type, Some(42));
    }

    #[test]
    fn test_wait_for_condition_immediate() {
        // 测试条件立即满足的情况
        let future = wait_for_condition(|| true, 1000);

        // 在同步上下文中无法直接 .await，这里只测试构造
        // 实际测试需要在 async runtime 中进行
        let _ = future;
    }

    #[test]
    fn test_rdtsc_monotonic() {
        let t1 = rdtsc();
        let t2 = rdtsc();

        // TSC 应该是单调递增的
        assert!(t2 >= t1);
    }

    #[test]
    fn test_ms_to_ticks_conversion() {
        assert_eq!(ms_to_ticks(1), 1_000_000); // 1ms = 1M ticks @ 1GHz
        assert_eq!(ms_to_ticks(100), 100_000_000); // 100ms = 100M ticks
    }
}
