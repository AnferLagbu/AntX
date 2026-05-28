//! Credo v1 Identity & Capability System
//!
//! Domain Identity (DID) + capability matrix + session management + audit.
//! Credo: 密码决定身份 | 能力来自授予 | POSIX DAC + 能力双路径

#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

pub use serial_println;

pub mod types;
pub mod sha256;
pub mod capability;
pub mod identity;
pub mod grant;
pub mod bootstrap;
pub mod engine;
pub mod session;
pub mod audit;
pub mod storage;
pub mod ffi;

pub use types::*;
pub use identity::IdentityTable;
pub use session::SessionManager;
pub use audit::AuditLog;