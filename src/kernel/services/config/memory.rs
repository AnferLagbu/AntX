#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯常量定义。
//! 内存布局常量: 页/栈/堆/用户地址空间 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/config/memory.rs, 2026-06-16 提取到 services.
//! 纯常量定义 (页大小/栈/堆/ASLR 基址), 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export + ASLR 运行时函数 + 测试.

// ============================================================================
// 页大小
// ============================================================================

/// 页面大小 (字节, 4 KiB). 必须是 2 的幂.
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

/// 用户态栈初始大小 (字节).
pub const USER_STACK_SIZE: u64 = 65536;

/// 栈保护区域 (字节), 用于捕获栈溢出.
pub const USER_STACK_GUARD: u64 = 4096;

/// 用户栈顶 (可映射地址).
pub const USER_STACK_TOP: u64 = 0x7FFFFFFFE000;

/// 用户进程的内核栈大小 (字节).
pub const USER_KSTACK_SIZE: u64 = 16384;

/// 用户栈的最大自动扩展大小.
pub const USER_STACK_MAX_SIZE: u64 = 8 * 1024 * 1024;

/// 用户 ELF 可执行文件的默认加载地址 (非 PIE, ET_EXEC).
pub const USER_CODE_BASE: u64 = 0x400000;

// ============================================================================
// 用户态 ASLR (Address Space Layout Randomization)
// ============================================================================

/// ASLR 熵位数 — 栈/mmap/堆各区域的随机偏移位数.
///
/// 8 位 = 256 种偏移, 偏移范围 = 256 * PAGE_SIZE = 1 MiB.
/// Linux x86_64 默认 28 位 (mmap), 此处保守取 8 位以简化实现.
pub const ASLR_STACK_BITS: u64 = 8;
pub const ASLR_MMAP_BITS: u64 = 8;
pub const ASLR_HEAP_BITS: u64 = 8;
pub const ASLR_PIE_BITS: u64 = 8;

/// mmap 区域基址 (ASLR 偏移前).
///
/// 位于栈下方, 向下增长. 典型值: 0x7FFFF7xxx000 (glibc 区域).
pub const USER_MMAP_BASE: u64 = 0x7FFFF7000000;

/// 堆区域基址 (ASLR 偏移前).
///
/// 位于代码段上方, 由 brk() 扩展.
pub const USER_HEAP_BASE: u64 = 0x600000;

/// PIE 加载基址 (ASLR 偏移前).
///
/// ET_DYN ELF 在此基址 + 随机偏移处加载.
pub const USER_PIE_BASE: u64 = 0x555555554000;

// ============================================================================
// 内核栈
// ============================================================================

/// Per-process kernel stack size (bytes).
pub const KERNEL_STACK_SIZE: usize = 65536;
