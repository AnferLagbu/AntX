//! PWID v5 Management System
//!
//! Zero-concept + numeric privilege level + kernel isolation + First Token.
//! PWID初心: 密码决定身份 | 无预设特权 | 能力来自授予

#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

pub use serial_println;

pub mod types;
pub mod sha256;
pub mod capability;
pub mod kernel_cap;
pub mod table;
pub mod grant_record;
pub mod first_token;
pub mod engine;
pub mod session;
pub mod audit;
pub mod storage;
pub mod ffi;

pub use types::*;
pub use table::PwidTable;
pub use session::SessionManager;
pub use audit::AuditLog;
