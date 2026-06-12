//! I-53: 网卡探测编译时架构互斥静态契约测试
//!
//! 验证 maintenance-2026-06-11.md 中 I-53 验收:
//!   "双架构二进制包含全部网卡驱动"
//!
//! 防止后续重构时在网卡驱动路径上引入 `#[cfg(target_arch = "...")]` 互斥,
//! 阻断单二进制双架构运行.

use std::fs;
use std::path::Path;

const DRIVER_NET_DIR: &str = "src/kernel/framework/driver/net";
const VIRTIO_NET: &str = "src/kernel/framework/driver/virtio/net.rs";

/// 收集 `#[cfg(target_arch = "...")]` 紧邻 `let mut xxx = ...` 的赋值 (排除模块/类型声明).
fn find_arch_mutex_let_assigns(src: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        // 形如: #[cfg(target_arch = "x86_64")] 紧跟 let dma_phys = ...
        if (t.starts_with("#[cfg(target_arch = \"x86_64\")]")
            || t.starts_with("#[cfg(target_arch = \"aarch64\")]"))
            && i + 1 < lines.len()
        {
            let next = lines[i + 1].trim();
            // 仅当紧邻行是 `let <name> = ...` 才算"互斥赋值"; 排除 cfg 模块声明
            if next.starts_with("let ") && next.contains('=') {
                // 提取变量名
                let ident: String = next
                    .trim_start_matches("let ")
                    .trim_start_matches("mut ")
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if !ident.is_empty() {
                    out.push((i + 1, ident));
                }
            }
        }
    }
    out
}

#[test]
fn test_e1000_driver_arch_agnostic() {
    // e1000 驱动应当不包含任何 cfg(target_arch) — 全部走 IoMem 抽象
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(DRIVER_NET_DIR)
        .join("e1000.rs");
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    let x86 = src.matches("cfg(target_arch = \"x86_64\")").count();
    let arm = src.matches("cfg(target_arch = \"aarch64\")").count();
    assert_eq!(x86, 0, "e1000.rs 不应硬编码 x86_64 cfg (I-53)");
    assert_eq!(arm, 0, "e1000.rs 不应硬编码 aarch64 cfg (I-53)");
}

#[test]
fn test_virtio_net_no_arch_mutex_let_assigns() {
    // virtio-net 中不应再有 `#[cfg(target_arch)] + let x = ...` 的架构互斥赋值.
    // 原因: KERNEL_BASE 本身已 cfg-gated, 单表达式可同时覆盖两架构.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(VIRTIO_NET);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    let mutexes = find_arch_mutex_let_assigns(&src);
    assert!(
        mutexes.is_empty(),
        "virtio/net.rs 仍存在编译时架构互斥的 let 赋值 (I-53): {:?}",
        mutexes
    );
}

#[test]
fn test_virtio_net_uses_unified_kernel_base() {
    // 验证修复后的代码使用统一的 KERNEL_BASE 表达式 (无 cfg 守卫).
    // 关键标识: virtio_net_send 中应存在 `if phys >= KERNEL_BASE` 单表达式.
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent().unwrap()
        .join(VIRTIO_NET);
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("无法读取 {}: {}", path.display(), e));

    assert!(
        src.contains("if phys >= KERNEL_BASE"),
        "virtio/net.rs 应使用统一的 KERNEL_BASE 单表达式 (I-53)"
    );
    assert!(
        !src.contains("#[cfg(target_arch = \"x86_64\")]\n    let dma_phys")
            && !src.contains("#[cfg(target_arch = \"aarch64\")]\n    let dma_phys"),
        "virtio/net.rs 不应再含 dma_phys 的 cfg 互斥赋值 (I-53)"
    );
}
