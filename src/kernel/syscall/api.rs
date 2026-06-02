//! 系统调用 API 层
//!
//! POSIX + Credo 私有 syscall 的统一分发入口,
//! 用户态→内核态的唯一合法路径。
//!
//! ## 调用方契约
//! - `boot::isr.asm` —— 中断/异常入口 (int 0x80 / syscall 指令)
//! - `idt::handlers` —— ISR 存根调用 `syscall_dispatch_from_frame`
//! - `proc::exec::load_elf` —— execve 时验证用户指针
//! - `credo::api` —— 能力检查路径复用 `validate_user_ptr`
//! - `chitin::user_driver` —— 用户态驱动透传
//!
//! ## 内部接口
//! - `types.rs` —— `SyscallHandler` 函数指针类型, `Errno`, syscall 编号常量
//! - `mmap.rs` —— mmap/munmap/mprotect 实现
//! - `mod.rs` —— `syscall_dispatch()` 核心分发器 (所有 sys_* 实现)
//!
//! ## 安全约束
//! - 所有公开函数均通过 `validate_user_ptr` / `validate_user_buf` 检查用户指针
//! - 用户指针必须在 [1, 0x7FFFFFFFE000) 范围内
//! - 缓冲区范围必须完全在用户地址空间内
//! - `syscall_dispatch` / `syscall_dispatch_from_frame` 必须在中断上下文调用
//! - `syscall_register` 仅在启动阶段单线程调用
//!
//! ## 性能特征
//! - 分发路径: O(1) match 分支, 编译器优化为跳转表
//! - 指针验证: 两次比较, ≤ 5ns
//! - 覆盖 70+ POSIX syscall + 40+ Credo 私有 syscall

pub use super::types::*;
/// Internal: user-pointer validation (only exported for syscall internals)
pub use super::validate_user_buf;
/// Internal: user-pointer validation (only exported for syscall internals)
pub use super::validate_user_ptr;
