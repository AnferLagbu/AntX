#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。纯常量与全局状态。
//! KASLR 配置 — services 层策略主体
//!
//! ## T6-9 迁移记录
//!
//! 原属 framework/config/kaslr.rs, 2026-06-16 提取到 services.
//! 纯常量与全局状态 (AtomicU64), 0 unsafe, 0 外部依赖.
//! framework 仅保留 re-export.

use core::sync::atomic::{AtomicU64, Ordering};

/// KASLR 是否启用 (派生自 Cargo feature `kaslr`).
pub const KASLR_ENABLED: bool = cfg!(feature = "kaslr");

/// 偏移对齐粒度 (2MB, 与 2M-huge-page 对齐, 与 x86_64/aarch64 linker 一致).
pub const KASLR_ALIGN: u64 = 0x200000;

/// 默认偏移 — 未启用 KASLR 时为 0, 等同"加载到 linker 脚本指定的地址".
pub const KASLR_DEFAULT_OFFSET: u64 = 0;

/// 最大允许偏移 (1 GB). 超过此值可能侵入其他子系统地址空间.
pub const KASLR_MAX_OFFSET: u64 = 0x4000_0000;

/// 实际加载时由 bootloader/entry 写入的偏移量.
///
/// 默认值为 0, 含义是"未应用 KASLR". 当 `KASLR_ENABLED` 为 true 时, 该值
/// 在启动早期应被设置为一个对齐到 `KASLR_ALIGN` 的非零值.
pub static KASLR_BASE_OFFSET: AtomicU64 = AtomicU64::new(KASLR_DEFAULT_OFFSET);

/// 设置运行时 KASLR 基址偏移 (由 bootloader/entry 调用).
///
/// 写指针不要求互斥 (AtomicU64 自然线程安全); 但只在启动极早期调用一次,
/// 之后多核并发读.
pub fn set_kaslr_offset(offset: u64) {
    KASLR_BASE_OFFSET.store(offset, Ordering::Release);
}

/// 获取当前 KASLR 基址偏移.
pub fn get_kaslr_offset() -> u64 {
    KASLR_BASE_OFFSET.load(Ordering::Acquire)
}

/// 检查 `offset` 是否满足 KASLR 对齐要求.
#[inline]
pub fn is_aligned(offset: u64) -> bool {
    (offset & (KASLR_ALIGN - 1)) == 0
}

/// 校验运行时 KASLR 状态与配置的一致性.
///
/// 启动期自检:
/// - 当 `KASLR_ENABLED = true` 时, 实际偏移必须**非零**
/// - 偏移必须对齐到 `KASLR_ALIGN`
/// - 偏移必须不超过 `KASLR_MAX_OFFSET`
///
/// 返回错误细节 (供 validate 框架统一报告).
pub fn validate_kaslr_offset() -> Result<(), &'static str> {
    let off = get_kaslr_offset();

    if !is_aligned(off) {
        return Err("KASLR offset not aligned to KASLR_ALIGN");
    }
    if off > KASLR_MAX_OFFSET {
        return Err("KASLR offset exceeds KASLR_MAX_OFFSET");
    }
    if KASLR_ENABLED && off == 0 {
        return Err("KASLR enabled but runtime offset is zero");
    }
    Ok(())
}
