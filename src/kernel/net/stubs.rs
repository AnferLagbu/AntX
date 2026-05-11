//! 网络 / lwIP 兼容桩
//!
//! lwIP C 源码已在 C→Rust 迁移中移除。
//! 此文件为 Rust 网络代码引用的 C 符号提供空桩实现，
//! 确保内核链接通过。网络功能待后续完整重写。

use core::ffi::c_void;

// ── lwIP 协议栈入口 ─────────────────────────────────────
#[no_mangle] pub unsafe extern "C" fn lwip_init() {}
#[no_mangle] pub unsafe extern "C" fn ethernet_input(_p: *mut c_void, _netif: *mut c_void, _ethhdr: *mut c_void) -> i32 { 0 }
#[no_mangle] pub unsafe extern "C" fn netif_add(_netif: *mut c_void, _ipaddr: *mut c_void, _netmask: *mut c_void, _gw: *mut c_void, _state: *mut c_void, _init: extern "C" fn(*mut c_void) -> i32, _input: extern "C" fn(*mut c_void, *mut c_void) -> i32) -> *mut c_void { core::ptr::null_mut() }
#[no_mangle] pub unsafe extern "C" fn netif_set_default(_netif: *mut c_void) {}
#[no_mangle] pub unsafe extern "C" fn netif_set_status_callback(_netif: *mut c_void, _cb: extern "C" fn(*mut c_void)) {}
#[no_mangle] pub unsafe extern "C" fn netif_set_up(_netif: *mut c_void) {}
#[no_mangle] pub unsafe extern "C" fn netif_set_link_up(_netif: *mut c_void) {}
#[no_mangle] pub unsafe extern "C" fn netif_set_link_down(_netif: *mut c_void) {}
#[no_mangle] pub unsafe extern "C" fn dhcp_start(_netif: *mut c_void) -> i32 { 0 }
#[no_mangle] pub unsafe extern "C" fn dhcp_stop(_netif: *mut c_void) {}
#[no_mangle] pub unsafe extern "C" fn dhcp_inform(_netif: *mut c_void) {}
#[no_mangle] pub unsafe extern "C" fn dns_gethostbyname(_name: *const i8, _addr: *mut c_void, _found: extern "C" fn(*const i8, *mut c_void, *mut c_void), _arg: *mut c_void) -> i32 { -1 }
#[no_mangle] pub unsafe extern "C" fn httpd_init() {}
#[no_mangle] pub unsafe extern "C" fn pbuf_free(_p: *mut c_void) -> u8 { 0 }
#[no_mangle] pub unsafe extern "C" fn raw_new(_type_: u8) -> *mut c_void { core::ptr::null_mut() }
#[no_mangle] pub unsafe extern "C" fn raw_bind(_pcb: *mut c_void, _ipaddr: *mut c_void) -> i32 { -1 }
#[no_mangle] pub unsafe extern "C" fn raw_recv(_pcb: *mut c_void, _recv: extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *const u8, u16)) {}
#[no_mangle] pub unsafe extern "C" fn e1000_dump_stats() {}

// ── kmalloc 扩展 ────────────────────────────────────────
#[no_mangle] pub unsafe extern "C" fn kmalloc_align(size: u64, _align: u64) -> *mut c_void {
    extern "C" { fn kmalloc(size: u64) -> *mut c_void; }
    kmalloc(size)
}
