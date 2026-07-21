//! IOAPIC (I/O 高级可编程中断控制器) 驱动
//!
//! 将外部硬件中断路由到指定 CPU 核心。

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const IOAPIC_BASE_DEFAULT: u64 = 0xFEC00000;

const IOREGSEL: u32 = 0x00;
const IOWIN: u32 = 0x10;

const IOAPIC_VER: u32 = 0x01;
/// IOAPIC ID 寄存器 (读写, bit[24:27] 为 APIC ID)
const IOAPIC_ID: u32 = 0x00;
/// IOAPIC 仲裁 ID 寄存器 (只读, bit[24:27] 为仲裁 ID)
const IOAPIC_ARB: u32 = 0x02;
const IOREDTBL_BASE: u32 = 0x10;

const REDTBL_MASK: u64 = 1 << 16;
const REDTBL_LEVEL: u64 = 1 << 15;

// IOAPIC 投递模式 (重定向表 bit[8:10], 与 Local APIC LVT 编码一致)
const DELIVERY_FIXED: u64 = 0x000;
/// 最低优先级投递 — 同一 APIC ID 组中选择最低优先级 CPU
const DELIVERY_LOWEST: u64 = 0x100;
/// 系统管理中断 — 固件与 OS 通信机制
const DELIVERY_SMI: u64 = 0x200;
/// 不可屏蔽中断 — 硬件错误报告 (ECC 内存错误、看门狗超时)
const DELIVERY_NMI: u64 = 0x400;
/// INIT 投递 — 用于处理器初始化序列
const DELIVERY_INIT: u64 = 0x500;
/// 8259A 兼容中断 — 遗留 ISA 设备 (ExtINT 需配合 Local APIC LINT0)
const DELIVERY_EXTINT: u64 = 0x700;

static IOAPIC_BASE: AtomicU64 = AtomicU64::new(0);
static IOAPIC_INITIALIZED: AtomicBool = AtomicBool::new(false);
static IOAPIC_MAX_IRQ: AtomicU64 = AtomicU64::new(0);

fn ioapic_read(reg: u32) -> u32 {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::ptr::write_volatile((base + IOREGSEL as u64) as *mut u32, reg);
        core::ptr::read_volatile((base + IOWIN as u64) as *const u32)
    }
}

fn ioapic_write(reg: u32, value: u32) {
    let base = IOAPIC_BASE.load(Ordering::Acquire);
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
    unsafe {
        core::ptr::write_volatile((base + IOREGSEL as u64) as *mut u32, reg);
        core::ptr::write_volatile((base + IOWIN as u64) as *mut u32, value);
    }
}

fn ioapic_read_redirection(irq: u8) -> u64 {
    let reg = IOREDTBL_BASE + (irq as u32) * 2;
    let low = ioapic_read(reg);
    let high = ioapic_read(reg + 1);
    ((high as u64) << 32) | (low as u64)
}

fn ioapic_write_redirection(irq: u8, value: u64) {
    let reg = IOREDTBL_BASE + (irq as u32) * 2;
    ioapic_write(reg, value as u32);
    ioapic_write(reg + 1, (value >> 32) as u32);
}

pub fn init(base_addr: u64) {
    let base = if base_addr == 0 {
        IOAPIC_BASE_DEFAULT
    } else {
        base_addr
    };
    IOAPIC_BASE.store(base, Ordering::Release);

    let ver = ioapic_read(IOAPIC_VER);
    let max_irq = ((ver >> 16) & 0xFF) as u64 + 1;
    IOAPIC_MAX_IRQ.store(max_irq, Ordering::Release);

    for irq in 0..24u8 {
        ioapic_write_redirection(irq, REDTBL_MASK | DELIVERY_FIXED | (irq as u64 + 32));
    }

    IOAPIC_INITIALIZED.store(true, Ordering::Release);
}

pub fn is_initialized() -> bool {
    IOAPIC_INITIALIZED.load(Ordering::Acquire)
}

pub fn get_max_irq() -> u8 {
    IOAPIC_MAX_IRQ.load(Ordering::Acquire) as u8
}

pub fn set_irq(irq: u8, vector: u8, apic_id: u8, masked: bool) {
    set_irq_with_mode(irq, vector, apic_id, masked, DELIVERY_FIXED);
}

/// 设置 IRQ 的投递模式、vector 和目标 APIC ID
///
/// # 参数
/// - `irq`: IOAPIC IRQ 编号 (0-23)
/// - `vector`: 中断向量号 (0x20-0xFF)
/// - `apic_id`: 目标 Local APIC ID
/// - `masked`: 是否屏蔽此 IRQ
/// - `mode`: 投递模式 (DELIVERY_FIXED/SMI/NMI/EXTINT 等)
pub fn set_irq_with_mode(irq: u8, vector: u8, apic_id: u8, masked: bool, mode: u64) {
    if !is_initialized() {
        return;
    }
    let mut entry: u64 = vector as u64 | mode | ((apic_id as u64) << 56);
    if masked {
        entry |= REDTBL_MASK;
    }
    ioapic_write_redirection(irq, entry);
}

pub fn mask_irq(irq: u8) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    ioapic_write_redirection(irq, entry | REDTBL_MASK);
}

pub fn unmask_irq(irq: u8) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    ioapic_write_redirection(irq, entry & !REDTBL_MASK);
}

pub fn set_irq_level(irq: u8, level_triggered: bool) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    if level_triggered {
        ioapic_write_redirection(irq, entry | REDTBL_LEVEL);
    } else {
        ioapic_write_redirection(irq, entry & !REDTBL_LEVEL);
    }
}

pub fn route_irq_to_cpu(irq: u8, apic_id: u8) {
    if !is_initialized() {
        return;
    }
    let entry = ioapic_read_redirection(irq);
    let new_entry = (entry & !(0xFFu64 << 56)) | ((apic_id as u64) << 56);
    ioapic_write_redirection(irq, new_entry);
}

/// 返回 Fixed 投递模式常量
pub fn delivery_fixed() -> u64 { DELIVERY_FIXED }
/// 返回 Lowest Priority 投递模式常量
pub fn delivery_lowest() -> u64 { DELIVERY_LOWEST }
/// 返回 SMI 投递模式常量
pub fn delivery_smi() -> u64 { DELIVERY_SMI }
/// 返回 NMI 投递模式常量
pub fn delivery_nmi() -> u64 { DELIVERY_NMI }
/// 返回 INIT 投递模式常量
pub fn delivery_init() -> u64 { DELIVERY_INIT }
/// 返回 ExtINT 投递模式常量 (8259A 兼容)
pub fn delivery_extint() -> u64 { DELIVERY_EXTINT }

// ============================================================================
// IOAPIC ID / 仲裁 API (多 IOAPIC 支持)
// ============================================================================

/// 读取当前 IOAPIC 的 ID (bit[24:27])
///
/// 多 IOAPIC 系统中, 每个 IOAPIC 有唯一 ID, 用于中断路由决策.
/// 单 IOAPIC 系统中返回该唯一控制器的 ID.
pub fn get_id() -> u8 {
    if !is_initialized() { return 0; }
    let val = ioapic_read(IOAPIC_ID);
    ((val >> 24) & 0x0F) as u8
}

/// 设置当前 IOAPIC 的 ID (bit[24:27])
///
/// # Safety
///
/// 多 IOAPIC 系统中, ID 冲突会导致中断路由错误.
/// 仅在 MADT 枚举阶段由初始化代码调用.
pub fn set_id(id: u8) {
    if !is_initialized() { return; }
    let val = ioapic_read(IOAPIC_ID);
    let new_val = (val & !(0x0F << 24)) | ((id as u32 & 0x0F) << 24);
    ioapic_write(IOAPIC_ID, new_val);
}

/// 读取当前 IOAPIC 的仲裁 ID (只读, bit[24:27])
///
/// 仲裁 ID 由硬件固定, 用于多 IOAPIC 中断分配时的优先级仲裁.
pub fn get_arbitration_id() -> u8 {
    if !is_initialized() { return 0; }
    let val = ioapic_read(IOAPIC_ARB);
    ((val >> 24) & 0x0F) as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn ioapic_init(base_addr: u64) {
    init(base_addr);
}
#[unsafe(no_mangle)]
pub extern "C" fn ioapic_mask_irq(irq: u8) {
    mask_irq(irq);
}
#[unsafe(no_mangle)]
pub extern "C" fn ioapic_unmask_irq(irq: u8) {
    unmask_irq(irq);
}
#[unsafe(no_mangle)]
pub extern "C" fn ioapic_set_irq(irq: u8, vector: u8, apic_id: u8, masked: bool) {
    set_irq(irq, vector, apic_id, masked);
}
#[unsafe(no_mangle)]
pub extern "C" fn ioapic_set_irq_with_mode(
    irq: u8,
    vector: u8,
    apic_id: u8,
    masked: bool,
    mode: u64,
) {
    set_irq_with_mode(irq, vector, apic_id, masked, mode);
}
#[unsafe(no_mangle)]
pub extern "C" fn ioapic_route_irq_to_cpu(irq: u8, apic_id: u8) {
    route_irq_to_cpu(irq, apic_id);
}
