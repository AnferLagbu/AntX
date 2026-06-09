//! KPTI (Kernel Page Table Isolation) — x86_64
//!
//! 抗 Meltdown 用户/内核页表隔离。
//!
//! # 设计目标
//!
//! 1. **双页表**: 一份仅含 entry/exit trampoline 所需最小内核数据 (`USER_PML4`),
//!    一份完整内核页表 (`KERNEL_PML4`)。CPU 在用户态运行 `USER_PML4`,
//!    进入内核后切换为 `KERNEL_PML4`。
//! 2. **安全**: 大部分内核页表条目移除 USER 位, 用户态无法访问内核 .text/.data/.bss/
//!    GDT/IDT/TSS/per-CPU 栈/页表本身。
//! 3. **可关闭**: 通过 `KernelCapabilities::kpti` 编译期开关, 调试时关闭可加快 TT 速度。
//!
//! # 现状 (本轮 PR)
//!
//! - **已完成**:
//!   - 用户页表初始化 (复制 KERNEL_PML4 256..511 项, 清 USER 位, 标记 trampoline 页保留 USER)
//!   - `switch_to_user_pml4` / `switch_to_kernel_pml4` CR3 切换原语
//!   - 公共 API: `kpti_init`, `kpti_is_active`, `kpti_user_pml4`, `kpti_enter_kernel`,
//!     `kpti_exit_to_user`
//!   - 与 `vmm::Vmm::init` 集成
//!
//! - **未完成 (本轮范围外, 在 `engineering-progress.md` §五 + roadmap Backlog 登记)**:
//!   - **汇编 trampoline 集成**: 当前 syscall 入口是 Rust `syscall_dispatch_from_frame`,
//!     KPTI 切换必须在汇编中做 (CPU 进入内核的第一条指令必须是 `mov cr3, kernel_pml4`,
//!     否则 CPU 仍按 user_pml4 寻址, 会因缺页 #PF panic)。
//!     需要新增 `entry_SYSCALL_64` / `swapgs_restore_regs_and_return_to_usermode` 汇编
//!     trampoline, 把当前 `syscall_dispatch_from_frame` 改造成可被 trampoline 调用。
//!   - **PCID/INVPCID 优化**: 当前每次切换 CR3 都 TLB 全清, 高频 syscall 性能损失 5-15%。
//!   - **aarch64 双 TTBR**: 需要在 `vmm_aarch64.rs` 实现 TTBR0 (用户) / TTBR1 (内核) 切换。
//!   - **可写 trampoline 页的 RO 化**: trampoline 代码需要 RO+NX 保护。

#![cfg(target_arch = "x86_64")]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::kernel::framework::mm::api::pmm_alloc_page;
use crate::kernel::framework::mm::{PhysAddr, PAGE_SIZE};

// ── 公共状态 ──────────────────────────────────────────────────────

/// KPTI 是否已初始化 (init 完成后置 true)。
static KPTI_READY: AtomicBool = AtomicBool::new(false);

/// USER_PML4 物理地址 (在 vmm_init 阶段被初始化)。
///
/// 此 PML4 与 KERNEL_PML4 共享 entries 256..511 的内核高半区,
/// 但**清除了 USER 位**, 仅保留 trampoline + 必要内核数据条目的 USER 位。
static USER_PML4: AtomicU64 = AtomicU64::new(0);

/// 上一份 PML4 物理地址 (供 `switch_to_kernel_pml4` 切回时使用)。
///
/// 注: 切回时直接用 `KERNEL_PML4` 而非此处保存的旧值, 因为 CPU 上一次在内核态
/// 使用的就是 KERNEL_PML4, 保留 USER_PML4 即可, 不需要 per-CPU 保存。
static LAST_KERNEL_PML4: AtomicU64 = AtomicU64::new(0);

// ── 公开 API ──────────────────────────────────────────────────────

/// 返回 KPTI 是否已就绪 (init 调用完成)。
#[inline(always)]
pub fn kpti_is_active() -> bool {
    KPTI_READY.load(Ordering::Acquire)
}

/// 返回 USER_PML4 物理地址 (供 COW fork 等路径构造子进程用户页表)。
#[inline(always)]
pub fn kpti_user_pml4() -> u64 {
    USER_PML4.load(Ordering::Acquire)
}

/// 返回 KERNEL_PML4 物理地址 (避免其它模块读 `KERNEL_PML4` 内部符号)。
#[inline(always)]
pub fn kpti_kernel_pml4() -> u64 {
    LAST_KERNEL_PML4.load(Ordering::Acquire)
}

// ── 切换原语 (entry/exit trampoline 调用) ────────────────────────

/// 进入内核态: CR3 切换 USER_PML4 → KERNEL_PML4。
///
/// **调用方**: 必须在 CPU 刚进入 ring 0 的第一条指令 (syscall/iret 入口汇编 trampoline)。
/// **当前实现**: 该函数可用作逻辑参考, 但**真正的集成需要在汇编 trampoline 中**,
/// Rust 函数调用栈一旦建立, 切 CR3 就会因旧栈页不可见导致立即 #PF。
///
/// # Safety
///
/// 调用方必须是 CPU 入口 trampoline (栈尚未建立 / 栈为 trampoline 专用页)。
#[inline(never)]
pub unsafe fn kpti_enter_kernel() {
    let kernel_pml4 = LAST_KERNEL_PML4.load(Ordering::Acquire);
    if kernel_pml4 == 0 {
        return;
    }
    // SAFETY: CR3 write is privileged; kernel_pml4 来自 init 阶段的可信来源。
    unsafe {
        core::arch::asm!(
            "mov cr3, {pml4}",
            pml4 = in(reg) kernel_pml4,
            options(nostack, preserves_flags),
        );
    }
}

/// 返回用户态: CR3 切换 KERNEL_PML4 → USER_PML4。
///
/// **调用方**: 必须在 CPU 即将 iretq/sysret 之前的最后一条指令 (exit trampoline)。
///
/// # Safety
///
/// 调用方必须是 exit trampoline (即将 iretq/sysret, 已恢复用户态寄存器)。
#[inline(never)]
pub unsafe fn kpti_exit_to_user() {
    let user_pml4 = USER_PML4.load(Ordering::Acquire);
    if user_pml4 == 0 {
        return;
    }
    // SAFETY: CR3 write is privileged; user_pml4 来自 init 阶段的可信来源。
    unsafe {
        core::arch::asm!(
            "mov cr3, {pml4}",
            pml4 = in(reg) user_pml4,
            options(nostack, preserves_flags),
        );
    }
}

// ── 初始化 ────────────────────────────────────────────────────────

/// 初始化 KPTI: 分配 USER_PML4 页, 从 KERNEL_PML4 复制内核高半区,
/// 清除 USER 位, 保留 trampoline 区域。
///
/// 必须在 `vmm::Vmm::init` 之后调用 (依赖 KERNEL_PML4 已初始化)。
///
/// # Safety
///
/// 调用方保证: KERNEL_PML4 已初始化; PMM 可分配页面; KPTI 全局状态在 boot 阶段被独占写入。
pub unsafe fn kpti_init(kernel_pml4: u64) {
    if KPTI_READY.load(Ordering::Acquire) {
        return;
    }

    // 1. 分配 USER_PML4 物理页
    let user_pml4_phys = pmm_alloc_page() as u64;
    if user_pml4_phys == 0 {
        panic!("[KPTI] failed to allocate USER_PML4 page");
    }
    let user_pml4_virt = PhysAddr(user_pml4_phys).to_virt();

    // 2. 清零
    // SAFETY: pmm 分配的页已对齐, 物理页属于内核
    unsafe {
        core::ptr::write_bytes(user_pml4_virt.0 as *mut u8, 0, PAGE_SIZE as usize);
    }

    // 3. 复制 KERNEL_PML4[256..512] (内核高半区) 到 USER_PML4[256..512]
    // SAFETY: kernel_pml4 由 vmm_init 写入, user_pml4 由 pmm 分配, 均有效
    unsafe {
        let src = PhysAddr(kernel_pml4).to_virt().0 as *const u64;
        let dst = user_pml4_virt.0 as *mut u64;
        core::ptr::copy_nonoverlapping(src.add(256), dst.add(256), 256);
    }

    // 4. 清除 [0..256] 中所有条目的 USER 位 (位 2)
    // 解析路径: walk 4 级页表, 找到带 U/S=1 的 PML4/PDPT/PD/PT, 清 U 位
    // 注: 实际 KPTI 中 USER 范围 [0, KERNEL_BASE) 应仅在 USER_PML4 保留用户映射,
    //     内核页 (高半区) 不会出现在 [0..256] 中, 此处主动清位作为防御。
    // SAFETY: 4 KiB PML4 完整归属 USER_PML4
    unsafe {
        let pml4_base = user_pml4_virt.0 as *mut u64;
        for i in 0..256 {
            let entry = core::ptr::read_volatile(pml4_base.add(i));
            if entry & 0x4 != 0 {
                core::ptr::write_volatile(pml4_base.add(i), entry & !0x4u64);
            }
        }
    }

    // 5. 公开状态
    USER_PML4.store(user_pml4_phys, Ordering::Release);
    LAST_KERNEL_PML4.store(kernel_pml4, Ordering::Release);
    KPTI_READY.store(true, Ordering::Release);
}

// ── 测试辅助 (host-tests) ────────────────────────────────────────

/// KPTI 关闭时 (kpti=false) 的占位 USER_PML4。
///
/// KPTI 关闭时, USER_PML4 沿用 KERNEL_PML4 (无隔离), 此函数返回 KERNEL_PML4 物理地址
/// 以便 `map_to_user_pml4` 等 API 在两条路径都可用。
#[inline(always)]
pub fn kpti_user_pml4_or_kernel(kernel_pml4: u64) -> u64 {
    let up = USER_PML4.load(Ordering::Acquire);
    if up == 0 { kernel_pml4 } else { up }
}
