//! 内联字符串操作 (no_std, 编译期可优化)

#![allow(dead_code)]

#[inline]
pub unsafe fn strlen(s: *const u8) -> usize {
    let mut n = 0;
    unsafe {
        while *s.add(n) != 0 {
            n += 1;
        }
    }
    n
}

#[inline]
pub unsafe fn memcpy(dst: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    unsafe {
        while i < n {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
    }
    dst
}

#[inline]
pub unsafe fn memset(dst: *mut u8, c: u8, n: usize) -> *mut u8 {
    let mut i = 0;
    unsafe {
        while i < n {
            *dst.add(i) = c;
            i += 1;
        }
    }
    dst
}
