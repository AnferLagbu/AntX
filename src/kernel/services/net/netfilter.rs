#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//! Netfilter 包过滤框架 — services 层策略主体
//!
//! ## T3-2 迁移记录
//!
//! 原属 framework/net/netfilter.rs, 2026-06-16 提取到 services.
//! 纯策略代码 (规则 CRUD + CIDR 匹配 + 钩子回调管理 + syscall), 0 unsafe.
//! framework 仅保留 re-export.

use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::string::String;
use alloc::vec::Vec;

use crate::kernel::framework::sync::IrqSpinLock;
use crate::kernel::framework::syscall::Errno;

// ============================================================================
// 常量
// ============================================================================

/// 最大规则数
pub const MAX_RULES: usize = 64;

/// 最大钩子回调数 (每个钩子点)
pub const MAX_HOOKS_PER_POINT: usize = 8;

// ============================================================================
// 钩子点
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NfHook {
    Prerouting = 0,
    Input = 1,
    Forward = 2,
    Output = 3,
    Postrouting = 4,
}

impl NfHook {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Prerouting),
            1 => Some(Self::Input),
            2 => Some(Self::Forward),
            3 => Some(Self::Output),
            4 => Some(Self::Postrouting),
            _ => None,
        }
    }

    pub const COUNT: usize = 5;
}

// ============================================================================
// 判定结果
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum NfVerdict {
    Accept = 0,
    Drop = 1,
    Stolen = 2,
    Queue = 3,
}

impl NfVerdict {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Accept),
            1 => Some(Self::Drop),
            2 => Some(Self::Stolen),
            3 => Some(Self::Queue),
            _ => None,
        }
    }
}

// ============================================================================
// 包信息
// ============================================================================

#[derive(Debug, Clone)]
pub struct NfPacketInfo {
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
    pub in_ifindex: Option<u32>,
    pub out_ifindex: Option<u32>,
}

// ============================================================================
// 过滤规则
// ============================================================================

#[derive(Debug, Clone)]
pub struct NfRule {
    pub name: String,
    pub src_cidr: Option<([u8; 4], u8)>,
    pub dst_cidr: Option<([u8; 4], u8)>,
    pub src_port_range: Option<(u16, u16)>,
    pub dst_port_range: Option<(u16, u16)>,
    pub protocol: Option<u8>,
    pub verdict: NfVerdict,
    pub priority: i32,
}

impl NfRule {
    pub fn matches(&self, pkt: &NfPacketInfo) -> bool {
        if let Some((net, prefix_len)) = self.src_cidr {
            if !cidr_match(net, prefix_len, pkt.src_ip) {
                return false;
            }
        }
        if let Some((net, prefix_len)) = self.dst_cidr {
            if !cidr_match(net, prefix_len, pkt.dst_ip) {
                return false;
            }
        }
        if let Some((lo, hi)) = self.src_port_range {
            if pkt.src_port < lo || pkt.src_port > hi {
                return false;
            }
        }
        if let Some((lo, hi)) = self.dst_port_range {
            if pkt.dst_port < lo || pkt.dst_port > hi {
                return false;
            }
        }
        if let Some(proto) = self.protocol {
            if pkt.protocol != proto {
                return false;
            }
        }
        true
    }
}

fn cidr_match(net: [u8; 4], prefix_len: u8, addr: [u8; 4]) -> bool {
    if prefix_len == 0 {
        return true;
    }
    let mask = if prefix_len >= 32 {
        0xFF_FF_FF_FFu32
    } else {
        !((1u32 << (32 - prefix_len)) - 1)
    };
    let net_val = u32::from_be_bytes(net) & mask;
    let addr_val = u32::from_be_bytes(addr) & mask;
    net_val == addr_val
}

// ============================================================================
// 钩子回调
// ============================================================================

pub type NfHookFn = fn(NfHook, &NfPacketInfo) -> NfVerdict;

struct NfHookEntry {
    priority: i32,
    callback: NfHookFn,
}

// ============================================================================
// 全局状态
// ============================================================================

struct NfChain {
    rules: Vec<NfRule>,
    hooks: Vec<NfHookEntry>,
}

struct NfState {
    chains: [NfChain; NfHook::COUNT],
    hook_counts: [AtomicUsize; NfHook::COUNT],
}

static NF_STATE: IrqSpinLock<NfState> = IrqSpinLock::new(NfState {
    chains: [
        NfChain { rules: Vec::new(), hooks: Vec::new() },
        NfChain { rules: Vec::new(), hooks: Vec::new() },
        NfChain { rules: Vec::new(), hooks: Vec::new() },
        NfChain { rules: Vec::new(), hooks: Vec::new() },
        NfChain { rules: Vec::new(), hooks: Vec::new() },
    ],
    hook_counts: [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ],
});

// ============================================================================
// 核心 API
// ============================================================================

pub fn nf_add_rule(hook: NfHook, rule: NfRule) -> Result<(), i32> {
    let mut state = NF_STATE.lock();
    let idx = hook as usize;
    let chain = &mut state.chains[idx];

    if chain.rules.len() >= MAX_RULES {
        return Err(-(22i32));
    }

    chain.rules.push(rule);
    chain.rules.sort_by_key(|r| r.priority);
    Ok(())
}

pub fn nf_del_rule(hook: NfHook, name: &str) -> Result<(), i32> {
    let mut state = NF_STATE.lock();
    let idx = hook as usize;
    let chain = &mut state.chains[idx];

    let pos = chain.rules.iter().position(|r| r.name == name);
    match pos {
        Some(i) => {
            chain.rules.remove(i);
            Ok(())
        }
        None => Err(-(2i32)),
    }
}

pub fn nf_register_hook(hook: NfHook, priority: i32, callback: NfHookFn) -> Result<(), i32> {
    let mut state = NF_STATE.lock();
    let idx = hook as usize;
    let chain = &mut state.chains[idx];

    if chain.hooks.len() >= MAX_HOOKS_PER_POINT {
        return Err(-(12i32));
    }

    chain.hooks.push(NfHookEntry { priority, callback });
    chain.hooks.sort_by_key(|h| h.priority);
    Ok(())
}

pub fn nf_unregister_hook(hook: NfHook, callback: NfHookFn) -> Result<(), i32> {
    let mut state = NF_STATE.lock();
    let idx = hook as usize;
    let chain = &mut state.chains[idx];

    let pos = chain.hooks.iter().position(|h| h.callback as usize == callback as usize);
    match pos {
        Some(i) => {
            chain.hooks.remove(i);
            Ok(())
        }
        None => Err(-(2i32)),
    }
}

pub fn nf_hook(hook: NfHook, pkt: &NfPacketInfo) -> NfVerdict {
    let idx = hook as usize;

    NF_STATE.lock().hook_counts[idx].fetch_add(1, Ordering::Relaxed);

    let state = NF_STATE.lock();
    let chain = &state.chains[idx];

    for entry in chain.hooks.iter() {
        let verdict = (entry.callback)(hook, pkt);
        if verdict != NfVerdict::Accept {
            return verdict;
        }
    }

    for rule in chain.rules.iter() {
        if rule.matches(pkt) {
            return rule.verdict;
        }
    }

    NfVerdict::Accept
}

pub fn nf_hook_count(hook: NfHook) -> usize {
    let state = NF_STATE.lock();
    state.hook_counts[hook as usize].load(Ordering::Relaxed)
}

pub fn nf_list_rules(hook: NfHook) -> Vec<NfRule> {
    let state = NF_STATE.lock();
    state.chains[hook as usize].rules.clone()
}

// ============================================================================
// 安全封装 API (保持原有调用方兼容)
// ============================================================================

pub fn add_rule(hook: NfHook, rule: NfRule) -> Result<(), i32> {
    nf_add_rule(hook, rule)
}

pub fn del_rule(hook: NfHook, name: &str) -> Result<(), i32> {
    nf_del_rule(hook, name)
}

pub fn register_hook(hook: NfHook, priority: i32, callback: NfHookFn) -> Result<(), i32> {
    nf_register_hook(hook, priority, callback)
}

pub fn unregister_hook(hook: NfHook, callback: NfHookFn) -> Result<(), i32> {
    nf_unregister_hook(hook, callback)
}

pub fn hook(hook: NfHook, pkt: &NfPacketInfo) -> NfVerdict {
    nf_hook(hook, pkt)
}

pub fn hook_count(hook: NfHook) -> usize {
    nf_hook_count(hook)
}

pub fn list_rules(hook: NfHook) -> Vec<NfRule> {
    nf_list_rules(hook)
}

// ============================================================================
// Syscall 接口
// ============================================================================

pub fn sys_nf_add_rule(hook: u64, src_ip: u64, src_prefix: u64, dst_ip: u64, dst_prefix: u64, verdict: u64) -> i64 {
    let hook = match NfHook::from_u8(hook as u8) {
        Some(h) => h,
        None => return -(Errno::EINVAL as i64),
    };

    let vf = match NfVerdict::from_i32(verdict as i32) {
        Some(v) => v,
        None => return -(Errno::EINVAL as i64),
    };

    let rule = NfRule {
        name: alloc::format!("rule_{}", nf_hook_count(hook)),
        src_cidr: if src_prefix > 0 { Some(((src_ip as u32).to_be_bytes(), src_prefix as u8)) } else { None },
        dst_cidr: if dst_prefix > 0 { Some(((dst_ip as u32).to_be_bytes(), dst_prefix as u8)) } else { None },
        src_port_range: None,
        dst_port_range: None,
        protocol: None,
        verdict: vf,
        priority: 100,
    };

    match nf_add_rule(hook, rule) {
        Ok(()) => 0,
        Err(e) => e as i64,
    }
}

pub fn sys_nf_del_rule(hook: u64, rule_index: u64) -> i64 {
    let hook = match NfHook::from_u8(hook as u8) {
        Some(h) => h,
        None => return -(Errno::EINVAL as i64),
    };

    let name = alloc::format!("rule_{}", rule_index);
    match nf_del_rule(hook, &name) {
        Ok(()) => 0,
        Err(e) => e as i64,
    }
}
