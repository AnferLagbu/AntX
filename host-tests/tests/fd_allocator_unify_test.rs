//! I-51: AF_UNIX/smoltcp FD 分配器不重叠 — 静态契约测试
//!
//! 验证 maintenance-2026-06-11.md 中 I-51 验收:
//!   "全项目仅 1 个 fd 分配器" (本次仅修复重叠, 未合并分配器)
//!   "FD 编号不冲突"
//!
//! 修复前: UDS_FD_BASE=100 与 smoltcp [0, 256) 重叠 [100, 116).
//! 修复后: UDS_FD_BASE=1000, UDS 范围 [1000, 1016), 跳出 smoltcp 范围.

use std::fs;
use std::path::Path;

const UNIX_RS: &str = "src/kernel/framework/net/unix.rs";
const NET_INIT: &str = "src/kernel/framework/net/init.rs";

/// 从源文件中提取 `pub const XXX: i32 = NNN;` 的整数值
fn extract_pub_const_i32(src: &str, name: &str) -> Option<i32> {
    let needle = format!("pub const {}: i32 =", name);
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with(&needle) {
            // 形如: pub const UDS_FD_BASE: i32 = 1000;
            let rhs = t.split('=').nth(1)?.trim().trim_end_matches(';');
            return rhs.parse::<i32>().ok();
        }
    }
    None
}

/// 从源文件中提取 `pub const XXX: usize = NNN;` 或 `const XXX: usize = NNN;` 的整数值
fn extract_const_usize(src: &str, name: &str) -> Option<usize> {
    let needle_pub = format!("pub const {}: usize =", name);
    let needle_priv = format!("const {}: usize =", name);
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with(&needle_pub) || t.starts_with(&needle_priv) {
            let rhs = t.split('=').nth(1)?.trim().trim_end_matches(';');
            return rhs.parse::<usize>().ok();
        }
    }
    None
}

#[test]
fn test_uds_fd_base_outside_smoltcp_range() {
    // UDS_FD_BASE + MAX_UDS_FD 应 > smoltcp [0, MAX_SM_FD) 的右端点
    let uds = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(UNIX_RS)).expect("读 unix.rs");
    let net = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(NET_INIT)).expect("读 init.rs");

    let uds_base = extract_pub_const_i32(&uds, "UDS_FD_BASE")
        .expect("UDS_FD_BASE");
    let uds_max = extract_const_usize(&uds, "MAX_UDS_FD")
        .expect("MAX_UDS_FD");
    let sm_max = extract_const_usize(&net, "MAX_SM_FD")
        .expect("MAX_SM_FD");

    // UDS 起点 >= smoltcp 右端, 避免重叠
    assert!(
        uds_base as usize >= sm_max,
        "UDS_FD_BASE={} 应 ≥ smoltcp MAX_SM_FD={} (I-51, 历史 100 与 smoltcp [0,{}) 重叠)",
        uds_base, sm_max, sm_max
    );
    // UDS 范围合法
    assert!(uds_max > 0 && (uds_base as usize).checked_add(uds_max).is_some());
    // UDS 上界不溢出 i32
    assert!(uds_base.checked_add(uds_max as i32).is_some());
}

#[test]
fn test_uds_doc_states_fd_range() {
    // 模块 doc 必须明确 FD 范围, 防止后续误改回 100
    let uds = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(UNIX_RS)).expect("读 unix.rs");
    // 取 doc 头部 (前 2000 字符, 按 char 边界截断避免 UTF-8 误切)
    let head: String = uds.chars().take(2000).collect();
    assert!(
        head.contains("UDS_FD_BASE") && head.contains("MAX_UDS_FD"),
        "UDS 模块 doc 应说明 FD 范围 (I-51)"
    );
    // 注释里不能再说 smoltcp 是 "0..16"
    assert!(
        !head.contains("0..16") && !head.contains("`0..16`"),
        "UDS doc 不应再误称 smoltcp 范围为 0..16 (I-51 修复)"
    );
}

#[test]
fn test_no_fd_base_collides_with_smoltcp() {
    // 全项目所有 `*_FD_BASE: i32` 应 ≥ MAX_SM_FD, 避免与 smoltcp 共享
    let net = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(NET_INIT)).expect("读 init.rs");
    let sm_max = extract_const_usize(&net, "MAX_SM_FD").expect("MAX_SM_FD") as i32;

    // 检查 4 个已知的子系统 FD base
    let unix = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(UNIX_RS)).expect("读 unix.rs");
    let bases: &[(&str, &str)] = &[
        ("UDS_FD_BASE", UNIX_RS),
        ("EFD_FD_BASE", "src/kernel/framework/syscall/eventfd.rs"),
        ("SFD_FD_BASE", "src/kernel/framework/syscall/signalfd.rs"),
        ("INOTIFY_FD_BASE", "src/kernel/framework/fs/vfs/inotify.rs"),
    ];

    let mut bad: Vec<String> = Vec::new();
    for (name, path) in bases {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join(path);
        let src = fs::read_to_string(&p).unwrap_or_else(|_| String::new());
        if let Some(v) = extract_pub_const_i32(&src, name) {
            if v < sm_max {
                bad.push(format!("{}={} < MAX_SM_FD={} (in {})", name, v, sm_max, path));
            }
        }
    }
    // UDS 已修复; 其他 3 个本次范围外, 仅警告不强制
    // 但 UDS 必须通过
    let uds_base = extract_pub_const_i32(&unix, "UDS_FD_BASE").unwrap();
    assert!(
        uds_base >= sm_max,
        "UDS_FD_BASE={} < MAX_SM_FD={} (I-51)",
        uds_base, sm_max
    );
    if !bad.is_empty() {
        println!("[I-51 note] 其他 FD base 与 smoltcp 重叠, 后续修复: {:?}", bad);
    }
}
