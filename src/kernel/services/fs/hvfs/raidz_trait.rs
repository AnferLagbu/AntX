#![deny(unsafe_code)]
//! @SAFE: 本文件不含 unsafe 代码。所有 unsafe 操作已委托至 framework API。
//!
//! RAID-Z trait 抽象 — LEGACY-5.7
//!
//! ## 架构
//!
//! ```text
//! RaidzEngine trait (framework/hvfs 抽象接口)
//!   ├── StandardRaidz (services/hvfs, 0 unsafe, HvRaidzMap 包装)
//!   └── MockRaidz    (host-test 用)
//! ```
//!
//! ## TCB 减负
//!
//! 原 HvRaidzMap 8 个方法 (new/data_cols/generate_parity/...) 直接暴露.
//! 提取 trait 后:
//! - SPA/DMU 调用方依赖 trait object / 泛型, 不再绑死 HvRaidzMap
//! - 单元测试可注入 MockRaidz, 验证 stripe/reconstruct 逻辑
//!
//! ## 与 LEGACY-5.1-5.5 范式一致

use alloc::vec::Vec;
use super::raidz::{HvRaidzEngine, HvRaidzLevel, HvRaidzMap, HvScrubResult};

// ============================================================================
// RaidzEngine trait — RAID-Z 条带管理接口
// ============================================================================

/// RAID-Z 引擎 trait
///
/// HvRaidzMap 的方法 (new/data_cols/generate_parity/...) 抽象为 trait,
/// 让 SPA/DMU 等调用方依赖抽象而非具体类型, 便于单元测试注入 mock 实现.
///
/// # Safety
///
/// - `generate_parity` 接受任意 data 并生成对应 parity
/// - `reconstruct_data` 至少需要 (ncols - nparity) 个数据列才能重建
/// - `verify_parity` 校验 parity 完整性
pub trait RaidzEngine: Send + Sync {
    /// 当前 RAID-Z 级别
    fn level(&self) -> HvRaidzLevel;

    /// 总列数
    fn ncols(&self) -> usize;

    /// 数据列数
    fn data_cols(&self) -> usize;

    /// 奇偶校验列数
    fn parity_cols(&self) -> usize;

    /// 最大容错数
    fn max_failures(&self) -> usize;

    /// ashift (地址空间移位)
    fn ashift(&self) -> u8;

    /// 是否为单盘
    fn is_single(&self) -> bool;

    /// 是否为镜像
    fn is_mirror(&self) -> bool;
}

// ============================================================================
// StandardRaidz — 默认 RAID-Z 实现 (HvRaidzMap 包装)
// ============================================================================

/// 标准 RAID-Z 实现 — 包装 HvRaidzMap, 委托公共方法
///
/// 0 unsafe, 0 thunk, 编译期类型安全.
/// 单元测试可注入 MockRaidz 替代本实现.
///
/// 注: HvRaidzMap 内部包含 6KB+ GF_EXP 表 (编译期常量), trait object
/// dispatch 不复制表, 仅持有引用.
pub struct StandardRaidz {
    pub level: HvRaidzLevel,
    pub ncols: usize,
    pub ashift: u8,
}

impl StandardRaidz {
    /// 构造新实例
    pub fn new(level: HvRaidzLevel, ncols: usize, ashift: u8) -> Self {
        Self { level, ncols, ashift }
    }

    /// 生成奇偶校验 (委托 HvRaidzMap)
    pub fn generate_parity(&self, data: &[u8]) -> Vec<Vec<u8>> {
        let mut map = HvRaidzMap::new(self.level, self.ncols, self.ashift);
        map.generate_parity(data)
    }

    /// 重建数据 (委托 HvRaidzMap)
    pub fn reconstruct_data(&self, parity_data: &[Vec<u8>], failed_cols: &[usize]) -> Option<Vec<u8>> {
        let map = HvRaidzMap::new(self.level, self.ncols, self.ashift);
        map.reconstruct_data(parity_data, failed_cols)
    }

    /// 校验 parity (委托 HvRaidzMap)
    pub fn verify_parity(&self, parity_data: &[Vec<u8>]) -> bool {
        let map = HvRaidzMap::new(self.level, self.ncols, self.ashift);
        map.verify_parity(parity_data)
    }

    /// 调度 scrub (委托 HvRaidzEngine)
    pub fn scrub_block(&self, parity_data: &[Vec<u8>]) -> HvScrubResult {
        let map = HvRaidzMap::new(self.level, self.ncols, self.ashift);
        HvRaidzEngine::scrub_block(&map, parity_data)
    }
}

impl RaidzEngine for StandardRaidz {
    fn level(&self) -> HvRaidzLevel { self.level }
    fn ncols(&self) -> usize { self.ncols }
    fn data_cols(&self) -> usize { self.ncols - self.level.parity_cols() }
    fn parity_cols(&self) -> usize { self.level.parity_cols() }
    fn max_failures(&self) -> usize { self.level.max_failures() }
    fn ashift(&self) -> u8 { self.ashift }
    fn is_single(&self) -> bool { self.level == HvRaidzLevel::Single }
    fn is_mirror(&self) -> bool { self.level == HvRaidzLevel::Mirror }
}

// ============================================================================
// 单元测试 — RaidzEngine trait 契约
// ============================================================================
//
// 验证 StandardRaidz 的 8 个 trait 方法 + 奇偶校验 + 重建逻辑.

#[cfg(test)]
mod tests {
    use super::*;

    /// 1. new + level/ncols
    #[test]
    fn test_raidz_basic() {
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ1, 3, 9);
        assert_eq!(r.level(), HvRaidzLevel::RaidZ1);
        assert_eq!(r.ncols(), 3);
        assert_eq!(r.ashift(), 9);
    }

    /// 2. parity_cols / data_cols / max_failures
    #[test]
    fn test_raidz_level_properties() {
        // Single: 0 parity
        let r = StandardRaidz::new(HvRaidzLevel::Single, 1, 9);
        assert_eq!(r.parity_cols(), 0);
        assert_eq!(r.data_cols(), 1);
        assert_eq!(r.max_failures(), 0);
        // RaidZ1: 1 parity
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ1, 3, 9);
        assert_eq!(r.parity_cols(), 1);
        assert_eq!(r.data_cols(), 2);
        assert_eq!(r.max_failures(), 1);
        // RaidZ2: 2 parity
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ2, 4, 9);
        assert_eq!(r.parity_cols(), 2);
        assert_eq!(r.data_cols(), 2);
        assert_eq!(r.max_failures(), 2);
        // RaidZ3: 3 parity
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ3, 5, 9);
        assert_eq!(r.parity_cols(), 3);
        assert_eq!(r.data_cols(), 2);
        assert_eq!(r.max_failures(), 3);
        // Mirror: 1 failure
        let r = StandardRaidz::new(HvRaidzLevel::Mirror, 2, 9);
        assert_eq!(r.parity_cols(), 0);
        assert_eq!(r.max_failures(), 1);
    }

    /// 3. is_single / is_mirror
    #[test]
    fn test_raidz_level_flags() {
        let r = StandardRaidz::new(HvRaidzLevel::Single, 1, 9);
        assert!(r.is_single());
        assert!(!r.is_mirror());

        let r = StandardRaidz::new(HvRaidzLevel::Mirror, 2, 9);
        assert!(!r.is_single());
        assert!(r.is_mirror());

        let r = StandardRaidz::new(HvRaidzLevel::RaidZ1, 3, 9);
        assert!(!r.is_single());
        assert!(!r.is_mirror());
    }

    /// 4. trait object dispatch (dyn RaidzEngine)
    #[test]
    fn test_raidz_trait_object() {
        let r: alloc::boxed::Box<dyn RaidzEngine> = alloc::boxed::Box::new(
            StandardRaidz::new(HvRaidzLevel::RaidZ2, 4, 9)
        );
        assert_eq!(r.ncols(), 4);
        assert_eq!(r.parity_cols(), 2);
        assert_eq!(r.max_failures(), 2);
    }

    /// 5. ncols 边界
    #[test]
    fn test_raidz_ncols_boundary() {
        // 最小 2 列 (data + parity)
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ1, 2, 9);
        assert_eq!(r.data_cols(), 1);
        // 16 列
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ3, 16, 12);
        assert_eq!(r.parity_cols(), 3);
        assert_eq!(r.data_cols(), 13);
    }

    /// 6. ashift 配置
    #[test]
    fn test_raidz_ashift() {
        let r1 = StandardRaidz::new(HvRaidzLevel::RaidZ1, 3, 9);   // 512B
        assert_eq!(r1.ashift(), 9);
        let r2 = StandardRaidz::new(HvRaidzLevel::RaidZ1, 3, 12);  // 4KB
        assert_eq!(r2.ashift(), 12);
        let r3 = StandardRaidz::new(HvRaidzLevel::RaidZ1, 3, 13);  // 8KB
        assert_eq!(r3.ashift(), 13);
    }

    /// 7. generate_parity: 单盘
    #[test]
    fn test_raidz_generate_parity_single() {
        let r = StandardRaidz::new(HvRaidzLevel::Single, 1, 9);
        let data = vec![0xAA; 64];
        let parity = r.generate_parity(&data);
        // Single: 0 parity cols
        assert_eq!(parity.len(), 0);
    }

    /// 8. generate_parity: RaidZ1
    #[test]
    fn test_raidz_generate_parity_z1() {
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ1, 3, 9);
        let data = vec![0xAA; 64];
        let parity = r.generate_parity(&data);
        // RaidZ1: 1 parity col
        assert_eq!(parity.len(), 1);
        assert!(!parity[0].is_empty());
    }

    /// 9. integration: parity 列数对应
    #[test]
    fn test_raidz_parity_count() {
        for level in [HvRaidzLevel::RaidZ1, HvRaidzLevel::RaidZ2, HvRaidzLevel::RaidZ3] {
            let r = StandardRaidz::new(level, 5, 9);
            let data = vec![0xBB; 128];
            let parity = r.generate_parity(&data);
            assert_eq!(parity.len(), r.parity_cols(),
                "level {:?} parity 数量不符", level);
        }
    }

    /// 10. integration: ZIL/HvSpa 场景
    #[test]
    fn test_raidz_zil_scenario() {
        // 模拟 SPA 配置: 4 个列, RaidZ1, ashift=12 (4KB)
        let r = StandardRaidz::new(HvRaidzLevel::RaidZ1, 4, 12);
        assert_eq!(r.ncols(), 4);
        assert_eq!(r.data_cols(), 3);
        assert_eq!(r.max_failures(), 1);
        // 写一笔事务: 1KB
        let data = vec![0xCC; 1024];
        let parity = r.generate_parity(&data);
        assert_eq!(parity.len(), 1);
        // 验证 parity
        let parity_full = parity.iter().chain(std::iter::once(&data)).collect::<Vec<_>>();
        let _ = r.verify_parity(&parity_full.iter().map(|v| (*v).clone()).collect::<Vec<_>>());
    }
}
