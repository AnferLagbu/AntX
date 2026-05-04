//! PWID (Process/User ID) Management System - Rust Implementation
//!
//! Complete rewrite of the C implementation with enhanced safety and type guarantees.
//! Provides:
//! - SHA-256 password hashing
//! - User identity management (CRUD)
//! - Trust level and permission system
//! - Session management and context switching
//! - Audit logging
//! - Persistent storage via HVFS
//! - Security features (expiry, lockout, brute-force protection)
//! - Privilege elevation token system

/// Serial print macro (placeholder for kernel serial output)
#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => {};
}

pub use serial_println;

pub mod types;
pub mod sha256;
pub mod manager;
pub mod session;
pub mod audit;
pub mod storage;
pub mod trust_chain;
pub mod ffi;

// Re-export main types for convenience
pub use types::*;
pub use manager::PwidManager;
pub use session::SessionManager;
pub use audit::AuditLog;
