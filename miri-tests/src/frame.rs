//! 物理页帧抽象 (Miri 验证版)
//!
//! 与内核 `kernel/framework/frame.rs` 等价, 用于验证
//! 对齐算术 / 边界检查 / 类型转换的正确性。

pub const PAGE_SIZE: usize = 4096;
pub const MAX_ORDER: u8 = 11; // 2^11 = 2048 页 = 8 MiB

/// 物理地址 (裸 u64, Miri 友好)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PhysAddr(u64);

impl PhysAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// 检查是否页对齐
    pub fn is_page_aligned(self) -> bool {
        self.0.is_multiple_of(PAGE_SIZE as u64)
    }
}

/// 页帧 (对齐到 PAGE_SIZE)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub phys: PhysAddr,
    pub order: u8,
}

impl Frame {
    /// # Safety
    /// - `phys` 必须页对齐
    /// - `order` 必须 ≤ MAX_ORDER
    pub unsafe fn from_raw(phys: PhysAddr, order: u8) -> Self {
        debug_assert!(phys.is_page_aligned(), "Frame must be page-aligned");
        debug_assert!(order <= MAX_ORDER, "order must be <= MAX_ORDER");
        Self { phys, order }
    }

    /// 帧覆盖的字节数 = 2^order * PAGE_SIZE
    pub fn size_bytes(&self) -> usize {
        // 检查溢出
        let pages = 1u64 << self.order.min(20); // 限制 shift, 避免溢出
        (pages as usize) * PAGE_SIZE
    }

    /// 帧结束物理地址 (不含)
    pub fn end(&self) -> PhysAddr {
        PhysAddr(self.phys.as_u64() + self.size_bytes() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_alignment_check() {
        let aligned = PhysAddr::new(0x1000);
        assert!(aligned.is_page_aligned());

        let unaligned = PhysAddr::new(0x1234);
        assert!(!unaligned.is_page_aligned());
    }

    #[test]
    fn frame_size_calculation() {
        // SAFETY: 0 页对齐, order=0 合法
        let f = unsafe { Frame::from_raw(PhysAddr::new(0), 0) };
        assert_eq!(f.size_bytes(), PAGE_SIZE);

        // SAFETY: order=2 合法
        let f = unsafe { Frame::from_raw(PhysAddr::new(0x10000), 2) };
        assert_eq!(f.size_bytes(), 4 * PAGE_SIZE);
    }

    #[test]
    fn frame_end_calculation() {
        // SAFETY: 0x1000 对齐, order=0
        let f = unsafe { Frame::from_raw(PhysAddr::new(0x1000), 0) };
        assert_eq!(f.end(), PhysAddr::new(0x2000));
    }

    #[test]
    fn frame_no_overflow() {
        // 即使 order = MAX_ORDER 也不溢出 (内部已 clamp)
        // SAFETY: order=MAX_ORDER
        let f = unsafe { Frame::from_raw(PhysAddr::new(0), MAX_ORDER) };
        let size = f.size_bytes();
        assert!(size > 0);
    }
}
