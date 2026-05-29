use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LwipErr {
    Ok = 0,
    Mem = -1,
    Buf = -2,
    Timeout = -3,
    Rte = -4,
    Inprogress = -5,
    Val = -6,
    Wouldblock = -7,
    Addrinuse = -8,
    Already = -9,
    Isconn = -10,
    Notconn = -11,
    Aborted = -12,
    Connrst = -13,
    Nobufs = -14,
    Udp = -15,
    Tcp = -16,
    Dns = -17,
    If = -18,
}

impl Default for LwipErr {
    fn default() -> Self {
        Self::Ok
    }
}

impl From<i32> for LwipErr {
    fn from(code: i32) -> Self {
        match code {
            0 => Self::Ok,
            -1 => Self::Mem,
            -2 => Self::Buf,
            -3 => Self::Timeout,
            _ => Self::If,
        }
    }
}

static SYS_TICKS: AtomicU32 = AtomicU32::new(0);

pub static NET_READY: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn sys_now() -> u32 {
    SYS_TICKS.load(Ordering::Relaxed) * 10
}

#[no_mangle]
pub extern "C" fn sys_tick_inc() {
    SYS_TICKS.fetch_add(1, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct SysProt(pub u64);

#[no_mangle]
pub static mut errno: i32 = 0;

#[cfg(not(feature = "kernel_test"))]
pub use super::types_ffi::*;
