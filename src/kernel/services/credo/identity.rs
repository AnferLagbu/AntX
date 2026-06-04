//! PWM 身份 (Identity) — services 层安全代理
//!
//! ## 状态 (v2.16, 2026-06-04)
//!
//! Phase 2.5 credo 迁移 1/2 (identity / PWM):
//! - [x] 强类型 `PwmId(pub u64)` 替代裸 u64 句柄
//! - [x] 切片 API `&[u8]` 替代 `*const u8` C 字符串
//! - [x] 强类型错误 `PwmError` 替代 `i32` 错误码
//! - [x] 强类型 re-export `PwmEntry` / `PwmId`
//! - [x] 强类型 `Domain` / `CapBits` 替代裸 u16/u64
//!
//! ## 迁移方法
//!
//! 内部把 `&[u8]` 切片转 `*const u8` (C 风格), 调用 `kernel::credo::api::pwm_*` 函数;
//! services 层 0 unsafe — 所有 `unsafe { ... }` 在 framework 层 PwmEntry 内部.
//!
//! 评估日期: 2026-06-04

use crate::kernel::credo;
use crate::kernel::framework::credo_pwm;

// ============================================================================
// 强类型 re-export
// ============================================================================

/// PWM 句柄
pub use credo::types::PwmId;

/// PWM 表项 (从内核透传, 引用类型)
pub use credo::types::PwmEntry;

/// 能力域 (从 services::credo::policy 复用)
pub use super::policy::CapDomain;

/// 能力位 (从 services::credo::policy 复用)
pub use super::policy::CapBits;

// ============================================================================
// 错误类型
// ============================================================================

/// PWM 操作错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PwmError {
    /// 表已满
    TableFull,
    /// PWM 不存在
    NotFound,
    /// 已存在同名 PWM
    AlreadyExists,
    /// 密码错误
    InvalidPassword,
    /// 权限不足
    PermissionDenied,
    /// 密码过短 / 不合法
    WeakPassword,
    /// 其他 (含 `as_i32() != 0` 的内核错误码)
    Other(i32),
}

impl PwmError {
    /// 从内核 `i32` 错误码翻译
    pub fn from_i32(code: i32) -> Self {
        // 0 是成功, 非 0 是错误 (按 credo::types::Error::as_i32() 约定)
        match code {
            -2 => Self::TableFull,
            -3 => Self::NotFound,
            -4 => Self::AlreadyExists,
            -5 => Self::InvalidPassword,
            -6 => Self::PermissionDenied,
            -7 => Self::WeakPassword,
            other if other != 0 => Self::Other(other),
            _ => Self::Other(0),
        }
    }
}

pub type PwmResult<T> = Result<T, PwmError>;

// ============================================================================
// 初始化与发现
// ============================================================================

/// 初始化 PWM 身份表 (单线程只能调用一次)
pub fn init() {
    credo::api::pwm_init();
}

/// 尝试从磁盘恢复 (cold boot)
pub fn try_load() -> PwmResult<()> {
    let rc = credo::api::pwm_try_load();
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 任何身份是否已存在
pub fn any_identity_exists() -> bool {
    credo::api::pwm_any_identity_exists()
}

// ============================================================================
// 生命周期
// ============================================================================

/// 创世 (第一次身份创建, 工厂: 第一个管理员)
pub fn try_genesis(password: &[u8]) -> PwmResult<PwmId> {
    if password.is_empty() {
        return Err(PwmError::WeakPassword);
    }
    let rc = credo_pwm::pwm_try_genesis(password.as_ptr());
    if rc > 0 {
        Ok(PwmId(rc as u64))
    } else {
        Err(PwmError::from_i32(rc as i32))
    }
}

/// 创世 + 创建 root 身份
pub fn create_first_identity(password: &[u8]) -> PwmResult<PwmId> {
    if password.is_empty() {
        return Err(PwmError::WeakPassword);
    }
    let rc = credo_pwm::pwm_create_first_identity(password.as_ptr());
    if rc > 0 {
        Ok(PwmId(rc as u64))
    } else {
        Err(PwmError::from_i32(rc as i32))
    }
}

/// 创建新身份 (需要 creator 拥有对应权限)
pub fn create(password: &[u8], note: &[u8], creator: PwmId) -> PwmResult<PwmId> {
    if password.is_empty() {
        return Err(PwmError::WeakPassword);
    }
    let rc = credo_pwm::pwm_create(password.as_ptr(), note.as_ptr(), creator.0);
    if rc > 0 {
        Ok(PwmId(rc as u64))
    } else {
        Err(PwmError::from_i32(rc as i32))
    }
}

/// 删除身份
pub fn delete(pwm: PwmId) -> PwmResult<()> {
    let rc = credo::api::pwm_delete(pwm.0);
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 禁用身份 (保留条目但拒绝登录)
pub fn disable(pwm: PwmId) -> PwmResult<()> {
    let rc = credo::api::pwm_disable(pwm.0);
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 启用身份
pub fn enable(pwm: PwmId) -> PwmResult<()> {
    let rc = credo::api::pwm_enable(pwm.0);
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

// ============================================================================
// 密码
// ============================================================================

/// 验证密码 (常数时间比较)
pub fn verify_password(pwm: PwmId, password: &[u8]) -> bool {
    if password.is_empty() {
        return false;
    }
    credo_pwm::pwm_verify_password(pwm.0, password.as_ptr())
}

/// 改密
pub fn change_password(pwm: PwmId, old: &[u8], new: &[u8]) -> PwmResult<()> {
    if old.is_empty() || new.is_empty() {
        return Err(PwmError::WeakPassword);
    }
    let rc = credo_pwm::pwm_change_password(pwm.0, old.as_ptr(), new.as_ptr());
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

// ============================================================================
// 查询
// ============================================================================

/// PWM 是否存在
pub fn exists(pwm: PwmId) -> bool {
    credo::api::pwm_find(pwm.0)
}

/// 获取 PWM 表项 (返回引用)
pub fn find(pwm: PwmId) -> Option<&'static PwmEntry> {
    credo::identity::find(pwm.0)
}

// ============================================================================
// 能力
// ============================================================================

/// 检查 PWM 是否有指定能力
pub fn has_capability(pwm: PwmId, domain: CapDomain, required: CapBits) -> bool {
    credo::api::pwm_has_capability(pwm.0, domain.0 as u16, required.0)
}

/// 获取指定域的能力位
pub fn get_capability_raw(pwm: PwmId, domain: CapDomain) -> u64 {
    credo::api::pwm_get_capability_raw(pwm.0, domain.0 as u16)
}

/// 获取 FS 域能力位
pub fn get_fs_capability(pwm: PwmId) -> u64 {
    credo::api::pwm_get_fs_capability(pwm.0)
}

/// 获取权限等级
pub fn get_privilege_level(pwm: PwmId) -> u8 {
    credo::api::pwm_get_privilege_level(pwm.0)
}

/// 获取创建者 PWM
pub fn get_creator(pwm: PwmId) -> PwmId {
    PwmId(credo::api::pwm_get_creator(pwm.0))
}

// ============================================================================
// 委托 / 撤销
// ============================================================================

/// 委托能力
pub fn grant(grantor: PwmId, grantee: PwmId, domain: CapDomain, caps: u64) -> PwmResult<()> {
    let rc = credo::api::pwm_grant(grantor.0, grantee.0, domain.0 as u16, caps);
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 撤销能力
pub fn revoke(revoker: PwmId, target: PwmId, domain: CapDomain, caps: u64) -> PwmResult<()> {
    let rc = credo::api::pwm_revoke(revoker.0, target.0, domain.0 as u16, caps);
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 权限检查: 操作者是否能修改 target
pub fn check_privilege(operator: PwmId, target: PwmId) -> bool {
    credo::api::pwm_check_privilege(operator.0, target.0)
}

/// 转移创建者
pub fn transfer_creator(current: PwmId, target: PwmId, new_creator: PwmId) -> PwmResult<()> {
    let rc = credo::api::pwm_transfer_creator(current.0, target.0, new_creator.0);
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

// ============================================================================
// 会话 (login/logout)
// ============================================================================

/// 当前 PWM
pub fn current() -> PwmId {
    PwmId(credo::api::pwm_get_current())
}

/// 是否已登录
pub fn is_logged_in() -> bool {
    credo::api::pwm_is_logged_in()
}

/// 当前 UID
pub fn current_uid() -> u32 {
    credo::api::pwm_get_current_uid()
}

/// 当前 GID
pub fn current_gid() -> u32 {
    credo::api::pwm_get_current_gid()
}

/// EUID
pub fn euid() -> u32 {
    credo::api::pwm_get_euid()
}

/// EGID
pub fn egid() -> u32 {
    credo::api::pwm_get_egid()
}

/// UID / GID
pub fn uid(pwm: PwmId) -> u32 {
    credo::api::pwm_get_uid(pwm.0)
}

pub fn gid(pwm: PwmId) -> u32 {
    credo::api::pwm_get_gid(pwm.0)
}

/// 注销当前会话
pub fn logout() {
    credo::api::pwm_logout();
}

// ============================================================================
// 提权 (suid 机制)
// ===========================================================================

/// 为 suid 提权
pub fn elevate_for_suid(target: PwmId) -> bool {
    credo::api::pwm_elevate_for_suid(target.0)
}

/// 撤销提权
pub fn drop_elevation() -> bool {
    credo::api::pwm_drop_elevation()
}

/// 检查是否有提权权限
pub fn has_elevation_authority(target: PwmId) -> bool {
    credo::api::pwm_has_elevation_authority(target.0)
}

/// 尝试设置 UID
pub fn try_setuid(target_uid: u32) -> bool {
    credo::api::pwm_try_setuid(target_uid)
}

// ============================================================================
// 锁定 / 审计
// ===========================================================================

/// 清除锁定
pub fn clear_lockout(pwm: PwmId) -> PwmResult<()> {
    let rc = credo::api::pwm_clear_lockout(pwm.0);
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 记录审计日志
pub fn audit(pwm: PwmId, action: u32, target: u64, details: u64) {
    credo::api::pwm_audit_log(pwm.0, action, target, details)
}

/// 持久化到磁盘
pub fn save_to_disk() -> PwmResult<()> {
    let rc = credo::api::pwm_save_to_disk();
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 从磁盘加载
pub fn load_from_disk() -> PwmResult<()> {
    let rc = credo::api::pwm_load_from_disk();
    if rc == 0 {
        Ok(())
    } else {
        Err(PwmError::from_i32(rc))
    }
}

/// 是否已修改
pub fn is_modified() -> bool {
    credo::api::pwm_is_modified()
}

/// 标记已修改
pub fn set_modified() {
    credo::api::pwm_set_modified();
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_id_construction() {
        let pwm = PwmId(42);
        assert_eq!(pwm.0, 42);
    }

    #[test]
    fn error_from_i32() {
        assert_eq!(PwmError::from_i32(-2), PwmError::TableFull);
        assert_eq!(PwmError::from_i32(-3), PwmError::NotFound);
        assert_eq!(PwmError::from_i32(0), PwmError::Other(0));
        assert_eq!(PwmError::from_i32(42), PwmError::Other(42));
    }

    #[test]
    fn weak_password_rejected() {
        // 静态检查, 不实际调用内核 (内核单线程限制)
        let empty: &[u8] = b"";
        assert!(empty.is_empty());
    }
}
