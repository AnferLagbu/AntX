//! IoMem 别名检测压测 (Miri 验证版)
//!
//! 与内核 `kernel/framework/iomem.rs` 的 `AliasRegistry` 行为等价,
//! 验证:
//! - 重叠区间冲突检测 (start < other_end && end > other_start)
//! - 邻接区间不视为冲突 (start == other_end)
//! - 零长度区域边界
//! - 注销后位置交换正确性
//! - 满载后插入返回错误
//!
//! 为避免 cargo test 并发执行的全局状态问题, 所有测试使用**本地**
//! AliasRegistry 实例, 与内核中 spin::Mutex 全局等价行为通过类型隔离。

const MAX_MMIO_MAPPINGS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasError {
    Full,
    Overlap { conflict_with: &'static str },
}

pub struct AliasRegistry {
    entries: [(u64, usize, &'static str); MAX_MMIO_MAPPINGS],
    count: usize,
}

impl AliasRegistry {
    pub const fn new() -> Self {
        Self {
            entries: [(0, 0, ""); MAX_MMIO_MAPPINGS],
            count: 0,
        }
    }

    /// 检查 [phys, phys+len) 是否与已注册区域重叠
    pub fn check_conflict(&self, phys: u64, len: usize) -> Option<&'static str> {
        if len == 0 {
            return None; // 零长度区域无冲突
        }
        let end = phys.saturating_add(len as u64);
        for i in 0..self.count {
            let (b, l, name) = self.entries[i];
            if l == 0 {
                continue;
            }
            let existing_end = b.saturating_add(l as u64);
            if phys < existing_end && end > b {
                return Some(name);
            }
        }
        None
    }

    pub fn register(
        &mut self,
        phys: u64,
        len: usize,
        name: &'static str,
    ) -> Result<(), AliasError> {
        if self.count >= MAX_MMIO_MAPPINGS {
            return Err(AliasError::Full);
        }
        if let Some(conflict) = self.check_conflict(phys, len) {
            return Err(AliasError::Overlap { conflict_with: conflict });
        }
        self.entries[self.count] = (phys, len, name);
        self.count += 1;
        Ok(())
    }

    /// 注销: 找到 phys 完全匹配的条目, 与最后一个交换
    pub fn unregister(&mut self, phys: u64) {
        for i in 0..self.count {
            if self.entries[i].0 == phys {
                self.entries[i] = self.entries[self.count - 1];
                self.count -= 1;
                return;
            }
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_conflict_disjoint() {
        let mut r = AliasRegistry::new();
        // 完全不相交
        assert!(r.register(0x1000, 0x100, "a").is_ok());
        assert!(r.register(0x2000, 0x100, "b").is_ok());
        assert_eq!(r.count(), 2);
    }

    #[test]
    fn no_conflict_adjacent() {
        let mut r = AliasRegistry::new();
        // 邻接: 0..0x100 与 0x100..0x200 不重叠
        assert!(r.register(0x0, 0x100, "a").is_ok());
        assert!(r.register(0x100, 0x100, "b").is_ok());
        assert_eq!(r.count(), 2);
    }

    #[test]
    fn conflict_overlap_partial() {
        let mut r = AliasRegistry::new();
        // 0..0x100 与 0x80..0x180 部分重叠
        assert!(r.register(0x0, 0x100, "a").is_ok());
        let res = r.register(0x80, 0x100, "b");
        assert!(matches!(res, Err(AliasError::Overlap { conflict_with: "a" })));
    }

    #[test]
    fn conflict_contained() {
        let mut r = AliasRegistry::new();
        // 0..0x100 包含 0x40..0x60
        assert!(r.register(0x0, 0x100, "a").is_ok());
        let res = r.register(0x40, 0x20, "b");
        assert!(matches!(res, Err(AliasError::Overlap { conflict_with: "a" })));
    }

    #[test]
    fn conflict_contain() {
        let mut r = AliasRegistry::new();
        // 0x40..0x60 被 0..0x100 包含
        assert!(r.register(0x40, 0x20, "a").is_ok());
        let res = r.register(0x0, 0x100, "b");
        assert!(matches!(res, Err(AliasError::Overlap { conflict_with: "a" })));
    }

    #[test]
    fn zero_length_no_conflict() {
        let mut r = AliasRegistry::new();
        // 零长度区间永远不冲突 (但登记 0 长度无意义, 行为退化)
        assert!(r.register(0x0, 0, "z").is_ok());
        assert!(r.check_conflict(0x0, 0).is_none());
    }

    #[test]
    fn unregister_swap() {
        let mut r = AliasRegistry::new();
        // 注销中间条目: 用末尾条目填充
        r.register(0x100, 0x10, "a").unwrap();
        r.register(0x200, 0x10, "b").unwrap();
        r.register(0x300, 0x10, "c").unwrap();
        assert_eq!(r.count(), 3);

        r.unregister(0x200);
        assert_eq!(r.count(), 2);

        // 重新登记 0x200 应成功 (因为 b 已注销)
        assert!(r.register(0x200, 0x10, "b2").is_ok());
        // c 仍应在
        assert!(r.check_conflict(0x300, 0x10).is_some());
    }

    #[test]
    fn unregister_nonexistent() {
        let mut r = AliasRegistry::new();
        // 注销不存在的 phys 是 no-op
        r.unregister(0xdead_beef);
        // 不应 panic
        assert_eq!(r.count(), 0);
    }

    #[test]
    fn saturating_overflow_safe() {
        let mut r = AliasRegistry::new();
        // phys + len 接近 u64::MAX, saturating_add 防溢出
        assert!(r.register(u64::MAX - 10, 100, "huge").is_ok());
        // 触发求和溢出
        let res = r.check_conflict(u64::MAX, 1);
        // huge 区域 [u64::MAX-10, u64::MAX+90) 经过 saturating 后是
        // [u64::MAX-10, u64::MAX], 与 [u64::MAX, u64::MAX+1) 不重叠
        assert!(res.is_none());
    }

    #[test]
    fn full_registry_rejects() {
        let mut r = AliasRegistry::new();
        // 注满注册表
        for i in 0..MAX_MMIO_MAPPINGS {
            r.register((i as u64) * 0x1000, 0x10, "filler").unwrap();
        }
        // 再注册应失败
        let res = r.register(0xffffff, 0x10, "overflow");
        assert!(matches!(res, Err(AliasError::Full)));
    }

    #[test]
    fn stress_random_patterns() {
        // 压测: 1000 个随机区间, 验证 count 始终 ≤ MAX
        let mut r = AliasRegistry::new();
        let mut ok_count = 0;
        let mut err_count = 0;
        for i in 0..1000u64 {
            let phys = (i.wrapping_mul(0x9E3779B97F4A7C15)) % 0x1_0000_0000;
            let len = ((i % 7) + 1) * 0x10;
            match r.register(phys, len as usize, "stress") {
                Ok(()) => ok_count += 1,
                Err(_) => err_count += 1,
            }
            // 不变式: count 永远 ≤ MAX
            assert!(r.count() <= MAX_MMIO_MAPPINGS);
        }
        // 至少有一些成功
        assert!(ok_count > 0);
        // 总和 = 1000
        assert_eq!(ok_count + err_count, 1000);
    }

    #[test]
    fn stress_lifecycle() {
        // 压测: 反复 register/unregister, 验证不变量
        let mut r = AliasRegistry::new();
        for round in 0..100 {
            for i in 0..32 {
                let phys = (round * 32 + i) * 0x1000;
                r.register(phys, 0x100, "cycle").unwrap();
            }
            assert_eq!(r.count(), 32);
            for i in 0..32 {
                let phys = (round * 32 + i) * 0x1000;
                r.unregister(phys);
            }
            assert_eq!(r.count(), 0);
        }
    }
}
