//! I/O APIC (Advanced Programmable Interrupt Controller) - x86_64 实现
//!
//! ## 功能概览
//!
//! - **中断路由**: 将外部中断分发到指定 CPU 核心
//! - **多核支持**: 每个 CPU 可以有独立的中断向量映射
//! - **优先级管理**: 支持中断优先级和仲裁
//! - **IRQ 重映射**: 将 IRQ (0-23) 映射到向量 (32-255)
//!
//! ## 对比 C 版本 (ioapic.c, ~70行)
//!
//! **功能复刻 + 增强**:
//! ✅ 位域结构体 (替代手动位操作)
//! ✅ 枚举表示触发模式/极性
//! ✅ 安全的寄存器访问封装
//! ✅ 完整的重定向条目 (Redirection Entry) 文档

// ============================================================================
// 常量定义
// ============================================================================

/// I/O APIC 寄存器索引偏移 (I/O Register Index)
pub const IOAPIC_REGSEL: u32 = 0xFEC00000;

/// I/O APIC 寄存器数据窗口 (I/O Register Data Window)
pub const IOAPIC_REGWIN: u32 = 0xFEC00010;

/// I/O APIC ID 寄存器索引
pub const IOAPIC_ID: u8 = 0x00;

/// I/O APIC Version 寄存器索引
pub const IOAPIC_VER: u8 = 0x01;

/// I/O APIC Arbitration ID 寄存器索引
pub const IOAPIC_ARB: u8 = 0x02;

/// Redirection Table 起始索引 (每个条目占 2 个寄存器: low + high)
pub const IOAPIC_REDTAB: u8 = 0x10;

/// 最大 IRQ 数量 (标准 I/O APIC 支持 24 个 IRQ)
pub const IOAPIC_MAX_IRQ: usize = 24;

/// 默认中断向量起始值 (避开异常向量 0-31)
pub const IOAPIC_DEFAULT_VECTOR_BASE: u8 = 32;

// ============================================================================
// 枚举定义 (类型安全替代魔法数字)
// ============================================================================

/// 中断触发模式 (Trigger Mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TriggerMode {
    /// 边沿触发 (Edge-triggered)
    Edge = 0,
    /// 电平触发 (Level-triggered)
    Level = 1,
}

/// 中断极性 (Polarity)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Polarity {
    /// 高电平有效 (Active High)
    ActiveHigh = 0,
    /// 低电平有效 (Active Low)
    ActiveLow = 1,
}

/// 投递模式 (Delivery Mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryMode {
    /// 正常投递 (Fixed)
    Fixed = 0x00,
    /// 低优先级投递 (Lowest Priority)
    Lowest = 0x01,
    /// SMI (System Management Interrupt)
    SMI = 0x02,
    /// NMI (Non-Maskable Interrupt)
    NMI = 0x04,
    /// INIT (Initialization)
    Init = 0x05,
    /// ExtINT (External Interrupt, 用于 8259 PIC 模拟)
    ExtInt = 0x07,
}

/// 目标模式 (Destination Mode)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DestinationMode {
    /// 物理模式 (Physical): 目标是单个 APIC ID
    Physical = 0,
    /// 逻辑模式 (Logical): 目标是 APIC 逻辑组
    Logical = 1,
}

/// 中断屏蔽状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskState {
    /// 未屏蔽 (启用中断)
    Unmasked = 0,
    /// 已屏蔽 (禁用中断)
    Masked = 1,
}

// ============================================================================
// 数据结构定义
// ============================================================================

/// I/O APIC Version 信息 (从 VER 寄存器读取)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct IoApicVersion {
    /// 版本号 (通常为 0x20)
    pub version: u8,
    /// 最大重定向条目数减 1 (即实际数量 = max_redirections + 1)
    pub max_redirections: u8,
    /// 保留位
    pub reserved: u16,
}

/// I/O APIC 重定向表条目 (Redirection Table Entry, 64-bit)
///
/// 内存布局:
/// ```text
/// Bits   Field               Description
/// ----   ------------------- ----------------------------------------
/// 7:0    Vector              中断向量号 (32-255)
/// 8      Delivery Mode       投递模式 (00=Fixed, 01=Lowest...)
/// 10:8   Destination Mode    目标模式 (0=Physical, 1=Logical)
/// 11     Delivery Status     只读 (0=Idle, 1=Send Pending)
/// 12     Pin Polarity         极性 (0=High, 1=Low)
/// 13     Remote IRR           只读 (中断接收确认)
/// 14     Trigger Mode        触发模式 (0=Edge, 1=Level)
/// 15     Mask                 屏蔽标志 (0=Enable, 1=Disable)
/// 48:16  Reserved            必须为 0
/// 55:48  Destination Field    目标 APIC ID (Physical) 或 逻辑组
/// 63:56  Reserved            必须为 0
/// ```
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct IoApicRedirEntry {
    /// 低 32 位 (Vector + Delivery Info + Mask)
    low: u32,
    /// 高 32 位 (Destination Field)
    high: u32,
}

impl IoApicRedirEntry {
    /// 创建默认的重定向条目 (所有字段为零)
    pub const fn zeroed() -> Self {
        Self { low: 0, high: 0 }
    }
    
    /// 设置中断向量 (bits 7:0)
    #[inline]
    pub fn set_vector(&mut self, vector: u8) {
        self.low = (self.low & !0xFF) | (vector as u32);
    }
    
    /// 获取中断向量
    #[inline]
    pub const fn vector(&self) -> u8 {
        (self.low & 0xFF) as u8
    }
    
    /// 设置投递模式 (bits 10:8)
    #[inline]
    pub fn set_delivery_mode(&mut self, mode: DeliveryMode) {
        self.low = (self.low & !(0x07 << 8)) | ((mode as u32) << 8);
    }
    
    /// 设置目标模式 (bit 11)
    #[inline]
    pub fn set_destination_mode(&mut self, mode: DestinationMode) {
        self.low = (self.low & !(1 << 11)) | ((mode as u32) << 11);
    }
    
    /// 设置极性 (bit 13)
    #[inline]
    pub fn set_polarity(&mut self, polarity: Polarity) {
        self.low = (self.low & !(1 << 13)) | ((polarity as u32) << 13);
    }
    
    /// 设置触发模式 (bit 15)
    #[inline]
    pub fn set_trigger_mode(&mut self, mode: TriggerMode) {
        self.low = (self.low & !(1 << 15)) | ((mode as u32) << 15);
    }
    
    /// 设置屏蔽标志 (bit 16)
    #[inline]
    pub fn set_mask(&mut self, mask: MaskState) {
        self.low = (self.low & !(1 << 16)) | ((mask as u32) << 16);
    }
    
    /// 获取屏蔽状态
    #[inline]
    pub const fn is_masked(&self) -> bool {
        (self.low >> 16) & 1 != 0
    }
    
    /// 设置目标 APIC ID (bits 55:48)
    #[inline]
    pub fn set_destination(&mut self, apic_id: u8) {
        self.high = ((apic_id as u32) << 24) & 0xFF000000;
    }
    
    /// 获取目标 APIC ID
    #[inline]
    pub const fn destination(&self) -> u8 {
        ((self.high >> 24) & 0xFF) as u8
    }
    
    /// 快速创建一个标准的边沿触发、高电平有效的重定向条目
    pub fn new_standard(vector: u8, destination: u8) -> Self {
        let mut entry = Self::zeroed();
        entry.set_vector(vector);
        entry.set_delivery_mode(DeliveryMode::Fixed);
        entry.set_destination_mode(DestinationMode::Physical);
        entry.set_polarity(Polarity::ActiveHigh);
        entry.set_trigger_mode(TriggerMode::Edge);
        entry.set_mask(MaskState::Unmasked);
        entry.set_destination(destination);
        entry
    }
}

// ============================================================================
// I/O APIC 驱动
// ============================================================================

/// I/O APIC 实例 (单例, 因为大多数 x86 系统只有一个 I/O APIC)
pub struct IoApic {
    /// 是否已初始化
    initialized: bool,
    
    /// I/O APIC ID (通常为 0 或 1)
    id: u8,
    
    /// 版本信息
    version: IoApicVersion,
    
    /// 最大 IRQ 数量 (从版本寄存器读取)
    max_irq: usize,
}

impl IoApic {
    /// 创建新的 I/O APIC 实例 (未初始化)
    pub const fn new() -> Self {
        Self {
            initialized: false,
            id: 0,
            version: IoApicVersion {
                version: 0,
                max_redirections: 0,
                reserved: 0,
            },
            max_irq: 0,
        }
    }
    
    /// 初始化 I/O APIC
    ///
    /// 读取版本信息, 检测最大支持的 IRQ 数量。
    ///
    /// # Returns
    /// Ok(()) - 成功
    /// Err(&str) - 错误描述 (硬件不存在等)
    pub fn init(&mut self) -> Result<(), &'static str> {
        // 读取版本寄存器
        let ver_raw = Self::read_reg(IOAPIC_VER);
        
        self.version = IoApicVersion {
            version: (ver_raw & 0xFF) as u8,
            max_redirections: ((ver_raw >> 16) & 0xFF) as u8,
            reserved: ((ver_raw >> 24) & 0xFF) as u16,
        };
        
        // 计算最大 IRQ 数
        self.max_irq = (self.version.max_redirections + 1) as usize;
        
        // 读取 ID 寄存器
        let id_raw = Self::read_reg(IOAPIC_ID);
        self.id = ((id_raw >> 24) & 0x0F) as u8;
        
        self.initialized = true;
        
        Ok(())
    }
    
    /// 设置 IRQ 的重定向条目
    ///
    /// # Arguments
    /// * `irq` - IRQ 号 (0-23)
    /// * `entry` - 重定向表条目配置
    pub fn set_irq_entry(&self, irq: usize, entry: &IoApicRedirEntry) -> Result<(), ()> {
        if irq >= self.max_irq || !self.initialized {
            return Err(());
        }
        
        // 每个重定向条目占用 2 个寄存器: low (偶数索引) 和 high (奇数索引)
        let base_index = IOAPIC_REDTAB as u32 + (irq as u32) * 2;
        
        Self::write_reg(base_index, entry.low);
        Self::write_reg(base_index + 1, entry.high);
        
        Ok(())
    }
    
    /// 快速设置 IRQ (使用默认参数)
    ///
    /// # Arguments
    /// * `irq` - IRQ 号
    /// * `vector` - 中断向量
    /// * `destination` - 目标 APIC ID
    pub fn setup_irq(&self, irq: usize, vector: u8, destination: u8) -> Result<(), ()> {
        let entry = IoApicRedirEntry::new_standard(vector, destination);
        self.set_irq_entry(irq, &entry)
    }
    
    /// 屏蔽指定 IRQ (禁用该中断)
    pub fn mask_irq(&self, irq: usize) -> Result<(), ()> {
        if irq >= self.max_irq {
            return Err(());
        }
        
        let base_index = IOAPIC_REDTAB as u32 + (irq as u32) * 2;
        let mut low = Self::read_reg(base_index);
        low |= (MaskState::Masked as u32) << 16;
        Self::write_reg(base_index, low);
        
        Ok(())
    }
    
    /// 取消屏蔽指定 IRQ (启用该中断)
    pub fn unmask_irq(&self, irq: usize) -> Result<(), ()> {
        if irq >= self.max_irq {
            return Err(());
        }
        
        let base_index = IOAPIC_REDTAB as u32 + (irq as u32) * 2;
        let mut low = Self::read_reg(base_index);
        low &= !((MaskState::Masked as u32) << 16);
        Self::write_reg(base_index, low);
        
        Ok(())
    }
    
    /// 获取版本信息
    pub fn get_version(&self) -> IoApicVersion {
        self.version
    }
    
    /// 获取最大 IRQ 数量
    pub fn get_max_irq(&self) -> usize {
        self.max_irq
    }
    
    // ========================================================================
    // 内部辅助方法 (private)
    // ========================================================================
    
    /// 读取 I/O APIC 寄存器
    ///
    /// 先写入 REGSEL 选择寄存器, 再从 REGWIN 读取数据。
    #[inline(always)]
    fn read_reg(index: u8) -> u32 {
        let value: u32;
        
        unsafe {
            // 写入寄存器索引
            core::arch::asm!(
                "mov dword ptr [{0}], {1:e}",
                in(reg) IOAPIC_REGSEL,
                in(reg) index as u32,
                options(nomem, nostack, preserves_flags),
            );
            
            // 读取数据窗口
            core::arch::asm!(
                "mov {0:e}, dword ptr [{1}]",
                out(reg) value,
                in(reg) IOAPIC_REGWIN,
                options(nomem, nostack, preserves_flags),
            );
        }
        
        value
    }
    
    /// 写入 I/O APIC 寄存器
    #[inline(always)]
    fn write_reg(index: u8, value: u32) {
        unsafe {
            // 写入寄存器索引
            core::arch::asm!(
                "mov dword ptr [{0}], {1:e}",
                in(reg) IOAPIC_REGSEL,
                in(reg) index as u32,
                options(nomem, nostack, preserves_flags),
            );
            
            // 写入数据窗口
            core::arch::asm!(
                "mov dword ptr [{0}], {1:e}",
                in(reg) IOAPIC_REGWIN,
                in(reg) value,
                options(nomem, nostack, preserves_flags),
            );
        }
    }
}

impl Default for IoApic {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// FFI 导出接口
// ============================================================================

/// 初始化 I/O APIC (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn ioapic_init() -> i32 {
    static mut IOAPIC: IoApic = IoApic::new();
    
    match unsafe { IOAPIC.init() } {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// 设置 IRQ 重定向 (FFI兼容)
/// FFI export function (C-callable)
#[allow(dead_code)]
#[no_mangle]
/// FFI export function (C-callable)
pub extern "C" fn ioapic_setup_irq(irq: usize, vector: u8, dest: u8) -> i32 {
    static mut IOAPIC: IoApic = IoApic::new();
    
    // 注意: 这里简化了全局状态管理
    // 实际应使用 OnceCell 或类似机制
    unsafe {
        if let Err(_) = IOAPIC.init() {
            return -1;
        }
        
        match IOAPIC.setup_irq(irq, vector, dest) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    
    #[test]
    fn test_ioapic_constants() {
        assert_eq!(IOAPIC_REGSEL, 0xFEC00000);
        assert_eq!(IOAPIC_REGWIN, 0xFEC00010);
        assert_eq!(IOAPIC_MAX_IRQ, 24);
        assert_eq!(IOAPIC_DEFAULT_VECTOR_BASE, 32);
    }
    
    #[test]
    fn test_redir_entry_creation() {
        let entry = IoApicRedirEntry::new_standard(40, 0);
        
        assert_eq!(entry.vector(), 40);
        assert!(!entry.is_masked());
        assert_eq!(entry.destination(), 0);
    }
    
    #[test]
    fn test_redir_entry_modification() {
        let mut entry = IoApicRedirEntry::zeroed();
        
        // 设置各种属性
        entry.set_vector(33);
        entry.set_trigger_mode(TriggerMode::Level);
        entry.set_polarity(Polarity::ActiveLow);
        entry.set_mask(MaskState::Masked);
        entry.set_delivery_mode(DeliveryMode::Lowest);
        entry.set_destination(1);
        
        // 验证设置
        assert_eq!(entry.vector(), 33);
        assert!(entry.is_masked());
        assert_eq!(entry.destination(), 1);
    }
    
    #[test]
    fn test_enum_values() {
        assert_eq!(TriggerMode::Edge as u8, 0);
        assert_eq!(TriggerMode::Level as u8, 1);
        
        assert_eq!(Polarity::ActiveHigh as u8, 0);
        assert_eq!(Polarity::ActiveLow as u8, 1);
        
        assert_eq!(DeliveryMode::Fixed as u8, 0x00);
        assert_eq!(DeliveryMode::NMI as u8, 0x04);
        
        assert_eq!(MaskState::Unmasked as u8, 0);
        assert_eq!(MaskState::Masked as u8, 1);
    }
}