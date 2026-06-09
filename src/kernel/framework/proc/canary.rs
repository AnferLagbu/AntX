//! 用户态 Stack Canary (P1 #14) — 最小 stub 版本
//!
//! 排查 aarch64 LLVM 22 codegen bug: 暂使用 stub 实现, 后续扩展时需要
//! 重新评估 inline 链是否再次触发 aarch64 codegen bug.
use core::sync::atomic::{AtomicU64, Ordering};

static ENTROPY_POOL: AtomicU64 = AtomicU64::new(0x1234_5678_DEAD_BEEFu64);
static PER_PROC_SEED: AtomicU64 = AtomicU64::new(0x5A5A_5A5A_5A5A_5A5Au64);

pub fn next_random_u64() -> u64 {
    ENTROPY_POOL.load(Ordering::Acquire)
}

pub fn generate_canary() -> u64 {
    // 占位实现: 取熵池高 7 字节, 低字节强制为 0 (Linux/glibc 兼容)
    next_random_u64() & 0xFFFF_FFFF_FFFF_FF00
}

pub fn set_per_proc_seed(seed: u64) {
    PER_PROC_SEED.store(seed, Ordering::Release);
}

/// stub: 不真正写用户内存, 返回 0
///
/// 完整实现 (`copy_to_user`) 在 aarch64 上触发 LLVM 22 codegen bug, 暂 stub.
///
/// TODO(TRACK-081BC6): 恢复真实实现. 详见 docs/plan/engineering-progress.md §五.1
/// 和 docs/plan/kernel-roadmap.md §Backlog.
/// - 移除 stub 行为
/// - 实现: 检查 `check_user_buf(buf, len)` -> 生成随机字节 -> `copy_to_user`
/// - 注意: 链路上 `copy_to_user` 含 inline asm, 必须 `#[inline(never)]` 或拆分函数
pub fn get_random_bytes(_buf: u64, _len: usize) -> usize {
    0
}

/// stub: 不真正写用户内存, 返回 0
///
/// 完整实现 (`copy_to_user`) 在 aarch64 上触发 LLVM 22 codegen bug, 暂 stub.
///
/// TODO(TRACK-F0ED2E): 恢复真实实现. 详见 docs/plan/engineering-progress.md §五.1
/// - 移除 stub 行为
/// - 实现: `check_user_buf(buf, 8)` -> `process_get_current_canary` -> 8 字节
///   LE 写入用户 buffer
/// - `process_get_current_canary` 见下, 也需 `#[inline(never)]`
pub fn write_canary_to_user(_buf: u64, _len: usize) -> i64 {
    0
}

/// stub: 返回熵池生成的 canary, 不读 `Process::stack_canary`
///
/// 完整实现 (`PROCESS_TABLE.with_process` 闭包) 在 aarch64 上触发 LLVM 22
/// codegen bug, 暂 stub. 实际安全语义在调用方 (services) 兜底.
///
/// TODO(TRACK-FA2B11): 恢复真实实现. 详见 docs/plan/engineering-progress.md §五.1
/// - 移除 stub 行为
/// - 实现: `let pid = process_get_current_pid(); PROCESS_TABLE.with_process(pid, |p| p.stack_canary.load(Acquire))`
/// - 注意: `with_process` 闭包 inline 间接影响 inline asm 链, 必须 `#[inline(never)]`
pub fn process_get_current_canary() -> u64 {
    generate_canary()
}
