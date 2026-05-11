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

/// C 兼容别名: syscall_handler (int 0x80 汇编入口)
#[no_mangle] pub unsafe extern "C" fn syscall_handler() {}

/// 恢复中断 (int 0x82)
#[no_mangle] pub unsafe extern "C" fn isr0x82() {}

// ── IRQ / ISR 空桩 (由汇编 stub 或 IDT 模块的动态注册替代) ──
macro_rules! irq_stub { ($n:expr, $name:ident) => {
    #[no_mangle] pub unsafe extern "C" fn $name() {}
}; }

irq_stub!(0,  irq0);  irq_stub!(1,  irq1);  irq_stub!(2,  irq2);  irq_stub!(3,  irq3);
irq_stub!(4,  irq4);  irq_stub!(5,  irq5);  irq_stub!(6,  irq6);  irq_stub!(7,  irq7);
irq_stub!(8,  irq8);  irq_stub!(9,  irq9);  irq_stub!(10, irq10); irq_stub!(11, irq11);
irq_stub!(12, irq12); irq_stub!(13, irq13); irq_stub!(14, irq14); irq_stub!(15, irq15);

macro_rules! isr_stub { ($n:expr, $name:ident) => {
    #[no_mangle] pub unsafe extern "C" fn $name() {}
}; }

isr_stub!(0,  isr0);  isr_stub!(1,  isr1);  isr_stub!(2,  isr2);  isr_stub!(3,  isr3);
isr_stub!(4,  isr4);  isr_stub!(5,  isr5);  isr_stub!(6,  isr6);  isr_stub!(7,  isr7);
isr_stub!(8,  isr8);  isr_stub!(9,  isr9);  isr_stub!(10, isr10); isr_stub!(11, isr11);
isr_stub!(12, isr12); isr_stub!(13, isr13); isr_stub!(14, isr14); isr_stub!(15, isr15);
isr_stub!(16, isr16); isr_stub!(17, isr17); isr_stub!(18, isr18); isr_stub!(19, isr19);
isr_stub!(20, isr20); isr_stub!(21, isr21); isr_stub!(22, isr22); isr_stub!(23, isr23);
isr_stub!(24, isr24); isr_stub!(25, isr25); isr_stub!(26, isr26); isr_stub!(27, isr27);
isr_stub!(28, isr28); isr_stub!(29, isr29); isr_stub!(30, isr30); isr_stub!(31, isr31);

// ── kmalloc 扩展 ────────────────────────────────────────
#[no_mangle] pub unsafe extern "C" fn kmalloc_align(size: u64, _align: u64) -> *mut c_void {
    extern "C" { fn kmalloc(size: u64) -> *mut c_void; }
    kmalloc(size)
}
