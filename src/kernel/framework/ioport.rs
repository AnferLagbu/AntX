//! IoPort — x86 PIO 安全封装 (TCB)
//!
//! 封装 x86_64 in/out 指令, 提供端口范围校验。
//! aarch64 上无 PIO 语义, 编译为空壳。使用时需 feature gate。
//!
//! ## 适用场景
//!
//! ✅ x86_64 Legacy 设备 (PCI 配置空间, ATA, VGA, 串口)
//! ❌ aarch64 (使用 MMIO 代替)
//!
//! ## SAFETY 不变量
//!
//! - port 必须是有效的 I/O 端口地址 (0..65536)。
//! - PIO 指令使用 nostack, nomem 选项避免编译器重排。

#![allow(dead_code)]

#[cfg(target_arch = "x86_64")]
use core::arch::asm;

/// x86 I/O 端口句柄。
///
/// 封装一个有效的 I/O 端口范围, 提供类型安全的读写。
pub struct IoPort {
    base: u16,
    len: u16,
    name: &'static str,
}

impl IoPort {
    /// 创建 PIO 句柄。
    ///
    /// # SAFETY
    /// - `base..base+len` 必须是有效的 I/O 端口范围。
    /// - 不与任何其他 `IoPort` 实例重叠 (由调用方通过 PCI BAR 信息保证)。
    pub unsafe fn new(base: u16, len: u16, name: &'static str) -> Result<Self, &'static str> {
        if len == 0 {
            return Err("IoPort: zero-length port range");
        }
        if base.checked_add(len).is_none() {
            return Err("IoPort: port range overflow");
        }
        Ok(Self { base, len, name })
    }

    /// 基端口号
    #[inline(always)]
    pub fn base(&self) -> u16 {
        self.base
    }

    fn check_offset(&self, offset: u16, size: u16) -> Result<u16, &'static str> {
        if offset.saturating_add(size) > self.len {
            return Err("IoPort: access out of bounds");
        }
        Ok(self.base + offset)
    }

    /// 读取 u8
    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub fn read_u8(&self, offset: u16) -> u8 {
        let port = self.check_offset(offset, 1).unwrap_or_else(|e| panic!("IoPort::read_u8: {}", e));
        unsafe {
            let val: u8;
            asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
            val
        }
    }

    /// 读取 u16
    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub fn read_u16(&self, offset: u16) -> u16 {
        let port = self.check_offset(offset, 2).unwrap_or_else(|e| panic!("IoPort::read_u16: {}", e));
        unsafe {
            let val: u16;
            asm!("in ax, dx", in("dx") port, out("ax") val, options(nomem, nostack));
            val
        }
    }

    /// 读取 u32
    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub fn read_u32(&self, offset: u16) -> u32 {
        let port = self.check_offset(offset, 4).unwrap_or_else(|e| panic!("IoPort::read_u32: {}", e));
        unsafe {
            let val: u32;
            asm!("in eax, dx", in("dx") port, out("eax") val, options(nomem, nostack));
            val
        }
    }

    /// 写入 u8
    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub fn write_u8(&self, offset: u16, val: u8) {
        let port = self.check_offset(offset, 1).unwrap_or_else(|e| panic!("IoPort::write_u8: {}", e));
        unsafe {
            asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
        }
    }

    /// 写入 u16
    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub fn write_u16(&self, offset: u16, val: u16) {
        let port = self.check_offset(offset, 2).unwrap_or_else(|e| panic!("IoPort::write_u16: {}", e));
        unsafe {
            asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack));
        }
    }

    /// 写入 u32
    #[cfg(target_arch = "x86_64")]
    #[inline]
    pub fn write_u32(&self, offset: u16, val: u32) {
        let port = self.check_offset(offset, 4).unwrap_or_else(|e| panic!("IoPort::write_u32: {}", e));
        unsafe {
            asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack));
        }
    }
}

// SAFETY: IoPort 封装了独占的 PIO 端口范围, 内核态独占访问。
unsafe impl Send for IoPort {}
unsafe impl Sync for IoPort {}
