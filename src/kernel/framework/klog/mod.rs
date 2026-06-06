#![allow(dead_code)]
//! AntX 内核日志系统 (KLog)
//!
//! 自举设计 — 零外部依赖:
//!   1. 内建 COM1 串口驱动 (直接 port I/O, 无需 driver 子系统)
//!   2. 128KB 环形缓冲区 (Atomic + Mutex 保护)
//!   3. 级别/分类双重过滤
//!   4. RDTSC 时间戳
//!
//! 所有内核模块通过 klog_* FFI 统一输出。

use core::sync::atomic::{AtomicU8, Ordering};

// ============================================================================
// 格式化宏 — 统一输出格式: [时间戳][级别][分类] 消息
//
// 输出格式: <ts_s>.<ts_us> [LEVEL] [CATEGORY] message\n
//
// 使用方式:
//   klog!(Info, Boot, "QueenX starting...");
//   klog_warn!(Kernel, "warning message");
//   klog_err!(Driver, "driver error: {}", code);
// ============================================================================

pub const KLOG_BUF: usize = 256;

pub struct KlogWriter {
    buf: [u8; KLOG_BUF],
    pos: usize,
}
impl KlogWriter {
    pub const fn new() -> Self {
        Self {
            buf: [0; KLOG_BUF],
            pos: 0,
        }
    }
}

impl core::fmt::Write for KlogWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let len = bytes.len().min(self.buf.len() - self.pos);
        self.buf[self.pos..self.pos + len].copy_from_slice(&bytes[..len]);
        self.pos += len;
        Ok(())
    }
}

impl KlogWriter {
    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.pos]
    }
}

pub struct CursorWriter<'a> {
    buf: &'a mut [u8],
    cursor: &'a mut usize,
}

impl<'a> CursorWriter<'a> {
    pub fn new(buf: &'a mut [u8], cursor: &'a mut usize) -> Self {
        Self { buf, cursor }
    }
}

impl<'a> core::fmt::Write for CursorWriter<'a> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.buf.len() - *self.cursor;
        let to_write = bytes.len().min(remaining);
        self.buf[*self.cursor..*self.cursor + to_write].copy_from_slice(&bytes[..to_write]);
        *self.cursor += to_write;
        Ok(())
    }
}

#[macro_export]
macro_rules! klog_ffi {
    ($ffi_fn:ident, $($arg:tt)*) => {{
        extern "C" { fn $ffi_fn(msg: *const u8); }
        let mut buf: [u8; 256] = [0u8; 256];
        let mut cursor = 0;
        let _ = core::fmt::write(
            &mut $crate::kernel::framework::klog::CursorWriter::new(&mut buf, &mut cursor),
            format_args!($($arg)*),
        );
        if cursor > 0 {
            unsafe { $ffi_fn(buf.as_ptr()); }
        }
    }};
}

/// 通用格式化日志宏 — 单入口，所有模块共用
#[macro_export]
macro_rules! klog_fmt {
    ($lvl:ident, $cat:ident, $($arg:tt)*) => {{
        let mut w = $crate::kernel::framework::klog::KlogWriter::new();
        let _ = core::fmt::Write::write_fmt(&mut w, format_args!($($arg)*));
        unsafe {
            $crate::kernel::framework::klog::klog_write(
                $crate::kernel::framework::klog::LogLevel::$lvl as u8,
                $crate::kernel::framework::klog::LogCategory::$cat as u8,
                core::ptr::null(), core::ptr::null(), 0,
                w.as_slice().as_ptr() as *const u8,
            );
        }
    }};
}

/// 便捷宏: 按级别
#[macro_export]
macro_rules! klog_info  { ($cat:ident, $($arg:tt)*) => { $crate::klog_fmt!(Info,  $cat, $($arg)*) }; }
#[macro_export]
macro_rules! klog_warn  { ($cat:ident, $($arg:tt)*) => { $crate::klog_fmt!(Warn,  $cat, $($arg)*) }; }
#[macro_export]
macro_rules! klog_err   { ($cat:ident, $($arg:tt)*) => { $crate::klog_fmt!(Error, $cat, $($arg)*) }; }
#[macro_export]
macro_rules! klog_debug { ($cat:ident, $($arg:tt)*) => { $crate::klog_fmt!(Debug, $cat, $($arg)*) }; }
#[macro_export]
macro_rules! klog_crit  { ($cat:ident, $($arg:tt)*) => { $crate::klog_fmt!(Crit,  $cat, $($arg)*) }; }

/// 便捷宏: 按类别+级别 (常用组合)
#[macro_export]
macro_rules! klog_boot_info  { ($($arg:tt)*) => { $crate::klog_info!(Boot, $($arg)*) }; }
#[macro_export]
macro_rules! klog_kern_warn { ($($arg:tt)*) => { $crate::klog_warn!(Kernel, $($arg)*) }; }
#[macro_export]
macro_rules! klog_kern_err  { ($($arg:tt)*) => { $crate::klog_err!(Kernel, $($arg)*) }; }
#[macro_export]
macro_rules! klog_drv_warn  { ($($arg:tt)*) => { $crate::klog_warn!(Driver, $($arg)*) }; }
#[macro_export]
macro_rules! klog_drv_err   { ($($arg:tt)*) => { $crate::klog_err!(Driver, $($arg)*) }; }

#[macro_export]
macro_rules! klog_error { ($($arg:tt)*) => { $crate::klog_err!(Kernel, $($arg)*) }; }
#[macro_export]
macro_rules! klog_slab  { ($($arg:tt)*) => { $crate::klog_info!(Memory, $($arg)*) }; }
#[macro_export]
macro_rules! klog_info_simple { ($($arg:tt)*) => { $crate::klog_info!(Kernel, $($arg)*) }; }

// ============================================================================
// 端口 I/O 原语 (无需 driver 框架)
// ============================================================================

#[cfg(target_arch = "x86_64")]
mod serial_impl {
    const COM1: u16 = 0x3F8;

    #[inline(always)]
    unsafe fn port_outb(port: u16, value: u8) {
        crate::arch!(outb(port, value));
    }

    #[inline(always)]
    unsafe fn port_inb(port: u16) -> u8 {
        crate::arch!(inb(port))
    }

    pub fn serial_init() {
        unsafe {
            port_outb(COM1 + 1, 0x00);
            port_outb(COM1 + 3, 0x80);
            port_outb(COM1, 0x03);
            port_outb(COM1 + 1, 0x00);
            port_outb(COM1 + 3, 0x03);
            port_outb(COM1 + 2, 0xC7);
            port_outb(COM1 + 4, 0x0B);
        }
    }

    pub fn serial_putc(c: u8) {
        unsafe {
            while (port_inb(COM1 + 5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            port_outb(COM1, c);
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod serial_impl {
    use crate::kernel::framework::arch::aarch64::uart;

    pub fn serial_init() {
        // UART already initialized in entry.rs
    }

    pub fn serial_putc(c: u8) {
        unsafe {
            uart::putc(c);
        }
    }
}

fn serial_putc_chained(c: u8) {
    serial_impl::serial_putc(c);
}

fn serial_newline() {
    serial_impl::serial_putc(b'\r');
    serial_impl::serial_putc(b'\n');
}

pub fn serial_write_bytes(data: &[u8]) {
    for &byte in data {
        if byte == b'\n' {
            serial_newline();
        } else {
            serial_putc_chained(byte);
        }
    }
}

// ============================================================================
// 环形缓冲区
// ============================================================================

const RING_SIZE: usize = 128 * 1024;

struct RingBuf {
    data: [u8; RING_SIZE],
    head: usize,
    tail: usize,
    total: u64,
}

impl RingBuf {
    const fn new() -> Self {
        Self {
            data: [0; RING_SIZE],
            head: 0,
            tail: 0,
            total: 0,
        }
    }

    fn push(&mut self, b: u8) {
        self.data[self.tail] = b;
        self.tail = (self.tail + 1) % RING_SIZE;
        if self.tail == self.head {
            self.head = (self.head + 1) % RING_SIZE;
        }
        self.total = self.total.wrapping_add(1);
    }

    fn push_str(&mut self, s: &[u8]) {
        for &b in s {
            self.push(b);
        }
    }
}

struct RingLock {
    inner: core::cell::UnsafeCell<RingBuf>,
}
unsafe impl Sync for RingLock {}

static RING: RingLock = RingLock {
    inner: core::cell::UnsafeCell::new(RingBuf::new()),
};

// ============================================================================
// 全局状态
// ============================================================================

static MIN_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);
pub static KLOG_INIT: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

// ============================================================================
// 日志级别 / 分类
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Note = 2,
    Warn = 3,
    Error = 4,
    Crit = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(clippy::upper_case_acronyms)]  // IPC 子系统名
pub enum LogCategory {
    Boot = 0,
    Kernel = 1,
    Memory = 2,
    Process = 3,
    FS = 4,
    Net = 5,
    Driver = 6,
    Syscall = 7,
    IPC = 8,
    Security = 9,
    Test = 10,
    Sync = 11,
    Swap = 12,
}

impl LogLevel {
    fn prefix(&self) -> &[u8] {
        match self {
            LogLevel::Debug => b"[DBG] ",
            LogLevel::Info => b"[INFO]",
            LogLevel::Note => b"[NOTE]",
            LogLevel::Warn => b"[WARN]",
            LogLevel::Error => b"[ERR] ",
            LogLevel::Crit => b"[CRIT]",
        }
    }
}

impl LogCategory {
    fn name(&self) -> &[u8] {
        match self {
            LogCategory::Boot => b"BOOT",
            LogCategory::Kernel => b"KERN",
            LogCategory::Memory => b"MEM",
            LogCategory::Process => b"PROC",
            LogCategory::FS => b"FS",
            LogCategory::Net => b"NET",
            LogCategory::Driver => b"DRV",
            LogCategory::Syscall => b"SYSCALL",
            LogCategory::IPC => b"IPC",
            LogCategory::Security => b"SEC",
            LogCategory::Test => b"TEST",
            LogCategory::Sync => b"SYNC",
            LogCategory::Swap => b"SWAP",
        }
    }
}

// ============================================================================
// 辅助
// ============================================================================

unsafe fn cstr_slice(ptr: *const u8) -> &'static [u8] {
    if ptr.is_null() {
        return b"(null)";
    }
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
        if len > 1024 {
            return b"(truncated)";
        }
    }
    core::slice::from_raw_parts(ptr, len)
}

fn rdtsc() -> u64 {
    crate::arch!(timestamp())
}

/// 格式化 u64 → 栈上 buffer (最小化依赖)
fn format_ts(buf: &mut [u8; 32], tsc: u64) -> &[u8] {
    // 假设 ~3GHz TSC, 显示秒.微秒
    let total_us = tsc / 3000;
    let sec = total_us / 1_000_000;
    let us = total_us % 1_000_000;
    let mut pos = 0usize;
    // 写 sec
    let s = sec;
    if s == 0 {
        buf[pos] = b'0';
        pos += 1;
    } else {
        let mut tmp = [0u8; 20];
        let mut n = 0;
        let mut v = s;
        while v > 0 {
            tmp[n] = (v % 10) as u8 + b'0';
            n += 1;
            v /= 10;
        }
        while n > 0 {
            n -= 1;
            buf[pos] = tmp[n];
            pos += 1;
        }
    }
    buf[pos] = b'.';
    pos += 1;
    // 写 us (6 位补零)
    for i in (0..6).rev() {
        let digit = (us / 10u64.pow(i)) % 10;
        buf[pos] = digit as u8 + b'0';
        pos += 1;
    }
    buf[pos] = b' ';
    pos += 1;
    &buf[..pos]
}

// ============================================================================
// 核心输出
// ============================================================================

fn klog_output(level: LogLevel, cat: LogCategory, msg: &[u8]) {
    let tsc = rdtsc();
    let mut ts_buf = [0u8; 32];
    let ts = format_ts(&mut ts_buf, tsc);

    let saved_if = crate::arch!(interrupt_disable());

    serial_write_bytes(ts);
    serial_write_bytes(level.prefix());
    serial_write_bytes(b" ");
    serial_write_bytes(b"[");
    serial_write_bytes(cat.name());
    serial_write_bytes(b"] ");
    serial_write_bytes(msg);
    serial_newline();

    crate::arch!(interrupt_restore(saved_if));

    let ring = unsafe { &mut *RING.inner.get() };
    ring.push_str(ts);
    ring.push_str(level.prefix());
    ring.push(b' ');
    ring.push(b'[');
    ring.push_str(cat.name());
    ring.push(b']');
    ring.push(b' ');
    ring.push_str(msg);
    ring.push(b'\n');

    crate::kernel::framework::console::gfx_console_write(msg);
}

// ============================================================================
// 公共 API
// ============================================================================

pub fn klog_set_level(level: LogLevel) {
    MIN_LEVEL.store(level as u8, Ordering::Relaxed);
}

pub fn klog_get_level() -> LogLevel {
    match MIN_LEVEL.load(Ordering::Relaxed) {
        0 => LogLevel::Debug,
        1 => LogLevel::Info,
        2 => LogLevel::Note,
        3 => LogLevel::Warn,
        4 => LogLevel::Error,
        _ => LogLevel::Crit,
    }
}

// ============================================================================
// FFI 桩 → 全部实现在此
// ============================================================================

#[no_mangle]
///
/// # Safety
///
/// Must be called exactly once before any logging. Caller ensures serial hardware is present.
pub unsafe extern "C" fn klog_init() {
    serial_impl::serial_init();
    KLOG_INIT.store(true, Ordering::Release);
    klog_output(LogLevel::Info, LogCategory::Boot, b"KLog initialized");
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_write(
    level: u8,
    cat: u8,
    _file: *const u8,
    _func: *const u8,
    _line: u32,
    fmt: *const u8,
) -> i32 {
    if fmt.is_null() {
        return -1;
    }

    let lvl = match level {
        0 => LogLevel::Debug,
        1 => LogLevel::Info,
        2 => LogLevel::Note,
        3 => LogLevel::Warn,
        4 => LogLevel::Error,
        5 => LogLevel::Crit,
        _ => return -1,
    };

    let category = match cat {
        0 => LogCategory::Boot,
        1 => LogCategory::Kernel,
        2 => LogCategory::Memory,
        3 => LogCategory::Process,
        4 => LogCategory::FS,
        5 => LogCategory::Net,
        6 => LogCategory::Driver,
        7 => LogCategory::Syscall,
        8 => LogCategory::IPC,
        9 => LogCategory::Security,
        10 => LogCategory::Test,
        _ => LogCategory::Kernel,
    };

    let min = MIN_LEVEL.load(Ordering::Relaxed);
    if (level as i32) < (min as i32) {
        return 0;
    }

    let msg = cstr_slice(fmt as *const u8);
    klog_output(lvl, category, msg);
    0
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_ffi_info(msg: *const u8) {
    if msg.is_null() {
        return;
    }
    let s = cstr_slice(msg);
    klog_output(LogLevel::Info, LogCategory::Kernel, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_ffi_warn(msg: *const u8) {
    if msg.is_null() {
        return;
    }
    let s = cstr_slice(msg);
    klog_output(LogLevel::Warn, LogCategory::Kernel, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_ffi_error(msg: *const u8) {
    if msg.is_null() {
        return;
    }
    let s = cstr_slice(msg);
    klog_output(LogLevel::Error, LogCategory::Kernel, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_net(fmt: *const u8) {
    if fmt.is_null() {
        return;
    }
    let s = cstr_slice(fmt as *const u8);
    klog_output(LogLevel::Info, LogCategory::Net, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_net_err(fmt: *const u8) {
    if fmt.is_null() {
        return;
    }
    let s = cstr_slice(fmt as *const u8);
    klog_output(LogLevel::Error, LogCategory::Net, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_init_msg(fmt: *const i8) {
    if fmt.is_null() {
        return;
    }
    let s = cstr_slice(fmt as *const u8);
    klog_output(LogLevel::Info, LogCategory::Boot, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_kern(fmt: *const i8) {
    if fmt.is_null() {
        return;
    }
    let s = cstr_slice(fmt as *const u8);
    klog_output(LogLevel::Info, LogCategory::Kernel, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_syscall(fmt: *const i8) {
    if fmt.is_null() {
        return;
    }
    let s = cstr_slice(fmt as *const u8);
    klog_output(LogLevel::Info, LogCategory::Syscall, s);
}

#[no_mangle]
///
/// # Safety
///
/// `msg`/`fmt` is a valid pointer to a null-terminated C string in kernel-accessible memory.
pub unsafe extern "C" fn klog_info(fmt: *const i8) {
    if fmt.is_null() {
        return;
    }
    let s = cstr_slice(fmt as *const u8);
    klog_output(LogLevel::Info, LogCategory::Kernel, s);
}
