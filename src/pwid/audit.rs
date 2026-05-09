//! Audit Log System
//!
//! Records security-relevant events for forensics and compliance.

use super::types::*;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Maximum number of audit entries to keep
const MAX_AUDIT_ENTRIES: usize = 256;

/// Audit Log - records PWID-related security events
pub struct AuditLog {
    /// Array of audit entries
    entries: core::cell::UnsafeCell<[AuditEntry; MAX_AUDIT_ENTRIES]>,
    
    /// Current number of entries
    count: AtomicUsize,
}

impl AuditLog {
    pub const fn new() -> Self {
        const DEFAULT_ENTRY: AuditEntry = AuditEntry {
            timestamp: 0,
            pwid: 0,
            action: 0,
            result: 0,
            target_pwid: 0,
            details: 0,
        };
        
        Self {
            entries: core::cell::UnsafeCell::new([DEFAULT_ENTRY; MAX_AUDIT_ENTRIES]),
            count: AtomicUsize::new(0),
        }
    }

    /// Record an audit event
    pub fn log(&self, pwid: u64, action: u32, result: u32, target_pwid: u64, details: u64) {
        let mut count = self.count.load(Ordering::Acquire);
        
        // If at capacity, shift entries (FIFO) — attempt atomic bump
        loop {
            if count >= MAX_AUDIT_ENTRIES {
                unsafe {
                    let entries = &mut *self.entries.get();
                    for i in 0..MAX_AUDIT_ENTRIES - 1 {
                        entries[i] = entries[i + 1];
                    }
                }
                match self.count.compare_exchange_weak(count, MAX_AUDIT_ENTRIES - 1, Ordering::Release, Ordering::Relaxed) {
                    Ok(_) => break,
                    Err(c) => count = c,
                }
            } else {
                break;
            }
        }
        
        // Add new entry
        if let Ok(idx) = self.count.fetch_update(Ordering::Acquire, Ordering::Relaxed, |c| {
            if c < MAX_AUDIT_ENTRIES { Some(c + 1) } else { None }
        }) {
            unsafe {
                let entries = &mut *self.entries.get();
                entries[idx] = AuditEntry {
                    timestamp: get_current_time(),
                    pwid,
                    action,
                    result,
                    target_pwid,
                    details,
                };
            }
        }
    }

    /// Dump all audit entries to serial
    pub fn dump(&self) {
        let count = self.count.load(Ordering::Acquire);
        
        serial_println!("\n=== PWID Audit Log ===");
        
        unsafe {
            let entries = &*self.entries.get();
            
            for i in 0..count.min(MAX_AUDIT_ENTRIES) {
                let _e = &entries[i];
                
                serial_println!("  [{}] PWID:0x{:016X} Action:{} Result:{}",
                               e.timestamp, e.pwid, e.action, e.result);
            }
        }
        
        serial_println!("=====================");
    }

    /// Get entry count
    pub fn get_count(&self) -> usize {
        self.count.load(Ordering::Acquire)
    }

    /// Get all entries (for persistence)
    pub fn get_entries(&self) -> &[AuditEntry] {
        let count = self.count.load(Ordering::Acquire).min(MAX_AUDIT_ENTRIES);
        
        unsafe {
            let entries = &*self.entries.get();
            &entries[..count]
        }
    }
}

/// Get current time from TSC register
fn get_current_time() -> u64 {
    unsafe {
        let tsc: u64;
        core::arch::asm!(
            "rdtsc",
            out("eax") tsc,
            options(nostack, nomem)
        );
        tsc / 3_000_000_000u64
    }
}

// Global instance
static mut GLOBAL_AUDIT: AuditLog = AuditLog::new();

/// Get reference to global audit log
pub fn get_audit() -> &'static AuditLog {
    unsafe { &GLOBAL_AUDIT }
}
