//! 配置 / 内存布局校验 (Miri 验证版)
//!
//! 与内核 `kernel/config/validate.rs` 等价, 验证:
//! - 对齐检查 (`is_multiple_of`)
//! - 范围检查不溢出
//! - 错误码枚举的穷尽匹配

pub const PAGE_SIZE: u64 = 4096;
pub const SLAB_DEFAULT_SIZE: u64 = 8192;
pub const MAX_STACK_SIZE: u64 = 16 * 1024 * 1024; // 16 MiB
pub const MIN_STACK_SIZE: u64 = 16 * 1024; // 16 KiB

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    PageSizeInvalid,
    SlabNotPageAligned,
    StackOutOfRange,
    MemoryLayoutInvalid,
}

pub fn validate_memory_layout() -> Result<(), ConfigError> {
    // 1. PAGE_SIZE 必须是 2 的幂
    if !PAGE_SIZE.is_power_of_two() {
        return Err(ConfigError::PageSizeInvalid);
    }
    // 2. SLAB_DEFAULT_SIZE 必须按页对齐
    if !SLAB_DEFAULT_SIZE.is_multiple_of(PAGE_SIZE) {
        return Err(ConfigError::SlabNotPageAligned);
    }
    // 3. 栈尺寸合法性
    if MIN_STACK_SIZE > MAX_STACK_SIZE {
        return Err(ConfigError::StackOutOfRange);
    }
    // 4. 不允许 SLAB 比页小
    if SLAB_DEFAULT_SIZE < PAGE_SIZE {
        return Err(ConfigError::MemoryLayoutInvalid);
    }
    Ok(())
}

/// 内存区域描述
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemRegion {
    pub base: u64,
    pub size: u64,
}

impl MemRegion {
    pub fn new(base: u64, size: u64) -> Self {
        Self { base, size }
    }

    /// 区域是否包含 [addr, addr+len)
    ///
    /// 关键: 避免 `addr + len` 溢出
    pub fn contains(&self, addr: u64, len: u64) -> bool {
        if len == 0 {
            return true; // 空区间, 所有区域都"包含"
        }
        // 防止 addr + len 溢出 u64
        if addr.checked_add(len).is_none() {
            return false;
        }
        // 防止 self.base + self.size 溢出
        let end = match self.base.checked_add(self.size) {
            Some(e) => e,
            None => return false,
        };
        addr >= self.base && addr.checked_add(len).unwrap() <= end
    }

    /// 检查与另一区域是否重叠
    pub fn overlaps(&self, other: &MemRegion) -> bool {
        if self.size == 0 || other.size == 0 {
            return false;
        }
        let self_end = match self.base.checked_add(self.size) {
            Some(e) => e,
            None => return true, // 溢出视为重叠 (安全保守)
        };
        let other_end = match other.base.checked_add(other.size) {
            Some(e) => e,
            None => return true,
        };
        self.base < other_end && other.base < self_end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_passes() {
        assert_eq!(validate_memory_layout(), Ok(()));
    }

    #[test]
    fn contains_normal() {
        let r = MemRegion::new(0x1000, 0x2000); // [0x1000, 0x3000)
        assert!(r.contains(0x1000, 0));
        assert!(r.contains(0x1000, 0x100));
        assert!(r.contains(0x2f00, 0x100));
        assert!(!r.contains(0x3000, 0x100));
        assert!(!r.contains(0x500, 0x100));
    }

    #[test]
    fn contains_no_overflow() {
        // addr + len 接近 u64::MAX 不溢出
        let r = MemRegion::new(0, u64::MAX);
        assert!(r.contains(0, 1));
        // 触发 addr + len 溢出
        let r = MemRegion::new(0, 0x1000);
        assert!(!r.contains(u64::MAX, 1));
    }

    #[test]
    fn overlaps_basic() {
        let a = MemRegion::new(0, 100);
        let b = MemRegion::new(50, 100);
        assert!(a.overlaps(&b));

        let c = MemRegion::new(200, 100);
        assert!(!a.overlaps(&c));

        let d = MemRegion::new(100, 100);
        assert!(!a.overlaps(&d)); // 邻接不算重叠
    }

    #[test]
    fn overlaps_empty() {
        let a = MemRegion::new(0, 0);
        let b = MemRegion::new(50, 100);
        assert!(!a.overlaps(&b));
    }
}
