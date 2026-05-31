//! Credo v1 Identity & Capability System
//!
//! Domain Identity (DID) + capability matrix + session management + audit.
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
pub mod ffi;
pub mod grant;
pub mod identity;
pub mod session;
pub mod sha256;
pub mod storage;
pub mod types;

pub use audit::AuditLog;
pub use identity::IdentityTable;
pub use session::SessionManager;
pub use types::*;
