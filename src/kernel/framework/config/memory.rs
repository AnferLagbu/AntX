//! 内存布局常量 + ASLR 运行时函数 — framework 层 re-export + 运行时
//!
//! ## T6-9 迁移记录
//!
//! 纯常量定义 (页大小/栈/堆/ASLR 基址)
//! 已于 2026-06-16 迁移到 `services::config::memory`.
//! 本文件仅保留 ASLR 运行时函数 (依赖 TSC) + re-export 保持调用方兼容.

// re-export services 层常量
pub use crate::kernel::services::config::memory::*;

// ============================================================================
// 用户态 ASLR 随机偏移生成
// ============================================================================

/// 生成 ASLR 随机偏移 (页对齐).
///
/// 使用 TSC 作为熵源, 取低 `bits` 位作为偏移, 再乘以 `PAGE_SIZE`.
/// 偏移范围 = [0, (2^bits - 1) * `PAGE_SIZE`].
///
/// # Arguments
/// * `bits` - 熵位数 (如 `ASLR_STACK_BITS` = 8)
///
/// # Returns
/// 页对齐的随机偏移 (字节)
#[inline]
pub fn aslr_random_offset(bits: u64) -> u64 {
    let tsc = crate::kernel::framework::cpu::read_tsc();
    let mask = (1u64 << bits) - 1;
    (tsc & mask) * PAGE_SIZE
}

/// 生成带 ASLR 随机偏移的栈顶地址.
///
/// 栈顶 = `USER_STACK_TOP` - `aslr_random_offset(ASLR_STACK_BITS)`
#[inline]
pub fn aslr_stack_top() -> u64 {
    USER_STACK_TOP - aslr_random_offset(ASLR_STACK_BITS)
}

/// 生成带 ASLR 随机偏移的 mmap 基址.
#[inline]
pub fn aslr_mmap_base() -> u64 {
    USER_MMAP_BASE - aslr_random_offset(ASLR_MMAP_BITS)
}

/// 生成带 ASLR 随机偏移的堆基址.
#[inline]
pub fn aslr_heap_base() -> u64 {
    USER_HEAP_BASE + aslr_random_offset(ASLR_HEAP_BITS)
}

/// 生成带 ASLR 随机偏移的 PIE 加载基址.
#[inline]
pub fn aslr_pie_base() -> u64 {
    USER_PIE_BASE + aslr_random_offset(ASLR_PIE_BITS)
}

// ============================================================================
// 内核测试
// ============================================================================

#[cfg(feature = "kernel_test")]
fn test_aslr_constants() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{TestResult, check};
    // ASLR 偏移必须是页对齐的
    check!(PAGE_SIZE > 0, "PAGE_SIZE > 0");
    check!(USER_STACK_TOP > USER_STACK_SIZE, "stack_top > stack_size");
    check!(USER_PIE_BASE > 0, "PIE base > 0");
    check!(USER_MMAP_BASE > USER_PIE_BASE, "mmap > PIE");
    check!(USER_HEAP_BASE >= USER_CODE_BASE, "heap >= code");
    // ASLR 偏移范围不超过 1 MiB (8 bits * 4KB = 1MB)
    let max_offset = ((1u64 << ASLR_STACK_BITS) - 1) * PAGE_SIZE;
    check!(max_offset <= 1024 * 1024, "ASLR offset <= 1MiB");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_aslr_random_offset_range() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{TestResult, check};
    // 多次调用应产生不同偏移 (概率性测试, 可能偶尔失败)
    let o1 = aslr_random_offset(8);
    let o2 = aslr_random_offset(8);
    // 偏移应页对齐
    check!(o1.is_multiple_of(PAGE_SIZE), "offset1 page-aligned");
    check!(o2.is_multiple_of(PAGE_SIZE), "offset2 page-aligned");
    // 偏移应在有效范围内
    let max = ((1u64 << 8) - 1) * PAGE_SIZE;
    check!(o1 <= max, "offset1 in range");
    check!(o2 <= max, "offset2 in range");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_aslr_stack_top_range() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{TestResult, check};
    let top = aslr_stack_top();
    // 栈顶应低于 USER_STACK_TOP
    check!(top <= USER_STACK_TOP, "stack_top <= USER_STACK_TOP");
    // 栈顶应足够高 (在用户空间上半部分)
    check!(top > 0x7FFF00000000, "stack_top in upper half");
    // 栈顶应页对齐
    check!(top.is_multiple_of(PAGE_SIZE), "stack_top page-aligned");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
fn test_aslr_pie_base_range() -> crate::kernel::framework::tests::TestResult {
    use crate::kernel::framework::tests::{TestResult, check};
    let base = aslr_pie_base();
    // PIE 基址应 >= USER_PIE_BASE
    check!(base >= USER_PIE_BASE, "PIE base >= USER_PIE_BASE");
    // PIE 基址应在用户空间
    check!(base < USER_STACK_TOP, "PIE base < stack_top");
    // 页对齐
    check!(base.is_multiple_of(PAGE_SIZE), "PIE base page-aligned");
    TestResult::Pass
}

#[cfg(feature = "kernel_test")]
pub fn register_aslr_tests() {
    use crate::kernel::framework::tests::runner;
    let r = runner();
    r.register("aslr", "constants", test_aslr_constants);
    r.register("aslr", "random_offset_range", test_aslr_random_offset_range);
    r.register("aslr", "stack_top_range", test_aslr_stack_top_range);
    r.register("aslr", "pie_base_range", test_aslr_pie_base_range);
}
