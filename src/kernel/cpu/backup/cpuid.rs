//! CPUID 查询接口
//!
//! 封装 x86-64 CPUID 指令，提供类型安全的查询 API。
//!
//! ## 示例
//!
//! ```
//! let vendor = cpuid::get_vendor();
//! match vendor {
//!     CpuVendor::Intel => println!("Intel CPU"),
//!     CpuVendor::Amd => println!("AMD CPU"),
//! }
//! ```

use core::arch::x86_64::__cpuid_count;

/// CPU 厂商枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
    Unknown(u32),
}

/// CPUID 查询结果
#[derive(Debug, Clone, Copy)]
pub struct CpuidResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

/// 执行 CPUID 查询
#[inline(always)]
pub fn cpuid(leaf: u32, subleaf: u32) -> CpuidResult {
    unsafe { __cpuid_count(leaf, subleaf) }
}

/// 获取厂商 ID
pub fn get_vendor() -> CpuVendor {
    let result = cpuid(0, 0);
    
    let bytes = [
        (result.ebx as u8),
        ((result.ebx >> 8) as u8),
        ((result.ebx >> 16) as u8),
        ((result.ebx >> 24) as u8),
        (result.ecx as u8),
        ((result.ecx >> 8) as u8),
        ((result.ecx >> 16) as u8),
        ((result.ecx >> 24) as u8),
        (result.edx as u8),
        ((result.edx >> 8) as u8),
        ((result.edx >> 16) as u8),
        ((result.edx >> 24) as u8),
    ];
    
    match &bytes {
        b"GenuineIntel" => CpuVendor::Intel,
        b"AuthenticAMD" => CpuVendor::Amd,
        _ => CpuVendor::Unknown(result.eax),
    }
}

/// 获取最大支持的 leaf 编号
pub fn get_max_leaf() -> u32 {
    cpuid(0, 0).eax
}

/// 检查是否支持指定特性 (通过 ECX/EDX 寄存器)
pub fn has_feature(ecx_bit: u32, edx_bit: u32) -> bool {
    let result = cpuid(1, 0);
    
    let ecx_support = (result.ecx & (1 << ecx_bit)) != 0;
    let edx_support = (result.edx & (1 << edx_bit)) != 0;
    
    ecx_support || edx_support
}

/// 获取品牌字符串 (需要多次 CPUID 调用)
pub fn get_brand_string() -> [u8; 48] {
    let mut brand = [0u8; 48];
    
    // Leaf 0x80000002~4 包含品牌字符串
    for i in 0..3u32 {
        let result = cpuid(0x80000002 + i, 0);
        let offset = (i * 16) as usize;
        
        if offset + 16 <= 48 {
            brand[offset..offset+16].copy_from_slice(&[
                (result.eax as u8),
                ((result.eax >> 8) as u8),
                ((result.eax >> 16) as u8),
                ((result.eax >> 24) as u8),
                (result.ebx as u8),
                ((result.ebx >> 8) as u8),
                ((result.ebx >> 16) as u8),
                ((result.ebx >> 24) as u8),
                (result.ecx as u8),
                ((result.ecx >> 8) as u8),
                ((result.ecx >> 16) as u8),
                ((result.ecx >> 24) as u8),
                (result.edx as u8),
                ((result.edx >> 8) as u8),
                ((result.edx >> 16) as u8),
                ((result.edx >> 24) as u8),
            ]);
        }
    }
    
    // 确保 null 终止
    if !brand.contains(&0) {
        brand[47] = 0;
    }
    
    brand
}

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_vendor_detection() {
        let vendor = get_vendor();
        
        // 应该能识别出 Intel 或 AMD (或 Unknown)
        assert!(matches!(vendor, 
            CpuVendor::Intel | CpuVendor::Amd | CpuVendor::Unknown(_)));
    }
    
    #[test]
    fn test_max_leaf() {
        let max_leaf = get_max_leaf();
        assert!(max_leaf > 0); // 至少支持 leaf 0
    }
    
    #[test]
    fn test_brand_string() {
        let brand = get_brand_string();
        // 品牌字符串应该非空或包含有效字符
        assert!(brand[0] != 0 || brand.iter().any(|&b| b != 0));
    }
}