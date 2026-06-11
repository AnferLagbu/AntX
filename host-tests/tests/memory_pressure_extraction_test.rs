//! Memory Pressure 策略提取契约测试 (P1-I-01 D9)
//!
//! 验证 MemoryPressure / 阈值 / 状态机已从 framework/mm/pressure.rs
//! 提取到 services/mm/memory_pressure.rs.
//!
//! 静态契约:
//! 1. MemoryPressure 枚举必在 services
//! 2. services 文件必 deny unsafe_code
//! 3. framework 仅 re-export
//! 4. 核心 API 一致: set_thresholds / current_pressure / update_pressure
//! 5. services 文件不含 klog_ffi (避免 unsafe 边界)

use std::fs;

fn services_memory_pressure_rs() -> String {
    let path = format!(
        "{}/../src/kernel/services/mm/memory_pressure.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read services/mm/memory_pressure.rs")
}

fn framework_pressure_rs() -> String {
    let path = format!(
        "{}/../src/kernel/framework/mm/pressure.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read framework/mm/pressure.rs")
}

fn services_mm_mod_rs() -> String {
    let path = format!(
        "{}/../src/kernel/services/mm/mod.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read services/mm/mod.rs")
}

#[test]
fn memory_pressure_enum_in_services() {
    // P1-I-01 D9 验收: MemoryPressure 枚举必在 services
    let src = services_memory_pressure_rs();
    assert!(
        src.contains("pub enum MemoryPressure"),
        "P1-I-01 D9: MemoryPressure 枚举必在 services/mm/memory_pressure.rs"
    );
    assert!(
        src.contains("pub fn update_pressure"),
        "P1-I-01 D9: update_pressure 必在 services"
    );
    assert!(
        src.contains("pub fn set_thresholds"),
        "P1-I-01 D9: set_thresholds 必在 services"
    );
    assert!(
        src.contains("pub fn current_pressure"),
        "P1-I-01 D9: current_pressure 必在 services"
    );
}

#[test]
fn memory_pressure_services_denies_unsafe() {
    // P1-I-01 D9 验收: services 文件必 deny unsafe_code
    let src = services_memory_pressure_rs();
    assert!(
        src.contains("#![deny(unsafe_code)]"),
        "P1-I-01 D9: services/mm/memory_pressure.rs 必 #![deny(unsafe_code)]"
    );
}

#[test]
fn memory_pressure_services_uses_framework_atomic() {
    // P1-I-01 D9 验收: services 借用 framework 的 AtomicU8/AtomicU64 机制
    let src = services_memory_pressure_rs();
    assert!(
        src.contains("use core::sync::atomic::{AtomicU8, AtomicU64"),
        "P1-I-01 D9: 必使用 core 标准 Atomic 原语 (机制)"
    );
}

#[test]
fn framework_re_exports_memory_pressure() {
    // P1-I-01 D9 验收: framework 通过 re-export 引用
    let src = framework_pressure_rs();
    assert!(
        src.contains("pub use crate::kernel::services::mm::memory_pressure::*"),
        "P1-I-01 D9: framework/mm/pressure.rs 必 re-export services::mm::memory_pressure"
    );
    // framework 文件必无 enum 重复定义
    assert!(
        !src.contains("pub enum MemoryPressure"),
        "P1-I-01 D9: framework/mm/pressure.rs 不应重复定义 MemoryPressure"
    );
    // framework 文件必无 update_pressure 重复定义
    let update_count = src.matches("pub fn update_pressure").count();
    assert_eq!(
        update_count, 0,
        "P1-I-01 D9: framework 不应再 fn update_pressure, 重复 {} 次",
        update_count
    );
}

#[test]
fn services_mm_mod_exposes_memory_pressure() {
    // P1-I-01 D9 验收: services/mm/mod.rs 必 pub mod memory_pressure
    let src = services_mm_mod_rs();
    assert!(
        src.contains("pub mod memory_pressure"),
        "P1-I-01 D9: services/mm/mod.rs 必 pub mod memory_pressure"
    );
}

#[test]
fn memory_pressure_services_uses_4_level_state_machine() {
    // P1-I-01 D9 验收: 策略核心是 4 级状态机
    let src = services_memory_pressure_rs();
    // 必 4 个级别
    for variant in ["Normal", "Warning", "Critical", "Emergency"] {
        assert!(
            src.contains(&format!("    {} =", variant)),
            "P1-I-01 D9: MemoryPressure 必含 {} 变体",
            variant
        );
    }
}

#[test]
fn memory_pressure_services_uses_double_threshold() {
    // P1-I-01 D9 验收: 双重阈值 (绝对值 + 百分比)
    let src = services_memory_pressure_rs();
    // 三个阈值常量
    assert!(
        src.contains("FREE_PAGES_THRESHOLD_WARNING"),
        "P1-I-01 D9: 必含 warning 阈值"
    );
    assert!(
        src.contains("FREE_PAGES_THRESHOLD_CRITICAL"),
        "P1-I-01 D9: 必含 critical 阈值"
    );
    assert!(
        src.contains("FREE_PAGES_THRESHOLD_EMERGENCY"),
        "P1-I-01 D9: 必含 emergency 阈值"
    );
    // 阈值守卫: warning > critical > emergency
    assert!(
        src.contains("warning > critical && critical > emergency"),
        "P1-I-01 D9: set_thresholds 必验证 warn > crit > emer 顺序"
    );
}

#[test]
fn memory_pressure_services_no_klog_ffi() {
    // P1-I-01 D9 验收: services 文件不含 klog_ffi (避免 unsafe 边界)
    let src = services_memory_pressure_rs();
    assert!(
        !src.contains("klog_ffi!"),
        "P1-I-01 D9: services 文件不应含 klog_ffi! (触发 unsafe)"
    );
    assert!(
        !src.contains("crate::klog_ffi"),
        "P1-I-01 D9: services 文件不应含 crate::klog_ffi"
    );
}
