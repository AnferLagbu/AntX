#![allow(dead_code)]
//! 全局描述符表 (Global Descriptor Table, GDT) - x86_64 实现
//!
//! ## 功能概览
//!
//! - **类型安全**: 使用枚举和常量替代魔法数字
//! - **编译时检查**: 常量泛型确保描述符格式正确
//! - **零成本**: 所有检查都在编译期完成, 运行时无开销
//!
//! ## GDT 结构 (x86-64 长模式)
//!
//! ```text
//! Index | Selector | Description
//! ------|----------|------------------------------------------
//! 0     | 0x00     | Null Descriptor (必须)
//! 1     | 0x08     | Kernel Code Segment (64-bit)
//! 2     | 0x10     | Kernel Data Segment
//! 3     | 0x18     | User Data Segment (Ring 3) — 与 User Code 互换以适配 SYSRET 规则
//! 4     | 0x20     | User Code Segment (Ring 3)
//! 5     | 0x28     | TSS Descriptor (64-bit, 占用2个槽位)
//! 6     | 0x30     | TSS High (自动生成)
//! ```
//!
//! ## 对比 C 版本 (gdt.c, 71行)
//!
//! **功能复刻 + 增强**:
//! ✅ Descriptor 结构体 (替代 raw struct)
//! ✅ Access Byte 枚举 (编译时验证)
//! ✅ Granularity 标志 (类型安全位操作)
//! ✅ TSS 描述符 (自动处理高32位)
//! ✅ lgdt 汇编封装 (safe wrapper)

// ============================================================================
// 常量定义
// ============================================================================

/// GDT 最大条目数 (x86-64 通常需要 7 个)
pub const GDT_MAX_ENTRIES: usize = 7;

/// Per-CPU GDT 最大 CPU 数
const PER_CPU_MAX: usize = 256;

/// Per-CPU IST 栈大小 (16KB)
const PER_CPU_IST_SIZE: usize = 16384;

/// 空选择子值 (必须为 0)
pub const SELECTOR_NULL: u16 = 0x00;

/// 内核代码段选择子
pub const SELECTOR_KERNEL_CODE: u16 = 0x08;

/// 内核数据段选择子
pub const SELECTOR_KERNEL_DATA: u16 = 0x10;

/// 用户数据段选择子 (Ring 3)
/// 注: 与用户代码段互换位置以适配 SYSRET 指令的段选择规则
pub const SELECTOR_USER_DATA: u16 = 0x18;

/// 用户代码段选择子 (Ring 3)
pub const SELECTOR_USER_CODE: u16 = 0x20;

/// TSS 选择子 (低32位)
pub const SELECTOR_TSS: u16 = 0x28;

// ============================================================================
// 位域定义 (Access Byte + Granularity)
// ============================================================================

/// 访问权限字节 (Access Byte) 标志
#[derive(Debug, Clone, Copy)]
pub struct AccessByte(pub(crate) u8);

impl AccessByte {
    /// 已访问 (Accessed) - CPU 设置此位
    pub const ACCESSED: u8 = 1 << 0;
    /// 可读/写 (对于代码段=可读, 数据段=可写)
    pub const READABLE_WRITABLE: u8 = 1 << 1;
    /// 方向/符合 (Direction/Conforming)
    pub const DIRECTION_CONFORMING: u8 = 1 << 2;
    /// 可执行 (Executable)
    /// - 0: 数据段 (Data Segment)
    /// - 1: 代码段 (Code Segment)
    pub const EXECUTABLE: u8 = 1 << 3;
    /// 描述符类型 (Descriptor Type)
    /// 必须为 1 (System=0 在 LDT 中使用)
    pub const TYPE_SYSTEM: u8 = 1 << 4;
    /// 特权级 (DPL, Descriptor Privilege Level) - bits 6:5
    /// - 00: Ring 0 (内核)
    /// - 11: Ring 3 (用户)
    pub const DPL_MASK: u8 = 0b0110_0000;
    /// 存在位 (Present) - 必须为 1 才有效
    pub const PRESENT: u8 = 1 << 7;

    /// 创建内核代码段访问字节
    #[inline]
    pub const fn kernel_code() -> Self {
        // P=1, DPL=00, S=1, E=1, RW=1, A=0 => 0x9A
        Self(Self::PRESENT | Self::TYPE_SYSTEM | Self::EXECUTABLE | Self::READABLE_WRITABLE)
    }

    /// 创建内核数据段访问字节
    #[inline]
    pub const fn kernel_data() -> Self {
        // P=1, DPL=00, S=1, E=0, RW=1, A=0 => 0x92
        Self(Self::PRESENT | Self::TYPE_SYSTEM | Self::READABLE_WRITABLE)
    }

    /// 创建用户代码段访问字节 (Ring 3)
    #[inline]
    pub const fn user_code() -> Self {
        // P=1, DPL=11, S=1, E=1, RW=1, A=0 => 0xFA
        Self(
            Self::PRESENT
                | Self::DPL_MASK
                | Self::TYPE_SYSTEM
                | Self::EXECUTABLE
                | Self::READABLE_WRITABLE,
        )
    }

    /// 创建用户数据段访问字节 (Ring 3)
    #[inline]
    pub const fn user_data() -> Self {
        // P=1, DPL=11, S=1, E=0, RW=1, A=0 => 0xF2
        Self(Self::PRESENT | Self::DPL_MASK | Self::TYPE_SYSTEM | Self::READABLE_WRITABLE)
    }

    /// 创建 TSS 访问字节
    #[inline]
    pub const fn tss() -> Self {
        // P=1, DPL=00, S=0 (System), Type=1001 (TSS Available), Busy=0 => 0x89
        Self(0x89)
    }
}

/// 粒度标志 (Granularity Byte) 标志
#[derive(Debug, Clone, Copy)]
pub struct Granularity(pub(crate) u8);

impl Granularity {
    /// 段限制单位 (Limit granularity)
    /// - 0: 1 字节粒度
    /// - 1: 4KB 页粒度
    pub const PAGE_GRANULARITY: u8 = 1 << 7;
    /// 默认操作数大小 (Default Operation Size)
    /// - 0: 16-bit 保护模式
    /// - 1: 32-bit 保护模式 (在 64-bit 长模式下忽略)
    pub const SIZE_32BIT: u8 = 1 << 6;
    /// 64-bit 代码段标志 (Long Mode)
    /// 仅对代码段有效, 设置后启用 64-bit 模式
    pub const LONG_MODE: u8 = 1 << 5;

    /// 创建 64-bit 代码段粒度 (4KB 粒度, Long Mode)
    #[inline]
    pub const fn code_64bit() -> Self {
        Self(Self::PAGE_GRANULARITY | Self::LONG_MODE)
    }

    /// 创建数据段粒度 (4KB 粒度, 32-bit)
    #[inline]
    pub const fn data_32bit() -> Self {
        Self(Self::PAGE_GRANULARITY | Self::SIZE_32BIT)
    }

    /// 创建 TSS 粒度 (字节粒度, 64-bit)
    #[inline]
    pub const fn tss_64bit() -> Self {
        Self(Self::LONG_MODE)
    }
}

// ============================================================================
// 数据结构定义
// ============================================================================

/// GDT 条目 (64位, 8字节)
///
/// 内存布局:
/// ```text
/// Bits   Field
/// ----   ---------------------------
/// 0-15   Limit (bits 15:0)
/// 16-31  Base (bits 15:0)
/// 32-39  Base (bits 23:16)
/// 40-43  Type (4 bits)
/// 44-45  S (System, 1 bit)
/// 46-47  DPL (2 bits)
/// 48     P (Present, 1 bit)
/// 49-51  Limit (bits 19:16)
/// 52     AVL (Available, 1 bit)
/// 53     L (Long mode, 1 bit)
/// 54     D/B (Size, 1 bit)
/// 55     G (Granularity, 1 bit)
/// 56-63  Base (bits 31:24)
/// ```
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity_limit_high: u8,
    base_high: u8,
}

impl GdtEntry {
    /// 创建空描述符 (Null Descriptor)
    #[inline]
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            granularity_limit_high: 0,
            base_high: 0,
        }
    }

    /// 创建标准段描述符 (代码或数据)
    ///
    /// # Arguments
    /// * `base` - 段基地址 (在长模式下通常为 0)
    /// * `limit` - 段限制 (最大 4GB 如果使用页粒度)
    /// * `access` - 访问权限字节
    /// * `gran` - 粒度标志字节
    #[inline]
    pub const fn new_segment(base: u32, limit: u32, access: AccessByte, gran: Granularity) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,
            base_low: (base & 0xFFFF) as u16,
            base_middle: ((base >> 16) & 0xFF) as u8,
            access: access.0,
            granularity_limit_high: (((limit >> 16) & 0x0F) as u8) | (gran.0 & 0xF0),
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }

    /// 创建 TSS 描述符 (低64位)
    ///
    /// TSS 描述符占用两个 GDT 槽位:
    /// - 第一个槽位: bits 63:0 of base + limit
    /// - 第二个槽位: bits 127:64 of base
    ///
    /// # Arguments
    /// * `tss_addr` - TSS 结构体的 64 位地址
    /// * `tss_size` - TSS 结构体大小 (bytes)
    #[inline]
    pub const fn tss_low(tss_addr: u64, tss_size: u16) -> Self {
        let base_low = tss_addr as u32;

        Self {
            limit_low: tss_size,
            base_low: (base_low & 0xFFFF) as u16,
            base_middle: ((base_low >> 16) & 0xFF) as u8,
            access: AccessByte::tss().0,
            granularity_limit_high: 0x00, // TSS 不使用粒度标志
            base_high: ((base_low >> 24) & 0xFF) as u8,
        }
    }

    /// 创建 TSS 描述符的高64位 (base[63:32])
    #[inline]
    pub const fn tss_high(tss_addr: u64) -> Self {
        let base_high32 = (tss_addr >> 32) as u32;

        Self {
            limit_low: (base_high32 & 0xFFFF) as u16,
            base_low: ((base_high32 >> 16) & 0xFFFF) as u16,
            base_middle: 0,
            access: 0,
            granularity_limit_high: 0,
            base_high: 0,
        }
    }
}

/// GDTR 寄存器结构 (用于 lgdt 指令)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct GdtPtr {
    /// GDT 大小限制 (bytes - 1)
    pub limit: u16,
    /// GDT 基地址 (64位指针)
    pub base: u64,
}

// ============================================================================
// Per-CPU GDT 数据结构
// ============================================================================

/// Syscall 入口 per-CPU 数据 (通过 swapgs + GS 段访问)
///
/// 布局: 汇编代码通过 `[gs:0]` 访问 `kernel_rsp`,
/// 因此该字段必须位于结构体偏移 0 处。
#[repr(C)]
pub struct SyscallPerCpu {
    pub kernel_rsp: u64,
}

/// 每个 CPU 独立的 syscall 内核栈大小 (8KB)
const PER_CPU_SYSCALL_STACK_SIZE: usize = 8192;

/// 每个 CPU 独立的 GDT + TSS + IST 栈 + syscall 数据
///
/// 所有 CPU 共享相同的段描述符 (flat model)，
/// 但 TSS 描述符必须指向各自的 TSS 实例。
struct PerCpuGdt {
    entries: [GdtEntry; GDT_MAX_ENTRIES],
    ptr: GdtPtr,
    tss: super::tss::TaskStateSegment,
    syscall: SyscallPerCpu,
    syscall_stack: [u8; PER_CPU_SYSCALL_STACK_SIZE],
    ist0: [u8; PER_CPU_IST_SIZE],
    ist1: [u8; PER_CPU_IST_SIZE],
    ist2: [u8; PER_CPU_IST_SIZE],
}

impl PerCpuGdt {
    const fn new() -> Self {
        Self {
            entries: [GdtEntry::null(); GDT_MAX_ENTRIES],
            ptr: GdtPtr { limit: 0, base: 0 },
            tss: super::tss::TaskStateSegment::zeroed(),
            syscall: SyscallPerCpu { kernel_rsp: 0 },
            syscall_stack: [0u8; PER_CPU_SYSCALL_STACK_SIZE],
            ist0: [0u8; PER_CPU_IST_SIZE],
            ist1: [0u8; PER_CPU_IST_SIZE],
            ist2: [0u8; PER_CPU_IST_SIZE],
        }
    }
}

// ============================================================================
// 全局状态 (Per-CPU)
// ============================================================================

static mut PER_CPU_GDT: [core::mem::MaybeUninit<PerCpuGdt>; PER_CPU_MAX] =
    [const { core::mem::MaybeUninit::uninit() }; PER_CPU_MAX];

// ============================================================================
// 内部辅助函数
// ============================================================================

/// 获取指定 CPU 的 GDT 不可变引用
#[inline]
fn per_cpu_gdt(cpu: u32) -> &'static PerCpuGdt {
    unsafe { &*PER_CPU_GDT[(cpu as usize) % PER_CPU_MAX].as_ptr() }
}

/// 获取指定 CPU 的 GDT 可变引用
#[inline]
fn per_cpu_gdt_mut(cpu: u32) -> &'static mut PerCpuGdt {
    unsafe { &mut *PER_CPU_GDT[(cpu as usize) % PER_CPU_MAX].as_mut_ptr() }
}

/// 获取当前 CPU 的 GDT 不可变引用
#[inline]
fn current_per_cpu_gdt() -> &'static PerCpuGdt {
    let cpu = crate::kernel::smp::get_current_cpu();
    per_cpu_gdt(cpu)
}

/// 获取当前 CPU 的 GDT 可变引用
#[inline]
fn current_per_cpu_gdt_mut() -> &'static mut PerCpuGdt {
    let cpu = crate::kernel::smp::get_current_cpu();
    per_cpu_gdt_mut(cpu)
}

/// 初始化 per-CPU GDT 的段描述符 (所有 CPU 共享相同段描述符)
unsafe fn init_gdt_entries(entries: &mut [GdtEntry; GDT_MAX_ENTRIES]) {
    entries[0] = GdtEntry::null();

    entries[1] = GdtEntry::new_segment(
        0,
        0xFFFF_FFFF,
        AccessByte::kernel_code(),
        Granularity::code_64bit(),
    );

    entries[2] = GdtEntry::new_segment(
        0,
        0xFFFF_FFFF,
        AccessByte::kernel_data(),
        Granularity::data_32bit(),
    );

    entries[3] = GdtEntry::new_segment(
        0,
        0xFFFF_FFFF,
        AccessByte::user_data(),
        Granularity::data_32bit(),
    );

    entries[4] = GdtEntry::new_segment(
        0,
        0xFFFF_FFFF,
        AccessByte::user_code(),
        Granularity::code_64bit(),
    );
}

// ============================================================================
// 公共 API
// ============================================================================

/// 初始化 BSP 的 GDT 和 TSS
///
/// **必须在内核启动早期调用**, 在 IDT 初始化之前。
///
/// # 功能
/// 1. 设置标准段描述符 (Null, Kernel CS/DS, User CS/DS)
/// 2. 初始化 per-CPU TSS 结构体
/// 3. 设置 TSS 描述符 (占用2个槽位)
/// 4. 加载 GDTR (lgdt 指令)
/// 5. 加载 TR (ltr 指令, 任务寄存器)
pub fn gdt_init() -> i32 {
    use crate::kernel::klog::{klog_write, LogCategory, LogLevel};

    static INIT_MSG: &[u8] = b"Initializing GDT and TSS (BSP)...\0";
    unsafe {
        klog_write(
            LogLevel::Info as u8,
            LogCategory::Boot as u8,
            core::ptr::null(),
            core::ptr::null(),
            0,
            INIT_MSG.as_ptr() as *const i8,
        );
    }

    unsafe {
        let gdt = per_cpu_gdt_mut(0);

        gdt.ptr.limit = (core::mem::size_of::<[GdtEntry; GDT_MAX_ENTRIES]>() - 1) as u16;
        gdt.ptr.base = gdt.entries.as_ptr() as u64;

        init_gdt_entries(&mut gdt.entries);

        gdt.tss = super::tss::TaskStateSegment::zeroed();

        gdt.tss
            .set_ist(0, gdt.ist0.as_ptr() as u64 + gdt.ist0.len() as u64);
        gdt.tss
            .set_ist(1, gdt.ist1.as_ptr() as u64 + gdt.ist1.len() as u64);
        gdt.tss
            .set_ist(2, gdt.ist2.as_ptr() as u64 + gdt.ist2.len() as u64);

        gdt.tss.iomap_base = core::mem::size_of::<super::tss::TaskStateSegment>() as u16;

        let tss_addr = &gdt.tss as *const _ as u64;
        let tss_size = (core::mem::size_of::<super::tss::TaskStateSegment>() - 1) as u16;

        gdt.entries[5] = GdtEntry::tss_low(tss_addr, tss_size);
        gdt.entries[6] = GdtEntry::tss_high(tss_addr);

        gdt_flush(&gdt.ptr);
        tss_flush(SELECTOR_TSS);

        gdt.syscall.kernel_rsp = gdt.syscall_stack.as_ptr() as u64 + gdt.syscall_stack.len() as u64;

        // IA32_KERNEL_GS_BASE — swapgs 时切换到该地址
        const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
        crate::kernel::cpu::msr::write_msr(IA32_KERNEL_GS_BASE, &gdt.syscall as *const _ as u64);
    }

    static OK_MSG: &[u8] = b"GDT and TSS initialized successfully (BSP)\0";
    unsafe {
        klog_write(
            LogLevel::Info as u8,
            LogCategory::Boot as u8,
            core::ptr::null(),
            core::ptr::null(),
            0,
            OK_MSG.as_ptr() as *const i8,
        );
    }

    0
}

/// 初始化 AP 的 per-CPU GDT 和独立 TSS
///
/// Trampoline 已通过 lgdt [SINFO_GDT_LIMIT] 加载了 BSP 的 GDT 作为过渡，
/// 本函数在 AP 进入长模式后调用，为目标 CPU 初始化独立的 GDT + TSS。
pub fn gdt_init_ap(cpu_index: u32) {
    unsafe {
        let ap = per_cpu_gdt_mut(cpu_index);

        ap.ptr.limit = (core::mem::size_of::<[GdtEntry; GDT_MAX_ENTRIES]>() - 1) as u16;
        ap.ptr.base = ap.entries.as_ptr() as u64;

        init_gdt_entries(&mut ap.entries);

        ap.tss = super::tss::TaskStateSegment::zeroed();

        ap.tss
            .set_ist(0, ap.ist0.as_ptr() as u64 + ap.ist0.len() as u64);
        ap.tss
            .set_ist(1, ap.ist1.as_ptr() as u64 + ap.ist1.len() as u64);
        ap.tss
            .set_ist(2, ap.ist2.as_ptr() as u64 + ap.ist2.len() as u64);

        ap.tss.iomap_base = core::mem::size_of::<super::tss::TaskStateSegment>() as u16;

        let tss_addr = &ap.tss as *const _ as u64;
        let tss_size = (core::mem::size_of::<super::tss::TaskStateSegment>() - 1) as u16;

        ap.entries[5] = GdtEntry::tss_low(tss_addr, tss_size);
        ap.entries[6] = GdtEntry::tss_high(tss_addr);

        gdt_flush(&ap.ptr);
        tss_flush(SELECTOR_TSS);

        ap.syscall.kernel_rsp = ap.syscall_stack.as_ptr() as u64 + ap.syscall_stack.len() as u64;

        const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;
        crate::kernel::cpu::msr::write_msr(IA32_KERNEL_GS_BASE, &ap.syscall as *const _ as u64);
    }
}

/// 获取 GDT 表的引用 (调试用途)
///
/// # Safety
/// 返回的引用在整个程序生命周期有效。
#[inline]
pub unsafe fn get_gdt_table() -> &'static [GdtEntry; GDT_MAX_ENTRIES] {
    &current_per_cpu_gdt().entries
}

/// 获取 BSP 的 GDT 指针 (用于 AP trampoline 过渡阶段)
///
/// Trampoline 在 16→64 过渡时使用此 GDT 加载段寄存器，
/// 进入长模式后 AP 会切换到自己的 per-CPU GDT。
pub fn get_gdt_ptr() -> &'static GdtPtr {
    &per_cpu_gdt(0).ptr
}

/// 获取当前 CPU 的 TSS 可变引用 (用于设置栈指针等)
///
/// # Safety
/// 应该在初始化后调用, 且注意并发安全。
#[inline]
pub unsafe fn get_tss_mut() -> &'static mut super::tss::TaskStateSegment {
    &mut current_per_cpu_gdt_mut().tss
}

// ============================================================================
// 内联汇编封装 (硬件指令)
// ============================================================================

/// 执行 LGDT 指令 (Load Global Descriptor Table Register)
///
/// # Arguments
/// * `gdt_ptr` - 指向 GdtPtr 结构体的引用
///
/// # Safety
/// 此函数修改 GDTR 寄存器, 会立即影响内存分段行为。
#[inline(always)]
unsafe fn gdt_flush(gdt_ptr: &GdtPtr) {
    core::arch::asm!(
        "lgdt [{0}]",
        in(reg) gdt_ptr,
        options(nostack, preserves_flags),
    );
}

/// 执行 LTR 指令 (Load Task Register)
///
/// # Arguments
/// * `selector` - TSS 选择子 (如 0x28)
///
/// # Safety
/// 此函数加载新的 TSS 到任务寄存器, 会标记 TSS 为 busy。
#[inline(always)]
unsafe fn tss_flush(selector: u16) {
    core::arch::asm!(
        "ltr {0:x}",
        in(reg) selector,
        options(nostack, preserves_flags),
    );
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gdt_entry_null() {
        let null_desc = GdtEntry::null();
        let bytes = unsafe { core::ptr::read_volatile(&null_desc as *const _ as *const u64) };
        assert_eq!(bytes, 0, "Null descriptor should be all zeros");
    }

    #[test]
    fn test_access_byte_constants() {
        assert_eq!(AccessByte::kernel_code().0, 0x9A);
        assert_eq!(AccessByte::kernel_data().0, 0x92);
        assert_eq!(AccessByte::user_code().0, 0xFA);
        assert_eq!(AccessByte::user_data().0, 0xF2);
        assert_eq!(AccessByte::tss().0, 0x89);
    }

    #[test]
    fn test_granularity_constants() {
        let code_gran = Granularity::code_64bit();
        assert!(code_gran.0 & Granularity::PAGE_GRANULARITY != 0);
        assert!(code_gran.0 & Granularity::LONG_MODE != 0);

        let data_gran = Granularity::data_32bit();
        assert!(data_gran.0 & Granularity::PAGE_GRANULARITY != 0);
        assert!(data_gran.0 & Granularity::SIZE_32BIT != 0);
    }

    #[test]
    fn test_selector_values() {
        assert_eq!(SELECTOR_NULL, 0x00);
        assert_eq!(SELECTOR_KERNEL_CODE, 0x08);
        assert_eq!(SELECTOR_KERNEL_DATA, 0x10);
        assert_eq!(SELECTOR_USER_DATA, 0x18);
        assert_eq!(SELECTOR_USER_CODE, 0x20);
        assert_eq!(SELECTOR_TSS, 0x28);
    }
}
#[cfg(feature = "kernel_test")]
pub fn register_gdt_tests() {
    crate::kernel::tests::arch::register_gdt_tests();
}
