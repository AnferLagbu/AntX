//! 内核调试与跟踪基础设施 (TCB)
//!
//! 提供两类核心能力:
//!
//! - **ftrace**: 函数级跟踪, ring buffer 事件记录, kprobe-like 动态插桩
//! - **KGDB**: 内核调试器桩, 通过串口与外部 gdb 通信
//!
//! ## 边界
//!
//! 全部位于 framework 层, 可在 unsafe 上下文使用。services 层通过
//! `services::debug` 安全封装暴露给用户态。
//!
//! ## 子模块
//!
//! - [ringbuf](file:///home/anfer/Code/AntX/src/kernel/framework/debug/ringbuf.rs) — 单生产者单消费者环形缓冲区
//! - [ftrace](file:///home/anfer/Code/AntX/src/kernel/framework/debug/ftrace.rs) — 跟踪点/事件记录
//! - [kgdb](file:///home/anfer/Code/AntX/src/kernel/framework/debug/kgdb.rs) — KGDB 桩
//! - [api](file:///home/anfer/Code/AntX/src/kernel/framework/debug/api.rs) — 公共 re-export
// 调试子系统占位, 待 ftrace/kgdb 集成后启用。
// 保留文件级 allow: ebpf 子模块大量内部类型和函数待调试路径启用后使用。
#![allow(dead_code)]

pub mod api;
/// D4: eBPF 扩展包过滤器
pub mod ebpf;
pub mod ftrace;
pub mod kgdb;
pub mod ringbuf;
