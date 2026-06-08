#![allow(dead_code)]
//! IOAPIC (I/O Advanced Programmable Interrupt Controller) Driver
//!
//! Routes external hardware interrupts to specific CPU cores.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const IOAPIC_BASE_DEFAULT: u64 = 0xFEC00000;

const IOREGSEL: u32 = 0x00;
const IOWIN: u32 = 0x10;

const IOAPIC_ID: u32 = 0x00;
const IOAPIC_VER: u32 = 0x01;
const IOAPIC_ARB: u32 = 0x02;
const IOREDTBL_BASE: u32 = 0x10;

const REDTBL_MASK: u64 = 1 << 16;
const REDTBL_LEVEL: u64 = 1 << 15;
const REDTBL_LOW_PRIORITY: u64 = 1 << 13;
const REDTBL_LOGICAL: u64 = 1 << 11;

const DELIVERY_FIXED: u64 = 0x000;
const DELIVERY_SMI: u64 = 0x200;
const DELIVERY_NMI: u64 = 0x400;
const DELIVERY_EXTINT: u64 = 0x700;

static IOAPIC_BASE: AtomicU64 = AtomicU64::new(0);
static IOAPIC_INITIALIZED: AtomicBool = AtomicBool::new(false);
static IOAPIC_MAX_IRQ: AtomicU64 = AtomicU64::new(0);

fn ioapic_read(reg: u32) -> u32 {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::ptr::write_volatile((base + IOREGSEL as u64) as *mut u32, reg);
        core::ptr::read_volatile((base + IOWIN as u64) as *const u32)
    }
}

fn ioapic_write(reg: u32, value: u32) {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::ptr::write_volatile((base + IOREGSEL as u64) as *mut u32, reg);
        core::ptr::write_volatile((base + IOWIN as u64) as *mut u32, value);
    }
}

fn ioapic_read_redirection(irq: u8) -> u64 {
    let reg = IOREDTBL_BASE + (irq as u32) * 2;
    let low = ioapic_read(reg);
    let high = ioapic_read(reg + 1);
    ((high as u64) << 32) | (low as u64)
}

fn ioapic_write_redirection(irq: u8, value: u64) {
    let reg = IOREDTBL_BASE + (irq as u32) * 2;
    ioapic_write(reg, value as u32);
    ioapic_write(reg + 1, (value >> 32) as u32);
}

pub fn init(base_addr: u64) {
    let base = if base_addr == 0 {
        IOAPIC_BASE_DEFAULT
    } else {
        base_addr
    };
    IOAPIC_BASE.store(base, Ordering::Release);

    let ver = ioapic_read(IOAPIC_VER);
    let max_irq = ((ver >> 16) & 0xFF) as u64 + 1;
    IOAPIC_MAX_IRQ.store(max_irq, Ordering::Release);

    for irq in 0..24u8 {
        ioapic_write_redirection(irq, REDTBL_MASK | DELIVERY_FIXED | (irq as u64 + 32));
    }

    IOAPIC_INITIALIZED.store(true, Ordering::Release);
}

pub fn is_initialized() -> bool {
    IOAPIC_INITIALIZED.load(Ordering::Acquire)
}

pub fn get_max_irq() -> u8 {
    IOAPIC_MAX_IRQ.load(Ordering::Acquire) as u8
}

pub fn set_irq(irq: u8, vector: u8, apic_id: u8, masked: bool) {
    if !is_initialized() {
        return;
    }
    let mut entry: u64 = vector as u64 | ((apic_id as u64) << 56);
    if masked {
        entry |= REDTBL_MASK;
    }
    ioapic_write_redirection(irq, entry);
}

pub fn mask_irq(irq: u8) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    ioapic_write_redirection(irq, entry | REDTBL_MASK);
}

pub fn unmask_irq(irq: u8) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    ioapic_write_redirection(irq, entry & !REDTBL_MASK);
}

pub fn set_irq_level(irq: u8, level_triggered: bool) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    if level_triggered {
        ioapic_write_redirection(irq, entry | REDTBL_LEVEL);
    } else {
        ioapic_write_redirection(irq, entry & !REDTBL_LEVEL);
    }
}

pub fn route_irq_to_cpu(irq: u8, apic_id: u8) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    let new_entry = (entry & !(0xFFu64 << 56)) | ((apic_id as u64) << 56);
    ioapic_write_redirection(irq, new_entry);
}

#[no_mangle]
pub extern "C" fn ioapic_init(base_addr: u64) {
    init(base_addr);
}
#[no_mangle]
pub extern "C" fn ioapic_mask_irq(irq: u8) {
    mask_irq(irq);
}
#[no_mangle]
pub extern "C" fn ioapic_unmask_irq(irq: u8) {
    unmask_irq(irq);
}
#[no_mangle]
pub extern "C" fn ioapic_set_irq(irq: u8, vector: u8, apic_id: u8, masked: bool) {
    set_irq(irq, vector, apic_id, masked);
}
#[no_mangle]
pub extern "C" fn ioapic_route_irq_to_cpu(irq: u8, apic_id: u8) {
    route_irq_to_cpu(irq, apic_id);
}
