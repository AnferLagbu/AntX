//! UEFI 运行时服务 — 固件接口抽象
//!
//! ## 设计
//!
//! UEFI (Unified Extensible Firmware Interface) 替代传统 BIOS, 提供标准化的
//! 固件服务接口. 本模块实现内核对 UEFI 运行时服务的访问:
//!
//! 1. **运行时服务**: GetTime/SetTime, GetVariable/SetVariable, ResetSystem
//! 2. **GOP (Graphics Output Protocol)**: 帧缓冲区信息
//! 3. **变量存储**: UEFI 变量读写 (BootOrder, ConOut 等)
//! 4. **内存映射**: UEFI 内存描述符转换
//!
//! ### 与 Linux 的差异
//!
//! 1. **无 efivarfs**: 不挂载文件系统接口, 使用 syscall
//! 2. **无 EFI_PSTORE**: 不支持 pstore 后端
//! 3. **无 EFI_RNG**: 不使用 UEFI 随机数
//! 4. **运行时服务通过物理地址映射访问**: SetVirtualAddressMap 后
//!
//! ## SAFETY
//!
//! 本模块属于 framework/TCB, 允许 unsafe.
//! UEFI 运行时服务调用涉及物理地址映射和固件调用.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use alloc::vec;
use alloc::vec::Vec;
use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock;

// ============================================================================
// 常量
// ============================================================================

/// UEFI 变量最大名称长度 (字符)
pub const EFI_MAX_VAR_NAME: usize = 1024;
/// UEFI 变量最大数据大小
pub const EFI_MAX_VAR_DATA: usize = 32768;
/// UEFI 变量属性: 非易失性
pub const EFI_VARIABLE_NON_VOLATILE: u32 = 0x00000001;
/// UEFI 变量属性: 引导服务访问
pub const EFI_VARIABLE_BOOTSERVICE_ACCESS: u32 = 0x00000002;
/// UEFI 变量属性: 运行时访问
pub const EFI_VARIABLE_RUNTIME_ACCESS: u32 = 0x00000004;

// ============================================================================
// EFI 时间
// ============================================================================

/// EFI 时间结构
#[derive(Debug, Clone, Copy, Default)]
pub struct EfiTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub nanosecond: u32,
    pub timezone: i16,   // 0=UTC, -2047=未指定
    pub daylight: u8,    // bit0=ADJUST, bit1=DST
}

impl EfiTime {
    pub fn to_unix_ns(&self) -> u64 {
        // 简化: 转换为秒数
        let days_before_month: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let y = self.year as u64;
        let m = self.month as u64;
        let d = self.day as u64;
        let leap_years = (y - 1) / 4 - (y - 1) / 100 + (y - 1) / 400;
        let is_leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let extra = if is_leap && m > 2 { 1 } else { 0 };
        let days = y * 365 + leap_years + days_before_month.get(m as usize - 1).unwrap_or(&0) + d + extra;
        // 1970-01-01 基准
        let epoch_days = 1970 * 365 + (1970 - 1) / 4 - (1970 - 1) / 100 + (1970 - 1) / 400 + 1;
        let unix_days = days.saturating_sub(epoch_days);
        let unix_secs = unix_days * 86400
            + self.hour as u64 * 3600
            + self.minute as u64 * 60
            + self.second as u64;
        unix_secs * 1_000_000_000 + self.nanosecond as u64
    }
}

// ============================================================================
// EFI 内存类型
// ============================================================================

/// EFI 内存类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EfiMemoryType {
    Reserved = 0,
    LoaderCode = 1,
    LoaderData = 2,
    BootServicesCode = 3,
    BootServicesData = 4,
    RuntimeServicesCode = 5,
    RuntimeServicesData = 6,
    Conventional = 7,
    Unusable = 8,
    AcpiReclaim = 9,
    AcpiNvs = 10,
    MemoryMappedIo = 11,
    MemoryMappedIoPortSpace = 12,
    PalCode = 13,
    Persistent = 14,
}

impl EfiMemoryType {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::LoaderCode,
            2 => Self::LoaderData,
            3 => Self::BootServicesCode,
            4 => Self::BootServicesData,
            5 => Self::RuntimeServicesCode,
            6 => Self::RuntimeServicesData,
            7 => Self::Conventional,
            8 => Self::Unusable,
            9 => Self::AcpiReclaim,
            10 => Self::AcpiNvs,
            11 => Self::MemoryMappedIo,
            12 => Self::MemoryMappedIoPortSpace,
            13 => Self::PalCode,
            14 => Self::Persistent,
            _ => Self::Reserved,
        }
    }
}

// ============================================================================
// EFI 内存描述符
// ============================================================================

/// EFI 内存描述符
#[derive(Debug, Clone, Copy)]
pub struct EfiMemoryDescriptor {
    pub memory_type: EfiMemoryType,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

// ============================================================================
// GOP 模式信息
// ============================================================================

/// GOP 像素格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EfiPixelFormat {
    RedGreenBlueReserved8BitPerColor = 0,
    BlueGreenRedReserved8BitPerColor = 1,
    BitMask = 2,
    BltOnly = 3,
}

/// GOP 模式信息
#[derive(Debug, Clone, Copy)]
pub struct EfiGopModeInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: EfiPixelFormat,
    pub pixels_per_scan_line: u32,
    pub frame_buffer_base: u64,
    pub frame_buffer_size: u64,
}

// ============================================================================
// UEFI 变量
// ============================================================================

/// UEFI 变量 (软件模拟)
#[derive(Debug, Clone)]
pub struct EfiVariable {
    /// 变量名 (UTF-8)
    pub name: Vec<u8>,
    /// GUID (16 字节)
    pub guid: [u8; 16],
    /// 属性
    pub attributes: u32,
    /// 数据
    pub data: Vec<u8>,
}

// ============================================================================
// UEFI 子系统
// ============================================================================

/// UEFI 子系统
pub struct UefiSubsystem {
    /// 系统表物理地址
    system_table_addr: AtomicU64,
    /// GOP 模式信息
    gop_mode: IrqSpinLock<Option<EfiGopModeInfo>>,
    /// UEFI 变量存储 (软件模拟)
    variables: IrqSpinLock<Vec<EfiVariable>>,
    /// 内存映射
    memory_map: IrqSpinLock<Vec<EfiMemoryDescriptor>>,
    /// 是否已初始化
    initialized: AtomicBool,
    /// 是否有 UEFI 固件
    has_uefi: AtomicBool,
}

impl UefiSubsystem {
    pub const fn new() -> Self {
        Self {
            system_table_addr: AtomicU64::new(0),
            gop_mode: IrqSpinLock::new(None),
            variables: IrqSpinLock::new(Vec::new()),
            memory_map: IrqSpinLock::new(Vec::new()),
            initialized: AtomicBool::new(false),
            has_uefi: AtomicBool::new(false),
        }
    }

    /// 初始化
    pub fn init(&self, system_table_addr: u64) {
        if self.initialized.load(Ordering::Acquire) {
            return;
        }

        self.system_table_addr.store(system_table_addr, Ordering::Release);
        self.has_uefi.store(system_table_addr != 0, Ordering::Release);

        if system_table_addr != 0 {
            // SAFETY: system_table_addr 由引导加载器传入
            // 在实际实现中, 这里会解析 EFI_SYSTEM_TABLE
            self.parse_system_table(system_table_addr);
        }

        // 初始化默认变量
        self.init_default_variables();

        self.initialized.store(true, Ordering::Release);
        crate::klog_ffi!(
            klog_ffi_info,
            "[UEFI] initialized: system_table={:#x}, has_firmware={}",
            system_table_addr,
            system_table_addr != 0
        );
    }

    /// 解析系统表 (简化)
    fn parse_system_table(&self, _addr: u64) {
        // TODO: 实际解析 EFI_SYSTEM_TABLE
        // 1. 验证签名 (0x5453595320494249)
        // 2. 提取 RuntimeServices 指针
        // 3. 提取 BootServices (ExitBootServices 前可用)
        // 4. 提取 ConfigurationTable (ACPI, SMBIOS 等)
    }

    /// 初始化默认变量
    fn init_default_variables(&self) {
        let mut vars = self.variables.lock();

        // BootOrder
        vars.push(EfiVariable {
            name: b"BootOrder".to_vec(),
            guid: [0x84; 16], // 全局变量 GUID (简化)
            attributes: EFI_VARIABLE_NON_VOLATILE | EFI_VARIABLE_BOOTSERVICE_ACCESS | EFI_VARIABLE_RUNTIME_ACCESS,
            data: vec![0, 0], // Boot0000
        });

        // ConOut (控制台输出)
        vars.push(EfiVariable {
            name: b"ConOut".to_vec(),
            guid: [0x84; 16],
            attributes: EFI_VARIABLE_NON_VOLATILE | EFI_VARIABLE_BOOTSERVICE_ACCESS | EFI_VARIABLE_RUNTIME_ACCESS,
            data: vec![],
        });

        // SecureBoot
        vars.push(EfiVariable {
            name: b"SecureBoot".to_vec(),
            guid: [0x77; 16], // EFI_GLOBAL_VARIABLE
            attributes: EFI_VARIABLE_BOOTSERVICE_ACCESS | EFI_VARIABLE_RUNTIME_ACCESS,
            data: vec![0], // 0 = disabled
        });
    }

    /// 获取 UEFI 变量
    pub fn get_variable(&self, name: &[u8], guid: &[u8; 16]) -> Option<(u32, Vec<u8>)> {
        let vars = self.variables.lock();
        for v in vars.iter() {
            if v.name == name && v.guid == *guid {
                return Some((v.attributes, v.data.clone()));
            }
        }
        None
    }

    /// 设置 UEFI 变量
    pub fn set_variable(&self, name: &[u8], guid: &[u8; 16], attrs: u32, data: &[u8]) -> bool {
        if name.len() > EFI_MAX_VAR_NAME || data.len() > EFI_MAX_VAR_DATA {
            return false;
        }

        let mut vars = self.variables.lock();
        // 查找已有变量
        for v in vars.iter_mut() {
            if v.name == name && v.guid == *guid {
                v.attributes = attrs;
                v.data = data.to_vec();
                return true;
            }
        }
        // 新变量
        vars.push(EfiVariable {
            name: name.to_vec(),
            guid: *guid,
            attributes: attrs,
            data: data.to_vec(),
        });
        true
    }

    /// 删除 UEFI 变量
    pub fn delete_variable(&self, name: &[u8], guid: &[u8; 16]) -> bool {
        let mut vars = self.variables.lock();
        let before = vars.len();
        vars.retain(|v| !(v.name == name && v.guid == *guid));
        vars.len() != before
    }

    /// 列出所有变量
    pub fn list_variables(&self) -> Vec<(Vec<u8>, [u8; 16])> {
        let vars = self.variables.lock();
        vars.iter().map(|v| (v.name.clone(), v.guid)).collect()
    }

    /// 获取时间
    pub fn get_time(&self) -> EfiTime {
        // 简化: 从内核时钟转换
        let ns = crate::kernel::framework::timer::tick::ticks_to_ns(
            crate::kernel::framework::timer::tick::get_ticks()
        );
        let secs = ns / 1_000_000_000;
        let nsec = (ns % 1_000_000_000) as u32;

        // Unix 时间戳转日期 (简化)
        let days = secs / 86400;
        let time_of_day = secs % 86400;

        // 简单的日期计算
        let mut year = 1970u16;
        let mut remaining_days = days;
        loop {
            let days_in_year = if is_leap_year(year) { 366 } else { 365 };
            if remaining_days < days_in_year {
                break;
            }
            remaining_days -= days_in_year;
            year += 1;
        }

        let days_in_months = if is_leap_year(year) {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut month = 1u8;
        for &dim in &days_in_months {
            if remaining_days < dim {
                break;
            }
            remaining_days -= dim;
            month += 1;
        }

        EfiTime {
            year,
            month,
            day: (remaining_days + 1) as u8,
            hour: (time_of_day / 3600) as u8,
            minute: ((time_of_day % 3600) / 60) as u8,
            second: (time_of_day % 60) as u8,
            nanosecond: nsec,
            timezone: 0, // UTC
            daylight: 0,
        }
    }

    /// 设置时间 (软件模拟)
    pub fn set_time(&self, _time: &EfiTime) -> bool {
        // TODO: 调用 EFI_RUNTIME_SERVICES.SetTime
        // 在软件模拟中, 这需要调整内核时钟
        true
    }

    /// 设置 GOP 模式信息
    pub fn set_gop_mode(&self, mode: EfiGopModeInfo) {
        *self.gop_mode.lock() = Some(mode);
    }

    /// 获取 GOP 模式信息
    pub fn get_gop_mode(&self) -> Option<EfiGopModeInfo> {
        *self.gop_mode.lock()
    }

    /// 设置内存映射
    pub fn set_memory_map(&self, map: Vec<EfiMemoryDescriptor>) {
        *self.memory_map.lock() = map;
    }

    /// 获取内存映射
    pub fn get_memory_map(&self) -> Vec<EfiMemoryDescriptor> {
        self.memory_map.lock().clone()
    }

    /// 是否有 UEFI 固件
    pub fn has_uefi(&self) -> bool {
        self.has_uefi.load(Ordering::Acquire)
    }

    /// 是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    /// 获取变量数量
    pub fn variable_count(&self) -> usize {
        self.variables.lock().len()
    }
}

fn is_leap_year(year: u16) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

// ============================================================================
// 全局实例
// ============================================================================

/// 全局 UEFI 子系统
static UEFI: UefiSubsystem = UefiSubsystem::new();

/// 初始化 UEFI
pub fn uefi_init(system_table_addr: u64) {
    UEFI.init(system_table_addr);
}

/// 获取全局 UEFI 子系统
pub fn uefi_subsystem() -> &'static UefiSubsystem {
    &UEFI
}

/// UEFI 是否已初始化
pub fn uefi_is_initialized() -> bool {
    UEFI.is_initialized()
}

// ============================================================================
// 系统调用
// ============================================================================

/// sys_uefi — UEFI 系统调用
///
/// `a0`: cmd
///   0 = get_variable(name_ptr: a1, guid_ptr: a2) → (attrs, data_ptr)
///   1 = set_variable(name_ptr: a1, guid_ptr: a2, attrs: a3, data_ptr: a4, data_size: a5)
///   2 = delete_variable(name_ptr: a1, guid_ptr: a2)
///   3 = get_time() → ns
///   4 = set_time(ns: a1)
///   5 = get_gop_mode() → fb_base
///   6 = list_variables() → count
///   7 = has_uefi() → bool
///   8 = is_initialized() → bool
#[no_mangle]
pub fn sys_uefi(cmd: u64, a1: u64, a2: u64) -> i64 {
    if !uefi_is_initialized() && cmd != 8 {
        return -(11i64); // EAGAIN
    }

    match cmd {
        0 => {
            // get_variable (简化: 返回是否存在)
            // 实际实现需要 copy_from_user 读取 name/guid
            let _ = (a1, a2);
            0
        }
        1 => {
            // set_variable (简化)
            let _ = (a1, a2);
            0
        }
        2 => {
            // delete_variable (简化)
            let _ = (a1, a2);
            0
        }
        3 => {
            // get_time → ns
            let time = uefi_subsystem().get_time();
            time.to_unix_ns() as i64
        }
        4 => {
            // set_time
            let _ = a1;
            0
        }
        5 => {
            // get_gop_mode → fb_base
            match uefi_subsystem().get_gop_mode() {
                Some(mode) => mode.frame_buffer_base as i64,
                None => 0,
            }
        }
        6 => {
            // list_variables → count
            uefi_subsystem().list_variables().len() as i64
        }
        7 => {
            // has_uefi
            uefi_subsystem().has_uefi() as i64
        }
        8 => {
            // is_initialized
            uefi_is_initialized() as i64
        }
        _ => -(38i64), // ENOSYS
    }
}
