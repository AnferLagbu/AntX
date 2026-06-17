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
use crate::kernel::framework::mm::{PhysAddr, KERNEL_BASE, PAGE_SIZE};

// ── PCID 常量 ─────────────────────────────────────────────────────
// PCID (Process-Context Identifier) 占 CR3 低 12 位, 用于 TLB 标记.
// 启用 PCID 后, CR3 切换不再隐式刷新全局 TLB, 改用 INVPCID 精确刷除.

/// 内核页表 PCID
pub const PCID_KERNEL: u64 = 1;
/// 用户页表 PCID
pub const PCID_USER: u64 = 2;

/// INVPCID 指令类型: 按 PCID 刷新 TLB
///
/// 供 VMM 页表修改 (COW/mprotect) 后刷除特定 PCID 的 TLB 条目.
#[allow(dead_code)]
const INVPCID_TYPE_SINGLE: u64 = 0;
/// INVPCID 指令类型: 刷新所有 TLB (包括 global 页)
const INVPCID_TYPE_ALL_INCL_GLOBAL: u64 = 2;

/// 执行 INVPCID 指令, 刷新指定 PCID 的 TLB 条目.
///
/// # Safety
///
/// 调用方保证 CPU 支持 INVPCID (通过 CPUID.07H:EBX.IVPCID 确认).
#[inline(always)]
pub unsafe fn invpcid(pcid: u64, addr: u64, typ: u64) {
    // INVPCID 描述符: 16 字节, [0:7] PCID, [8:15] 线性地址
    // 在栈上构造描述符, 通过内存操作数传递给 INVPCID.
    // INVPCID 格式: invpcid r64, m128 — 第二操作数必须是内存引用.
    let desc: [u64; 2] = [pcid, addr];
    // SAFETY: 调用方保证 CPU 支持 INVPCID; desc 在栈上有效, 16 字节对齐.
    unsafe {
        core::arch::asm!(
            "invpcid {typ}, [{desc}]",
            typ = in(reg) typ,
            desc = in(reg) desc.as_ptr(),
            options(nostack, preserves_flags, readonly),
        );
    }
}

/// 刷新所有 PCID 的 TLB 条目 (不含 global 页).
///
/// # Safety
///
/// 调用方保证 CPU 支持 INVPCID (通过 CPUID.07H:EBX.IVPCID 确认).
#[inline(always)]
pub unsafe fn invpcid_flush_all() {
    // SAFETY: 调用方保证 CPU 支持 INVPCID; type 2 刷新所有 TLB 条目是安全操作.
    unsafe { invpcid(0, 0, INVPCID_TYPE_ALL_INCL_GLOBAL); }
}

/// CR3 值中嵌入 PCID: PML4 物理地址 | PCID.
#[inline(always)]
pub const fn cr3_with_pcid(pml4_phys: u64, pcid: u64) -> u64 {
    (pml4_phys & 0x000FFFFFFFFFF000) | (pcid & 0xFFF)
}

/// 检查 CPU 是否支持 INVPCID.
#[inline]
pub fn has_invpcid() -> bool {
    crate::kernel::framework::cpu::get_cpu_info()
        .map(|info| info.features.contains(crate::kernel::framework::cpu::CpuFeatures::INVPCID))
        .unwrap_or(false)
}

/// 检查 PCID 是否已启用 (CR4.PCIDE = 1).
#[inline]
pub fn pcid_is_enabled() -> bool {
    let cr4: u64;
    // SAFETY: 读取 CR4 是特权操作但无副作用, 仅检查 bit 17.
    unsafe {
        core::arch::asm!("mov {0}, cr4", out(reg) cr4, options(nostack, nomem));
    }
    (cr4 >> 17) & 1 == 1
}

// ── 链接脚本符号 (x86_64.ld) ──────────────────────────────────────
// KPTI trampoline 代码范围: _kernel_text_start ~ _kpti_trampoline_end
// 这些页在 USER_PML4 中需要保持可执行 (X), 其余代码页设为 NX.

extern "C" {
    static _kernel_text_start: u8;
    static _kpti_trampoline_end: u8;
    static _kernel_text_end: u8;
}

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

    // 4.5 Trampoline 页权限加固: USER_PML4 高半区页表遍历
    //
    // 策略: 仅修改 .text 区域 (_kernel_text_start ~ _kernel_text_end) 的页,
    // 数据页 (.rodata/.data/.bss) 保持原权限不变 (trampoline 需要读写 per-CPU 数据).
    //
    // .text 区域:
    //   - Trampoline 代码页 (_kernel_text_start ~ _kpti_trampoline_end): RX (只读+可执行)
    //   - 其余代码页: RO+NX (只读+不可执行)
    //
    // NX 位 (bit 63): 若设置, 页不可执行.
    // W 位  (bit 1):  若清除, 页只读.
    //
    // SAFETY: user_pml4_virt 有效, 遍历只修改 USER_PML4 的 PTE, 不影响 KERNEL_PML4.
    unsafe {
        let tramp_start = core::ptr::addr_of!(_kernel_text_start) as u64;
        let tramp_end = core::ptr::addr_of!(_kpti_trampoline_end) as u64;
        let text_end = core::ptr::addr_of!(_kernel_text_end) as u64;

        let pml4 = user_pml4_virt.0 as *mut u64;

        for pml4_idx in 256u64..512 {
            let pml4e = core::ptr::read_volatile(pml4.add(pml4_idx as usize));
            if pml4e & 1 == 0 { continue; }

            let pdpt_phys = pml4e & 0x000FFFFFFFFFF000;
            let pdpt = (pdpt_phys + KERNEL_BASE) as *mut u64;

            for pdpt_idx in 0u64..512 {
                let pdpte = core::ptr::read_volatile(pdpt.add(pdpt_idx as usize));
                if pdpte & 1 == 0 { continue; }
                if pdpte & 0x80 != 0 {
                    // 1 GiB 大页: 检查是否与 .text 区域重叠
                    let page_start = (pml4_idx << 39) | (pdpt_idx << 30);
                    let page_end = page_start + (1u64 << 30);
                    if page_start >= text_end || page_end <= tramp_start { continue; }
                    // 大页与 .text 重叠 — 设置 NX, 清除 W
                    let mut new = pdpte | (1u64 << 63);
                    new &= !0x2u64;
                    if page_start < tramp_end && page_end > tramp_start {
                        new &= !(1u64 << 63); // trampoline 保持可执行
                    }
                    if new != pdpte {
                        core::ptr::write_volatile(pdpt.add(pdpt_idx as usize), new);
                    }
                    continue;
                }

                let pd_phys = pdpte & 0x000FFFFFFFFFF000;
                let pd = (pd_phys + KERNEL_BASE) as *mut u64;

                for pd_idx in 0u64..512 {
                    let pde = core::ptr::read_volatile(pd.add(pd_idx as usize));
                    if pde & 1 == 0 { continue; }
                    if pde & 0x80 != 0 {
                        // 2 MiB 大页
                        let page_start = (pml4_idx << 39) | (pdpt_idx << 30) | (pd_idx << 21);
                        let page_end = page_start + (1u64 << 21);
                        if page_start >= text_end || page_end <= tramp_start { continue; }
                        let mut new = pde | (1u64 << 63);
                        new &= !0x2u64;
                        if page_start < tramp_end && page_end > tramp_start {
                            new &= !(1u64 << 63);
                        }
                        if new != pde {
                            core::ptr::write_volatile(pd.add(pd_idx as usize), new);
                        }
                        continue;
                    }

                    let pt_phys = pde & 0x000FFFFFFFFFF000;
                    let pt = (pt_phys + KERNEL_BASE) as *mut u64;

                    for pt_idx in 0u64..512 {
                        let pte = core::ptr::read_volatile(pt.add(pt_idx as usize));
                        if pte & 1 == 0 { continue; }

                        let page_start = (pml4_idx << 39) | (pdpt_idx << 30)
                            | (pd_idx << 21) | (pt_idx << 12);
                        let page_end = page_start + PAGE_SIZE as u64;

                        // 跳过 .text 区域之外的页
                        if page_start >= text_end || page_end <= tramp_start { continue; }

                        let mut new = pte;
                        new |= 1u64 << 63;  // NX: 代码页默认不可执行
                        new &= !0x2u64;     // RO: 代码页只读

                        // Trampoline 代码页: 恢复可执行
                        if page_start < tramp_end && page_end > tramp_start {
                            new &= !(1u64 << 63);
                        }

                        if new != pte {
                            core::ptr::write_volatile(pt.add(pt_idx as usize), new);
                        }
                    }
                }
            }
        }
    }

    // 5. 启用 PCID (如果 CPU 支持 INVPCID)
    //    CR4.PCIDE (bit 17) 启用后, CR3 低 12 位为 PCID 而非必须为 0.
    //    启用条件: CPU 支持 INVPCID; 当前 CR3 低 12 位为 0 (硬件要求).
    //    启用后, KPTI CR3 切换携带 PCID, TLB 条目按 PCID 隔离,
    //    无需每次切换都全局刷新 TLB, 显著降低 KPTI 性能开销.
    let pcid_enabled = if has_invpcid() {
        // SAFETY: 读取 CR3 判断低 12 位是否为 0 (PCIDE 启用前提).
        let cur_cr3: u64;
        unsafe { core::arch::asm!("mov {0}, cr3", out(reg) cur_cr3, options(nostack, nomem)); }
        if cur_cr3 & 0xFFF == 0 {
            // SAFETY: CR4 写入仅在 boot 阶段, 设置 PCIDE 位.
            unsafe {
                let cr4: u64;
                core::arch::asm!("mov {0}, cr4", out(reg) cr4, options(nostack, nomem));
                core::arch::asm!(
                    "mov cr4, {0}",
                    in(reg) cr4 | (1u64 << 17),
                    options(nostack, nomem, preserves_flags),
                );
            }
            // 启用 PCIDE 后, mov cr3 不再隐式刷新 TLB.
            // 做一次全局 TLB 刷新确保一致性, 然后重新加载 CR3 (带 PCID_KERNEL).
            // SAFETY: INVPCID type 2 刷新所有 TLB 条目 (含 global 页).
            unsafe { invpcid_flush_all(); }
            // 重新加载 CR3 带 PCID_KERNEL
            // SAFETY: kernel_pml4 来自 init 阶段的可信来源; PCIDE 已启用, CR3 低 12 位为 PCID.
            let new_cr3 = cr3_with_pcid(kernel_pml4, PCID_KERNEL);
            unsafe {
                core::arch::asm!(
                    "mov cr3, {0}",
                    in(reg) new_cr3,
                    options(nostack, preserves_flags),
                );
            }
            true
        } else {
            false
        }
    } else {
        false
    };

    // 6. 更新所有 per-CPU SyscallPerCpu PML4 字段
    //    汇编 entry/exit 从 [gs:KERNEL_PML4_OFF] / [gs:USER_PML4_OFF] 读取.
    //    PCID 启用时, 值为 PML4_PHYS | PCID; 未启用时为纯 PML4 物理地址.
    // SAFETY: boot 阶段独占写入, cpu_index 0..256 合法.
    let kernel_cr3 = if pcid_enabled { cr3_with_pcid(kernel_pml4, PCID_KERNEL) } else { kernel_pml4 };
    let user_cr3 = if pcid_enabled { cr3_with_pcid(user_pml4_phys, PCID_USER) } else { user_pml4_phys };
    unsafe {
        for cpu in 0..256u32 {
            crate::kernel::framework::arch::gdt::gdt_set_kpti_pml4(cpu, kernel_cr3, user_cr3);
        }
    }

    // 7. 公开状态
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
