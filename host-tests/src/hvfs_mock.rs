pub mod kernel {
    pub mod sync {
        pub mod mutex {
            use std::sync::{Mutex as StdMutex, MutexGuard};

            pub struct Mutex<T: ?Sized> {
                inner: StdMutex<T>,
            }

            impl<T> Mutex<T> {
                pub const fn new(val: T) -> Self {
                    Mutex {
                        inner: StdMutex::new(val),
                    }
                }
            }

            impl<T: ?Sized> Mutex<T> {
                pub fn lock(&self) -> MutexGuard<'_, T> {
                    self.inner.lock().unwrap_or_else(|e| e.into_inner())
                }
            }
        }
    }
    pub mod fs {
        pub mod hvfs {
            pub use crate::hvfs::*;
        }
        pub mod vfs {
            pub mod types {
                pub use crate::hvfs_mock::KernelError;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelError {
    NotFound,
    AlreadyExists,
    NoSpace,
    PermissionDenied,
    InvalidArgument,
    NotDirectory,
    IsDirectory,
    NotEmpty,
    ReadOnly,
    IoError,
    NotSupported,
    Overflow,
    NotMounted,
    NotInitialized,
    IsReadOnly,
    NameTooLong,
    InvalidObject,
}

impl KernelError {
    pub fn as_i32(self) -> i32 {
        match self {
            Self::NotFound => -2,
            Self::AlreadyExists => -3,
            Self::NoSpace => -4,
            Self::PermissionDenied => -5,
            Self::InvalidArgument => -6,
            Self::NotDirectory => -7,
            Self::IsDirectory => -8,
            Self::NotEmpty => -9,
            Self::ReadOnly => -10,
            Self::IoError => -11,
            Self::NotSupported => -12,
            Self::Overflow => -13,
            Self::NotMounted => -14,
            Self::NotInitialized => -15,
            Self::IsReadOnly => -16,
            Self::NameTooLong => -17,
            Self::InvalidObject => -18,
        }
    }
}

pub type KernelResult<T> = Result<T, KernelError>;

use std::sync::atomic::{AtomicU64, Ordering};
static TICK_COUNTER: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
pub extern "C" fn timer_get_ticks() -> u64 {
    TICK_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[no_mangle]
pub extern "C" fn ata_disk_present(_disk: u8) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn ata_read_sector(_disk: u8, _sector: u32, _buf: *mut u8) -> i32 {
    -1
}

#[no_mangle]
pub extern "C" fn ata_write_sector(_disk: u8, _sector: u32, _buf: *const u8) -> i32 {
    -1
}

#[no_mangle]
pub extern "C" fn klog_ffi_info(msg: *const u8) {
    let mut len = 0usize;
    while len < 256 {
        let b = unsafe { *msg.add(len) };
        if b == 0 {
            break;
        }
        len += 1;
    }
    if len > 0 {
        let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(msg, len)) };
        print!("{}", s);
    }
}

#[no_mangle]
pub extern "C" fn pwid_get_privilege_level(_pwid: u64) -> u8 {
    3
}

#[no_mangle]
pub extern "C" fn pwid_has_capability(_pwid: u64, _domain: u16, _required: u64) -> bool {
    true
}

#[cfg(test)]
mod kernel_error_tests {
    use super::KernelError;

    #[test]
    fn kernel_error_codes_unique() {
        let codes: Vec<i32> = vec![
            KernelError::NotFound.as_i32(),
            KernelError::AlreadyExists.as_i32(),
            KernelError::NoSpace.as_i32(),
            KernelError::PermissionDenied.as_i32(),
            KernelError::InvalidArgument.as_i32(),
            KernelError::NotDirectory.as_i32(),
            KernelError::IsDirectory.as_i32(),
            KernelError::NotEmpty.as_i32(),
            KernelError::ReadOnly.as_i32(),
            KernelError::IoError.as_i32(),
            KernelError::NotSupported.as_i32(),
            KernelError::Overflow.as_i32(),
            KernelError::NotMounted.as_i32(),
            KernelError::NotInitialized.as_i32(),
            KernelError::IsReadOnly.as_i32(),
            KernelError::NameTooLong.as_i32(),
            KernelError::InvalidObject.as_i32(),
        ];
        let mut sorted = codes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "all error codes must be unique");
    }

    #[test]
    fn kernel_error_codes_negative() {
        let variants: Vec<KernelError> = vec![
            KernelError::NotFound,
            KernelError::AlreadyExists,
            KernelError::NoSpace,
            KernelError::PermissionDenied,
            KernelError::InvalidArgument,
            KernelError::NotDirectory,
            KernelError::IsDirectory,
            KernelError::NotEmpty,
            KernelError::ReadOnly,
            KernelError::IoError,
            KernelError::NotSupported,
            KernelError::Overflow,
            KernelError::NotMounted,
            KernelError::NotInitialized,
            KernelError::IsReadOnly,
            KernelError::NameTooLong,
            KernelError::InvalidObject,
        ];
        for v in &variants {
            assert!(
                v.as_i32() < 0,
                "{:?}.as_i32() should be negative, got {}",
                v,
                v.as_i32()
            );
        }
    }

    #[test]
    fn kernel_error_not_found_code() {
        assert_eq!(KernelError::NotFound.as_i32(), -2);
    }

    #[test]
    fn kernel_error_permission_denied_code() {
        assert_eq!(KernelError::PermissionDenied.as_i32(), -5);
    }

    #[test]
    fn kernel_error_not_initialized_code() {
        assert_eq!(KernelError::NotInitialized.as_i32(), -15);
    }

    #[test]
    fn kernel_error_no_space_code() {
        assert_eq!(KernelError::NoSpace.as_i32(), -4);
    }

    #[test]
    fn kernel_error_equality() {
        assert_eq!(KernelError::NotFound, KernelError::NotFound);
        assert_ne!(KernelError::NotFound, KernelError::PermissionDenied);
    }

    #[test]
    fn kernel_result_ok() {
        let r: super::KernelResult<i32> = Ok(42);
        assert_eq!(r.unwrap(), 42);
    }

    #[test]
    fn kernel_result_err() {
        let r: super::KernelResult<i32> = Err(KernelError::NotFound);
        assert_eq!(r.unwrap_err(), KernelError::NotFound);
    }
}
