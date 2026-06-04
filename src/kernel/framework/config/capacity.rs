//! 系统容量常量: 进程/线程/IRQ/文件/会话
//!
//! 所有 per-XXX 数组大小必须以此模块为唯一权威来源。

// ============================================================================
// CPU / 中断容量
// ============================================================================

/// Maximum number of CPUs supported by the kernel.
/// 用于 `static [T; MAX_CPUS]` 类 per-CPU 数组。
pub const MAX_CPUS: usize = 1024;

/// Maximum IRQ number supported.
pub const MAX_IRQS: usize = 256;

// ============================================================================
// 进程 / 线程容量
// ============================================================================

/// Maximum number of processes system-wide.
///
/// 权威: `proc::process::ProcessTable` 的数组大小, 决定 PID 空间。
pub const MAX_PROCESSES: usize = 256;

/// Maximum number of threads system-wide.
pub const MAX_THREADS: usize = 128;

/// Maximum number of threads per single process.
pub const MAX_THREADS_PER_PROCESS: usize = 16;

// ============================================================================
// 文件 / 会话容量
// ============================================================================

/// Maximum number of file descriptors per process.
pub const MAX_OPEN_FILES: usize = 32;

/// Maximum number of login sessions.
pub const MAX_SESSIONS: usize = 16;
