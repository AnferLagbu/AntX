//! PWID Context Types
//!
//! 定义权限检查上下文相关的类型（v4 规范 L1/L2 层预留）

use super::types::*;

/// 会话类型 (L1 Sensitivity Label 预留)
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum SessionType {
    Local = 0,
    Remote = 1,
    Service = 2,
}

impl Default for SessionType {
    fn default() -> Self { SessionType::Local }
}

/// 登录方式
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum LoginMethod {
    Password = 0,
    Token = 1,
    Biometric = 2,
    Elevated = 3,  // 权限提升
}

impl Default for LoginMethod {
    fn default() -> Self { LoginMethod::Password }
}

/// 时间段限制 (L1 预留)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeOfDay {
    pub start_hour: u8,
    pub end_hour: u8,
}

impl Default for TimeOfDay {
    fn default() -> Self {
        TimeOfDay { start_hour: 0, end_hour: 24 }
    }
}

/// 会话上下文信息
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub session_type: SessionType,
    pub login_method: LoginMethod,
    pub login_time: u64,
}

/// 权限检查完整上下文 (v4 五层模型 L0-L4 输入)
#[derive(Debug, Clone)]
pub struct PermissionContext {
    /// 当前请求的 PWID
    pub pwid: u64,
    
    /// 目标对象/资源
    pub target: u64,
    
    /// 请求的操作域和位掩码
    pub domain: CapDomain,
    pub requested_caps: CapBits,
    
    /// 会话上下文
    pub session_context: SessionContext,
    
    /// 是否写操作
    pub is_write: bool,
    
    /// 时间上下文 (L1 Sensitivity Label 预留)
    pub time_context: TimeContext,
    
    /// 位置上下文 (L1 预留)
    pub location_context: LocationContext,
}

/// 位置上下文 (L1 预留)
#[derive(Debug, Clone, Default)]
pub struct LocationContext {
    pub is_local: bool,
    pub path: Option<alloc::string::String>,  // L1 预留：路径信息
}

impl LocationContext {
    /// 获取路径 (L1 预留)
    pub fn get_path(&self) -> Option<&str> {
        self.path.as_ref().map(|s| s.as_str())
    }
}

/// 时间上下文 (L1 预留)
#[derive(Debug, Clone, Default)]
pub struct TimeContext {
    pub time_of_day: TimeOfDay,
}

impl TimeContext {
    /// 检查当前时间是否在允许的时间段内
    pub fn matches_mask(&self, _allowed_times: u8) -> bool {
        // L1 预留：暂时返回 true（时间限制功能待实现）
        true
    }
}

impl Default for PermissionContext {
    fn default() -> Self {
        Self {
            pwid: 0,
            target: 0,
            domain: 0,
            requested_caps: 0,
            session_context: SessionContext::default(),
            is_write: false,
            time_context: TimeContext::default(),
            location_context: LocationContext::default(),
        }
    }
}

impl PermissionContext {
    /// 计算组合风险分数 (v4 L1 Sensitivity Label 预留)
    /// 返回值范围: 0-255 (越高越危险, u8 类型以匹配 SensitivityPolicy.max_risk_score)
    pub fn get_combined_risk(&self) -> u8 {
        let mut risk: u16 = 0;
        
        // 基础风险：写操作比读操作更危险
        if self.is_write { risk += 200; }
        
        // 登录方式风险
        match self.session_context.login_method {
            LoginMethod::Password => risk += 100,
            LoginMethod::Token    => risk += 150,
            LoginMethod::Biometric => risk += 50,
            LoginMethod::Elevated => risk += 300,  // 权限提升最危险
        }
        
        // 会话类型风险
        match self.session_context.session_type {
            SessionType::Local   => risk += 0,
            SessionType::Remote  => risk += 150,
            SessionType::Service => risk += 100,
        }
        
        risk.min(255) as u8  // 上限 255 (u8)
    }
}
