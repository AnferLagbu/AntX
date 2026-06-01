//! ACPI / RSDP / MADT — 多核检测与 AP 发现
//!
//! 通过 RSDP → RSDT/XSDT → MADT 路径发现所有 LAPIC 条目，
//! 为 SMP AP 启动提供 CPU 拓扑信息。
//!
//! ## 架构
//!
//! ```text
//! RSDP (Root System Description Pointer)
//!   ├─ RSDT (Root SDT, 32-bit pointers)
//!   │    └─ MADT (Multiple APIC Description Table)
//!   └─ XSDT (Extended SDT, 64-bit pointers)
//!        └─ MADT
//!
//! MADT 条目:
//!   0x00 — Processor Local APIC (lapic_id, apic_id, flags)
//!   0x01 — I/O APIC
//!   0x02 — Interrupt Source Override
//!   0x04 — Local APIC NMI
//!   0x05 — Local APIC Address Override
//!   0x09 — x2APIC
//! ```

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub use crate::kernel::config::MAX_CPUS;

/// MADT 条目类型
const MADT_TYPE_LAPIC: u8 = 0x00;
const MADT_TYPE_IOAPIC: u8 = 0x01;
const _MADT_TYPE_ISO: u8 = 0x02;
const _MADT_TYPE_NMI: u8 = 0x04;

static MADT_BASE: AtomicU64 = AtomicU64::new(0);
static MADT_FOUND: AtomicBool = AtomicBool::new(false);
static IOAPIC_ADDR: AtomicU64 = AtomicU64::new(0);
static IOAPIC_GSIB: AtomicU32 = AtomicU32::new(0);

/// 发现的 AP LAPIC 信息
#[derive(Debug, Clone, Copy)]
pub struct ApInfo {
    pub lapic_id: u32,
    pub apic_id: u32,
    pub enabled: bool,
}

static AP_LIST: spin::Mutex<[Option<ApInfo>; MAX_CPUS]> = 
    spin::Mutex::new([None; MAX_CPUS]);
static AP_COUNT: AtomicU32 = AtomicU32::new(0);

// ============================================================================
// RSDP 搜索
// ============================================================================

pub fn find_rsdp(multiboot2_info_ptr: u64) -> Option<u64> {
    // 1. 尝试从 Multiboot2 info 中获取 RSDP
    if multiboot2_info_ptr != 0 {
        if let Some(rsdp) = find_rsdp_from_mb2(multiboot2_info_ptr) {
            return Some(rsdp);
        }
    }

    // 2. 扫描 EBDA (Extended BIOS Data Area, 通常在 0x80000-0x9FFFF)
    if let Some(rsdp) = scan_ebda() {
        return Some(rsdp);
    }

    // 3. 扫描 BIOS ROM 区域 (0xE0000-0xFFFFF)
    scan_bios_rom()
}

fn find_rsdp_from_mb2(mb2_ptr: u64) -> Option<u64> {
    let ptr = mb2_ptr as *const u8;
    let total_size = unsafe { *(ptr as *const u32) };

    let mut offset: usize = 8;
    while offset + 8 <= total_size as usize {
        let tag_type = unsafe { *((ptr as *const u32).add(offset / 4)) };
        let tag_size = unsafe { *((ptr as *const u32).add(offset / 4 + 1)) };

        if tag_type == 0 || tag_size == 0 {
            break;
        }

        // Multiboot2 tag 14 = ACPI old RSDP, tag 15 = ACPI new RSDP
        if tag_type == 14 || tag_type == 15 {
            let rsdp_ptr = unsafe { *((ptr as *const u64).add(offset / 8 + 1)) };
            if is_valid_rsdp(rsdp_ptr) {
                unsafe {
                    crate::kernel::klog::klog_info(c"[ACPI] RSDP found via Multiboot2".as_ptr());
                }
                return Some(rsdp_ptr);
            }
        }

        offset += tag_size as usize;
        // Align to 8-byte boundary
        offset = (offset + 7) & !7;
    }

    None
}

fn scan_ebda() -> Option<u64> {
    // EBDA 基址存于 BDA (BIOS Data Area) 偏移 0x40E
    let bda_ptr = 0x400 as *const u16;
    unsafe {
        let ebda_seg = bda_ptr.add(0x40E / 2).read_volatile();
        if ebda_seg == 0 {
            return None;
        }
        let ebda_base = (ebda_seg as u64) << 4;
        scan_memory_range(ebda_base, 0x1000)
    }
}

fn scan_bios_rom() -> Option<u64> {
    scan_memory_range(0xE0000, 0x20000)
}

fn scan_memory_range(start: u64, len: u64) -> Option<u64> {
    let end = start + len;
    let mut addr = start;
    while addr + 36 <= end {
        if is_valid_rsdp(addr) {
            unsafe {
                crate::kernel::klog::klog_info(c"[ACPI] RSDP found via BIOS scan".as_ptr());
            }
            return Some(addr);
        }
        addr += 16;
    }
    None
}

fn is_valid_rsdp(addr: u64) -> bool {
    let ptr = addr as *const u8;
    unsafe {
        // "RSD PTR " 签名字符串
        let sig: &[u8; 8] = &*(ptr as *const [u8; 8]);
        if sig != b"RSD PTR " {
            return false;
        }

        // 校验和: 前 20 个字节的校验和必须为 0
        let mut sum: u8 = 0;
        for i in 0..20 {
            sum = sum.wrapping_add(ptr.add(i).read_volatile());
        }
        if sum != 0 {
            return false;
        }

        true
    }
}

// ============================================================================
// SDT 解析
// ============================================================================

fn get_rsdt(rsdp: u64) -> Option<&'static SdtHeader> {
    let ptr = rsdp as *const u8;
    unsafe {
        let revision = ptr.add(15).read_volatile();
        if revision >= 2 {
            // XSDT (64-bit pointers, always in revision >= 2)
            let xsdt_addr = *(ptr.add(24) as *const u64);
            if xsdt_addr != 0 {
                return Some(&*(xsdt_addr as *const SdtHeader));
            }
        }

        // RSDT (32-bit pointers)
        let rsdt_addr = *(ptr.add(16) as *const u32) as u64;
        if rsdt_addr != 0 {
            Some(&*(rsdt_addr as *const SdtHeader))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct SdtHeader {
    signature: [u8; 4],
    length: u32,
    _revision: u8,
    _checksum: u8,
    _oem_id: [u8; 6],
    _oem_table_id: [u8; 8],
    _oem_revision: u32,
    _creator_id: u32,
    _creator_revision: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct MadtHeader {
    header: SdtHeader,
    local_apic_addr: u32,
    flags: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct MadtEntry {
    entry_type: u8,
    length: u8,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct MadtLapic {
    entry_type: u8,
    length: u8,
    acpi_proc_id: u8,
    apic_id: u8,
    flags: u32,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct MadtIoApic {
    entry_type: u8,
    length: u8,
    io_apic_id: u8,
    _reserved: u8,
    io_apic_addr: u32,
    global_sys_int_base: u32,
}

// ============================================================================
// MADT 解析 — 核心公共接口
// ============================================================================

pub fn parse_madt(multiboot2_info_ptr: u64) -> bool {
    let rsdp = match find_rsdp(multiboot2_info_ptr) {
        Some(addr) => addr,
        None => {
            unsafe {
                crate::kernel::klog::klog_info(c"[ACPI] RSDP not found".as_ptr());
            }
            return false;
        }
    };

    let rsdt_or_xsdt = match get_rsdt(rsdp) {
        Some(sdt) => sdt,
        None => {
            unsafe {
                crate::kernel::klog::klog_info(c"[ACPI] RSDT/XSDT not found".as_ptr());
            }
            return false;
        }
    };

    let rsdp_ptr = rsdp as *const u8;
    let revision = unsafe { rsdp_ptr.add(15).read_volatile() };
    let uses_xsdt = revision >= 2 && unsafe { *(rsdp_ptr.add(24) as *const u64) } != 0;

    let table_count = if uses_xsdt {
        (rsdt_or_xsdt.length - 12) / 8
    } else {
        (rsdt_or_xsdt.length - 12) / 4
    } as usize;

    unsafe {
        let entries_ptr = (rsdt_or_xsdt as *const SdtHeader).add(1) as *const u8;

        for i in 0..table_count {
            let madt_ptr: u64 = if uses_xsdt {
                *(entries_ptr.add(i * 8) as *const u64)
            } else {
                *(entries_ptr.add(i * 4) as *const u32) as u64
            };

            if madt_ptr == 0 {
                continue;
            }

            let header = &*(madt_ptr as *const SdtHeader);
            if header.signature == *b"APIC" {
                parse_madt_entries(madt_ptr);
                MADT_BASE.store(madt_ptr, Ordering::Release);
                MADT_FOUND.store(true, Ordering::Release);
                return true;
            }
        }
    }

    unsafe {
        crate::kernel::klog::klog_info(c"[ACPI] MADT not found in RSDT/XSDT".as_ptr());
    }
    false
}

fn parse_madt_entries(madt_ptr: u64) {
    let madt = unsafe { &*(madt_ptr as *const MadtHeader) };
    let _lapic_base = madt.local_apic_addr as u64;

    let entries_start = madt_ptr as usize + core::mem::size_of::<MadtHeader>();
    let entries_end = madt_ptr as usize + madt.header.length as usize;

    let mut offset = entries_start;
    while offset + 2 <= entries_end {
        let entry = unsafe { &*(offset as *const MadtEntry) };

        match entry.entry_type {
            MADT_TYPE_LAPIC => {
                if offset + core::mem::size_of::<MadtLapic>() > entries_end {
                    break;
                }
                let lapic = unsafe { &*(offset as *const MadtLapic) };
                let idx = AP_COUNT.load(Ordering::Acquire) as usize;
                if idx < MAX_CPUS {
                    let enabled = (lapic.flags & 0x1) != 0;
                    AP_LIST.lock()[idx] = Some(ApInfo {
                        lapic_id: lapic.apic_id as u32,
                        apic_id: lapic.acpi_proc_id as u32,
                        enabled,
                    });
                    AP_COUNT.store((idx + 1) as u32, Ordering::Release);
                }
            }
            MADT_TYPE_IOAPIC => {
                if offset + core::mem::size_of::<MadtIoApic>() > entries_end {
                    break;
                }
                let ioapic = unsafe { &*(offset as *const MadtIoApic) };
                IOAPIC_ADDR.store(ioapic.io_apic_addr as u64, Ordering::Release);
                IOAPIC_GSIB.store(ioapic.global_sys_int_base, Ordering::Release);
            }
            _ => {}
        }

        if entry.length == 0 {
            break;
        }
        offset += entry.length as usize;
    }

    unsafe {
        let _count = AP_COUNT.load(Ordering::Acquire);
        crate::kernel::klog::klog_info(c"[ACPI] MADT: LAPIC base=0xXXXXXXXX, AP count=N".as_ptr());
    }
}

// ============================================================================
// 公共查询 API
// ============================================================================

pub fn get_ap_count() -> u32 {
    AP_COUNT.load(Ordering::Acquire)
}

pub fn get_ap_list() -> [Option<ApInfo>; MAX_CPUS] {
    *AP_LIST.lock()
}

pub fn get_ap(index: usize) -> Option<ApInfo> {
    AP_LIST.lock()[index]
}

pub fn has_madt() -> bool {
    MADT_FOUND.load(Ordering::Acquire)
}

pub fn get_ioapic_addr() -> u64 {
    IOAPIC_ADDR.load(Ordering::Acquire)
}

pub fn get_ioapic_gsib() -> u32 {
    IOAPIC_GSIB.load(Ordering::Acquire)
}

pub fn get_lapic_base() -> u64 {
    if !MADT_FOUND.load(Ordering::Acquire) {
        return 0;
    }
    let madt = unsafe { &*(MADT_BASE.load(Ordering::Acquire) as *const MadtHeader) };
    madt.local_apic_addr as u64
}

#[no_mangle]
pub extern "C" fn acpi_parse_madt(mb2_ptr: u64) -> bool {
    parse_madt(mb2_ptr)
}

#[no_mangle]
pub extern "C" fn acpi_get_ap_count() -> u32 {
    get_ap_count()
}
