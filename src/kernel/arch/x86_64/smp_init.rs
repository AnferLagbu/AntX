//! SMP 初始化 — BSP 侧 AP 启动管理
//!
//! 管理 AP 的发现、资源分配及启动流程。
//！
//! ## 启动流程
//！
//! ```text
//! 1. 分配 per-CPU 内核栈 (16KB each)
//! 2. 拷贝 trampoline 代码到 0x8000
//! 3. 填充 ApStartupInfo (CR3, entry, stack, GDT ptr)
//! 4. 发送 INIT IPI → 等待 10ms → 发送 SIPI (×2)
//! 5. 轮询 AP_INFO_READY 标志 → AP 上线
//! ```

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const TRAMPOLINE_BASE: u64 = 0x8000;
const AP_INFO_OFFSET: u64 = 8; // 2-byte jmp + 6-byte pad to align SINFO
const AP_STACK_SIZE: usize = 16384;

static AP_STARTUP_LOCK: spin::Mutex<()> = spin::Mutex::new(());
static AP_STARTED_COUNT: AtomicU32 = AtomicU32::new(0);
static SMP_FULLY_INITIALIZED: AtomicBool = AtomicBool::new(false);

extern "C" {
    fn ap_trampoline_start();
    fn ap_trampoline_end();
}

/// 每个 AP 的 per-CPU 数据
#[repr(C, align(4096))]
struct ApPerCpu {
    stack: [u8; AP_STACK_SIZE],
}

/// ApStartupInfo 在 0x8000 的内存布局
#[repr(C, packed)]
struct ApStartupInfo {
    cr3: u64,
    entry: u64,
    gdt_limit: u16,
    gdt_base: u64,
    stack: u64,
    lapic_id: u32,
    ready: u32,
    cpu_index: u32,
    _pad: u32,
}

unsafe impl Send for ApPerCpu {}

/// 为每个 AP 分配 per-CPU 数据 (存储原始指针以保持 Copy)
static mut AP_PER_CPU: [Option<*mut ApPerCpu>; super::acpi::MAX_CPUS] =
    [None; super::acpi::MAX_CPUS];

// ============================================================================
// 公开 API
// ============================================================================

pub fn init() {
    super::acpi::parse_madt(0);

    let ap_count = super::acpi::get_ap_count();
    if ap_count <= 1 {
        klog(b"[SMP] No APs found (single-core system)\0");
        return;
    }

    let bsp_lapic_id = super::apic::get_id();

    klog(b"[SMP] Found APs, starting...\0");

    unsafe { copy_trampoline(); }

    let ap_list = super::acpi::get_ap_list();
    let mut cpu_index: u32 = 1;

    for ap in ap_list.iter().flatten() {
        if ap.lapic_id == bsp_lapic_id { continue; }
        if !ap.enabled { continue; }

        unsafe { start_ap(ap.lapic_id, cpu_index); }
        cpu_index += 1;
    }

    AP_STARTED_COUNT.store(cpu_index, Ordering::Release);
    SMP_FULLY_INITIALIZED.store(true, Ordering::Release);

    klog(b"[SMP] All APs started successfully\0");
}

// ============================================================================
// 内部实现
// ============================================================================

unsafe fn copy_trampoline() {
    let src = ap_trampoline_start as *const u8;
    let end = ap_trampoline_end as *const u8;
    let size = end as usize - src as usize;

    let dst = TRAMPOLINE_BASE as *mut u8;

    for i in 0..size {
        dst.add(i).write_volatile(src.add(i).read_volatile());
    }
}

unsafe fn start_ap(lapic_id: u32, cpu_index: u32) {
    if cpu_index as usize >= super::acpi::MAX_CPUS { return; }

    let _lock = AP_STARTUP_LOCK.lock();

    let per_cpu = alloc::boxed::Box::new(ApPerCpu {
        stack: [0u8; AP_STACK_SIZE],
    });
    let stack_top = per_cpu.stack.as_ptr() as u64 + AP_STACK_SIZE as u64;
    AP_PER_CPU[cpu_index as usize] = Some(alloc::boxed::Box::into_raw(per_cpu));

    let cr3_val = crate::kernel::mm::vmm::get_kernel_pml4();
    let gdt_ptr = super::gdt::get_gdt_ptr();
    let entry_addr = ap_entry as *const () as u64;

    let info = (TRAMPOLINE_BASE + AP_INFO_OFFSET) as *mut ApStartupInfo;
    (*info).cr3 = cr3_val;
    (*info).entry = entry_addr;
    (*info).gdt_limit = gdt_ptr.limit;
    (*info).gdt_base = gdt_ptr.base;
    (*info).stack = stack_top;
    (*info).lapic_id = lapic_id;
    (*info).ready = 0;
    (*info).cpu_index = cpu_index;

    // INIT IPI → 10ms → SIPI × 2
    send_init_ipi(lapic_id);
    timer_udelay(10000);
    send_sipi(lapic_id, 0x08);
    timer_udelay(200);
    send_sipi(lapic_id, 0x08);

    // 等待 AP 就绪 (最多 100ms)
    let mut timeout = 1000;
    while timeout > 0 {
        let info_ref = (TRAMPOLINE_BASE + AP_INFO_OFFSET) as *const ApStartupInfo;
        if unsafe { (*info_ref).ready } != 0 {
            break;
        }
        timer_udelay(100);
        timeout -= 1;
    }

    if timeout == 0 {
        klog(b"[SMP] AP startup timed out\0");
    }
}

unsafe fn send_init_ipi(lapic_id: u32) {
    super::apic::apic_write(0x310, (lapic_id & 0xFF) << 24);
    super::apic::apic_write(0x300, (5 << 8) | (1 << 14) | (1 << 15));
}

unsafe fn send_sipi(lapic_id: u32, vector: u8) {
    super::apic::apic_write(0x310, (lapic_id & 0xFF) << 24);
    super::apic::apic_write(0x300, (6 << 8) | (vector as u32));
}

unsafe fn timer_udelay(us: u32) {
    for _ in 0..us {
        core::arch::asm!("out dx, al", in("dx") 0x80u16, in("al") 0u8, options(nomem, nostack));
    }
}

fn klog(msg: &[u8]) {
    unsafe {
        crate::kernel::klog::klog_info(msg.as_ptr() as *const i8);
    }
}

// ============================================================================
// AP Entry Point (由 trampoline 调用)
// ============================================================================

extern "C" fn ap_entry(lapic_id: u32) -> ! {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)); }

    let cpu_index = unsafe {
        (TRAMPOLINE_BASE as *const ApStartupInfo).read_volatile().cpu_index
    };

    super::apic::init();
    super::gdt::gdt_load_on_ap();
    crate::kernel::smp::register_cpu(lapic_id);
    crate::kernel::proc::cpu_queue::init_cpu_queue(cpu_index, 0);

    unsafe { core::arch::asm!("sti", options(nomem, nostack)); }

    loop {
        crate::arch!(halt());
    }
}

// ============================================================================
// FFI 导出
// ============================================================================

#[no_mangle]
pub extern "C" fn smp_init_bsp() {
    init();
}

#[no_mangle]
pub extern "C" fn smp_ready() -> bool {
    SMP_FULLY_INITIALIZED.load(Ordering::Acquire)
}

#[no_mangle]
pub extern "C" fn smp_get_ap_count() -> u32 {
    AP_STARTED_COUNT.load(Ordering::Acquire)
}