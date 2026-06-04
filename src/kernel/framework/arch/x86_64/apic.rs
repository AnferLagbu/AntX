#![allow(dead_code)]
//! Local APIC (Advanced Programmable Interrupt Controller) Driver
//!
//! x86_64 Local APIC for per-CPU interrupt control, timer, and IPI.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const APIC_BASE_MSR: u32 = 0x1B;
const APIC_BASE_ADDR_MASK: u64 = 0xFFFF_F000;
const APIC_BASE_ENABLE: u64 = 1 << 11;

const APIC_ID: u32 = 0x020;
const APIC_VERSION: u32 = 0x030;
const APIC_TPR: u32 = 0x080;
const APIC_EOI: u32 = 0x0B0;
const APIC_SVR: u32 = 0x0F0;
const APIC_ISR_BASE: u32 = 0x100;
const APIC_TMR_BASE: u32 = 0x180;
const APIC_IRR_BASE: u32 = 0x200;
const APIC_ESR: u32 = 0x280;
const APIC_ICR_LOW: u32 = 0x300;
const APIC_ICR_HIGH: u32 = 0x310;
const APIC_LVT_TIMER: u32 = 0x320;
const APIC_LVT_THERMAL: u32 = 0x330;
const APIC_LVT_PERF: u32 = 0x340;
const APIC_LVT_LINT0: u32 = 0x350;
const APIC_LVT_LINT1: u32 = 0x360;
const APIC_LVT_ERROR: u32 = 0x370;
const APIC_TIMER_ICR: u32 = 0x380;
const APIC_TIMER_CCR: u32 = 0x390;
const APIC_TIMER_DCR: u32 = 0x3E0;

const SVR_ENABLE: u32 = 1 << 8;
const LVT_MASK: u32 = 1 << 16;
const LVT_TIMER_PERIODIC: u32 = 1 << 17;
const LVT_DELIVERY_FIXED: u32 = 0x000;
const LVT_DELIVERY_SMI: u32 = 0x200;
const LVT_DELIVERY_NMI: u32 = 0x400;
const LVT_DELIVERY_EXTINT: u32 = 0x700;

const ICR_ASSERT: u64 = 1 << 14;
const ICR_LEVEL: u64 = 1 << 15;
const ICR_BROADCAST: u64 = 1 << 19;
const ICR_ALL_EXCLUDE_SELF: u64 = 1 << 18 | 1 << 19;

const TIMER_DIV_1: u32 = 0x0B;
const TIMER_DIV_2: u32 = 0x00;
const TIMER_DIV_4: u32 = 0x01;
const TIMER_DIV_8: u32 = 0x02;
const TIMER_DIV_16: u32 = 0x03;
const TIMER_DIV_32: u32 = 0x04;
const TIMER_DIV_64: u32 = 0x05;
const TIMER_DIV_128: u32 = 0x06;

static APIC_BASE: AtomicU64 = AtomicU64::new(0);
static APIC_INITIALIZED: AtomicBool = AtomicBool::new(false);
static APIC_TIMER_CALIBRATED: AtomicBool = AtomicBool::new(false);
static APIC_TIMER_HZ: AtomicU64 = AtomicU64::new(0);

fn rdmsr(msr: u32) -> u64 {
    let (high, low): (u32, u32);
    unsafe {
        core::arch::asm!("rdmsr", out("eax") low, out("edx") high, in("ecx") msr, options(nomem, nostack));
    }
    ((high as u64) << 32) | (low as u64)
}

fn wrmsr(msr: u32, value: u64) {
    unsafe {
        core::arch::asm!("wrmsr", in("ecx") msr, in("eax") value as u32, in("edx") (value >> 32) as u32, options(nomem, nostack));
    }
}

pub fn apic_read(reg: u32) -> u32 {
    let base = APIC_BASE.load(Ordering::Acquire);
    unsafe { core::ptr::read_volatile((base + reg as u64) as *const u32) }
}

pub fn apic_write(reg: u32, value: u32) {
    let base = APIC_BASE.load(Ordering::Acquire);
    unsafe {
        core::ptr::write_volatile((base + reg as u64) as *mut u32, value);
    }
}

pub fn init() {
    let msr = rdmsr(APIC_BASE_MSR);
    let base = msr & APIC_BASE_ADDR_MASK;

    if base == 0 {
        return;
    }

    wrmsr(APIC_BASE_MSR, base | APIC_BASE_ENABLE);

    APIC_BASE.store(base, Ordering::Release);

    apic_write(APIC_SVR, SVR_ENABLE | 0xFF);

    apic_write(APIC_LVT_TIMER, LVT_MASK);
    apic_write(APIC_LVT_THERMAL, LVT_MASK);
    apic_write(APIC_LVT_PERF, LVT_MASK);
    apic_write(APIC_LVT_LINT0, LVT_DELIVERY_EXTINT);
    apic_write(APIC_LVT_LINT1, LVT_MASK);
    apic_write(APIC_LVT_ERROR, LVT_MASK);

    apic_write(APIC_TPR, 0);

    apic_write(APIC_ESR, 0);
    apic_read(APIC_ESR);

    APIC_INITIALIZED.store(true, Ordering::Release);
}

pub fn is_initialized() -> bool {
    APIC_INITIALIZED.load(Ordering::Acquire)
}

pub fn get_id() -> u32 {
    if !is_initialized() {
        return 0;
    }
    apic_read(APIC_ID) >> 24
}

pub fn get_version() -> u32 {
    if !is_initialized() {
        return 0;
    }
    apic_read(APIC_VERSION) & 0xFF
}

pub fn eoi() {
    if is_initialized() {
        apic_write(APIC_EOI, 0);
    }
}

pub fn send_ipi(apic_id: u8, vector: u8) {
    if !is_initialized() {
        return;
    }
    apic_write(APIC_ICR_HIGH, (apic_id as u32) << 24);
    apic_write(APIC_ICR_LOW, vector as u32 | ICR_ASSERT as u32);
    while apic_read(APIC_ICR_LOW) & (1 << 12) != 0 {}
}

pub fn broadcast_ipi(vector: u8) {
    if !is_initialized() {
        return;
    }
    apic_write(APIC_ICR_HIGH, 0);
    apic_write(
        APIC_ICR_LOW,
        vector as u32 | ICR_ALL_EXCLUDE_SELF as u32 | ICR_ASSERT as u32,
    );
    while apic_read(APIC_ICR_LOW) & (1 << 12) != 0 {}
}

pub fn init_timer(vector: u8, periodic: bool, divisor: u32) {
    if !is_initialized() {
        return;
    }

    let div_val = match divisor {
        1 => TIMER_DIV_1,
        2 => TIMER_DIV_2,
        4 => TIMER_DIV_4,
        8 => TIMER_DIV_8,
        16 => TIMER_DIV_16,
        32 => TIMER_DIV_32,
        64 => TIMER_DIV_64,
        128 => TIMER_DIV_128,
        _ => TIMER_DIV_16,
    };

    apic_write(APIC_TIMER_DCR, div_val);

    let mode = if periodic { LVT_TIMER_PERIODIC } else { 0 };
    apic_write(APIC_LVT_TIMER, mode | (vector as u32));
}

pub fn set_timer_count(count: u32) {
    if is_initialized() {
        apic_write(APIC_TIMER_ICR, count);
    }
}

pub fn get_timer_count() -> u32 {
    if !is_initialized() {
        return 0;
    }
    apic_read(APIC_TIMER_CCR)
}

pub fn calibrate_timer(_pit_hz: u64, target_ms: u64) -> u64 {
    if !is_initialized() {
        return 0;
    }

    apic_write(APIC_TIMER_DCR, TIMER_DIV_16);
    apic_write(APIC_LVT_TIMER, LVT_MASK);
    apic_write(APIC_TIMER_ICR, 0xFFFFFFFF);

    extern "C" {
        fn timer_sleep_busy(ms: u64);
    }
    unsafe {
        timer_sleep_busy(target_ms);
    }

    let remaining = apic_read(APIC_TIMER_CCR);
    let elapsed = 0xFFFFFFFFu32 - remaining;

    let ticks_per_ms = elapsed as u64 / target_ms;
    let apic_hz = ticks_per_ms * 16 * 1000;

    APIC_TIMER_HZ.store(apic_hz, Ordering::Release);
    APIC_TIMER_CALIBRATED.store(true, Ordering::Release);

    apic_hz
}

pub fn get_timer_hz() -> u64 {
    APIC_TIMER_HZ.load(Ordering::Acquire)
}

pub fn is_timer_calibrated() -> bool {
    APIC_TIMER_CALIBRATED.load(Ordering::Acquire)
}

pub fn mask_lint0() {
    if is_initialized() {
        apic_write(APIC_LVT_LINT0, apic_read(APIC_LVT_LINT0) | LVT_MASK);
    }
}

pub fn mask_lint1() {
    if is_initialized() {
        apic_write(APIC_LVT_LINT1, apic_read(APIC_LVT_LINT1) | LVT_MASK);
    }
}

pub fn unmask_lint0(mode: u32) {
    if is_initialized() {
        apic_write(APIC_LVT_LINT0, mode);
    }
}

pub fn unmask_lint1(mode: u32) {
    if is_initialized() {
        apic_write(APIC_LVT_LINT1, mode);
    }
}

#[no_mangle]
pub extern "C" fn apic_init() {
    init();
}
#[no_mangle]
pub extern "C" fn apic_eoi() {
    eoi();
}
#[no_mangle]
pub extern "C" fn apic_is_ready() -> bool {
    is_initialized()
}
#[no_mangle]
pub extern "C" fn apic_get_id() -> u32 {
    get_id()
}
#[no_mangle]
pub extern "C" fn apic_send_ipi(apic_id: u8, vector: u8) {
    send_ipi(apic_id, vector);
}
#[no_mangle]
pub extern "C" fn apic_broadcast_ipi(vector: u8) {
    broadcast_ipi(vector);
}
#[no_mangle]
pub extern "C" fn apic_init_timer(vector: u8, periodic: bool, divisor: u32) {
    init_timer(vector, periodic, divisor);
}
#[no_mangle]
pub extern "C" fn apic_set_timer_count(count: u32) {
    set_timer_count(count);
}
#[no_mangle]
pub extern "C" fn apic_calibrate_timer(pit_hz: u64, target_ms: u64) -> u64 {
    calibrate_timer(pit_hz, target_ms)
}
