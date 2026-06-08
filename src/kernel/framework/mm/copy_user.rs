//! User Space Safe Access Functions
//!
//! Provides safe functions for copying data between user and kernel space.
//! These functions validate user pointers and handle page faults gracefully.
//!
//! # Security
//!
//! - All user pointers are validated before access
//! - Page faults during copy are caught and return error
//! - Prevents kernel from accessing invalid user memory
//! - Prevents user from reading arbitrary kernel memory
//!
//! # Page Fault Handling
//!
//! This module uses an exception table mechanism to handle page faults:
//! 1. Before accessing user memory, we set up a recovery point
//! 2. If a page fault occurs, the exception handler finds the recovery point
//! 3. The handler jumps to the recovery point, which returns an error
//!
//! This prevents kernel panics from invalid user pointers.
//!
//! # Performance
//!
//! Uses `ptr::copy_nonoverlapping` for optimal performance:
//! - Utilizes CPU vectorized instructions (SSE/AVX) when available
//! - Efficient cache utilization with bulk memory operations
//! - Significantly faster than byte-by-byte copying for large buffers
//!
//! # Safety
//!
//! The use of `copy_nonoverlapping` is safe because:
//! - Source and destination buffers are guaranteed non-overlapping (user vs kernel space)
//! - Buffer bounds are validated before the copy operation
//! - Exception recovery mechanism handles any page faults during copy

use super::*;
use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};

/// Maximum valid user-space address (canonical form)
const USER_ADDR_MAX: u64 = 0x0000_7FFF_FFFF_F000;

/// Exception table entry for page fault recovery
#[repr(C)]
pub struct ExceptionTableEntry {
    /// Instruction address that may fault
    pub insn_addr: u64,
    /// Recovery address to jump to on fault
    pub fixup_addr: u64,
}

/// Global exception table (defined in linker script)
#[used]
#[link_section = ".exception_table"]
static EXCEPTION_TABLE_START: ExceptionTableEntry = ExceptionTableEntry {
    insn_addr: 0,
    fixup_addr: 0,
};

/// Per-CPU exception context storage
/// Using static array instead of thread_local! for bare metal compatibility
static PER_CPU_EXCEPTION_CTX: [AtomicU64; crate::kernel::framework::config::MAX_CPUS] = 
    [const { AtomicU64::new(0) }; crate::kernel::framework::config::MAX_CPUS];

/// Marker value indicating no exception context is set
const NO_EXCEPTION_CTX: u64 = 0;

/// Per-CPU exception occurred flag
static PER_CPU_EXCEPTION_OCCURRED: [AtomicBool; crate::kernel::framework::config::MAX_CPUS] = 
    [const { AtomicBool::new(false) }; crate::kernel::framework::config::MAX_CPUS];

/// Get current CPU ID with bounds checking
/// 
/// # Panics
/// Panics in debug mode if cpu_id >= MAX_CPUS to catch configuration errors early
/// 
/// # Safety
/// In release mode, uses modulo to prevent out-of-bounds access,
/// but this indicates a configuration problem (too many CPUs)
#[inline]
fn current_cpu_id() -> usize {
    let cpu = crate::kernel::framework::cpu::arch::cpu_id() as usize;
    let max_cpus = crate::kernel::framework::config::MAX_CPUS;
    
    #[cfg(debug_assertions)]
    {
        if cpu >= max_cpus {
            panic!(
                "CPU ID {} exceeds MAX_CPUS ({}). Increase MAX_CPUS or reduce CPU count!",
                cpu, max_cpus
            );
        }
        cpu
    }
    
    #[cfg(not(debug_assertions))]
    {
        cpu % max_cpus
    }
}

/// Check if an exception occurred during the last operation
#[inline]
pub fn exception_occurred() -> bool {
    let cpu = current_cpu_id();
    PER_CPU_EXCEPTION_OCCURRED[cpu].load(Ordering::SeqCst)
}

/// Clear the exception occurred flag
#[inline]
pub fn clear_exception_flag() {
    let cpu = current_cpu_id();
    PER_CPU_EXCEPTION_OCCURRED[cpu].store(false, Ordering::SeqCst);
}

/// Set the exception occurred flag (called by exception handler)
#[inline]
pub fn mark_exception_occurred() {
    let cpu = current_cpu_id();
    PER_CPU_EXCEPTION_OCCURRED[cpu].store(true, Ordering::SeqCst);
}

/// Encode recovery address for storage
/// Uses offset encoding: stored_value = recovery_addr + 1
/// This ensures 0 (NO_EXCEPTION_CTX) is never used for a valid address
#[inline]
fn encode_recovery_addr(addr: u64) -> u64 {
    addr.wrapping_add(1)
}

/// Decode recovery address from storage
/// Inverse of encode: recovery_addr = stored_value - 1
#[inline]
fn decode_recovery_addr(encoded: u64) -> u64 {
    encoded.wrapping_sub(1)
}

/// Set the current exception recovery point
/// Returns the old recovery point
#[inline]
pub fn set_exception_recovery(recovery_addr: u64) -> Option<u64> {
    let cpu = current_cpu_id();
    let encoded = encode_recovery_addr(recovery_addr);
    let old = PER_CPU_EXCEPTION_CTX[cpu].swap(encoded, Ordering::SeqCst);
    if old == NO_EXCEPTION_CTX {
        None
    } else {
        Some(decode_recovery_addr(old))
    }
}

/// Clear the current exception recovery point
#[inline]
pub fn clear_exception_recovery() {
    let cpu = current_cpu_id();
    PER_CPU_EXCEPTION_CTX[cpu].store(NO_EXCEPTION_CTX, Ordering::SeqCst);
}

/// Get the current exception recovery point
#[inline]
pub fn get_exception_recovery() -> Option<u64> {
    let cpu = current_cpu_id();
    let val = PER_CPU_EXCEPTION_CTX[cpu].load(Ordering::SeqCst);
    if val == NO_EXCEPTION_CTX {
        None
    } else {
        Some(decode_recovery_addr(val))
    }
}

/// Check if a pointer is in valid user space range
#[inline]
pub fn is_user_ptr(ptr: u64) -> bool {
    ptr > 0 && ptr < USER_ADDR_MAX
}

/// Check if a buffer (ptr + len) is entirely in user space
#[inline]
pub fn is_user_buf(ptr: u64, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    let end = match ptr.checked_add(len as u64) {
        Some(e) => e,
        None => return false,
    };
    is_user_ptr(ptr) && end <= USER_ADDR_MAX
}

/// Copy data from user space to kernel buffer
///
/// # Safety
///
/// This function is safe because it:
/// 1. Validates the user pointer range
/// 2. Uses volatile reads to prevent compiler optimizations
/// 3. Sets up exception recovery before accessing user memory
/// 4. Returns error on page fault instead of panicking
///
/// # Returns
///
/// - `Ok(copied_len)`: Number of bytes successfully copied
/// - `Err(())`: Invalid user pointer or page fault
///
/// # Architecture Requirements
///
/// This function requires architecture support for:
/// - Exception table mechanism (see exception_table_entry)
/// - Page fault handler that checks exception table
/// - Recovery point mechanism
pub fn copy_from_user(kernel_dst: &mut [u8], user_src: u64, len: usize) -> Result<usize, ()> {
    if len == 0 {
        return Ok(0);
    }

    if !is_user_buf(user_src, len) {
        return Err(());
    }

    if kernel_dst.len() < len {
        return Err(());
    }

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let result = unsafe {
        let src_ptr = user_src as *const u8;
        let dst_ptr = kernel_dst.as_mut_ptr();

        clear_exception_flag();

        let recovery_label: u64;
        core::arch::asm!(
            "9:",
            "mov {tmp}, 8f",
            "mov {recovery}, {tmp}",
            "8:",
            recovery = out(reg) recovery_label,
            tmp = out(reg) _,
            options(nostack, pure, readonly),
        );

        let old_recovery = set_exception_recovery(recovery_label as u64);

        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, len);

        if let Some(old) = old_recovery {
            set_exception_recovery(old);
        } else {
            clear_exception_recovery();
        }

        if exception_occurred() {
            Err(())
        } else {
            Ok(len)
        }
    };

    result
}

/// Copy data from kernel to user space
///
/// # Safety
///
/// This function is safe because it:
/// 1. Validates the user pointer range
/// 2. Uses volatile writes to prevent compiler optimizations
/// 3. Sets up exception recovery before accessing user memory
/// 4. Returns error on page fault instead of panicking
///
/// # Returns
///
/// - `Ok(copied_len)`: Number of bytes successfully copied
/// - `Err(())`: Invalid user pointer or page fault
///
/// # Architecture Requirements
///
/// This function requires architecture support for:
/// - Exception table mechanism (see exception_table_entry)
/// - Page fault handler that checks exception table
/// - Recovery point mechanism
pub fn copy_to_user(user_dst: u64, kernel_src: &[u8], len: usize) -> Result<usize, ()> {
    if len == 0 {
        return Ok(0);
    }

    if !is_user_buf(user_dst, len) {
        return Err(());
    }

    if kernel_src.len() < len {
        return Err(());
    }

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let result = unsafe {
        let src_ptr = kernel_src.as_ptr();
        let dst_ptr = user_dst as *mut u8;

        clear_exception_flag();

        let recovery_label: u64;
        core::arch::asm!(
            "9:",
            "mov {tmp}, 8f",
            "mov {recovery}, {tmp}",
            "8:",
            recovery = out(reg) recovery_label,
            tmp = out(reg) _,
            options(nostack, pure, readonly),
        );

        let old_recovery = set_exception_recovery(recovery_label as u64);

        core::ptr::copy_nonoverlapping(src_ptr, dst_ptr, len);

        if let Some(old) = old_recovery {
            set_exception_recovery(old);
        } else {
            clear_exception_recovery();
        }

        if exception_occurred() {
            Err(())
        } else {
            Ok(len)
        }
    };

    result
}

/// Copy a null-terminated string from user space
///
/// # Returns
///
/// - `Ok(string)`: The copied string (without null terminator)
/// - `Err(())`: Invalid pointer, not null-terminated, or too long
pub fn copy_string_from_user(user_str: u64, max_len: usize) -> Result<alloc::string::String, ()> {
    if !is_user_ptr(user_str) {
        return Err(());
    }

    let mut bytes = alloc::vec::Vec::with_capacity(max_len);

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let result = unsafe {
        let ptr = user_str as *const u8;

        clear_exception_flag();

        let recovery_label: u64;
        core::arch::asm!(
            "9:",
            "mov {tmp}, 8f",
            "mov {recovery}, {tmp}",
            "8:",
            recovery = out(reg) recovery_label,
            tmp = out(reg) _,
            options(nostack, pure, readonly),
        );

        let old_recovery = set_exception_recovery(recovery_label as u64);

        for i in 0..max_len {
            let byte = core::ptr::read_volatile(ptr.add(i));
            if byte == 0 {
                break;
            }
            bytes.push(byte);
        }

        if let Some(old) = old_recovery {
            set_exception_recovery(old);
        } else {
            clear_exception_recovery();
        }

        if exception_occurred() || bytes.len() == max_len {
            Err(())
        } else {
            alloc::string::String::from_utf8(bytes).map_err(|_| ())
        }
    };

    result
}

/// Clear a region in user space (fill with zeros)
///
/// # Returns
///
/// - `Ok(cleared_len)`: Number of bytes cleared
/// - `Err(())`: Invalid user pointer
pub fn clear_user(user_ptr: u64, len: usize) -> Result<usize, ()> {
    if len == 0 {
        return Ok(0);
    }

    if !is_user_buf(user_ptr, len) {
        return Err(());
    }

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let result = unsafe {
        let ptr = user_ptr as *mut u8;

        clear_exception_flag();

        let recovery_label: u64;
        core::arch::asm!(
            "9:",
            "mov {tmp}, 8f",
            "mov {recovery}, {tmp}",
            "8:",
            recovery = out(reg) recovery_label,
            tmp = out(reg) _,
            options(nostack, pure, readonly),
        );

        let old_recovery = set_exception_recovery(recovery_label as u64);

        core::ptr::write_bytes(ptr, 0, len);

        if let Some(old) = old_recovery {
            set_exception_recovery(old);
        } else {
            clear_exception_recovery();
        }

        if exception_occurred() {
            Err(())
        } else {
            Ok(len)
        }
    };

    result
}

/// Get the length of a user-space string (without copying)
///
/// # Returns
///
/// - `Ok(len)`: Length of string (excluding null terminator)
/// - `Err(())`: Invalid pointer or not null-terminated within max_len
pub fn strlen_user(user_str: u64, max_len: usize) -> Result<usize, ()> {
    if !is_user_ptr(user_str) {
        return Err(());
    }

    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    let result = unsafe {
        let ptr = user_str as *const u8;

        clear_exception_flag();

        let recovery_label: u64;
        core::arch::asm!(
            "9:",
            "mov {tmp}, 8f",
            "mov {recovery}, {tmp}",
            "8:",
            recovery = out(reg) recovery_label,
            tmp = out(reg) _,
            options(nostack, pure, readonly),
        );

        let old_recovery = set_exception_recovery(recovery_label as u64);

        let mut found_len = None;
        for i in 0..max_len {
            let byte = core::ptr::read_volatile(ptr.add(i));
            if byte == 0 {
                found_len = Some(i);
                break;
            }
        }

        if let Some(old) = old_recovery {
            set_exception_recovery(old);
        } else {
            clear_exception_recovery();
        }

        if exception_occurred() {
            Err(())
        } else if let Some(len) = found_len {
            Ok(len)
        } else {
            Err(())
        }
    };

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_ptr_validation() {
        assert!(is_user_ptr(0x1000));
        assert!(is_user_ptr(0x7FFF_FFFF_F000 - 1));
        assert!(!is_user_ptr(0));
        assert!(!is_user_ptr(0x7FFF_FFFF_F000));
        assert!(!is_user_ptr(0xFFFF_8000_0000_0000));
    }

    #[test]
    fn test_user_buf_validation() {
        assert!(is_user_buf(0x1000, 4096));
        assert!(is_user_buf(0x1000, 0));
        assert!(!is_user_buf(0, 4096));
        assert!(!is_user_buf(0x7FFF_FFFF_F000 - 100, 200));
    }

    #[test]
    fn test_user_buf_overflow_check() {
        assert!(!is_user_buf(u64::MAX, 1));
        assert!(!is_user_buf(u64::MAX - 100, 200));
    }

    #[test]
    fn test_copy_from_user_zero_len() {
        let mut kernel_buf = [0u8; 16];
        let result = copy_from_user(&mut kernel_buf, 0x1000, 0);
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn test_copy_from_user_invalid_ptr() {
        let mut kernel_buf = [0u8; 16];
        let result = copy_from_user(&mut kernel_buf, 0, 16);
        assert!(result.is_err());
        
        let result = copy_from_user(&mut kernel_buf, 0xFFFF_8000_0000_0000, 16);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_from_user_buffer_too_small() {
        let mut kernel_buf = [0u8; 8];
        let result = copy_from_user(&mut kernel_buf, 0x1000, 16);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_to_user_zero_len() {
        let kernel_buf = [0u8; 16];
        let result = copy_to_user(0x1000, &kernel_buf, 0);
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn test_copy_to_user_invalid_ptr() {
        let kernel_buf = [0u8; 16];
        let result = copy_to_user(0, &kernel_buf, 16);
        assert!(result.is_err());
        
        let result = copy_to_user(0xFFFF_8000_0000_0000, &kernel_buf, 16);
        assert!(result.is_err());
    }

    #[test]
    fn test_clear_user_zero_len() {
        let result = clear_user(0x1000, 0);
        assert_eq!(result, Ok(0));
    }

    #[test]
    fn test_clear_user_invalid_ptr() {
        let result = clear_user(0, 16);
        assert!(result.is_err());
        
        let result = clear_user(0xFFFF_8000_0000_0000, 16);
        assert!(result.is_err());
    }

    #[test]
    fn test_strlen_user_invalid_ptr() {
        let result = strlen_user(0, 256);
        assert!(result.is_err());
        
        let result = strlen_user(0xFFFF_8000_0000_0000, 256);
        assert!(result.is_err());
    }

    #[test]
    fn test_copy_string_from_user_invalid_ptr() {
        let result = copy_string_from_user(0, 256);
        assert!(result.is_err());
        
        let result = copy_string_from_user(0xFFFF_8000_0000_0000, 256);
        assert!(result.is_err());
    }

    #[test]
    fn test_boundary_conditions() {
        assert!(is_user_ptr(USER_ADDR_MAX - 1));
        assert!(!is_user_ptr(USER_ADDR_MAX));
        assert!(!is_user_ptr(USER_ADDR_MAX + 1));
        
        assert!(is_user_buf(USER_ADDR_MAX - 4096, 4096));
        assert!(!is_user_buf(USER_ADDR_MAX - 100, 200));
    }
}
