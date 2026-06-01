//! 内存布局常量: 页/栈/堆/用户地址空间
//!
//! 与架构无关, 跨架构统一表达。

// ============================================================================
// 页大小
// ============================================================================

/// Page size in bytes (4 KiB). Must be a power of two.
pub const PAGE_SIZE: u64 = 4096;

/// `log2(PAGE_SIZE)`. 编译期保证为 12。
pub const PAGE_SHIFT: u64 = 12;

// ============================================================================
// 大页 (Huge Page)
// ============================================================================

/// 2 MiB huge page.
pub const HUGE_PAGE_2M_SIZE: u64 = 2 * 1024 * 1024;
pub const HUGE_PAGE_2M_SHIFT: u64 = 21;

/// 1 GiB huge page.
pub const HUGE_PAGE_1G_SIZE: u64 = 1024 * 1024 * 1024;
pub const HUGE_PAGE_1G_SHIFT: u64 = 30;

// ============================================================================
// 用户态栈
// ============================================================================

/// User-space stack initial size (bytes).
pub const USER_STACK_SIZE: u64 = 65536;

/// Stack guard region (bytes) to catch overflow.
pub const USER_STACK_GUARD: u64 = 4096;

/// Top of user stack (mappable address).
pub const USER_STACK_TOP: u64 = 0x7FFFFFFFE000;

/// Kernel stack for user processes (bytes).
pub const USER_KSTACK_SIZE: u64 = 16384;

/// Maximum auto-expand size of user stack.
pub const USER_STACK_MAX_SIZE: u64 = 8 * 1024 * 1024;

/// Default load address for user ELF binaries.
pub const USER_CODE_BASE: u64 = 0x400000;

// ============================================================================
// 内核栈
// ============================================================================

/// Per-process kernel stack size (bytes).
pub const KERNEL_STACK_SIZE: usize = 65536;
