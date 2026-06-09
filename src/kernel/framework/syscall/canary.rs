//! Stack Canary / 熵源 系统调用 (P1 #14)
//!
//! 提供两个 QueenX 原生 syscall:
//! - [`sys_getrandom`]: 从内核熵源填充用户 buffer (Linux getrandom 语义)
//! - [`sys_get_canary`]: 返回当前进程 8 字节 stack canary
//!
//! ## Linux 兼容
//!
//! - `getrandom` 对应 Linux 318 (x86_64) / 278 (aarch64). 完整 flags 暂不实现
//!   (GRND_RANDOM / GRND_NONBLOCK), 仅支持最常用语义 (buf, buflen, 0).
//! - `get_canary` 是 QueenX 扩展, 无 Linux 对应, 编号 747 仅 QX 原生空间.
//!
//! ## 安全性
//!
//! - 用户指针必须合法 (`check_user_buf`)
//! - `getrandom` 写用户 buffer, copy_to_user 异常路径被覆盖
//! - `get_canary` 不写内存, 单纯返回 8 字节, 无内存风险
//!
//! ## 已知问题
//!
//! - 底层 `canary::get_random_bytes` / `canary::write_canary_to_user` /
//!   `canary::process_get_current_canary` 在 aarch64 上因 LLVM 22 codegen
//!   bug 暂被 stub 化, 因此 sys_getrandom 始终返回 0, sys_get_canary 始终
//!   返回 0 但不写用户 buffer. 详见 TRACK-B16EAD.

use crate::kernel::framework::proc::canary;

/// `getrandom(buf, buflen, flags)` — 熵源读取
///
/// ## 参数
///
/// - arg0: 用户 buffer 虚拟地址
/// - arg1: buffer 长度 (字节, 单次最大 256)
/// - arg2: flags, 当前忽略 (Linux 兼容位, 0 即默认)
///
/// `#[inline(never)]`: 链路上调用 `canary::get_random_bytes` -> `copy_to_user`
/// (含 inline asm). 阻止内联避开 aarch64 LLVM 22 codegen bug.
#[inline(never)]
pub fn sys_getrandom(arg0: u64, arg1: u64, _arg2: u64) -> i64 {
    let buf = arg0;
    let len = arg1 as usize;
    canary::get_random_bytes(buf, len) as i64
}

/// `get_canary(buf, buflen)` — 读取当前进程 8 字节 stack canary, 写入用户 buffer
///
/// ## 设计
///
/// syscall_dispatch 签名固定 i64, 不能直接返回 8-byte u64 (高位 1 截断).
/// 改用 buffer 输出: 用户态传入 8 字节 buffer, 内核写 canary 进去.
/// 返回 0 (成功) 或 -1 (失败, 例如用户指针非法).
///
/// ## 参数
///
/// - arg0: 用户 buffer 虚拟地址 (必须可写, 至少 8 字节)
/// - arg1: buffer 长度 (建议 8, 实际只写 8 字节)
///
/// 用户态 libc 启动序列:
/// ```c
/// uint64_t canary;
/// syscall(QX_GET_CANARY, &canary, 8);
/// // canary 的低字节恒为 0
/// ```
///
/// `#[inline(never)]` 关键: 此函数最终调用 `canary::process_get_current_canary`
/// 进而调用 `PROCESS_TABLE.with_process` 闭包. 若 inline 进入 `dispatch` 宏
/// 生成的大函数, 链路上 inline asm 间接导致 rustc 1.97 nightly + LLVM 22
/// 的 aarch64 codegen bug. 阻止内联后, 整个链路隔离到独立 codegen 单元.
#[inline(never)]
pub fn sys_get_canary(arg0: u64, arg1: u64) -> i64 {
    canary::write_canary_to_user(arg0, arg1 as usize)
}
