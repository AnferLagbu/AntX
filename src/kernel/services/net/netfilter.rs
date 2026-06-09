#![deny(unsafe_code)]
//! Netfilter 安全代理 — services 层 (0 unsafe)
//!
//! 封装 `framework::net::netfilter` 的安全 API.

// 重导出强类型
pub use crate::kernel::framework::net::netfilter::{
    NfHook, NfVerdict, NfPacketInfo, NfRule, MAX_RULES,
};

use crate::kernel::framework::net::netfilter::{
    nf_add_rule, nf_del_rule, nf_register_hook, nf_unregister_hook,
    nf_hook, nf_hook_count, nf_list_rules,
};

/// 添加规则 (安全封装)
pub fn add_rule(hook: NfHook, rule: NfRule) -> Result<(), i32> {
    nf_add_rule(hook, rule)
}

/// 删除规则 (安全封装)
pub fn del_rule(hook: NfHook, name: &str) -> Result<(), i32> {
    nf_del_rule(hook, name)
}

/// 注册钩子回调 (安全封装)
pub fn register_hook(hook: NfHook, priority: i32, callback: crate::kernel::framework::net::netfilter::NfHookFn) -> Result<(), i32> {
    nf_register_hook(hook, priority, callback)
}

/// 注销钩子回调 (安全封装)
pub fn unregister_hook(hook: NfHook, callback: crate::kernel::framework::net::netfilter::NfHookFn) -> Result<(), i32> {
    nf_unregister_hook(hook, callback)
}

/// 执行钩子过滤 (安全封装)
pub fn hook(hook: NfHook, pkt: &NfPacketInfo) -> NfVerdict {
    nf_hook(hook, pkt)
}

/// 查询钩子调用次数 (安全封装)
pub fn hook_count(hook: NfHook) -> usize {
    nf_hook_count(hook)
}

/// 列出规则 (安全封装)
pub fn list_rules(hook: NfHook) -> alloc::vec::Vec<NfRule> {
    nf_list_rules(hook)
}
