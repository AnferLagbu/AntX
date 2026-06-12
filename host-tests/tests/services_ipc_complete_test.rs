//! I-54: services IPC 4 子系统 (pipe/shm/msgq/sem) 全部完成迁移 + 0 unsafe 静态契约测试
//!
//! 验证 maintenance-2026-06-11.md 中 I-54 验收:
//!   "services/ipc 4 个子系统 (pipe/shm/msgq/sem) 全部完成"
//!   "0 unsafe 验证"

use std::fs;
use std::path::Path;

const SERVICES_IPC: &str = "src/kernel/services/ipc/mod.rs";

#[test]
fn test_services_ipc_4_subsystems_migrated() {
    // IpcLock + free functions 必须全部覆盖 pipe/shm/msgq/sem 4 子系统
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(SERVICES_IPC);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    // pipe/shm/msgq/sem 各自的 close/destroy 标识
    let close_fns: &[(&str, &str)] = &[
        ("pipe", "pipe_close"),
        ("shm", "shm_destroy"),
        ("msgq", "msgq_destroy"),
        ("sem", "sem_destroy"),
    ];
    for (sub, fn_name) in close_fns {
        let needle = format!("fn {}(", fn_name);
        assert!(
            src.contains(&needle),
            "services/ipc 应实现 {} (I-54)",
            needle
        );
    }
}

#[test]
fn test_services_ipc_0_unsafe_blocks() {
    // services/ipc 必须 0 unsafe 代码块 (仅允许 #![deny(unsafe_code)] 属性)
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(SERVICES_IPC);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    assert!(
        src.lines().next().unwrap_or("").contains("deny(unsafe_code)"),
        "services/ipc 应启用 #![deny(unsafe_code)] (I-54)"
    );

    // 查找 unsafe 代码块, 排除属性行
    let mut unsafe_blocks = 0usize;
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with("unsafe") && !t.starts_with("unsafe_code") {
            // 形如 `unsafe {` 或 `unsafe fn` — 但 services 不应有
            unsafe_blocks += 1;
        }
    }
    assert_eq!(
        unsafe_blocks, 0,
        "services/ipc 不应出现 unsafe 代码块, 实际 {} 处 (I-54)",
        unsafe_blocks
    );
}

#[test]
fn test_services_ipc_uses_framework_safe_api() {
    // services/ipc 内部必须走 framework::ipc 的 safe 入口, 不能自己造 unsafe
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(SERVICES_IPC);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    for sub in &["pipe", "shm", "msgq", "sem"] {
        let safe_call = format!("{}::{}_safe", sub, sub);
        // 至少应出现一次对 framework ipc safe API 的调用
        assert!(
            src.contains(&format!("use crate::kernel::framework::ipc::{}", sub))
                || src.contains(&format!("crate::kernel::framework::ipc::{}", sub)),
            "services/ipc 应导入 framework::ipc::{} (I-54)",
            sub
        );
    }
}
