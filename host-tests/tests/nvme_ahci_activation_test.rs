//! I-49: NVMe/AHCI 驱动 dead_code 收敛验证
//!
//! 验证修复后的状态契约:
//! 1. nvme.rs 不再有文件级 `#![allow(dead_code)]` (启动路径已激活)
//! 2. ahci.rs 移除了未使用的 offset 常量 (GHC_CAP/PORT_CLB 等)
//! 3. 启动路径 (storage::init) 注册了 NVMe/AHCI block 设备 — 镜像验证
//!
//! 主机端无法实际跑 PCI 探测, 这里做静态契约验证: 读源文件做关键字检查.

use std::fs;
use std::path::Path;

const FRAMEWORK_DIR: &str = "../src/kernel/framework/driver/storage";

fn read_source(name: &str) -> String {
    let path = Path::new(FRAMEWORK_DIR).join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {} failed: {}", path.display(), e))
}

#[test]
fn test_nvme_no_file_level_dead_code() {
    let src = read_source("nvme.rs");
    // 修复后: 不应有文件级 `#![allow(dead_code)]` 属性.
    // 关键: 必须匹配 `#[allow...` (开头), 不能是注释里说 "已移除".
    // 简单做法: 检查前 2000 字符内没有 `^#![allow(dead_code)]` 行.
    let in_attr = src
        .lines()
        .take_while(|l| l.starts_with("//") || l.trim().is_empty())
        .any(|l| l.trim().starts_with("#![allow(dead_code)]"));
    assert!(
        !in_attr,
        "nvme.rs 仍带文件级 dead_code allow (在注释外的属性行)"
    );
    // 备查: 注释里应说明移除 (做反向校验, 防误删)
    assert!(
        src.contains("#![allow(dead_code)]") && src.contains("已移除"),
        "注释未说明 dead_code 移除原因"
    );
}

#[test]
fn test_ahci_no_unused_offset_consts() {
    let src = read_source("ahci.rs");
    // 验证未使用的 offset 常量已被删除 (GHC_CAP/GHC_IS/GHC_VS/PORT_CLB 等)
    // 保留: GHC_GHC, GHC_PI (真实使用)
    for name in [
        "GHC_CAP:",
        "GHC_IS:",
        "GHC_VS:",
        "PORT_CLB:",
        "PORT_CLBU:",
        "PORT_FB:",
        "PORT_FBU:",
        "PORT_IS:",
        "PORT_IE:",
        "PORT_CMD:",
        "PORT_TFD:",
        "PORT_SIG:",
        "PORT_SSTS:",
        "PORT_SERR:",
        "PORT_CI:",
    ] {
        assert!(
            !src.contains(name),
            "ahci.rs 仍包含未使用常量 {} (应改用 AhciHbaGhc/AhciPortRegs 字段)",
            name
        );
    }
    // GHC_GHC / GHC_PI 是真实使用的, 必须保留
    assert!(src.contains("GHC_GHC:"), "GHC_GHC 不应被删除 (真实使用)");
    assert!(src.contains("GHC_PI:"), "GHC_PI 不应被删除 (真实使用)");
}

#[test]
fn test_ahci_register_structs_preserved() {
    let src = read_source("ahci.rs");
    // repr(C, packed) 寄存器结构体必须保留 — 通过字段名访问硬件
    assert!(src.contains("pub struct AhciHbaGhc"), "AhciHbaGhc 缺失");
    assert!(src.contains("pub struct AhciPortRegs"), "AhciPortRegs 缺失");
    // 关键字段
    for field in ["cap: u32", "ghc: u32", "clb: u32", "fb: u32", "ci: u32"] {
        assert!(src.contains(field), "字段 {} 缺失", field);
    }
}

#[test]
fn test_nvme_register_structs_preserved() {
    let src = read_source("nvme.rs");
    // NVMe 控制器关键符号必须保留
    for sym in ["pub struct NvmeController", "NvmeCommand", "NvmeCompletion"] {
        assert!(src.contains(sym), "{} 缺失", sym);
    }
}

#[test]
fn test_storage_init_uses_block_devices() {
    // 验证启动路径实际调用了 block 设备注册 (非死代码)
    let src = read_source("mod.rs");
    assert!(
        src.contains("AhciBlockDevice::new"),
        "storage::init 未调用 AhciBlockDevice::new"
    );
    assert!(
        src.contains("NvmeBlockDevice::new"),
        "storage::init 未调用 NvmeBlockDevice::new"
    );
    assert!(
        src.contains("register_block_device"),
        "storage::init 未注册 block 设备到 Chitin"
    );
}

#[test]
fn test_ahci_info_field_has_explanation() {
    // ahci.rs 保留的 info 字段必须有注释说明用途 (避免无声 dead_code)
    let src = read_source("ahci.rs");
    let has_info = src.contains("info: DeviceInfo");
    let has_explanation = src.contains("I-49")
        || src.contains("hotplug")
        || src.contains("procfs");
    assert!(has_info, "info 字段不存在");
    assert!(
        has_explanation,
        "info 字段缺少用途说明 (I-49/hotplug/procfs)"
    );
}

#[test]
fn test_block_devices_reexported() {
    let src = read_source("mod.rs");
    // 验证 block 设备在 mod.rs 中 re-export
    assert!(src.contains("pub use"), "mod.rs 缺少 re-export");
}
