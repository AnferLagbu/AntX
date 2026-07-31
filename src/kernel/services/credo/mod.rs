#![deny(unsafe_code)]
//! 身份与权限 — PWM/能力矩阵/会话 (services 层)
//!
//! ## 框内核中的 PWID 表达
//!
//! ```text
//! framework/credo/                  ← TCB (unsafe 允许)
//!   ├─ atomic_matrix.rs              ← 16×64 AtomicU64 物理存储
//!   ├─ password.rs                   ← SHA-256 + 常数时间比较
//!   └─ persist.rs                    ← 磁盘序列化
//!
//! services/credo/ (本模块)         ← 100% safe Rust
//!   ├─ policy.rs                     ← 能力检查策略 ✅
//!   ├─ grants.rs                     ← 委托规则 ✅
//!   ├─ sessions.rs                   ← 会话生命周期 ✅
//!   └─ audit.rs                      ← 审计日志生成 ✅
//! ```
//!
//! ## @SAFE
//! 本文件不含 `unsafe`. 所有硬件交互通过 `framework::credo` 的安全 API.

pub mod audit;
/// T6-8: PWM 能力常量定义 (原 framework/credo/capability.rs)
pub mod capability;
pub mod crypto;
pub mod grants;
pub mod identity;
/// D6: 安全启动 + TPM 安全封装
pub mod secure_boot;
pub mod policy;
pub mod sessions;
/// Credo 认证策略 — PWM 登录/登出/创建/删除/验证/授权
pub mod auth;
/// T6-8: SHA-256 哈希实现 (原 framework/credo/sha256.rs)
pub mod sha256;
/// T6-7: Credo 类型定义 (原 framework/credo/types.rs)
pub mod types;
pub mod uid;
/// Credo 私有存储子系统 — 块设备/格式化/分区 safe 代理
pub mod storage;

pub use audit::{AuditEvent, AuditEventKind, AuditLog, HashChainNode, AUDIT_BUFFER_SIZE};
pub use grants::{
    DelegationDeny, DelegationEngine, DelegationResult, GrantFlags, GrantRecord, GrantTable,
    MAX_GRANT_RECORDS,
};
pub use policy::{
    CapBits, CapDomain, CapabilityMatrix, DenyReason, GrantResult, InMemoryMatrix,
    PolicyEngine, PolicyResult, RevokeResult, VIABLE_FLOOR, CAP_DOMAINS,
};
pub use sessions::{
    LoginDeny, LoginResult, Session, SessionError, SessionId, SessionManager, SessionState,
    SessionTable, MAX_SESSIONS,
};
pub use identity::{PwmError, PwmId, PwmResult};
pub use capability::{
    CAP_DOMAIN_SYSTEM, CAP_DOMAIN_FS, CAP_DOMAIN_NET, CAP_DOMAIN_PROC,
    CAP_DOMAIN_DEVICE, CAP_DOMAIN_USER_MGMT, CAP_DOMAIN_IPC, CAP_DOMAIN_MEM,
    CAP_DOMAIN_TIME, DEVICE_CAP_MMIO, DEVICE_CAP_IRQ, DEVICE_CAP_DMA, DEVICE_CAP_BIND,
};
