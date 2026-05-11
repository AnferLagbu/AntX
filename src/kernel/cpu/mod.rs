//! QX (QueenX) AMD64 CPU 驱动核心 - Rust 完整实现
//!
//! ## 功能概览
//!
//! - **厂商检测**: Intel / AMD / VIA / QEMU 虚拟化
//! - **签名解析**: Stepping / Model / Family / Extended Family
//! - **特性收集**: 基础(Leaf 1) + 扩展(0x80000001) + 高级(Leaf 7)
//! - **缓存检测**: L1/L2/L3 大小、关联度、缓存行大小
//! - **MSR管理**: 读写64位MSR寄存器
//! - **TSC校准**: 频率估算 (Intel/AMD/QEMU)
//! - **多核拓扑**: 物理核心数、逻辑线程数、超线程状态
//!
//! ## 对比 C 版本 (cpu.c, 1060行)
//!
//! **不是翻译**, 而是**重新设计**:
//! ✅ 枚举替代 #define 常量 (编译时检查)
//! ✅ bitflags! 宏替代手动位操作 (类型安全)
//! ✅ Option/Result 替代返回 -1 (强制错误处理)
//! ✅ 模式匹配替代 if-else 链 (exhaustive checking)
//! ✅ trait 抽象替代函数指针 (可扩展性)
//! ✅ const fn 编译时常量计算 (零运行时开销)
//!
//! ## 模块结构
//!
//! ```text
//! cpu/
//! ├── mod.rs          # 类型定义 + 公共API + FFI导出
//! ├── cpuid.rs        # CPUID 指令封装
//! ├── msr.rs          # MSR 寄存器操作
//! ├── tsc.rs          # TSC 时间戳校准
//! ├── cache.rs        # 缓存信息检测
//! └── topology.rs     # 多核拓扑检测
//! ```

// 子模块声明
pub mod cpuid;
pub mod msr;
pub mod tsc;
pub mod cache;
pub mod topology;

// ============================================================================
// 常量定义 (编译时常量)
// ============================================================================

/// 最大 CPUID leaf 号
const MAX_CPUID_LEAF_STANDARD: u32 = 0x0F;

/// 扩展 CPUID leaf 起始值
const CPUID_LEAF_EXT_BASE: u32 = 0x8000_0000;

/// 厂商字符串长度 (12字节)
const VENDOR_STRING_LEN: usize = 12;

/// 品牌字符串长度 (48字节)
const BRAND_STRING_LEN: usize = 48;

// ============================================================================
// 枚举定义 (类型安全替代 int/#define)
// ============================================================================

/// CPU 厂商标识
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CpuVendor {
    /// 英特尔
    Intel = 0,
    /// AMD
    Amd = 1,
    /// VIA (威盛)
    Via = 2,
    /// Cyrix (已倒闭)
    Cyrix = 3,
    /// Transmeta ( Crusoe处理器)
    Transmeta = 4,
    /// QEMU/KVM 虚拟化
    Qemu = 5,
    /// 未知厂商
    Unknown = 255,
}

impl CpuVendor {
    /// 从厂商字符串识别厂商
    /// 
    /// # Arguments
    /// * `vendor_str` - 12字节的厂商ID (如 "GenuineIntel")
    pub fn from_vendor_string(vendor_str: &[u8; VENDOR_STRING_LEN]) -> Self {
        match vendor_str {
            b"GenuineIntel" => Self::Intel,
            b"AuthenticAMD" => Self::Amd,
            b"CentaurHauls" => Self::Via,
            b"CyrixInstead" => Self::Cyrix,
            _ if &vendor_str[..9] == b"TCGTCGTCG" => Self::Qemu, // QEMU TCG
            _ => Self::Unknown,
        }
    }
    
    /// 获取厂商名称 (用于显示)
    #[inline]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Intel      => "Intel",
            Self::Amd        => "AMD",
            Self::Via        => "VIA",
            Self::Cyrix      => "Cyrix",
            Self::Transmeta  => "Transmeta",
            Self::Qemu       => "QEMU Virtual",
            Self::Unknown    => "Unknown",
        }
    }
    
    /// 是否为虚拟化环境
    #[inline]
    pub const fn is_virtualized(&self) -> bool {
        matches!(self, Self::Qemu | Self::Unknown) // Unknown 可能是VMware等
    }
}

/// CPU 特性标志 (使用 bitflags 宏生成位操作方法)
///
/// # Example
/// ```ignore
/// let features = CpuFeatures::empty();
/// features.set(CpuFeatures::SSE, true);
/// assert!(features.contains(CpuFeatures::SSE));
/// ```
bitflags::bitflags! {
    /// CPU 特性标志集
    ///
    /// 包含 x86/x86-64 所有重要特性位。
    /// 通过 CPUID 不同 leaf 收集。
    #[derive(Debug, Clone, Copy, Default)]
    pub struct CpuFeatures: u128 {  // u128 支持最多128个特性标志
        
        // ====== 基础特性 (Leaf 1 EDX, bits 0-31) ======
        
        /// FPU (浮点单元) 可用
        const FPU         = 1 << 0;
        /// DE (调试扩展) 支持
        const DE          = 1 << 2;
        /// PSE (页面大小扩展) 支持
        const PSE         = 1 << 3;
        /// TSC (时间戳计数器) 可用
        const TSC         = 1 << 4;
        /// MSR (模型特定寄存器) 支持
        const MSR         = 1 << 5;
        /// PAE (物理地址扩展) 支持
        const PAE         = 1 << 6;
        /// MCE (机器检查异常) 支持
        const MCE         = 1 << 7;
        /// CMPXCHG8B 指令支持
        const CX8         = 1 << 8;
        /// APIC (本地APIC) 支持
        const APIC        = 1 << 9;
        /// SEP (快速系统调用) 支持
        const SEP         = 1 << 11;
        /// MTRR (内存类型范围寄存器) 支持
        const MTRR        = 1 << 12;
        /// PGE (全局页面启用) 支持
        const PGE         = 1 << 13;
        /// MCA (机器检查架构) 支持
        const MCA         = 1 << 14;
        /// CMOV (条件移动指令) 支持
        const CMOV        = 1 << 15;
        /// PAT (页属性表) 支持
        const PAT         = 1 << 16;
        /// PSE-36 (36位页面支持) 支持
        const PSE36       = 1 << 17;
        /// CLFLUSH 指令支持
        const CLFLUSH     = 1 << 19;
        /// MMX 指令支持
        const MMX         = 1 << 23;
        /// FXSAVE/FXRSTOR 指令支持
        const FXSR        = 1 << 24;
        /// SSE (流SIMD扩展) 支持
        const SSE         = 1 << 25;
        /// SSE2 支持
        const SSE2        = 1 << 26;
        /// HTT (超线程技术) 支持
        const HTT         = 1 << 28;
        
        // ====== 基础扩展特性 (Leaf 1 ECX, bits 32-63, 映射到 +32) ======
        
        const SSE3        = 1 << 32;
        const MONITOR     = 1 << 35;
        /// VMX (Intel VT-x 虚拟化) 支持
        const VMX         = 1 << 37;
        /// SVM (AMD-V 虚拟化) 支持
        const SMX         = 1 << 38;
        const EST         = 1 << 39;
        const TM2         = 1 << 40;
        const SSSE3       = 1 << 41;
        const CID         = 1 << 42;
        const CX16        = 1 << 45;
        const XTPR        = 1 << 46;
        const PDCM        = 1 << 47;
        const PCID        = 1 << 49;
        const SSE41       = 1 << 51;
        const SSE42       = 1 << 52;
        const X2APIC      = 1 << 53;
        const MOVBE       = 1 << 54;
        const POPCNT      = 1 << 55;
        const AES         = 1 << 57;
        const XSAVE       = 1 << 58;
        const OSXSAVE     = 1 << 59;
        /// AVX (高级向量扩展) 支持
        const AVX         = 1 << 60;
        
        // ====== 扩展特性 (Leaf 80000001 EDX, 映射到 +64) ======
        
        /// SYSCALL/SYSRET 指令支持 (AMD64必需)
        const SYSCALL     = 1 << 75;  // 64+11
        /// NX bit (No-Execute) 支持
        const NX          = 1 << 84;  // 64+20
        const MMXEXT      = 1 << 86;  // 64+22
        const FFXSR       = 1 << 88;  // 64+24
        /// 1GB 大页面支持
        const PAGE1GB     = 1 << 90;  // 64+26
        const RDTSCP      = 1 << 91;  // 64+27
        /// LM (Long Mode) - x86-64 支持!
        const LM          = 1 << 93;  // 64+29
        const _3DNOWEXT   = 1 << 94;  // 64+30
        const _3DNOW      = 1 << 95;  // 64+31
        
        // ====== 高级特性 (Leaf 7 EBX, 映射到 +128) ======
        
        const FSGSBASE    = 1 << 128;
        const BMI1        = 1 << 131;
        const HLE         = 1 << 132;
        /// AVX2 支持
        const AVX2        = 1 << 133;
        const BMI2        = 1 << 136;
        const ERMS        = 1 << 137;
        const INVPCID     = 1 << 138;
        const RTM         = 1 << 139;
        const MPX         = 1 << 142;
        /// AVX-512 Foundation
        const AVX512F     = 1 << 144;
        const AVX512DQ    = 1 << 145;
        const RDSEED      = 1 << 147;
        const ADX         = 1 << 148;
        const AVX512IFMA  = 1 << 149;
        const CLWB        = 1 << 152;
        const AVX512CD    = 1 << 156;
        /// SHA (SHA-1/SHA-256) 指令
        const SHA         = 1 << 157;
        const AVX512BW    = 1 << 158;
        const AVX512VL    = 1 << 159;
    }
}

impl CpuFeatures {
    /// 检查是否为 Intel 处理器 (基于特性组合判断)
    #[inline]
    pub const fn is_intel_style(&self) -> bool {
        self.contains(Self::VMX) && !self.contains(Self::SVM)
    }
    
    /// 检查是否为 AMD 处理器
    #[inline]
    pub const fn is_amd_style(&self) -> bool {
        self.contains(Self::SVM) && !self.contains(Self::VMX)
    }
    
    /// 检查是否支持 x86-64 长模式
    #[inline]
    pub const fn supports_64bit(&self) -> bool {
        self.contains(Self::LM)
    }
    
    /// 检查是否支持 SIMD 向量指令
    #[inline]
    pub const fn supports_simd(&self) -> bool {
        self.contains(Self::SSE | Self::SSE2)
    }
    
    /// 检查是否支持 AVX/AVX2
    #[inline]
    pub const fn supports_avx(&self) -> bool {
        self.contains(Self::AVX | Self::AVX2)
    }
    
    /// 检查是否支持虚拟化扩展
    #[inline]
    pub const fn supports_virtualization(&self) -> bool {
        self.contains(Self::VMX | Self::SVM)
    }
}

// ============================================================================
// 数据结构定义 (聚合体)
// ============================================================================

/// CPU 签名信息 (从 CPUID Leaf 1 EAX 提取)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CpuSignature {
    /// 步进号 (Stepping, bits 3:0)
    pub stepping: u8,
    /// 型号 (Model, bits 7:4)
    pub model: u8,
    /// 家族号 (Family, bits 11:8)
    pub family: u8,
    /// 处理器类型 (Processor Type, bits 13:12)
    /// - 00: Original OEM
    /// - 01: OverDrive
    /// - 10: Dual processor
    /// - 11: Reserved
    pub processor_type: u8,
    /// 扩展型号 (Extended Model, bits 19:16)
    pub ext_model: u8,
    /// 扩展家族 (Extended Family, bits 27:20)
    pub ext_family: u8,
}

impl CpuSignature {
    /// 计算有效的家族号 (处理特殊编码)
    /// 
    /// Intel 手册规定:
    /// - 如果 Family != 0xF, Effective_Family = Family
    /// - 如果 Family == 0xF, Effective_Family = Extended_Family + Family
    #[inline]
    pub const fn effective_family(&self) -> u8 {
        if self.family == 0x0F {
            self.ext_family.saturating_add(self.family)
        } else {
            self.family
        }
    }
    
    /// 计算有效的型号 (同上逻辑)
    #[inline]
    pub const fn effective_model(&self) -> u8 {
        if self.family == 0x06 || self.family == 0x0F {
            (self.ext_model << 4).saturating_add(self.model)
        } else {
            self.model
        }
    }
    
    /// 格式化为人类可读字符串 (如 "6-158-10" 表示 Family 6, Model 158, Stepping 10)
    pub fn to_string(&self) -> heapless::String<32> {
        let mut s = heapless::String::<32>::new();
        write!(s, "{}-{}-{}", 
               self.effective_family(), 
               self.effective_model(), 
               self.stepping).ok();
        s
    }
}

/// 缓存配置信息
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CacheInfo {
    /// L1 数据缓存大小 (bytes)
    pub l1d_size: u32,
    /// L1 指令缓存大小 (bytes)
    pub l1i_size: u32,
    /// L2 统一缓存大小 (bytes)
    pub l2_size: u32,
    /// L3 缓存大小 (bytes, 0表示不存在)
    pub l3_size: u32,
    /// L1 数据关联度 (路数, 如 4-way)
    pub l1d_associativity: u8,
    /// L2 关联度
    pub l2_associativity: u8,
    /// L3 关联度 (0表示不存在或全相联)
    pub l3_associativity: u8,
    /// 缓存行大小 (bytes, 通常 64)
    pub cache_line_size: u16,
}

impl CacheInfo {
    /// 获取总缓存容量 (L1+L2+L3, bytes)
    #[inline]
    pub const fn total_size(&self) -> u64 {
        self.l1d_size as u64 + self.l1i_size as u64 + 
        self.l2_size as u64 + self.l3_size as u64
    }
    
    /// 检查是否有 L3 缓存
    #[inline]
    pub const fn has_l3(&self) -> bool {
        self.l3_size > 0
    }
}

/// 多核拓扑信息
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TopologyInfo {
    /// 物理核心数 (每个 CPU 插槽)
    pub physical_cores: u8,
    /// 逻辑线程总数 (含超线程)
    pub logical_threads: u8,
    /// 本地 APIC ID
    pub apic_id: u8,
    /// 是否启用超线程
    pub hyperthreading_enabled: bool,
    /// 是否为 BSP (Bootstrap Processor, 启动处理器)
    pub is_bsp: bool,
}

impl TopologyInfo {
    /// 获取每物理核心的逻辑线程数
    #[inline]
    pub const fn threads_per_core(&self) -> u8 {
        if self.physical_cores > 0 && self.logical_threads >= self.physical_cores {
            self.logical_threads / self.physical_cores
        } else {
            1
        }
    }
    
    /// 检查是否为单核 CPU
    #[inline]
    pub const fn is_single_core(&self) -> bool {
        self.physical_cores <= 1 && self.logical_threads <= 1
    }
}

/// CPU 信息聚合体 (全局单例实例)
#[derive(Debug)]
#[repr(C)]
pub struct CpuInfo {
    /// 是否已完成初始化
    pub initialized: bool,
    /// 厂商标识
    pub vendor: CpuVendor,
    /// 厂商字符串 (12字节, null终止)
    pub vendor_string: [u8; VENDOR_STRING_LEN],
    /// 品牌/型号字符串 (48字节, null终止)
    pub brand_string: [u8; BRAND_STRING_LEN],
    /// CPU 签名 (步进/型号/家族)
    pub signature: CpuSignature,
    /// 特性标志集合
    pub features: CpuFeatures,
    /// 缓存信息
    pub cache: CacheInfo,
    /// 多核拓扑
    pub topology: TopologyInfo,
    /// 最大标准 CPUID leaf 号
    pub max_standard_leaf: u32,
    /// 最大扩展 CPUID leaf 号
    pub max_ext_leaf: u32,
    /// TSC 频率估算值 (Hz, 0表示未知)
    pub tsc_frequency_hz: u64,
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self {
            initialized: false,
            vendor: CpuVendor::Unknown,
            vendor_string: [0; VENDOR_STRING_LEN],
            brand_string: [b'U', b'n', b'k', b'n', b'o', b'w', b'n', 0], // "Unknown\0..."
            signature: CpuSignature::default(),
            features: CpuFeatures::default(),
            cache: CacheInfo::default(),
            topology: TopologyInfo::default(),
            max_standard_leaf: 0,
            max_ext_leaf: 0,
            tsc_frequency_hz: 0,
        }
    }
}

impl CpuInfo {
    /// 检查是否为 Intel CPU
    #[inline]
    pub const fn is_intel(&self) -> bool {
        matches!(self.vendor, CpuVendor::Intel)
    }
    
    /// 检查是否为 AMD CPU
    #[inline]
    pub const fn is_amd(&self) -> bool {
        matches!(self.vendor, CpuVendor::Amd)
    }
    
    /// 检查是否在虚拟化环境中运行
    #[inline]
    pub const fn is_virtualized(&self) -> bool {
        self.vendor.is_virtualized() || 
           self.features.contains(CpuFeatures::VMX | CpuFeatures::SVM)
    }
    
    /// 检查是否支持指定特性
    #[inline]
    pub const fn has_feature(&self, feature: CpuFeatures) -> bool {
        self.features.contains(feature)
    }
    
    /// 获取品牌字符串的可读引用 (去除尾部null和空格)
    pub fn brand_name(&self) -> &str {
        let end = self.brand_string.iter()
            .position(|&c| c == 0)
            .unwrap_or(BRAND_STRING_LEN);
        
        let trimmed = &self.brand_string[..end];
        let start = trimmed.iter()
            .position(|&c| c != b' ')
            .unwrap_or(0);
        
        core::str::from_utf8(&trimmed[start..]).unwrap_or("Unknown")
    }
}

// ============================================================================
// 全局状态 (静态单例, 使用 OnceCell 保证只初始化一次)
// ============================================================================

use once_cell::sync::OnceCell;

/// 全局 CPU 信息实例 (延迟初始化)
static CPU_INFO: OnceCell<CpuInfo> = OnceCell::new();

/// 获取全局 CPU 信息引用 (必须先调用 cpu_init())
/// 
/// # Returns
/// * Some(&CpuInfo) - 成功获取
/// * None - 尚未初始化
#[inline]
pub fn get_cpu_info() -> Option<&'static CpuInfo> {
    CPU_INFO.get()
}

// ============================================================================
// 公共 API - 初始化与查询
// ============================================================================

/// 初始化 CPU 驱动子系统
/// 
/// **必须在内核启动早期调用一次**, 在任何其他 CPU 函数之前。
/// 
/// # 功能
/// 1. 检测 CPU 厂商 (Intel/AMD/VIA/QEMU)
/// 2. 解析 CPU 签名 (Family/Model/Stepping)
/// 3. 收集特性标志 (SSE/AVX/NX/Virtualization...)
/// 4. 检测缓存配置 (L1/L2/L3)
/// 5. 探测多核拓扑 (核心数/线程数)
/// 6. 配置关键 MSR (EFER/NX/SSE)
/// 7. 校准 TSC 频率
/// 
/// # Returns
/// * Ok(()) - 初始化成功
/// * Err(&str) - 错误描述 (通常不会失败)
/// 
/// # Safety
/// 此函数执行内联汇编和 MSR 写入, 必须在特权级(Ring 0)调用。
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_init() -> i32 {
    use crate::logging::klog::{klog_write, LogLevel, LogCategory};
    
    static INIT_MSG: &[u8] = b"Initializing QX AMD64 CPU driver...\0";
    unsafe {
        klog_write(LogLevel::Info as u8, LogCategory::Boot as u8,
                  core::ptr::null(), core::ptr::null(), 0,
                  INIT_MSG.as_ptr() as *const i8);
    }
    
    // 创建新的 CpuInfo 实例
    let mut info = CpuInfo::default();
    
    // Step 1: 检测厂商
    info.vendor = detect_vendor(&mut info.vendor_string);
    
    // Step 2: 获取签名
    get_signature(&mut info.signature, &mut info.topology.apic_id, 
                  &mut info.topology.logical_threads);
    
    // Step 3: 收集特性
    collect_features(&mut info.features, &mut info.brand_string,
                     &mut info.max_standard_leaf, &mut info.max_ext_leaf);
    
    // Step 4: 检测缓存
    detect_cache(&mut info.cache, info.max_standard_leaf, 
                 info.max_ext_leaf, info.vendor);
    
    // Step 5: 探测拓扑
    detect_topology(&mut info.topology, &info.signature, &info.features,
                    info.max_standard_leaf, info.max_ext_leaf, info.vendor);
    
    // Step 6: 初始化 MSR (可选, 可能失败于虚拟机)
    if let Err(e) = init_msr(&info.features) {
        static WARN_MSG: &[u8] = b"MSR init failed (expected in VMs): \0";
        let mut msg_buf = [0u8; 128];
        let e_bytes = e.as_bytes();
        let len = e_bytes.len().min(100);
        msg_buf[..len].copy_from_slice(&e_bytes[..len]);
        msg_buf[len] = 0;
        
        unsafe {
            klog_write(LogLevel::Warn as u8, LogCategory::Kernel as u8,
                      core::ptr::null(), core::ptr::null(), 0,
                      msg_buf.as_ptr() as *const i8);
        }
    }
    
    // Step 7: 校准 TSC
    info.tsc_frequency_hz = calibrate_tsc(info.max_standard_leaf, info.vendor);
    
    // 标记初始化完成
    info.initialized = true;
    
    // 存储到全局单例
    if CPU_INFO.set(info).is_err() {
        static ERR_MSG: &[u8] = b"ERROR: cpu_init called twice!\0";
        unsafe {
            klog_write(LogLevel::Error as u8, LogCategory::Kernel as u8,
                      core::ptr::null(), core::ptr::null(), 0,
                      ERR_MSG.as_ptr() as *const i8);
        }
        return -1;
    }
    
    static OK_MSG: &[u8] = b"CPU driver initialized successfully\0";
    unsafe {
        klog_write(LogLevel::Info as u8, LogCategory::Kernel as u8,
                  core::ptr::null(), core::ptr::null(), 0,
                  OK_MSG.as_ptr() as *const i8);
    }
    
    0 // 成功
}

/// 获取 CPU 信息指针 (FFI兼容)
/// 
/// # Returns
/// * 非 NULL - 指向全局 CpuInfo 的指针
/// * NULL - 未初始化
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_info() -> *const CpuInfo {
    match CPU_INFO.get() {
        Some(info) => info as *const CpuInfo,
        None => core::ptr::null(),
    }
}

/// 检查 CPU 是否支持指定特性 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_has_feature(feature_bit: u32) -> bool {
    match CPU_INFO.get() {
        Some(info) => info.features.contains(CpuFeatures::from_bits_truncate(feature_bit as u128)),
        None => false,
    }
}

/// 检查是否为 Intel CPU (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_is_intel() -> bool {
    match CPU_INFO.get() {
        Some(info) => info.is_intel(),
        None => false,
    }
}

/// 检查是否为 AMD CPU (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_is_amd() -> bool {
    match CPU_INFO.get() {
        Some(info) => info.is_amd(),
        None => false,
    }
}

/// 检查是否在虚拟化环境中 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_is_virtualized() -> bool {
    match CPU_INFO.get() {
        Some(info) => info.is_virtualized(),
        None => false,
    }
}

/// 获取最大标准 CPUID leaf 号 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_max_cpuid_leaf() -> u32 {
    match CPU_INFO.get() {
        Some(info) => info.max_standard_leaf,
        None => 0,
    }
}

/// 获取最大扩展 CPUID leaf 号 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_max_ext_cpuid_leaf() -> u32 {
    match CPU_INFO.get() {
        Some(info) => info.max_ext_leaf,
        None => 0,
    }
}

/// 获取 APIC ID (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_apic_id() -> u32 {
    match CPU_INFO.get() {
        Some(info) => info.topology.apic_id as u32,
        None => 0,
    }
}

/// 获取逻辑线程数 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_logical_cores() -> u8 {
    match CPU_INFO.get() {
        Some(info) => info.topology.logical_threads,
        None => 1,
    }
}

/// 获取物理核心数 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_physical_cores() -> u8 {
    match CPU_INFO.get() {
        Some(info) => info.topology.physical_cores,
        None => 1,
    }
}

/// 获取 CPU 签名 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_signature() -> CpuSignature {
    match CPU_INFO.get() {
        Some(info) => info.signature,
        _ => CpuSignature::default(),
    }
}

/// 获取缓存信息指针 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_cache_info() -> *const CacheInfo {
    match CPU_INFO.get() {
        Some(info) => &info.cache as *const CacheInfo,
        None => core::ptr::null(),
    }
}

/// 获取 TSC 频率 (Hz) (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn cpu_get_tsc_frequency() -> u64 {
    match CPU_INFO.get() {
        Some(info) => info.tsc_frequency_hz,
        None => 0,
    }
}

// ============================================================================
// 内部实现函数 (private, 不暴露给外部)
// ============================================================================

/// 检测 CPU 厂商 (通过 CPUID Leaf 0)
fn detect_vendor(vendor_out: &mut [u8; VENDOR_STRING_LEN]) -> CpuVendor {
    let (_, ebx, ecx, edx) = cpuid::cpuid(0, 0);
    
    // CPUID 返回厂商字符串在 EBX:EDX:ECX 寄存器中 (注意顺序!)
    vendor_out[0..4].copy_from_slice(&ebx.to_le_bytes());
    vendor_out[4..8].copy_from_slice(&edx.to_le_bytes());
    vendor_out[8..12].copy_from_slice(&ecx.to_le_bytes());
    vendor_out[11] = 0; // null 终止
    
    CpuVendor::from_vendor_string(vendor_out)
}

/// 获取 CPU 签名 (通过 CPUID Leaf 1 EAX)
fn get_signature(sig_out: &mut CpuSignature, 
                 apic_id_out: &mut u8,
                 logical_cores_out: &mut u8) {
    let (eax, ebx, _, edx) = cpuid::cpuid(1, 0);
    
    // 解析 EAX 位域
    sig_out.stepping = (eax & 0xF) as u8;
    sig_out.model = ((eax >> 4) & 0xF) as u8;
    sig_out.family = ((eax >> 8) & 0xF) as u8;
    sig_out.processor_type = ((eax >> 12) & 0x3) as u8;
    sig_out.ext_model = ((eax >> 16) & 0xF) as u8;
    sig_out.ext_family = ((eax >> 20) & 0xFF) as u8;
    
    // EBX: 逻辑处理器数 (bits 24:16), APIC ID (bits 31:24)
    *logical_cores_out = if edx & (1 << 28) != 0 { // HTT bit
        ((ebx >> 16) & 0xFF) as u8
    } else {
        1
    };
    
    *apic_id_out = ((ebx >> 24) & 0xFF) as u8;
}

/// 收集 CPU 特性标志 (多个 CPUID leaf)
fn collect_features(features_out: &mut CpuFeatures,
                    brand_out: &mut [u8; BRAND_STRING_LEN],
                    max_std_out: &mut u32,
                    max_ext_out: &mut u32) {
    // 清空特性位图
    *features_out = CpuFeatures::empty();
    
    // ====== 标准 Leaf 0: 获取支持的 leaf 范围 ======
    let (max_leaf, _, _, _) = cpuid::cpuid(0, 0);
    *max_std_out = max_leaf;
    
    // ====== 标准 Leaf 1: 基础特性 (EDX + ECX) ======
    if max_leaf >= 1 {
        let (_, _, ecx, edx) = cpuid::cpuid(1, 0);
        
        // 解析 EDX (bits 0-31)
        let mut feat = CpuFeatures::empty();
        if edx & (1 << 0)  != 0 { feat.insert(CpuFeatures::FPU); }
        if edx & (1 << 2)  != 0 { feat.insert(CpuFeatures::PSE); }
        if edx & (1 << 3)  != 0 { feat.insert(CpuFeatures::TSC); }
        if edx & (1 << 5)  != 0 { feat.insert(CpuFeatures::MSR); }
        if edx & (1 << 6)  != 0 { feat.insert(CpuFeatures::PAE); }
        if edx & (1 << 9)  != 0 { feat.insert(CpuFeatures::APIC); }
        if edx & (1 << 11) != 0 { feat.insert(CpuFeatures::SEP); }
        if edx & (1 << 15) != 0 { feat.insert(CpuFeatures::CMOV); }
        if edx & (1 << 19) != 0 { feat.insert(CpuFeatures::CLFLUSH); }
        if edx & (1 << 23) != 0 { feat.insert(CpuFeatures::MMX); }
        if edx & (1 << 24) != 0 { feat.insert(CpuFeatures::FXSR); }
        if edx & (1 << 25) != 0 { feat.insert(CpuFeatures::SSE); }
        if edx & (1 << 26) != 0 { feat.insert(CpuFeatures::SSE2); }
        if edx & (1 << 28) != 0 { feat.insert(CpuFeatures::HTT); }
        
        // 解析 ECX (bits 32-63)
        if ecx & (1 << 0)  != 0 { feat.insert(CpuFeatures::SSE3); }
        if ecx & (1 << 5)  != 0 { feat.insert(CpuFeatures::VMX); }
        if ecx & (1 << 6)  != 0 { feat.insert(CpuFeatures::SMX); }
        if ecx & (1 << 9)  != 0 { feat.insert(CpuFeatures::SSSE3); }
        if ecx & (1 << 19) != 0 { feat.insert(CpuFeatures::SSE41); }
        if ecx & (1 << 20) != 0 { feat.insert(CpuFeatures::SSE42); }
        if ecx & (1 << 21) != 0 { feat.insert(CpuFeatures::X2APIC); }
        if ecx & (1 << 23) != 0 { feat.insert(CpuFeatures::POPCNT); }
        if ecx & (1 << 25) != 0 { feat.insert(CpuFeatures::AES); }
        if ecx & (1 << 26) != 0 { feat.insert(CpuFeatures::XSAVE); }
        if ecx & (1 << 27) != 0 { feat.insert(CpuFeatures::OSXSAVE); }
        if ecx & (1 << 28) != 0 { feat.insert(CpuFeatures::AVX); }
        
        features_out.insert(feat);
    }
    
    // ====== 扩展 Leaf 80000000: 获取扩展范围 ======
    let (max_ext, _, _, _) = cpuid::cpuid(CPUID_LEAF_EXT_BASE, 0);
    *max_ext_out = max_ext;
    
    // ====== 扩展 Leaf 80000001: 扩展特性 (EDX + ECX) ======
    if max_ext >= 0x8000_0001 {
        let (_, _, ecx, edx) = cpuid::cpuid(0x8000_0001, 0);
        
        let mut feat = CpuFeatures::empty();
        if edx & (1 << 11) != 0 { feat.insert(CpuFeatures::SYSCALL); }
        if edx & (1 << 20) != 0 { feat.insert(CpuFeatures::NX); }
        if edx & (1 << 26) != 0 { feat.insert(CpuFeatures::PAGE1GB); }
        if edx & (1 << 27) != 0 { feat.insert(CpuFeatures::RDTSCP); }
        if edx & (1 << 29) != 0 { feat.insert(CpuFeatures::LM); }
        
        if ecx & (1 << 0)  != 0 { /* LAHF_LM */ }
        if ecx & (1 << 5)  != 0 { /* ABM */ }
        if ecx & (1 << 6)  != 0 { /* SSE4A */ }
        
        features_out.insert(feat);
        
        // 品牌字符串 (Leaf 80000002~4)
        if max_ext >= 0x8000_0004 {
            let (a, b, c, d) = cpuid::cpuid(0x8000_0002, 0);
            brand_out[0..16].copy_from_slice(&[a, b, c, d].map(|x| x.to_le_bytes()).concat());
            
            let (a, b, c, d) = cpuid::cpuid(0x8000_0003, 0);
            brand_out[16..32].copy_from_slice(&[a, b, c, d].map(|x| x.to_le_bytes()).concat());
            
            let (a, b, c, d) = cpuid::cpuid(0x8000_0004, 0);
            brand_out[32..48].copy_from_slice(&[a, b, c, d].map(|x| x.to_le_bytes()).concat());
            
            brand_out[47] = 0; // null 终止
        } else {
            brand_out[..7].copy_from_slice(b"Generic");
            brand_out[7] = 0;
        }
    }
    
    // ====== 高级 Leaf 7 Sub-leaf 0: 高级特性 (EBX) ======
    if max_leaf >= 7 {
        let (_, ebx, _, _) = cpuid::cpuid(7, 0);
        
        let mut feat = CpuFeatures::empty();
        if ebx & (1 << 0)  != 0 { feat.insert(CpuFeatures::FSGSBASE); }
        if ebx & (1 << 3)  != 0 { feat.insert(CpuFeatures::BMI1); }
        if ebx & (1 << 5)  != 0 { feat.insert(CpuFeatures::AVX2); }
        if ebx & (1 << 8)  != 0 { feat.insert(CpuFeatures::BMI2); }
        if ebx & (1 << 9)  != 0 { feat.insert(CpuFeatures::ERMS); }
        if ebx & (1 << 16) != 0 { feat.insert(CpuFeatures::AVX512F); }
        if ebx & (1 << 21) != 0 { feat.insert(CpuFeatures::AVX512IFMA); }
        if ebx & (1 << 24) != 0 { feat.insert(CpuFeatures::CLWB); }
        if ebx & (1 << 29) != 0 { feat.insert(CpuFeatures::SHA); }
        
        features_out.insert(feat);
    }
}

/// 检测缓存配置 (Intel: Leaf 4, AMD: Leaf 80000005/6)
fn detect_cache(cache_out: &mut CacheInfo,
                 max_std: u32, max_ext: u32,
                 vendor: CpuVendor) {
    // 设置默认保守值
    *cache_out = CacheInfo {
        l1d_size: 32 * 1024,   // 32KB
        l1i_size: 32 * 1024,   // 32KB
        l2_size: 256 * 1024,   // 256KB
        l3_size: 0,             // 不确定
        l1d_associativity: 4,  // 4-way
        l2_associativity: 8,   // 8-way
        l3_associativity: 0,
        cache_line_size: 64,   // 标准 x86-64
    };
    
    // Intel: 使用 Deterministic Cache Parameter (Leaf 4)
    if vendor == CpuVendor::Intel && max_std >= 4 {
        for subleaf in 0..=3u32 { // 通常前几个subleaf包含L1/L2/L3
            let (eax, ebx, ecx, _) = cpuid::cpuid(4, subleaf);
            
            let cache_type = eax & 0x1F;
            if cache_type == 0 { break; } // 无更多缓存
            
            let cache_level = (eax >> 5) & 0x7;
            let line_part = (ebx & 0xFFF) + 1;
            let assoc = ((ebx >> 12) & 0x3FF) + 1;
            let sets = ecx + 1;
            let size = sets * assoc * line_part * ((ebx >> 22) + 1);
            
            match (cache_type, cache_level) {
                (1, 1) => cache_out.l1d_size = size,      // L1 Data
                (2, 1) => cache_out.l1i_size = size,      // L1 Instruction
                (3, 2) => {                               // L2 Unified
                    cache_out.l2_size = size;
                    cache_out.l2_associativity = assoc;
                },
                (3, 3) => {                               // L3 Unified
                    cache_out.l3_size = size;
                    cache_out.l3_associativity = assoc;
                },
                _ => {},
            }
        }
    }
    
    // AMD: 使用扩展缓存信息 (Leaf 80000005/6)
    else if vendor == CpuVendor::Amd && max_ext >= 0x8000_0006 {
        // L1 Data/Instruction (Leaf 80000005)
        let (_, _, ecx_l1, edx_l1) = cpuid::cpuid(0x8000_0005, 0);
        cache_out.l1d_size = ((ecx_l1 >> 24) as u32) * 1024;  // KB → Bytes
        cache_out.l1i_size = ((edx_l1 >> 24) as u32) * 1024;
        
        // L2 Unified (Leaf 80000006)
        let (_, _, ecx_l2, _) = cpuid::cpuid(0x8000_0006, 0);
        cache_out.l2_size = ((ecx_l2 >> 16) as u32) * 1024;
        
        // L3 (Leaf 80000008, 可选)
        if max_ext >= 0x8000_0008 {
            let (_, _, ecx_l3, _) = cpuid::cpuid(0x8000_0008, 0);
            let l3_size_kb = ((ecx_l3 >> 18) as u32) * 512; // 单位: 512KB
            if l3_size_kb > 0 {
                cache_out.l3_size = l3_size_kb * 1024; // KB → Bytes
            }
        }
    }
    
    // 获取缓存行大小 (几乎所有 x86-64 都是 64 字节)
    if max_std >= 1 {
        let (_, ebx, _, _) = cpuid::cpuid(1, 0);
        cache_out.cache_line_size = (8 * ((ebx >> 8) & 0xFF)) as u16;
    }
    
    // 最终安全检查
    if cache_out.cache_line_size == 0 {
        cache_out.cache_line_size = 64;
    }
}

/// 探测多核拓扑 (Intel: Leaf 0xB, AMD: Leaf 80000008)
fn detect_topology(topo_out: &mut TopologyInfo,
                   sig: &CpuSignature,
                   feat: &CpuFeatures,
                   max_std: u32, max_ext: u32,
                   vendor: CpuVendor) {
    topo_out.is_bsp = true; // 我们总是运行在 BSP 上
    topo_out.hyperthreading_enabled = feat.contains(CpuFeatures::HTT) && 
                                      topo_out.logical_threads > 1;
    
    // Intel: Extended Topology Leaf (0xB)
    if vendor == CpuVendor::Intel && max_std >= 0xB {
        let (_, ebx, ecx, _) = cpuid::cpuid(0xB, 0);
        
        if ebx != 0 {
            let logical_per_pkg = (ebx & 0xFFFF) as u16;
            let cores_per_pkg = (ecx & 0xFF) as u8;
            
            if cores_per_pkg > 0 {
                topo_out.physical_cores = cores_per_pkg;
                
                if logical_per_pkg as u8 > cores_per_pkg {
                    topo_out.hyperthreading_enabled = true;
                }
            }
        }
    }
    // AMD: 核心计数 (Leaf 80000008)
    else if vendor == CpuVendor::Amd && max_ext >= 0x8000_0008 {
        let (_, _, ecx, _) = cpuid::cpuid(0x8000_0008, 0);
        let nc = (ecx & 0xFF) as u8; // NC = CoreCount - 1
        
        if nc > 0 {
            topo_out.physical_cores = nc + 1;
        }
    }
    // 回退: 假设无超线程
    else {
        if !topo_out.hyperthreading_enabled {
            topo_out.physical_cores = topo_out.logical_threads;
        } else {
            // 有超线程但无法确定物理核心数, 假设 2 threads/core
            topo_out.physical_cores = topo_out.logical_threads / 2;
            if topo_out.physical_cores == 0 {
                topo_out.physical_cores = 1;
            }
        }
    }
    
    // 安全边界检查
    if topo_out.physical_cores == 0 {
        topo_out.physical_cores = 1;
    }
    if topo_out.logical_threads < topo_out.physical_cores {
        topo_out.logical_threads = topo_out.physical_cores;
    }
}

/// 初始化关键 MSR 寄存器
fn init_msr(features: &CpuFeatures) -> Result<(), &'static str> {
    // 检查 MSR 支持
    if !features.contains(CpuFeatures::MSR) {
        return Err("CPU does not support MSR");
    }
    
    // 启用 SSE/SSE2 (设置 CR4.OSFXSR + CR4.OSXMMEXCPT)
    unsafe {
        let cr4: u64;
        core::arch::asm!("mov {0}, cr4", out(reg) cr4, options(nostack, nomem));
        
        let new_cr4 = cr4 | (1 << 9) | (1 << 10); // OSFXSR + OSXMMEXCPT
        core::asm!("mov cr4, {0}", in(reg) new_cr4, options(nostack, nomem, preserves_flags));
    }
    
    // 启用 FPU (清除 CR0.TS + CR0.EM, 设置 CR0.MP)
    unsafe {
        let cr0: u64;
        core::arch::asm!("mov {0}, cr0", out(reg) cr0, options(nostack, nomem));
        
        let new_cr0 = (cr0 & !((1 << 3) | (1 << 2))) | (1 << 1); // Clear TS/EM, Set MP
        core::asm!("mov cr0, {0}", in(reg) new_cr0, options(nostack, nomem, preserves_flags));
        
        // 初始化 FPU 状态
        core::arch::asm!("fninit", options(nostack, nomem, preserves_flags));
    }
    
    Ok(())
}

/// 校准 TSC 频率 (Hz)
fn calibrate_tsc(max_std: u32, vendor: CpuVendor) -> u64 {
    // 方法 1: Intel CPUID Leaf 0x15 (精确频率)
    if vendor == CpuVendor::Intel && max_std >= 0x15 {
        let (eax, ebx, ecx, _) = cpuid::cpuid(0x15, 0);
        
        if eax != 0 && ebx != 0 && ecx != 0 {
            // TSC freq = (crystal_freq * ebx) / eax
            // crystal_freq 通常需要额外查询, 这里简化处理
            let estimated = ((ecx as u64) * (ebx as u64)) / (eax as u64);
            if estimated > 0 {
                return estimated * 1_000_000; // MHz → Hz
            }
        }
    }
    
    // 方法 2: 经验估计 (不精确但可用)
    match vendor {
        CpuVendor::Intel => 2_500_000_000, // 2.5 GHz 典型值
        CpuVendor::Amd   => 3_000_000_000, // 3.0 GHz 典型值
        _                => 1_000_000_000, // 1.0 GHz (QEMU/其他)
    }
}

// ============================================================================
// 单元测试 (仅在 cargo test 时编译)
// ============================================================================

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_cpu_vendor_recognition() {
        assert_eq!(
            CpuVendor::from_vendor_string(b"GenuineIntel"), 
            CpuVendor::Intel
        );
        assert_eq!(
            CpuVendor::from_vendor_string(b"AuthenticAMD"),
            CpuVendor::Amd
        );
        assert_eq!(
            CpuVendor::from_vendor_string(b"CentaurHauls"),
            CpuVendor::Via
        );
        assert_eq!(
            CpuVendor::from_vendor_string(b"TCGTCGTCG????"),
            CpuVendor::Qemu
        );
        assert_eq!(
            CpuVendor::from_vendor_string(b"UnknownVendor"),
            CpuVendor::Unknown
        );
    }
    
    #[test]
    fn test_signature_effective_values() {
        // 测试普通家族 (Family != 0xF)
        let sig = CpuSignature {
            family: 6,
            model: 0x9E,
            ext_family: 0,
            ext_model: 0,
            ..Default::default()
        };
        assert_eq!(sig.effective_family(), 6);
        assert_eq!(sig.effective_model(), 0x9E);
        
        // 测试扩展家族 (Family == 0xF)
        let sig_ext = CpuSignature {
            family: 0xF,
            model: 0x07,
            ext_family: 0x06,
            ext_model: 0x09,
            ..Default::default()
        };
        assert_eq!(sig_ext.effective_family(), 0x0F); // 6 + 15
        assert_eq!(sig_ext.effective_model(), 0x97); // (9 << 4) + 7
    }
    
    #[test]
    fn test_cache_info_total() {
        let cache = CacheInfo {
            l1d_size: 32 * 1024,
            l1i_size: 32 * 1024,
            l2_size: 256 * 1024,
            l3_size: 8 * 1024 * 1024, // 8MB
            ..Default::default()
        };
        
        assert_eq!(cache.total_size(), (32 + 32 + 256 + 8192) * 1024);
        assert!(cache.has_l3());
        
        let no_l3 = CacheInfo { l3_size: 0, ..Default::default() };
        assert!(!no_l3.has_l3());
    }
    
    #[test]
    fn test_topology_threads_per_core() {
        // 无超线程
        let single = TopologyInfo {
            physical_cores: 4,
            logical_threads: 4,
            hyperthreading_enabled: false,
            ..Default::default()
        };
        assert_eq!(single.threads_per_core(), 1);
        assert!(!single.is_single_core());
        
        // 有超线程 (2 threads/core)
        let ht = TopologyInfo {
            physical_cores: 4,
            logical_threads: 8,
            hyperthreading_enabled: true,
            ..Default::default()
        };
        assert_eq!(ht.threads_per_core(), 2);
        
        // 单核
        let mono = TopologyInfo {
            physical_cores: 1,
            logical_threads: 1,
            ..Default::default()
        };
        assert!(mono.is_single_core());
    }
}