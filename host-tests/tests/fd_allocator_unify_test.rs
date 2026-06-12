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
const FD_ALLOC_RS: &str = "src/kernel/framework/proc/fd_alloc.rs";

/// 从源文件中提取 `pub const XXX: i32 = NNN;` 的整数值
fn extract_pub_const_i32(src: &str, name: &str) -> Option<i32> {
    let needle = format!("pub const {}: i32 =", name);
    for line in src.lines() {
        let t = line.trim();
        if t.starts_with(&needle) {
            // 形如: pub const UDS_FD_BASE: i32 = 1000;
            // 或:   pub const UDS_FD_BASE: i32 = ...fd_alloc::FdPlan::UDS.base;
            let rhs = t.split('=').nth(1)?.trim().trim_end_matches(';');
            // 字面量
            if let Ok(v) = rhs.parse::<i32>() {
                return Some(v);
            }
            // TD-02: const 表达式 (委托给 FdPlan) — 读 fd_alloc.rs 单一来源
            for sub in &["UDS", "EVENT_FD", "SIGNAL_FD", "INOTIFY", "SMOLTCP"] {
                if rhs.contains(&format!("FdPlan::{}.base", sub))
                    || rhs.contains(&format!("FdPlan::{sub}.base"))
                {
                    return read_fdplan_base(sub);
                }
            }
            return None;
        }
    }
    None
}

/// 从 fd_alloc.rs 提取 FdPlan::<sub>.base 字面量
fn read_fdplan_base(sub: &str) -> Option<i32> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(FD_ALLOC_RS);
    let src = fs::read_to_string(&p).ok()?;
    let needle = format!("{}: FdRange = FdRange::new(", sub);
    for line in src.lines() {
        let t = line.trim();
        if t.contains(&needle) {
            let after = t.split("FdRange::new(").nth(1)?;
            let first = after.split(',').next()?.trim();
            return first.parse::<i32>().ok();
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
            if let Ok(v) = rhs.parse::<usize>() {
                return Some(v);
            }
            // TD-02: const 表达式委托给 FdPlan (capacity.as usize)
            for sub in &["UDS", "EVENT_FD", "SIGNAL_FD", "INOTIFY", "SMOLTCP"] {
                if rhs.contains(&format!("FdPlan::{}.capacity", sub)) {
                    return read_fdplan_capacity(sub);
                }
            }
            return None;
        }
    }
    None
}

/// 从 fd_alloc.rs 提取 FdPlan::<sub>.capacity 字面量
fn read_fdplan_capacity(sub: &str) -> Option<usize> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(FD_ALLOC_RS);
    let src = fs::read_to_string(&p).ok()?;
    let needle = format!("{}: FdRange = FdRange::new(", sub);
    for line in src.lines() {
        let t = line.trim();
        if t.contains(&needle) {
            let after = t.split("FdRange::new(").nth(1)?;
            let second = after.split(',').nth(1)?.trim().trim_end_matches(';').trim_end_matches(')');
            return second.parse::<usize>().ok();
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
    // TD-01: 所有 `*_FD_BASE: i32` 应 ≥ MAX_SM_FD, 避免与 smoltcp 共享
    let net = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(NET_INIT)).expect("读 init.rs");
    let sm_max = extract_const_usize(&net, "MAX_SM_FD").expect("MAX_SM_FD") as i32;

    // 4 个子系统 FD base
    let unix = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(UNIX_RS)).expect("读 unix.rs");
    let bases: &[(&str, &str)] = &[
        ("UDS_FD_BASE", UNIX_RS),
        ("EFD_FD_BASE", "src/kernel/framework/syscall/eventfd.rs"),
        ("SFD_FD_BASE", "src/kernel/framework/syscall/signalfd.rs"),
        ("INOTIFY_FD_BASE", "src/kernel/services/fs/inotify.rs"),
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
    assert!(bad.is_empty(),
        "TD-01: 全部 *FD_BASE 必须 ≥ MAX_SM_FD={}, 失败项: {:?}", sm_max, bad);

    // 4 个子系统 FD base 互不重叠
    let mut ranges: Vec<(&str, i32, i32)> = Vec::new();
    for (name, path) in bases {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join(path);
        let src = fs::read_to_string(&p).unwrap_or_default();
        let base = extract_pub_const_i32(&src, name).unwrap();
        // 找同名文件的 *_MAX_SLOTS 或 *MAX_* 字段
        let max = if let Some(m) = extract_const_usize(&src, "EFD_MAX_SLOTS") {
            m as i32
        } else if let Some(m) = extract_const_usize(&src, "SFD_MAX_SLOTS") {
            m as i32
        } else if let Some(m) = extract_const_usize(&src, "MAX_UDS_FD") {
            m as i32
        } else {
            // INOTIFY 没显式 MAX, 取保守 16
            16
        };
        ranges.push((name, base, base + max));
    }
    for i in 0..ranges.len() {
        for j in (i + 1)..ranges.len() {
            let (na, ba, ea) = ranges[i];
            let (nb, bb, eb) = ranges[j];
            assert!(ea <= bb || eb <= ba,
                "FD 范围重叠: [{}, {}) ({}) vs [{}, {}) ({})",
                ba, ea, na, bb, eb, nb);
        }
    }
    let _ = unix; // suppress unused warning
}

#[test]
fn test_fd_bases_in_smoltcp_safe_zone() {
    // TD-01 + I-51: 4 个子系统 FD base 全部 ≥ 256 且彼此不重叠
    // 集中验证 (避免上面的 extract_const_usize 跨文件歧义)
    let uds = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join(UNIX_RS)).expect("unix.rs");
    let efd = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join("src/kernel/framework/syscall/eventfd.rs")).expect("eventfd.rs");
    let sfd = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join("src/kernel/framework/syscall/signalfd.rs")).expect("signalfd.rs");
    let ino = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap().join("src/kernel/services/fs/inotify.rs")).expect("inotify.rs");

    let uds_b = extract_pub_const_i32(&uds, "UDS_FD_BASE").unwrap();
    let efd_b = extract_pub_const_i32(&efd, "EFD_FD_BASE").unwrap();
    let sfd_b = extract_pub_const_i32(&sfd, "SFD_FD_BASE").unwrap();
    let ino_b = extract_pub_const_i32(&ino, "INOTIFY_FD_BASE").unwrap();

    let uds_m = extract_const_usize(&uds, "MAX_UDS_FD").unwrap() as i32;
    let efd_m = extract_const_usize(&efd, "EFD_MAX_SLOTS").unwrap() as i32;
    let sfd_m = extract_const_usize(&sfd, "SFD_MAX_SLOTS").unwrap() as i32;
    // INOTIFY 没有显式 MAX_SLOTS, 取 INOTIFY_MAX_INSTANCES (实际为命名)
    let ino_m = extract_const_usize(&ino, "INOTIFY_MAX_INSTANCES")
        .or_else(|| extract_const_usize(&ino, "MAX_INSTANCES"))
        .unwrap_or(16) as i32;

    // 全部 ≥ 256 (smoltcp 上界)
    for (n, b) in [("UDS", uds_b), ("EFD", efd_b), ("SFD", sfd_b), ("INOTIFY", ino_b)] {
        assert!(b >= 256, "{} FD base {} < 256 (TD-01)", n, b);
    }

    // 验证范围 (用 Vec 排序后检查)
    let mut ranges = vec![
        ("UDS", uds_b, uds_b + uds_m),
        ("EFD", efd_b, efd_b + efd_m),
        ("SFD", sfd_b, sfd_b + sfd_m),
        ("INOTIFY", ino_b, ino_b + ino_m),
    ];
    ranges.sort_by_key(|r| r.1);
    for w in ranges.windows(2) {
        let (na, _, ea) = w[0];
        let (nb, bb, _) = w[1];
        assert!(ea <= bb, "FD 范围重叠: {} 上界 {} > {} 起点 {}",
            na, ea, nb, bb);
    }
}
