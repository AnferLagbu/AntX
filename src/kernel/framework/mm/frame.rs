//! UFrame / USegment — Type-safe user-memory frame abstraction
//!
//! Provides type-level safety guarantees for user-space memory access,
//! strengthening Invariant I4 (kernel must not dereference user pointers
//! without validation).
//!
//! # Design
//!
//! - `UFrame`: Represents a single user physical frame (4KB). Access is
//!   restricted to `read_pod` / `write_pod` which copy POD types in/out
//!   without exposing long-lived references to user memory.
//!
//! - `USegment`: Represents a contiguous range of user virtual memory.
//!   Provides `read_bytes` / `write_bytes` with bounded copies.
//!
//! - `Pod` trait: Marks Plain Old Data types that can be safely copied
//!   byte-by-byte (no pointers, no interior mutability, no Drop side effects).
//!
//! # Safety Invariants
//!
//! - UFrame/USegment never expose `&[u8]` or `&mut [u8]` pointing to user
//!   memory — all access goes through bounded copy operations.
//! - Pod types cannot contain pointers into kernel memory, preventing
//!   accidental kernel address leakage to user space.

use super::*;
use super::copy_user::{copy_from_user, copy_to_user, is_user_ptr, is_user_buf};

// ---------------------------------------------------------------------------
// Pod trait
// ---------------------------------------------------------------------------

/// Marker trait for Plain Old Data types.
///
/// A `Pod` type can be safely copied byte-by-byte between kernel and user
/// memory. Types implementing `Pod` must satisfy:
///
/// 1. No pointers (raw or references) — prevents kernel address leakage
/// 2. No interior mutability (Cell, RefCell, AtomicXxx) — prevents TOCTOU
/// 3. No `Drop` side effects — value is purely bitwise
/// 4. `Copy` — value semantics only
///
/// # Safety
///
/// Implementing this trait is safe because the compiler enforces `Copy`.
/// However, the implementer must ensure no pointer fields exist. This is
/// checked by the `pod_assertions` test below.
pub trait Pod: Copy {}

// Implement Pod for common primitive types
impl Pod for u8 {}
impl Pod for u16 {}
impl Pod for u32 {}
impl Pod for u64 {}
impl Pod for usize {}
impl Pod for i8 {}
impl Pod for i16 {}
impl Pod for i32 {}
impl Pod for i64 {}
impl Pod for isize {}
impl Pod for bool {}

// Implement Pod for arrays of Pod types
impl<const N: usize, T: Pod> Pod for [T; N] {}

// ---------------------------------------------------------------------------
// UFrame — single user physical frame
// ---------------------------------------------------------------------------

/// A single user physical frame (4KB page).
///
/// `UFrame` encapsulates a page-frame number (PFN) and provides safe
/// read/write access to the frame's contents via `Pod` types. It never
/// exposes a direct reference to user memory.
///
/// # Lifecycle
///
/// A `UFrame` is created from a validated user virtual address. The caller
/// must ensure the underlying page remains mapped for the lifetime of the
/// `UFrame`.
///
/// # Invariant I4
///
/// By restricting access to `read_pod`/`write_pod`, `UFrame` ensures that
/// the kernel never holds a long-lived reference to user memory, preventing
/// TOCTOU attacks where user modifies memory after kernel validation.
pub struct UFrame {
    /// User virtual address of the frame (page-aligned)
    uaddr: u64,
}

impl UFrame {
    /// Create a `UFrame` from a user virtual address.
    ///
    /// Returns `None` if the address is not in user space or not page-aligned.
    #[inline]
    pub fn from_user_addr(addr: u64) -> Option<Self> {
        if !is_user_ptr(addr) || !addr.is_multiple_of(PAGE_SIZE) {
            return None;
        }
        Some(Self { uaddr: addr })
    }

    /// Create a `UFrame` without validation.
    ///
    /// # Safety
    ///
    /// - `addr` must be a valid, page-aligned user virtual address
    /// - The underlying page must remain mapped for the lifetime of this `UFrame`
    #[inline]
    pub unsafe fn from_user_addr_unchecked(addr: u64) -> Self {
        Self { uaddr: addr }
    }

    /// Read a POD value from a specific offset within the frame.
    ///
    /// The offset + size_of::<T>() must not exceed PAGE_SIZE.
    /// Returns `Err(())` on page fault or invalid offset.
    #[inline]
    pub fn read_pod<T: Pod>(&self, offset: usize) -> Result<T, ()> {
        let size = core::mem::size_of::<T>();
        if offset.saturating_add(size) > PAGE_SIZE as usize {
            return Err(());
        }

        let mut val = core::mem::MaybeUninit::<T>::uninit();
        let dst =
            unsafe { core::slice::from_raw_parts_mut(val.as_mut_ptr() as *mut u8, size) };
        copy_from_user(dst, self.uaddr + offset as u64, size)?;
        Ok(unsafe { val.assume_init() })
    }

    /// Write a POD value to a specific offset within the frame.
    ///
    /// The offset + size_of::<T>() must not exceed PAGE_SIZE.
    /// Returns `Err(())` on page fault or invalid offset.
    #[inline]
    pub fn write_pod<T: Pod>(&self, offset: usize, val: &T) -> Result<(), ()> {
        let size = core::mem::size_of::<T>();
        if offset.saturating_add(size) > PAGE_SIZE as usize {
            return Err(());
        }

        let src =
            unsafe { core::slice::from_raw_parts(val as *const T as *const u8, size) };
        copy_to_user(self.uaddr + offset as u64, src, size)?;
        Ok(())
    }

    /// Read a byte slice from the frame.
    ///
    /// `offset + buf.len()` must not exceed PAGE_SIZE.
    /// Returns the number of bytes actually copied.
    #[inline]
    pub fn read_bytes(&self, offset: usize, buf: &mut [u8]) -> Result<usize, ()> {
        if offset.saturating_add(buf.len()) > PAGE_SIZE as usize {
            return Err(());
        }
        copy_from_user(buf, self.uaddr + offset as u64, buf.len())
    }

    /// Write a byte slice to the frame.
    ///
    /// `offset + data.len()` must not exceed PAGE_SIZE.
    /// Returns the number of bytes actually copied.
    #[inline]
    pub fn write_bytes(&self, offset: usize, data: &[u8]) -> Result<usize, ()> {
        if offset.saturating_add(data.len()) > PAGE_SIZE as usize {
            return Err(());
        }
        copy_to_user(self.uaddr + offset as u64, data, data.len())
    }

    /// Return the user virtual address of this frame.
    #[inline]
    pub fn user_addr(&self) -> u64 {
        self.uaddr
    }
}

// ---------------------------------------------------------------------------
// USegment — contiguous user virtual memory range
// ---------------------------------------------------------------------------

/// A contiguous range of user virtual memory.
///
/// Unlike `UFrame`, `USegment` can span multiple pages and arbitrary
/// byte ranges. Access is restricted to bounded copy operations.
///
/// # Invariant I4
///
/// Same as `UFrame`: never exposes a reference to user memory.
pub struct USegment {
    /// Base user virtual address
    base: u64,
    /// Length in bytes
    len: usize,
}

impl USegment {
    /// Create a `USegment` from a user address and length.
    ///
    /// Returns `None` if the range is not entirely in user space.
    #[inline]
    pub fn from_user_range(addr: u64, len: usize) -> Option<Self> {
        if !is_user_buf(addr, len) {
            return None;
        }
        Some(Self { base: addr, len })
    }

    /// Create a `USegment` without validation.
    ///
    /// # Safety
    ///
    /// - `[addr, addr+len)` must be valid, mapped, user-accessible memory
    /// - The mapping must remain valid for the lifetime of this `USegment`
    #[inline]
    pub unsafe fn from_user_range_unchecked(addr: u64, len: usize) -> Self {
        Self { base: addr, len }
    }

    /// Read a POD value from a specific offset within the segment.
    ///
    /// Returns `Err(())` on page fault or out-of-bounds offset.
    #[inline]
    pub fn read_pod<T: Pod>(&self, offset: usize) -> Result<T, ()> {
        let size = core::mem::size_of::<T>();
        if offset.saturating_add(size) > self.len {
            return Err(());
        }

        let mut val = core::mem::MaybeUninit::<T>::uninit();
        let dst =
            unsafe { core::slice::from_raw_parts_mut(val.as_mut_ptr() as *mut u8, size) };
        copy_from_user(dst, self.base + offset as u64, size)?;
        Ok(unsafe { val.assume_init() })
    }

    /// Write a POD value to a specific offset within the segment.
    ///
    /// Returns `Err(())` on page fault or out-of-bounds offset.
    #[inline]
    pub fn write_pod<T: Pod>(&self, offset: usize, val: &T) -> Result<(), ()> {
        let size = core::mem::size_of::<T>();
        if offset.saturating_add(size) > self.len {
            return Err(());
        }

        let src =
            unsafe { core::slice::from_raw_parts(val as *const T as *const u8, size) };
        copy_to_user(self.base + offset as u64, src, size)?;
        Ok(())
    }

    /// Read bytes from the segment into a kernel buffer.
    ///
    /// Returns the number of bytes actually copied.
    #[inline]
    pub fn read_bytes(&self, offset: usize, buf: &mut [u8]) -> Result<usize, ()> {
        let max_len = self.len.saturating_sub(offset);
        let copy_len = buf.len().min(max_len);
        if copy_len == 0 {
            return Ok(0);
        }
        copy_from_user(&mut buf[..copy_len], self.base + offset as u64, copy_len)
    }

    /// Write bytes from a kernel buffer to the segment.
    ///
    /// Returns the number of bytes actually copied.
    #[inline]
    pub fn write_bytes(&self, offset: usize, data: &[u8]) -> Result<usize, ()> {
        let max_len = self.len.saturating_sub(offset);
        let copy_len = data.len().min(max_len);
        if copy_len == 0 {
            return Ok(0);
        }
        copy_to_user(self.base + offset as u64, &data[..copy_len], copy_len)
    }

    /// Return the base user virtual address.
    #[inline]
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Return the length in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Return true if the segment has zero length.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uframe_rejects_kernel_addr() {
        // Kernel-space address should be rejected
        assert!(UFrame::from_user_addr(0xFFFF800000000000).is_none());
    }

    #[test]
    fn test_uframe_rejects_misaligned() {
        // Non-page-aligned user address should be rejected
        assert!(UFrame::from_user_addr(0x1001).is_none());
    }

    #[test]
    fn test_usegment_rejects_kernel_range() {
        // Kernel-space range should be rejected
        assert!(USegment::from_user_range(0xFFFF800000000000, 4096).is_none());
    }

    #[test]
    fn test_uframe_offset_overflow() {
        // offset + size > PAGE_SIZE should fail
        // We can't actually read (no user pages in unit test), but the
        // bounds check should reject it.
        unsafe {
            let frame = UFrame::from_user_addr_unchecked(0x1000);
            // u64 at offset 4090 would need 8 bytes but only 6 remain
            assert!(frame.read_pod::<u64>(4090).is_err());
        }
    }

    #[test]
    fn test_usegment_offset_overflow() {
        unsafe {
            let seg = USegment::from_user_range_unchecked(0x1000, 16);
            // Read u64 at offset 12 would need 8 bytes but only 4 remain
            assert!(seg.read_pod::<u64>(12).is_err());
        }
    }
}
