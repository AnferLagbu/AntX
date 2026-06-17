//! FdTable 策略提取契约测试 (P1-I-01)
//!
//! 验证 FdTable 已从 framework/proc/process.rs 提取到 services/proc/fd_table.rs.
//!
//! 静态契约:
//! 1. FdTable 类型定义必须位于 services/ (不是 framework/proc/process.rs)
//! 2. services 路径下文件必须 `#![deny(unsafe_code)]`
//! 3. framework 通过 re-export 引用, 不重复定义
//! 4. 核心 API 一致: alloc_fd / get_global_fd / close_fd
//!
//! 主机端测试: 模拟 FdTable 行为 (从源码扫描确认, 不直接执行内核代码).

use std::fs;

fn services_fd_table_rs() -> String {
    let path = format!(
        "{}/../src/kernel/services/proc/fd_table.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read services/proc/fd_table.rs")
}

fn framework_process_rs() -> String {
    let path = format!(
        "{}/../src/kernel/framework/proc/process.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read framework/proc/process.rs")
}

fn services_proc_mod_rs() -> String {
    let path = format!(
        "{}/../src/kernel/services/proc/mod.rs",
        env!("CARGO_MANIFEST_DIR")
    );
    fs::read_to_string(&path).expect("read services/proc/mod.rs")
}

#[test]
fn fd_table_defined_in_services() {
    // P1-I-01 验收: FdTable 类型定义必须位于 services/proc/fd_table.rs
    let src = services_fd_table_rs();
    assert!(
        src.contains("pub struct FdTable"),
        "P1-I-01: FdTable 必须定义在 services/proc/fd_table.rs"
    );
    assert!(
        src.contains("pub const MAX_FDS_PER_PROCESS"),
        "P1-I-01: MAX_FDS_PER_PROCESS 必须定义在 services/proc/fd_table.rs"
    );
}

#[test]
fn fd_table_services_module_denies_unsafe() {
    // P1-I-01 验收: services 层文件必须 deny unsafe_code
    let src = services_fd_table_rs();
    assert!(
        src.contains("#![deny(unsafe_code)]"),
        "P1-I-01: services/proc/fd_table.rs 必须 #![deny(unsafe_code)]"
    );
}

#[test]
fn fd_table_services_uses_framework_irq_spinlock() {
    // P1-I-01 验收: services 可使用 framework 提供的 safe API (IrqSpinLock)
    let src = services_fd_table_rs();
    assert!(
        src.contains("use crate::kernel::framework::sync::IrqSpinLock"),
        "P1-I-01: FdTable 应使用 framework::sync::IrqSpinLock"
    );
}

#[test]
fn framework_re_exports_fd_table_from_services() {
    // P1-I-01 验收: framework/proc/process.rs 通过 re-export 引用, 不重复定义
    let src = framework_process_rs();
    assert!(
        src.contains("pub use crate::kernel::services::proc::fd_table::{FdTable, MAX_FDS_PER_PROCESS}"),
        "P1-I-01: framework/proc/process.rs 必须 re-export services::fd_table"
    );
    // 不能有 struct FdTable 重复定义
    let struct_count = src.matches("pub struct FdTable").count();
    assert_eq!(
        struct_count, 0,
        "P1-I-01: framework/proc/process.rs 不应再定义 struct FdTable, 重复 {} 次",
        struct_count
    );
    // 不能有 const MAX_FDS_PER_PROCESS 重复定义
    let const_count = src.matches("const MAX_FDS_PER_PROCESS").count();
    assert_eq!(
        const_count, 0,
        "P1-I-01: framework 不应再 const MAX_FDS_PER_PROCESS, 重复 {} 次",
        const_count
    );
}

#[test]
fn services_proc_mod_exposes_fd_table() {
    // P1-I-01 验收: services/proc/mod.rs 必须 pub mod fd_table
    let src = services_proc_mod_rs();
    assert!(
        src.contains("pub mod fd_table"),
        "P1-I-01: services/proc/mod.rs 必须 pub mod fd_table"
    );
}

#[test]
fn fd_table_alloc_uses_first_fit_strategy() {
    // P1-I-01 验收: 分配策略是 first-fit 线性扫描
    let src = services_fd_table_rs();
    assert!(
        src.contains("for i in 0..MAX_FDS_PER_PROCESS"),
        "P1-I-01: alloc_fd 必须 first-fit 线性扫描 (O(MAX_FDS_PER_PROCESS))"
    );
    assert!(
        src.contains("entries[i] == -1"),
        "P1-I-01: alloc_fd 必检查 slot == -1 (空闲)"
    );
}

#[test]
fn fd_table_close_zeros_slot() {
    // P1-I-01 验收: close_fd 必清空 slot (entries[local_fd] = -1)
    let src = services_fd_table_rs();
    let close_fn = src
        .find("pub fn close_fd")
        .expect("close_fd not found");
    let body_start = src[close_fn..].find('{').unwrap() + close_fn;
    let body = &src[body_start..];
    assert!(
        body.contains("entries[local_fd] = -1"),
        "P1-I-01: close_fd 必清空 slot"
    );
    assert!(
        body.contains("local_fd >= MAX_FDS_PER_PROCESS"),
        "P1-I-01: close_fd 必检查 local_fd 越界"
    );
}
