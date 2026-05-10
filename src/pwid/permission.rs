use super::types::*;
use super::capability::*;
use super::trust_chain::*;
use super::token::*;
use super::context::*;  // 恢复 context 模块引用

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionResult {
    Allowed { source: AllowSource, audit_required: bool },
    Denied(DenyReason),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AllowSource {
    RootPrivilege,
    Owner,
    TrustChain(u64),
    Token(u64),
    OtherPermissions,
    ContextPolicy,
    CapabilityMatrix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    InvalidPwid = 1,
    Disabled = 2,
    Expired = 3,
    InsufficientCapability = 4,
    NoPermission = 5,
    TimeConstraint = 6,
    HighRisk = 7,
    PathRestriction = 8,
    TokenExpired = 9,
    TokenExhausted = 10,
    NotInScope = 11,
    Missing2FA = 12,
    RateLimited = 13,
    NotAuthenticated = 14,
}

impl core::fmt::Display for DenyReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DenyReason::InvalidPwid => write!(f, "Invalid PWID"),
            DenyReason::Disabled => write!(f, "Account disabled"),
            DenyReason::Expired => write!(f, "Account expired"),
            DenyReason::InsufficientCapability => write!(f, "Insufficient capability"),
            DenyReason::NoPermission => write!(f, "No permission"),
            DenyReason::TimeConstraint => write!(f, "Time constraint violation"),
            DenyReason::HighRisk => write!(f, "High risk session"),
            DenyReason::PathRestriction => write!(f, "Path restriction"),
            DenyReason::TokenExpired => write!(f, "Token expired"),
            DenyReason::TokenExhausted => write!(f, "Token exhausted"),
            DenyReason::NotInScope => write!(f, "Not in token scope"),
            DenyReason::Missing2FA => write!(f, "2FA required"),
            DenyReason::RateLimited => write!(f, "Rate limited"),
            DenyReason::NotAuthenticated => write!(f, "Not authenticated"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MandatoryReqs {
    pub require_trust_level: Option<TrustLevel>,
    pub require_min_security_score: Option<u16>,
    pub require_session_type: Option<SessionType>,
    pub require_login_method: Option<LoginMethod>,
    pub require_time_of_day: Option<TimeOfDay>,
}

impl Default for MandatoryReqs {
    fn default() -> Self {
        Self {
            require_trust_level: None,
            require_min_security_score: None,
            require_session_type: None,
            require_login_method: None,
            require_time_of_day: None,
        }
    }
}

impl MandatoryReqs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_min_trust(mut self, level: TrustLevel) -> Self {
        self.require_trust_level = Some(level);
        self
    }

    pub fn with_min_security_score(mut self, score: u16) -> Self {
        self.require_min_security_score = Some(score);
        self
    }

    pub fn check(&self, context: &PermissionContext) -> Result<(), DenyReason> {
        if let Some(_min_level) = self.require_trust_level {
            // Trust level verified in check_permission via pwid_level parameter
        }

        if let Some(min_score) = self.require_min_security_score {
            if context.get_combined_risk() as u16 > (1000 - min_score) {
                return Err(DenyReason::HighRisk);
            }
        }

        if let Some(session_type) = self.require_session_type {
            if context.session_context.session_type != session_type {
                return Err(DenyReason::NoPermission);
            }
        }

        if let Some(login_method) = self.require_login_method {
            if context.session_context.login_method != login_method {
                return Err(DenyReason::NoPermission);
            }
        }

        if let Some(time) = self.require_time_of_day {
            if context.time_context.time_of_day != time {
                return Err(DenyReason::TimeConstraint);
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ContextConstraints {
    pub allowed_times: u8,
    pub max_risk_score: u8,
    pub require_2fa_for_write: bool,
    pub idle_timeout_secs: u32,
}

impl Default for ContextConstraints {
    fn default() -> Self {
        Self {
            allowed_times: 0xFF,
            max_risk_score: 100,
            require_2fa_for_write: false,
            idle_timeout_secs: 3600,
        }
    }
}

impl ContextConstraints {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_time_constraint(mut self, times: u8) -> Self {
        self.allowed_times = times;
        self
    }

    pub fn with_max_risk(mut self, risk: u8) -> Self {
        self.max_risk_score = risk;
        self
    }

    pub fn require_2fa_for_write(mut self) -> Self {
        self.require_2fa_for_write = true;
        self
    }

    pub fn check(&self, context: &PermissionContext, is_write: bool) -> Result<(), DenyReason> {
        if !context.time_context.matches_mask(self.allowed_times) {
            return Err(DenyReason::TimeConstraint);
        }

        if context.get_combined_risk() > self.max_risk_score {
            return Err(DenyReason::HighRisk);
        }

        if is_write && self.require_2fa_for_write 
            && context.session_context.login_method != LoginMethod::Biometric
            && context.session_context.login_method != LoginMethod::Elevated {
            return Err(DenyReason::Missing2FA);
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct EnhancedPermissionChecker {
    trust_chain: TrustChain,
    token_manager: TokenManager,
}

unsafe impl Send for EnhancedPermissionChecker {}
unsafe impl Sync for EnhancedPermissionChecker {}

impl Default for EnhancedPermissionChecker {
    fn default() -> Self {
        Self {
            trust_chain: TrustChain::new(),
            token_manager: TokenManager::new(),
        }
    }
}

impl EnhancedPermissionChecker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check_permission(
        &self,
        pwid: u64,
        owner_pwid: u64,
        pwid_level: PwidLevel,
        pwid_flags: u8,
        pwid_caps: &CapabilityMatrix,
        access_type: CapBits,
        domain: CapDomain,
        other_perms: u16,
        context: &PermissionContext,
        constraints: Option<&ContextConstraints>,
        mandatory: Option<&MandatoryReqs>,
    ) -> PermissionResult {

        if (pwid_flags as u16) & PwidFlags::DISABLED.bits() != 0 {
            return PermissionResult::Denied(DenyReason::Disabled);
        }

        if (pwid_flags as u16) & PwidFlags::EXPIRED.bits() != 0 {
            return PermissionResult::Denied(DenyReason::Expired);
        }

        // ✅ 修复 P0-2: 移除 Root bypass — v4 设计要求"无天生权限"
        // 所有身份（包括 Root）必须通过能力矩阵检查
        // 旧代码: if pwid_level == PwidLevel::Root { return Allowed... }

        // v4: Mandatory trust level check — deny if level exceeds required minimum
        if let Some(mand) = mandatory {
            if let Some(min_level) = mand.require_trust_level {
                if (pwid_level as u32) > (min_level as u32) {
                    return PermissionResult::Denied(DenyReason::NoPermission);
                }
            }
        }

        if let Some(cons) = constraints {
            if let Err(reason) = cons.check(context, (access_type & FS_CAP_WRITE) != 0) {
                return PermissionResult::Denied(reason);
            }
        }

        if let Some(mand) = mandatory {
            if let Err(reason) = mand.check(context) {
                return PermissionResult::Denied(reason);
            }
        }

        if pwid_caps.has_capability(domain, access_type) {
            return PermissionResult::Allowed {
                source: AllowSource::CapabilityMatrix,
                audit_required: true,
            };
        }

        if pwid == owner_pwid {
            return PermissionResult::Allowed {
                source: AllowSource::Owner,
                audit_required: false,
            };
        }

        if let Some(_level) = self.trust_chain.check_chain(
            pwid,
            owner_pwid,
            domain,
            access_type,
            8,
        ) {
            return PermissionResult::Allowed {
                source: AllowSource::TrustChain(owner_pwid),
                audit_required: true,
            };
        }

        let valid_tokens = self.token_manager.find_valid_tokens(pwid, domain, access_type);
        for (_idx, token) in valid_tokens {
            if token.is_scoped() {
                // L1 预留：检查 Token 路径范围
                let _path = context.location_context.get_path();
                // 暂时跳过路径检查（L1 功能待完整实现）
                // if let Some(path) = _path { 
                //     if !token.check_scope(path) { continue; }
                // }
            }

            return PermissionResult::Allowed {
                source: AllowSource::Token(token.token_id),
                audit_required: true,
            };
        }

        let traditional_perm = (other_perms as CapBits) & 0x07;
        if domain == CAP_DOMAIN_FS && (traditional_perm & access_type) == access_type {
            return PermissionResult::Allowed {
                source: AllowSource::OtherPermissions,
                audit_required: false,
            };
        }

        PermissionResult::Denied(DenyReason::NoPermission)
    }

    pub fn get_trust_chain_mut(&mut self) -> &mut TrustChain {
        &mut self.trust_chain
    }

    pub fn get_token_manager_mut(&mut self) -> &mut TokenManager {
        &mut self.token_manager
    }

    pub fn create_elevation_token(
        &mut self,
        issuer: u64,
        holder: u64,
        domains: &[CapDomain],
        caps: &[CapBits],
        duration_secs: u64,
        max_uses: u32,
    ) -> Option<u64> {
        let now = get_time();
        let until = if duration_secs > 0 { now + duration_secs * 3_000_000_000 } else { 0 };

        let token = PwidToken::new(TokenType::Elevation, issuer, holder)
            .with_capabilities(domains, caps)
            .with_validity(now, until)
            .with_uses(max_uses)
            .with_flags(TOKEN_FLAG_AUDIT_ALL);

        self.token_manager.create(token)
    }

    pub fn use_elevation_token(
        &mut self,
        token_id: u64,
    ) -> Result<(), ()> {
        self.token_manager.use_token(token_id)
    }

    pub fn revoke_token(&mut self, token_id: u64, revoker: u64) -> bool {
        self.token_manager.revoke(token_id, revoker)
    }

    pub fn add_trust(
        &mut self,
        truster: u64,
        trusted: u64,
        level: TrustLevel,
        domain: CapDomain,
        mask: CapBits,
        expires_at: u64,
    ) -> Result<(), ()> {
        let entry = TrustEntry::new(trusted, truster, level, domain, mask, expires_at, 0);
        self.trust_chain.add(entry)
    }

    pub fn remove_trust(
        &mut self,
        truster: u64,
        trusted: u64,
        domain: CapDomain,
    ) -> bool {
        self.trust_chain.remove(truster, trusted, domain)
    }

    pub fn cleanup(&mut self) {
        self.trust_chain.clear_expired();
        self.token_manager.clear_expired();
    }
}

fn get_time() -> u64 {
    let tsc: u64;
    unsafe {
        core::arch::asm!("rdtsc", out("rax") tsc, out("rdx") _, options(nomem, nostack));
    }
    tsc
}
