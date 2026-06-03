//! 双架构一致性测试 (Miri 验证版)
//!
//! 验证 x86_64 与 aarch64 在 QueenX 关键路径上的**行为等价性**:
//! - 页大小常量
//! - 物理↔虚拟地址映射
//! - 缓存维护操作 (空 vs 显式)
//! - 字节序 (x86_64/aarch64 均为 LE, 但需验证)
//! - 原子操作宽度
//!
//! ## 测试策略
//!
//! 由于 Miri 在宿主架构上运行, 我们用**参数化**方式模拟两种架构:
//! 1. 定义 `ArchSpec` 枚举表示两种架构
//! 2. 在测试中分别用 x86_64 和 aarch64 规格运行同一场景
//! 3. 断言结果等价
//!
//! ## 与内核代码的关系
//!
//! 真正的双架构验证需要 QEMU x86_64 + QEMU aarch64 交叉测试 (3.5 真实目标).
//! 这里的测试是**算法等价性**验证, 确保逻辑层不依赖架构细节.

/// 架构规格
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    /// 基础页大小 (字节)
    pub fn base_page_size(self) -> u64 {
        match self {
            Arch::X86_64 => 4096, // 4 KiB
            Arch::Aarch64 => 4096, // 4 KiB (典型配置)
        }
    }

    /// 是否支持大页 (2 MiB / 1 GiB)
    pub fn supports_huge_pages(self) -> bool {
        match self {
            Arch::X86_64 => true, // 2 MiB / 1 GiB
            Arch::Aarch64 => true, // 2 MiB / 1 GiB (Contiguous bit)
        }
    }

    /// 是否需要显式 cache flush
    pub fn needs_cache_flush(self) -> bool {
        match self {
            Arch::X86_64 => false, // 硬件一致性
            Arch::Aarch64 => true, // 需 DC CVAU / DC IVAU
        }
    }

    /// 物理地址位宽
    pub fn phys_addr_bits(self) -> u8 {
        match self {
            Arch::X86_64 => 52, // MAXPHYADDR
            Arch::Aarch64 => 48, // 典型配置 (可配 32/36/40/42/44/48)
        }
    }

    /// 虚拟地址位宽
    pub fn virt_addr_bits(self) -> u8 {
        match self {
            Arch::X86_64 => 48,
            Arch::Aarch64 => 48, // 典型配置
        }
    }

    /// 字节序 (x86_64/aarch64 都是 LE, 但仍需验证)
    pub fn is_little_endian(self) -> bool {
        true
    }
}

/// 物理地址 (架构无关)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PhysAddr(u64);

impl PhysAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// 验证物理地址在架构允许范围内
    pub fn is_valid_for(self, arch: Arch) -> bool {
        self.0 < (1u64 << arch.phys_addr_bits())
    }
}

/// 虚拟地址 (架构无关)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VirtAddr(u64);

impl VirtAddr {
    pub const fn new(addr: u64) -> Self {
        Self(addr)
    }

    /// 验证虚拟地址在 canonical 范围内
    ///
    /// canonical 形式: [0, 2^(bits-1)) ∪ [2^64 - 2^(bits-1), 2^64)
    /// 即高位为符号扩展, 用户空间 0..2^47, 内核空间 -2^47..0
    pub fn is_valid_for(self, arch: Arch) -> bool {
        let bits = arch.virt_addr_bits() as u32;
        let half = 1u64 << (bits - 1);
        // 低半: [0, half)
        // 高半: [2^64 - half, 2^64)
        self.0 < half || self.0 >= (u64::MAX - half + 1)
    }
}

/// 缓存维护 (架构参数化)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheOp {
    pub needed: bool,
    pub kind: CacheOpKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOpKind {
    None,         // x86_64: 空操作
    CleanToPoU,   // aarch64: DC CVAU
    InvalToPoU,   // aarch64: DC IVAU
}

impl CacheOp {
    /// CPU→设备: flush cache
    pub fn for_cpu_to_device(arch: Arch) -> Self {
        if arch.needs_cache_flush() {
            Self { needed: true, kind: CacheOpKind::CleanToPoU }
        } else {
            Self { needed: false, kind: CacheOpKind::None }
        }
    }

    /// 设备→CPU: invalidate cache
    pub fn for_device_to_cpu(arch: Arch) -> Self {
        if arch.needs_cache_flush() {
            Self { needed: true, kind: CacheOpKind::InvalToPoU }
        } else {
            Self { needed: false, kind: CacheOpKind::None }
        }
    }
}

/// 原子操作宽度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicWidth {
    U8,
    U16,
    U32,
    U64,
    U128, // x86_64 CMPXCHG16B / aarch64 CASP
}

impl Arch {
    /// 支持的最大原子宽度
    pub fn max_atomic_width(self) -> AtomicWidth {
        match self {
            Arch::X86_64 => AtomicWidth::U128, // CMPXCHG16B
            Arch::Aarch64 => AtomicWidth::U128, // CASP / LDAPR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_page_size_equivalent() {
        // 两个架构基础页大小一致
        assert_eq!(Arch::X86_64.base_page_size(), Arch::Aarch64.base_page_size());
    }

    #[test]
    fn both_le() {
        // 两个架构都是 LE
        assert!(Arch::X86_64.is_little_endian());
        assert!(Arch::Aarch64.is_little_endian());
    }

    #[test]
    fn phys_addr_validity() {
        // 4 GiB = 2^32, 在 48-bit aarch64 和 52-bit x86_64 都合法
        let addr_4g = PhysAddr::new(0x1_0000_0000);
        assert!(addr_4g.is_valid_for(Arch::X86_64));
        assert!(addr_4g.is_valid_for(Arch::Aarch64));

        // 2^48, 超出 48-bit 但在 52-bit 范围内
        let addr_256t = PhysAddr::new(1u64 << 48);
        assert!(addr_256t.is_valid_for(Arch::X86_64)); // 52 bits OK
        assert!(!addr_256t.is_valid_for(Arch::Aarch64)); // 48 bits fail
    }

    #[test]
    fn phys_addr_max_boundary() {
        let max_48 = (1u64 << 48) - 1;
        let max_52 = (1u64 << 52) - 1;
        assert!(PhysAddr::new(max_48).is_valid_for(Arch::Aarch64));
        assert!(!PhysAddr::new(max_48 + 1).is_valid_for(Arch::Aarch64));
        assert!(PhysAddr::new(max_52).is_valid_for(Arch::X86_64));
    }

    #[test]
    fn virt_addr_validity() {
        // 用户地址空间 (低半, 0..2^47)
        let user_addr = VirtAddr::new(0x7fff_ffff_f000); // ~128 TiB
        assert!(user_addr.is_valid_for(Arch::X86_64));
        assert!(user_addr.is_valid_for(Arch::Aarch64));

        // 非 canonical 地址 (bit 47 0 但 bit 48+ 有值)
        let non_canonical = VirtAddr::new(0x8000_0000_0000); // bit 47 = 0, bit 48 = 1
        assert!(!non_canonical.is_valid_for(Arch::X86_64));
        assert!(!non_canonical.is_valid_for(Arch::Aarch64));

        // 内核地址空间 (高半, 2^64 - 2^47..2^64)
        let kernel_addr = VirtAddr::new(0xFFFF_8880_0000_0000);
        assert!(kernel_addr.is_valid_for(Arch::X86_64));
        assert!(kernel_addr.is_valid_for(Arch::Aarch64));
    }

    #[test]
    fn cache_op_x86_noop() {
        let op = CacheOp::for_cpu_to_device(Arch::X86_64);
        assert!(!op.needed);
        assert_eq!(op.kind, CacheOpKind::None);
    }

    #[test]
    fn cache_op_aarch64_flush() {
        let op = CacheOp::for_cpu_to_device(Arch::Aarch64);
        assert!(op.needed);
        assert_eq!(op.kind, CacheOpKind::CleanToPoU);
    }

    #[test]
    fn cache_op_aarch64_invalidate() {
        let op = CacheOp::for_device_to_cpu(Arch::Aarch64);
        assert!(op.needed);
        assert_eq!(op.kind, CacheOpKind::InvalToPoU);
    }

    #[test]
    fn cache_op_consistent() {
        // ToDevice 与 FromDevice 在 x86_64 都是空操作
        assert_eq!(
            CacheOp::for_cpu_to_device(Arch::X86_64),
            CacheOp::for_device_to_cpu(Arch::X86_64)
        );
        // aarch64 区分方向
        assert_ne!(
            CacheOp::for_cpu_to_device(Arch::Aarch64),
            CacheOp::for_device_to_cpu(Arch::Aarch64)
        );
    }

    #[test]
    fn atomic_width_128_supported() {
        // 双架构都支持 u128 原子
        assert_eq!(Arch::X86_64.max_atomic_width(), AtomicWidth::U128);
        assert_eq!(Arch::Aarch64.max_atomic_width(), AtomicWidth::U128);
    }

    #[test]
    fn huge_page_support() {
        // 双架构都支持大页
        assert!(Arch::X86_64.supports_huge_pages());
        assert!(Arch::Aarch64.supports_huge_pages());
    }

    /// 验证: 给定一个算法操作, 在两个架构上**结果必须等价**
    /// 这里用地址转换作为示例
    #[test]
    fn virt_to_phys_equivalent() {
        // 假设两个架构都用 identity map (内核地址空间)
        // 这是 QueenX 启动时的常见配置
        // 内核高地址: 0xFFFF_8880_0000_0000 范围 (48-bit canonical, x86/aarch64 一致)
        // 物理地址 (identity): 仅取低 48 位
        for arch in [Arch::X86_64, Arch::Aarch64] {
            for vaddr in [
                0xFFFF_8880_0000_0000u64,
                0xFFFF_8880_0010_0000,
                0xFFFF_8880_1000_0000,
            ] {
                // identity map: vaddr 截取低 48 位作为 paddr
                let paddr = vaddr & 0x0000_FFFF_FFFF_FFFF;
                let va = VirtAddr::new(vaddr);
                let pa = PhysAddr::new(paddr);
                assert!(va.is_valid_for(arch), "vaddr 0x{:x} invalid for {:?}", vaddr, arch);
                assert!(pa.is_valid_for(arch), "paddr 0x{:x} invalid for {:?}", paddr, arch);
            }
        }
    }

    /// 验证: 跨架构的 page 分配算法一致
    #[test]
    fn page_alloc_equivalent() {
        for arch in [Arch::X86_64, Arch::Aarch64] {
            let page_size = arch.base_page_size();
            // 4K page, 100 个连续页
            let total_size = page_size * 100;
            assert_eq!(total_size, 4096 * 100);
            // 100 页起始于 1 MiB
            let base = 0x10_0000u64;
            let end = base + total_size;
            assert!(PhysAddr::new(base).is_valid_for(arch));
            assert!(PhysAddr::new(end).is_valid_for(arch));
        }
    }
}
