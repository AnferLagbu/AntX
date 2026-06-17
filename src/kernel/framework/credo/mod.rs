//! Credo v1 Identity & Capability System
//!
//! 域身份 (DID) + 能力矩阵 + 会话管理 + 审计.
//! Credo: 密码决定身份 | 能力来自授予 | POSIX DAC + 能力双路径

#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

pub use serial_println;

pub mod audit;
pub mod bootstrap;
pub mod capability;
pub mod csprng;
pub mod engine;
pub mod api;
pub mod grant;
pub mod identity;
pub mod session;
pub mod sha256;
/// D6: 安全启动 + TPM 2.0
pub mod secure_boot;
pub mod storage;
pub mod types;

pub use audit::AuditLog;
pub use identity::IdentityTable;
pub use types::*;

// api 公共接口 re-export — 避免跨子系统直接访问 credo::api 内部
pub use api::*;

// session 公共接口 re-export — 避免跨子系统直接访问 credo::session 内部
pub use session::*;

// engine 公共接口 re-export — 避免跨子系统直接访问 credo::engine 内部
pub use engine::get_privilege_level;

// secure_boot 公共接口 re-export — 避免跨子系统直接访问 credo::secure_boot 内部
pub use secure_boot::*;

/// Credo 子系统初始化 — 安全启动 + TPM.
///
/// 从 scheduler_init 中分离, 消除 proc→credo 的初始化依赖.
/// 应在 scheduler_init 之后调用.
#[no_mangle]
pub fn credo_init() {
    use secure_boot::{secure_boot_init, tpm_init, Ed25519PubKey};
    // 默认平台密钥 (全零, 开发阶段)
    let default_pk = Ed25519PubKey::new([0u8; 32]);
    secure_boot_init(default_pk);
    tpm_init();
}
