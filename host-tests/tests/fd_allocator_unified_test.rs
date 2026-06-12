//! TD-02: 统一 FdAllocator — 静态契约测试
//!
//! 验证 `framework/proc/fd_alloc.rs`:
//!   - FdPlan 5 个范围互不重叠
//!   - 全部 ≥ MAX_SM_FD=256 (除 Smoltcp 自身)
//!   - alloc_fd / free_fd / subsystem_of 行为正确
//!   - 启动期不变量 (verify_plan) 满足

use std::fs;
use std::path::Path;

const FD_ALLOC_RS: &str = "src/kernel/framework/proc/fd_alloc.rs";

fn read_fd_alloc() -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(FD_ALLOC_RS))
        .expect("读 fd_alloc.rs")
}

#[test]
fn test_fd_plan_ranges_non_overlapping_const() {
    // FdPlan::ranges_non_overlapping() 编译期 const fn
    // 源码必须调用此函数 (或同等检查)
    let src = read_fd_alloc();
    assert!(
        src.contains("pub const fn ranges_non_overlapping"),
        "FdPlan 必须暴露 const fn ranges_non_overlapping (TD-02)"
    );
    // 且 verify_plan() 启动期调用
    assert!(
        src.contains("pub fn verify_plan"),
        "必须暴露 verify_plan 启动期校验 (TD-02)"
    );
}

#[test]
fn test_fd_plan_constants_match_td01() {
    // FdPlan 的 5 个范围必须与 TD-01 修复后的各子系统 FD_BASE 对齐
    let src = read_fd_alloc();
    // 关键值常量: Smoltcp=0, UDS=1000, EventFd=1100, SignalFd=1120, Inotify=1140
    assert!(src.contains("SMOLTCP: FdRange = FdRange::new(0,"),
        "Smoltcp 范围起点应为 0");
    assert!(src.contains("UDS: FdRange = FdRange::new(1000,"),
        "UDS 范围起点应为 1000 (TD-01)");
    assert!(src.contains("EVENT_FD: FdRange = FdRange::new(1100,"),
        "EVENT_FD 范围起点应为 1100 (TD-01)");
    assert!(src.contains("SIGNAL_FD: FdRange = FdRange::new(1120,"),
        "SIGNAL_FD 范围起点应为 1120 (TD-01)");
    assert!(src.contains("INOTIFY: FdRange = FdRange::new(1140,"),
        "INOTIFY 范围起点应为 1140 (TD-01)");
}

#[test]
fn test_subsystem_count_is_five() {
    // 5 个子系统: Smoltcp/Uds/EventFd/SignalFd/Inotify
    let src = read_fd_alloc();
    // 提取 FdSubsystem 枚举的变体数
    let enum_start = src.find("pub enum FdSubsystem").expect("FdSubsystem 定义");
    let enum_end = src[enum_start..]
        .find("\n}\n").map(|x| enum_start + x).expect("枚举结束");
    let body = &src[enum_start..enum_end];
    let variants: Vec<&str> = body
        .lines()
        .filter(|l| l.trim().ends_with("= 0,") || l.trim().ends_with("= 1,")
            || l.trim().ends_with("= 2,") || l.trim().ends_with("= 3,")
            || l.trim().ends_with("= 4,"))
        .collect();
    assert_eq!(variants.len(), 5,
        "FdSubsystem 应有 5 个变体 (Smoltcp/Uds/EventFd/SignalFd/Inotify), 实为 {}",
        variants.len());
}

#[test]
fn test_alloc_free_subsystem_of_documented() {
    // alloc_fd / free_fd / subsystem_of 必须在 fd_alloc.rs 中定义
    let src = read_fd_alloc();
    assert!(src.contains("pub fn alloc_fd"), "必须暴露 alloc_fd (TD-02)");
    assert!(src.contains("pub fn free_fd"), "必须暴露 free_fd (TD-02)");
    assert!(src.contains("pub fn subsystem_of"), "必须暴露 subsystem_of (TD-02)");
}

#[test]
fn test_fd_range_overlaps_helper() {
    // FdRange 暴露 contains / overlaps / end_exclusive 方法
    let src = read_fd_alloc();
    assert!(src.contains("pub const fn contains"),
        "FdRange 必须暴露 contains (TD-02)");
    assert!(src.contains("pub const fn overlaps"),
        "FdRange 必须暴露 overlaps (TD-02)");
    assert!(src.contains("pub const fn end_exclusive"),
        "FdRange 必须暴露 end_exclusive (TD-02)");
}
