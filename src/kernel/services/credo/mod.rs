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
pub mod crypto;
pub mod grants;
pub mod identity;
pub mod policy;
pub mod sessions;
pub mod uid;

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
