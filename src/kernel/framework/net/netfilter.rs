//! Netfilter 包过滤框架 (C5)
//!
//! 提供网络数据包在协议栈各阶段的过滤能力, 类似 Linux Netfilter 的钩子模型.
//!
//! ## 架构
//!
//! ```text
//! services/net/netfilter.rs (safe 代理)
//!     │
//!     ▼
//! framework/net/netfilter.rs (本文件, TCB)
//!     │
//!     ▼
//! 网络协议栈 (smoltcp Interface) 的 5 个钩子点
//! ```
//!
//! ## 钩子点 (Hook Points)
//!
//! | 钩子          | 位置                     | 用途               |
//! |---------------|--------------------------|--------------------|
//! | PREROUTING    | 收包后, 路由决策前        | DNAT / 早期过滤    |
//! | INPUT         | 本机收包, 交付上层前      | 入站过滤           |
//! | FORWARD       | 转发包, 路由后出站前      | 转发过滤 / NAT     |
//! | OUTPUT        | 本机发包, 路由后出站前    | 出站过滤           |
//! | POSTROUTING   | 出站前最后一跳            | SNAT / 晚期过滤    |
//!
//! ## 判定
//!
//! - ACCEPT: 放行
//! - DROP:   静默丢弃
//! - STOLEN: 钩子消费了包 (不再传递)
//! - QUEUE:  交给用户态 (后续实现)

use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::vec::Vec;

use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

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

/// Netfilter 钩子点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NfHook {
    /// 收包后, 路由决策前
    Prerouting = 0,
    /// 本机收包, 交付上层前
    Input = 1,
    /// 转发包
    Forward = 2,
    /// 本机发包
    Output = 3,
    /// 出站前最后一跳
    Postrouting = 4,
}

impl NfHook {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Prerouting),
            1 => Some(Self::Input),
            2 => Some(Self::Forward),
            3 => Some(Self::Output),
            4 => Some(Self::Postrouting),
            _ => None,
        }
    }

    /// 钩子点数量
    pub const COUNT: usize = 5;
}

// ============================================================================
// 判定结果
// ============================================================================

/// Netfilter 判定结果
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum NfVerdict {
    /// 放行
    Accept = 0,
    /// 静默丢弃
    Drop = 1,
    /// 钩子消费了包
    Stolen = 2,
    /// 交给用户态 (后续实现)
    Queue = 3,
}

impl NfVerdict {
    fn from_i32(v: i32) -> Option<Self> {
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

/// 网络包元信息 (传递给钩子回调)
#[derive(Debug, Clone)]
pub struct NfPacketInfo {
    /// 源 IP (4 字节 IPv4)
    pub src_ip: [u8; 4],
    /// 目的 IP (4 字节 IPv4)
    pub dst_ip: [u8; 4],
    /// 源端口 (TCP/UDP)
    pub src_port: u16,
    /// 目的端口 (TCP/UDP)
    pub dst_port: u16,
    /// 协议号 (6=TCP, 17=UDP, 1=ICMP)
    pub protocol: u8,
    /// 入接口索引 (可选)
    pub in_ifindex: Option<u32>,
    /// 出接口索引 (可选)
    pub out_ifindex: Option<u32>,
}

// ============================================================================
// 过滤规则
// ============================================================================

/// Netfilter 过滤规则
#[derive(Debug, Clone)]
pub struct NfRule {
    /// 规则名称
    pub name: alloc::string::String,
    /// 匹配源 IP CIDR (None = 不匹配)
    pub src_cidr: Option<([u8; 4], u8)>,
    /// 匹配目的 IP CIDR (None = 不匹配)
    pub dst_cidr: Option<([u8; 4], u8)>,
    /// 匹配源端口范围 (None = 不匹配)
    pub src_port_range: Option<(u16, u16)>,
    /// 匹配目的端口范围 (None = 不匹配)
    pub dst_port_range: Option<(u16, u16)>,
    /// 匹配协议 (None = 不匹配)
    pub protocol: Option<u8>,
    /// 匹配时的判定
    pub verdict: NfVerdict,
    /// 优先级 (数值越小越先匹配)
    pub priority: i32,
}

impl NfRule {
    /// 检查包是否匹配此规则
    pub fn matches(&self, pkt: &NfPacketInfo) -> bool {
        // 源 CIDR 匹配
        if let Some((net, prefix_len)) = self.src_cidr {
            if !cidr_match(net, prefix_len, pkt.src_ip) {
                return false;
            }
        }
        // 目的 CIDR 匹配
        if let Some((net, prefix_len)) = self.dst_cidr {
            if !cidr_match(net, prefix_len, pkt.dst_ip) {
                return false;
            }
        }
        // 源端口范围匹配
        if let Some((lo, hi)) = self.src_port_range {
            if pkt.src_port < lo || pkt.src_port > hi {
                return false;
            }
        }
        // 目的端口范围匹配
        if let Some((lo, hi)) = self.dst_port_range {
            if pkt.dst_port < lo || pkt.dst_port > hi {
                return false;
            }
        }
        // 协议匹配
        if let Some(proto) = self.protocol {
            if pkt.protocol != proto {
                return false;
            }
        }
        true
    }
}

/// CIDR 匹配
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
// 钩子回调类型
// ============================================================================

/// 钩子回调函数类型
///
/// 接收钩子点和包信息, 返回判定结果.
pub type NfHookFn = fn(NfHook, &NfPacketInfo) -> NfVerdict;

/// 注册的钩子
struct NfHookEntry {
    /// 优先级 (数值越小越先调用)
    priority: i32,
    /// 回调函数
    callback: NfHookFn,
}

// ============================================================================
// 全局 Netfilter 状态
// ============================================================================

/// 每个钩子点的规则链
struct NfChain {
    /// 过滤规则 (按 priority 排序)
    rules: Vec<NfRule>,
    /// 注册的钩子回调 (按 priority 排序)
    hooks: Vec<NfHookEntry>,
}

/// 全局 Netfilter 状态
struct NfState {
    /// 5 个钩子点的链
    chains: [NfChain; NfHook::COUNT],
    /// 统计: 各钩子点调用次数
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

/// 在指定钩子点添加规则
pub fn nf_add_rule(hook: NfHook, rule: NfRule) -> Result<(), i32> {
    let mut state = NF_STATE.lock();
    let idx = hook as usize;
    let chain = &mut state.chains[idx];

    if chain.rules.len() >= MAX_RULES {
        return Err(-(22i32)); // EINVAL
    }

    chain.rules.push(rule);
    // 按 priority 排序
    chain.rules.sort_by_key(|r| r.priority);
    Ok(())
}

/// 删除规则 (按名称)
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
        None => Err(-(2i32)), // ENOENT
    }
}

/// 注册钩子回调
pub fn nf_register_hook(hook: NfHook, priority: i32, callback: NfHookFn) -> Result<(), i32> {
    let mut state = NF_STATE.lock();
    let idx = hook as usize;
    let chain = &mut state.chains[idx];

    if chain.hooks.len() >= MAX_HOOKS_PER_POINT {
        return Err(-(12i32)); // ENOMEM
    }

    chain.hooks.push(NfHookEntry { priority, callback });
    chain.hooks.sort_by_key(|h| h.priority);
    Ok(())
}

/// 注销钩子回调
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
        None => Err(-(2i32)), // ENOENT
    }
}

/// 在指定钩子点执行过滤
///
/// 先调用所有注册的钩子回调, 再匹配规则链.
/// 任一回调/规则返回非 ACCEPT 即终止并返回该判定.
pub fn nf_hook(hook: NfHook, pkt: &NfPacketInfo) -> NfVerdict {
    let idx = hook as usize;

    // 统计
    NF_STATE.lock().hook_counts[idx].fetch_add(1, Ordering::Relaxed);

    let state = NF_STATE.lock();
    let chain = &state.chains[idx];

    // 1. 先调用钩子回调
    for entry in chain.hooks.iter() {
        let verdict = (entry.callback)(hook, pkt);
        if verdict != NfVerdict::Accept {
            return verdict;
        }
    }

    // 2. 再匹配规则链
    for rule in chain.rules.iter() {
        if rule.matches(pkt) {
            return rule.verdict;
        }
    }

    // 3. 默认放行
    NfVerdict::Accept
}

/// 查询钩子点统计
pub fn nf_hook_count(hook: NfHook) -> usize {
    let state = NF_STATE.lock();
    state.hook_counts[hook as usize].load(Ordering::Relaxed)
}

/// 列出指定钩子点的规则
pub fn nf_list_rules(hook: NfHook) -> Vec<NfRule> {
    let state = NF_STATE.lock();
    state.chains[hook as usize].rules.clone()
}

// ============================================================================
// Syscall 接口
// ============================================================================

use crate::kernel::framework::syscall::types::Errno;

/// sys_nf_add_rule — 添加 Netfilter 规则
///
/// # 参数
/// - a0: hook 点 (0-4)
/// - a1: 规则指针 (用户态 NfRule 序列化, 当前简化为关键字段)
/// - a2: 规则长度
///
/// 简化版: a0=hook, a1=src_ip_u32, a2=src_prefix, a3=dst_ip_u32, a4=dst_prefix, a5=verdict
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
        priority: 100, // 默认优先级
    };

    match nf_add_rule(hook, rule) {
        Ok(()) => 0,
        Err(e) => e as i64,
    }
}

/// sys_nf_del_rule — 删除 Netfilter 规则
///
/// # 参数
/// - a0: hook 点
/// - a1: 规则名称指针 (当前简化, 使用序号)
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
