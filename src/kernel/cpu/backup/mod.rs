//! CPU 管理子系统
//!
//! ## 功能概览
//!
//! - **CPUID 查询**: 厂商、型号、特性检测
//! - **MSR 读写**: Model Specific Register 操作
//! - **TSC 时间戳**: 高精度计时器校准
//! - **缓存信息**: L1/L2/L3 缓存配置
//! - **多核拓扑**: 核心数、线程数、拓扑结构
//!
//! ## 设计理念 (功能复刻)
//!
//! 对比 C 版本 (cpu.c, 1060行), 本实现:
//! - **类型安全**: 枚举替代 #define 宏
//! - **错误处理**: Result<T> 替代返回 -1
//! - **文档化**: 每个 public 函数都有 doc comment
//! - **可测试**: 内置 #[cfg(test)] 单元测试

pub mod init;
pub mod cpuid;
pub mod msr;
pub mod tsc;
pub mod cache;
pub mod topology;

/// CPU 信息聚合体 (全局单例)
#[derive(Debug)]
pub struct CpuInfo {
    pub vendor_id: [u8; 12],        // 厂商 ID ("GenuineIntel" 或 "AuthenticAMD")
    pub brand_string: [u8; 48],     // 品牌字符串 ("Intel(R) Core(TM) i7-...")
    pub family: u8,                 // CPU 家族号
    pub model: u8,                  // 型号
    pub stepping: u8,               // 步进
    pub features: CpuFeatures,       // 特性位图
    pub cache: CacheInfo,            // 缓存信息
    pub topology: CpuTopology,       // 多核拓扑
    pub tsc_freq_mhz: u64,           // TSC 频率 (MHz)
}

/// CPU 特性标志集
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuFeatures {
    pub has_sse: bool,
    pub has_sse2: bool,
    pub has_sse3: bool,
    pub has_ssse3: bool,
    pub has_sse4_1: bool,
    pub has_sse4_2: bool,
    pub has_avx: bool,
    pub has_avx2: bool,
    pub has_fma: bool,
    pub has_xsave: bool,
    pub has_pae: bool,
    pub has_nx: bool,              // NX bit (Execute Disable)
    pub has_vmex: bool,            // VMX (Intel VT-x)
    pub has_svm: bool,             // SVM (AMD-V)
    pub has_long_mode: bool,       // x86-64 支持
}

/// 缓存信息
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheInfo {
    pub l1d_size_kb: u32,          // L1 数据缓存
    pub l1i_size_kb: u32,          // L1 指令缓存
    pub l2_size_kb: u32,           // L2 缓存
    pub l3_size_kb: u32,           // L3 缓存
    pub l1d_line_size: u32,        // L1 行大小
    pub cache_line_size: u32,      // Cache line size (通常 64B)
}

/// 多核拓扑信息
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuTopology {
    pub core_count: u32,           // 物理核心数
    pub thread_count: u32,         // 逻辑线程数
    pub apic_id: u8,               // 本地 APIC ID
    pub is_bsp: bool,              // 是否为 BSP (Bootstrap Processor)
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self {
            vendor_id: [0; 12],
            brand_string: [0; 48],
            family: 0,
            model: 0,
            stepping: 0,
            features: CpuFeatures::default(),
            cache: CacheInfo::default(),
            topology: CpuTopology::default(),
            tsc_freq_mhz: 0,
        }
    }
}
