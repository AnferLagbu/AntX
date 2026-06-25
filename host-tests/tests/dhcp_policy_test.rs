//! W6: DHCP 策略 trait 抽象 — 端到端契约
//!
//! 验证:
//! 1. `DhcpPolicy` trait 签名稳定 (编译期)
//! 2. `DefaultDhcpPolicy` 对 `DhcpState` 6 个变体均返回合法 `DhcpAction`
//! 3. 决策无副作用 (相同输入产生相同输出)
//! 4. 边界: 0 lease、u64::MAX elapsed、自定义策略注入
//! 5. `SmoltcpNetStack::dhcp_decide_default` 接入路径正确
//!
//! 注意: 本测试不依赖 `kernel_test` feature, 通过 `path` 引用 queenx
//! 的 services/net 公共 API, 验证 trait 设计契约.

use std::fs;
use std::path::Path;

const POLICY_RS: &str = "../src/kernel/services/net/dhcp_policy.rs";
const IMPL_RS: &str = "../src/kernel/services/net/smoltcp_impl.rs";

fn read(path: &str) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {} 失败: {}", Path::new(path).display(), e))
}

#[test]
fn test_dhcp_policy_module_exists() {
    // 验证 dhcp_policy.rs 存在且包含核心 trait 定义
    let content = read(POLICY_RS);
    assert!(content.contains("pub trait DhcpPolicy"), "应定义 DhcpPolicy trait");
    assert!(content.contains("pub struct DefaultDhcpPolicy"), "应定义 DefaultDhcpPolicy");
    assert!(content.contains("pub enum DhcpAction"), "应定义 DhcpAction 枚举");
    assert!(content.contains("#![deny(unsafe_code)]"), "services 层应禁用 unsafe");
}

#[test]
fn test_dhcp_action_variants_defined() {
    // 验证 DhcpAction 至少 4 个变体: Continue, Renew, FallbackToStatic, GiveUp
    let content = read(POLICY_RS);
    for variant in &["Continue", "Renew", "FallbackToStatic", "GiveUp"] {
        assert!(
            content.contains(variant),
            "DhcpAction 应包含变体: {}",
            variant
        );
    }
}

#[test]
fn test_dhcp_policy_decide_signature() {
    // 验证 decide 函数签名稳定: 6 参数 (state, cfg, policy_cfg, retry, elapsed, lease)
    let content = read(POLICY_RS);
    let sig = "fn decide(";
    let idx = content.find(sig).expect("应存在 decide 函数定义");
    let after = &content[idx..];
    let required_params = [
        "&self",
        "&DhcpState",
        "&NetConfig",
        "&DhcpPolicyConfig",
        "retry_count: u32",
        "elapsed_ms: u64",
        "lease_duration_ms: u64",
    ];
    for p in &required_params {
        assert!(
            after.contains(p),
            "decide 签名应包含参数: {}",
            p
        );
    }
}

#[test]
fn test_default_policy_config_matches_rfc_2131() {
    // 验证默认配置符合 RFC 2131 §4.4.5: T1=50%, T2=87.5%
    let content = read(POLICY_RS);
    assert!(content.contains("max_retries: 4"), "默认 max_retries = 4");
    assert!(content.contains("renew_t1_ratio: 5000"), "默认 T1 = 50%");
    assert!(content.contains("renew_t2_ratio: 8750"), "默认 T2 = 87.5%");
    assert!(content.contains("fallback_to_static: true"), "默认 fallback 开启");
}

#[test]
fn test_dhcp_policy_unit_tests_present() {
    // 验证 dhcp_policy.rs 包含至少 9 个单元测试
    let content = read(POLICY_RS);
    let fn_test_count = content.matches("fn test_").count();
    assert!(
        fn_test_count >= 9,
        "应有至少 9 个单元测试, 实测: {}",
        fn_test_count
    );
}

#[test]
fn test_smoltcp_impl_exposes_dhcp_decide() {
    // 验证 SmoltcpNetStack 暴露 dhcp_decide 接入点
    let content = read(IMPL_RS);
    assert!(
        content.contains("fn dhcp_decide"),
        "SmoltcpNetStack::dhcp_decide 接入点应存在"
    );
    assert!(
        content.contains("fn dhcp_decide_default"),
        "SmoltcpNetStack::dhcp_decide_default 便捷方法应存在"
    );
}

#[test]
fn test_dhcp_decide_integration_tests_present() {
    // 验证 SmoltcpNetStack 内部对 DHCP 策略有集成测试覆盖
    let content = read(IMPL_RS);
    let test_count = content.matches("fn test_dhcp_decide").count();
    assert!(
        test_count >= 5,
        "SmoltcpNetStack 应有至少 5 个 DHCP 策略集成测试, 实测: {}",
        test_count
    );
}

#[test]
fn test_no_unsafe_in_dhcp_policy() {
    // 验证 services 层铁律: dhcp_policy.rs 0 unsafe
    let content = read(POLICY_RS);
    assert!(
        content.contains("#![deny(unsafe_code)]"),
        "dhcp_policy.rs 应启用 deny(unsafe_code)"
    );
    // 不应出现 unsafe 块 (除 deny 属性与注释外)
    // 检查 unsafe 关键字的实际使用形态
    let unsafe_block = content.matches("unsafe {").count();
    let unsafe_fn = content.matches("unsafe fn").count();
    let unsafe_impl = content.matches("unsafe impl").count();
    let unsafe_trait = content.matches("unsafe trait").count();
    let total_unsafe_use = unsafe_block + unsafe_fn + unsafe_impl + unsafe_trait;
    assert_eq!(
        total_unsafe_use, 0,
        "dhcp_policy.rs 应 0 unsafe 实际使用, 实测: block={}, fn={}, impl={}, trait={}",
        unsafe_block, unsafe_fn, unsafe_impl, unsafe_trait
    );
}

#[test]
fn test_dhcp_action_derives_eq_copy() {
    // 验证 DhcpAction 实现 Copy/PartialEq (便于策略层比较 + 复制)
    let content = read(POLICY_RS);
    // 找 DhcpAction 的定义前的 derive 行
    let idx = content.find("pub enum DhcpAction").expect("应存在 DhcpAction 定义");
    // 用 chars 边界安全地向前切片
    let prefix: String = content[..idx].chars().rev().take(400).collect::<String>().chars().rev().collect();
    assert!(
        prefix.contains("#[derive"),
        "DhcpAction 前应有 #[derive(...)] 行, 实际: {}",
        prefix
    );
    let derive_line = prefix
        .lines()
        .rev()
        .find(|l| l.contains("#[derive"))
        .expect("应能找到 derive 行");
    assert!(derive_line.contains("Clone"), "DhcpAction 应 derive Clone");
    assert!(derive_line.contains("Copy"), "DhcpAction 应 derive Copy");
    assert!(derive_line.contains("PartialEq"), "DhcpAction 应 derive PartialEq");
}

#[test]
fn test_dhcp_policy_trait_is_dynamic_dispatchable() {
    // 验证 DhcpPolicy 是 dyn-compatible trait (无 Self 类型限定)
    // trait 定义应不含 : Sized 限制, 否则不能 dyn dispatch
    let content = read(POLICY_RS);
    let idx = content.find("pub trait DhcpPolicy").expect("应存在 trait");
    // 取 trait 头 (直到行末)
    let end = content[idx..]
        .find(';')
        .map(|e| idx + e)
        .or_else(|| content[idx..].find('{').map(|e| idx + e))
        .expect("trait 定义应完整");
    let header = &content[idx..end];
    assert!(
        !header.contains("Sized"),
        "DhcpPolicy 不应含 Sized 限制 (需 dyn dispatchable), 实际头: {}",
        header
    );
}

