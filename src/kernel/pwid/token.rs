use super::types::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TokenType {
    Elevation = 0,
    Delegation = 1,
    Session = 2,
    Onetime = 3,
}

impl Default for TokenType {
    fn default() -> Self { TokenType::Elevation }
}

pub const TOKEN_FLAG_SINGLE_COMMAND: u32 = 0x01;
pub const TOKEN_FLAG_NO_TTY: u32         = 0x02;
pub const TOKEN_FLAG_REQUIRE_CONFIRM: u32 = 0x04;
pub const TOKEN_FLAG_AUDIT_ALL: u32      = 0x08;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct PwidToken {
    pub token_id: u64,
    pub issuer_pwid: u64,
    pub holder_pwid: u64,
    pub token_type: TokenType,
    pub cap_domains: [CapDomain; 8],
    pub capabilities: [CapBits; 8],
    pub scope_path: [u8; 256],
    pub valid_from: u64,
    pub valid_until: u64,
    pub max_uses: u32,
    pub current_uses: u32,
    pub flags: u32,
}

impl Default for PwidToken {
    fn default() -> Self {
        Self {
            token_id: 0,
            issuer_pwid: 0,
            holder_pwid: 0,
            token_type: TokenType::Elevation,
            cap_domains: [0; 8],
            capabilities: [0; 8],
            scope_path: [0; 256],
            valid_from: 0,
            valid_until: 0,
            max_uses: 0,
            current_uses: 0,
            flags: 0,
        }
    }
}

impl PwidToken {
    pub fn new(
        token_type: TokenType,
        issuer: u64,
        holder: u64,
    ) -> Self {
        let _time = get_time();
        Self {
            token_id: generate_token_id(),
            issuer_pwid: issuer,
            holder_pwid: holder,
            token_type,
            ..Default::default()
        }
    }

    pub fn with_capabilities(
        mut self,
        domains: &[CapDomain],
        caps: &[CapBits],
    ) -> Self {
        let count = domains.len().min(8).min(caps.len());
        for i in 0..count {
            self.cap_domains[i] = domains[i];
            self.capabilities[i] = caps[i];
        }
        self
    }

    pub fn with_scope(mut self, path: &str) -> Self {
        let bytes = path.as_bytes();
        let len = bytes.len().min(255);
        self.scope_path[..len].copy_from_slice(&bytes[..len]);
        self.scope_path[len] = 0;
        self
    }

    pub fn with_validity(mut self, from: u64, until: u64) -> Self {
        self.valid_from = from;
        self.valid_until = until;
        self
    }

    pub fn with_uses(mut self, max: u32) -> Self {
        self.max_uses = max;
        self
    }

    pub fn with_flags(mut self, flags: u32) -> Self {
        self.flags = flags;
        self
    }

    pub fn is_valid(&self) -> bool {
        let now = get_time();

        if self.valid_from > 0 && now < self.valid_from {
            return false;
        }

        if self.valid_until > 0 && now > self.valid_until {
            return false;
        }

        if self.max_uses > 0 && self.current_uses >= self.max_uses {
            return false;
        }

        true
    }

    pub fn has_capability(&self, domain: CapDomain, required: CapBits) -> bool {
        for i in 0..8 {
            if self.cap_domains[i] == domain {
                return (self.capabilities[i] & required) == required;
            }
        }
        false
    }

    pub fn use_token(&mut self) -> Result<(), ()> {
        if !self.is_valid() {
            return Err(());
        }

        self.current_uses += 1;

        if self.has_flag(TOKEN_FLAG_SINGLE_COMMAND) {
            self.valid_until = get_time() - 1;
        }

        Ok(())
    }

    pub fn has_flag(&self, flag: u32) -> bool {
        (self.flags & flag) != 0
    }

    pub fn is_scoped(&self) -> bool {
        self.scope_path[0] != 0
    }

    pub fn check_scope(&self, path: &str) -> bool {
        if !self.is_scoped() { return true; }

        let scope_end = self.scope_path.iter().position(|&b| b == 0).unwrap_or(256);
        let scope_str = core::str::from_utf8(&self.scope_path[..scope_end]).unwrap_or("/");

        path.starts_with(scope_str)
    }
}

pub const MAX_TOKENS: usize = 64;

#[derive(Debug, Clone)]
pub struct TokenManager {
    tokens: [PwidToken; MAX_TOKENS],
    count: usize,
    next_id: u64,
}

unsafe impl Send for TokenManager {}
unsafe impl Sync for TokenManager {}

impl Default for TokenManager {
    fn default() -> Self {
        Self {
            tokens: [PwidToken::default(); MAX_TOKENS],
            count: 0,
            next_id: 1,
        }
    }
}

impl TokenManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, mut token: PwidToken) -> Option<u64> {
        if self.count >= MAX_TOKENS {
            return None;
        }

        token.token_id = self.next_id;
        self.next_id += 1;

        self.tokens[self.count] = token;
        self.count += 1;

        Some(token.token_id)
    }

    pub fn find(&self, token_id: u64) -> Option<usize> {
        for i in 0..self.count {
            if self.tokens[i].token_id == token_id {
                return Some(i);
            }
        }
        None
    }

    pub fn get(&self, idx: usize) -> Option<&PwidToken> {
        if idx < self.count { Some(&self.tokens[idx]) } else { None }
    }

    pub fn get_mut(&mut self, idx: usize) -> Option<&mut PwidToken> {
        if idx < self.count { Some(&mut self.tokens[idx]) } else { None }
    }

    pub fn use_token(&mut self, token_id: u64) -> Result<(), ()> {
        let idx = match self.find(token_id) {
            Some(i) => i,
            None => return Err(()),
        };

        self.tokens[idx].use_token()
    }

    pub fn revoke(&mut self, token_id: u64, _revoker: u64) -> bool {
        let idx = match self.find(token_id) {
            Some(i) => i,
            None => return false,
        };

        self.tokens[idx].valid_until = get_time() - 1;
        
        self.compact();
        true
    }

    pub fn revoke_all_for_holder(&mut self, holder: u64) {
        for i in (0..self.count).rev() {
            if self.tokens[i].holder_pwid == holder {
                self.tokens[i].valid_until = get_time() - 1;
            }
        }
        self.compact();
    }

    pub fn find_valid_tokens(
        &self,
        holder: u64,
        domain: CapDomain,
        caps: CapBits,
    ) -> alloc::vec::Vec<(usize, &PwidToken)> {
        let mut result = alloc::vec::Vec::new();
        
        for i in 0..self.count {
            let t = &self.tokens[i];
            if t.holder_pwid == holder 
                && t.is_valid()
                && t.has_capability(domain, caps) {
                result.push((i, t));
            }
        }
        
        result
    }

    pub fn clear_expired(&mut self) {
        for t in self.tokens.iter_mut() {
            if !t.is_valid() {
                t.valid_until = 0;
                t.token_id = 0;
            }
        }
        self.compact();
    }

    fn compact(&mut self) {
        let mut write_idx = 0;
        for read_idx in 0..self.count {
            if self.tokens[read_idx].token_id != 0 {
                if read_idx != write_idx {
                    self.tokens[write_idx] = self.tokens[read_idx];
                }
                write_idx += 1;
            }
        }
        self.count = write_idx;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// ✅ P1-7 修复: 使用校准后的 TSC 频率和序列号生成唯一 Token ID
/// 
/// 改进点:
/// 1. 使用全局原子计数器避免快速创建时的碰撞
/// 2. 混合 TSC 高位 + 序列号 + 常数乘法增强随机性
/// 3. 避免仅依赖低 32 位 (变化快但可能重复)
fn generate_token_id() -> u64 {
    use core::sync::atomic::{AtomicU64, Ordering};
    
    static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);
    let seq = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
    
    let tsc: u64;
    unsafe {
        core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack));
    }
    
    // 组合: TSC 高 32 位 | 序列号(16位) | 哈希扰动
    ((tsc & 0xFFFFFFFF_00000000) << 16) 
        ^ ((seq & 0xFFFF) << 32) 
        ^ (tsc.wrapping_mul(0x9E3779B97F4A7C15)) // 黄金比例常数
}

/// ✅ P1-7 修复: 将 TSC 周期数转换为微秒 (μs)
/// 
/// 使用 cpu 模块提供的校准后频率进行转换:
/// - 如果有校准值: microseconds = (TSC * 1_000_000) / frequency_hz
/// - 如果无校准值: 回退到右移近似 (假设 ~2.5GHz)
/// 
/// 返回值单位: 微秒 (μs), 从系统启动开始计时
fn get_time() -> u64 {
    // ✅ 使用 FFI 接口获取校准后的 TSC 频率
    // 避免依赖 crate::kernel 模块 (该模块未在 lib.rs 中导出)
    extern "C" {
        fn cpu_get_tsc_frequency() -> u64;
    }
    
    let tsc: u64;
    unsafe {
        core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem,nostack));
    }
    
    let freq_hz = unsafe { cpu_get_tsc_frequency() };
    
    if freq_hz > 0 {
        (tsc * 1_000_000) / freq_hz
    } else {
        tsc >> 20
    }
}
