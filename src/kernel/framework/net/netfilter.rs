//! Netfilter 包过滤框架 — framework 层 re-export
//!
//! ## T3-2 迁移记录
//!
//! 策略代码 (规则 CRUD + CIDR 匹配 + 钩子回调管理 + syscall)
//! 已于 2026-06-16 迁移到 services::net::netfilter.
//! 本文件仅 re-export 保持调用方兼容.

pub use crate::kernel::services::net::netfilter::{
    MAX_RULES, MAX_HOOKS_PER_POINT,
    NfHook, NfVerdict, NfPacketInfo, NfRule, NfHookFn,
    nf_add_rule, nf_del_rule, nf_register_hook, nf_unregister_hook,
    nf_hook, nf_hook_count, nf_list_rules,
    sys_nf_add_rule, sys_nf_del_rule,
};
