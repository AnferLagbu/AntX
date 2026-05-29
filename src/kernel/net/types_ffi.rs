use super::types::SysProt;

extern "C" {
    #[link_name = "klog_net"]
    pub fn klog_net(fmt: *const i8, ...);

    #[link_name = "klog_net_err"]
    pub fn klog_net_err(fmt: *const i8, ...);

    #[link_name = "klog_init_msg"]
    pub fn klog_init_msg(fmt: *const i8, ...);
}

#[no_mangle]
pub extern "C" fn sys_init() {
    crate::kernel::net::types::NET_READY.store(false, core::sync::atomic::Ordering::Relaxed);
    unsafe { klog_net("sys_arch ready\0".as_ptr() as *const i8); }
}

#[no_mangle]
pub extern "C" fn sys_arch_protect() -> SysProt {
    let flags = crate::arch!(interrupt_disable()) as u64;
    SysProt(flags)
}

#[no_mangle]
pub extern "C" fn sys_arch_unprotect(pval: SysProt) {
    crate::arch!(interrupt_restore(pval.0 as usize));
}

#[no_mangle]
pub unsafe extern "C" fn rust_klog_net(fmt: *const i8) {
    let _ = fmt;
}

#[no_mangle]
pub extern "C" fn sys_mbox_trypost_fromisr(
    mbox: *mut crate::kernel::net::sys_arch::SysMbox,
    msg: *mut core::ffi::c_void,
) -> i32 {
    crate::kernel::net::sys_arch::sys_mbox_trypost(mbox, msg)
}

#[no_mangle]
pub extern "C" fn lwip_socket(_domain: i32, _type: i32, _protocol: i32) -> i32 { -1 }

#[no_mangle]
pub extern "C" fn lwip_bind(_s: i32, _name: *const core::ffi::c_void, _namelen: u32) -> i32 { -1 }

#[no_mangle]
pub extern "C" fn lwip_listen(_s: i32, _backlog: i32) -> i32 { -1 }

#[no_mangle]
pub extern "C" fn lwip_accept(_s: i32, _addr: *mut core::ffi::c_void, _addrlen: *mut u32) -> i32 { -1 }

#[no_mangle]
pub extern "C" fn lwip_connect(_s: i32, _name: *const core::ffi::c_void, _namelen: u32) -> i32 { -1 }

#[no_mangle]
pub extern "C" fn lwip_send(_s: i32, _data: *const core::ffi::c_void, _size: usize, _flags: i32) -> isize { -1 }

#[no_mangle]
pub extern "C" fn lwip_recv(_s: i32, _mem: *mut core::ffi::c_void, _len: usize, _flags: i32) -> isize { -1 }

#[no_mangle]
pub extern "C" fn lwip_close(_s: i32) -> i32 { -1 }
