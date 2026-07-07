use super::process::{FdTable, Process, PROCESS_TABLE};
use super::types::{ProcessContext, ProcessId, ProcessPriority, ProcessState};
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::kernel::framework::mm::KERNEL_BASE;
use crate::kernel::framework::sync::IrqSpinLock as Mutex;
use crate::klog_error;

#[unsafe(no_mangle)]
pub static user_entry_cr3: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
pub static user_entry_target: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" {
    fn pmm_alloc_page() -> *mut u8;
    fn pmm_alloc_pages(count: u64) -> *mut u8;
    fn pmm_free_page(page: *mut u8);
    fn vmm_create_user_page_table() -> u64;
    fn vmm_map_page_in_table(table: u64, vaddr: u64, paddr: u64, flags: u64);
    fn vmm_map_page(vaddr: u64, paddr: u64, flags: u64) -> i32;
    #[allow(dead_code)] // 待大页分裂路径启用后使用。
    fn vmm_split_2mb_page(vaddr: u64) -> i32;
    fn vmm_ensure_path_user(vaddr: u64);
    #[allow(dead_code)] // 待进程切换路径启用后使用。
    fn vmm_switch_page_table(table: u64);
    fn vmm_destroy_page_table(cr3: u64);
    fn vmm_get_physical_in_table(table: u64, vaddr: u64) -> u64;
    fn memset(s: *mut u8, c: i32, n: u64);
    fn memcpy(dest: *mut u8, src: *const u8, n: u64);
    fn kmalloc(size: u64) -> *mut u8;
}

/// 进程/调度规模常量 — 统一从 `super::types` 引用
///
/// 全部进程规模/栈/调度参数已在 `types.rs` 集中 re-export, 本文件仅 `use` 它们,
/// 避免分散定义与影子覆盖问题。
pub use super::types::{
    PAGE_SIZE,
    MAX_PROCESSES, MAX_OPEN_FILES,
    KERNEL_STACK_SIZE, USER_KSTACK_SIZE,
    USER_STACK_SIZE, USER_STACK_GUARD, USER_STACK_TOP, USER_STACK_MAX_SIZE, USER_CODE_BASE,
    SCHED_BOOST_INTERVAL,
    SCHED_LEVEL_0_QUANTUM, SCHED_LEVEL_1_QUANTUM, SCHED_LEVEL_2_QUANTUM, SCHED_LEVEL_3_QUANTUM,
    SCHED_RT_WATCHDOG_TICKS,
};

/// 派生常量: 用户栈自动扩展的下界 (USER_STACK_TOP - USER_STACK_MAX_SIZE)
pub const USER_STACK_EXPAND_LIMIT: u64 = USER_STACK_TOP - USER_STACK_MAX_SIZE;

/// PAGE_PRESENT / WRITABLE / USER — 旧式裸 u64 常量 (保留以兼容 C 端)
/// 业务层推荐使用 `framework::mm::PageFlags` 类型化抽象, FFI 边界通过 `.bits()` 转换.
#[deprecated(note = "use framework::mm::PageFlags 替代 (类型安全 + 编译期检查)")]
pub const PAGE_PRESENT: u64 = 1;
#[deprecated(note = "use framework::mm::PageFlags::WRITABLE 替代")]
pub const PAGE_WRITABLE: u64 = 2;
#[deprecated(note = "use framework::mm::PageFlags::USER 替代")]
pub const PAGE_USER: u64 = 4;

/// 类型化页面标志 (从 framework::mm 引入, 在 FFI 边界通过 .bits() 转 u64)
use crate::kernel::framework::mm::PageFlags;

pub const GDT_USER_DATA: u64 = 0x18;
pub const GDT_USER_CODE: u64 = 0x20;

pub const PT_LOAD: u32 = 1;

/// ELF 头部 / 程序头 — 重导出 elf.rs canonical 定义
///
/// 使用 `framework::proc::elf::Elf64Header` 作为唯一权威, 避免重复定义
/// 引起字段名不一致 (`machine` vs `e_machine` 等).
pub use super::elf::{Elf64Header as ElfHeader, Elf64Phdr as ElfPhdr};

#[repr(C)]
pub struct UserProcInfo {
    pub entry: u64,
    pub name: [u8; 64],
    pub code_size: u64,
    pub code_data: *const u8,
}

// === 特权层: UserProcess 裸指针/FFI 桥接集中地 ===
//
// 本子模块包含所有与裸指针 (`*mut UserProcess`)、extern "C" FFI 调用
// (kmalloc/memset/memcpy/vmm_*) 相关的 `unsafe` 代码。
//
// 上层 `user_proc.rs` 业务逻辑 (create/enter/setup_user_stack/load_elf_from_memory 等)
// 通过 `raw::UserProcRef` newtype 安全访问 UserProcess 字段, 保持 100% safe Rust。
//
// `unsafe impl Send/Sync` 在本子模块中声明, 因为类型定义本身需要这些 trait
// 才能被 `static USER_PROC_MANAGER` 使用。
pub(crate) mod raw {
    use super::*;

    // === UserProcess 安全访问封装 (Framekernel privilege wrapper) ===
    //
    // `*mut UserProcess` 在用户进程管理中作为索引句柄使用。将其封装为 `UserProcRef`
    // newtype 后, 所有 `unsafe { (*ptr).field }` 集中在 `UserProcRef` 内部方法中。
    //
    // # SAFETY invariant
    // - 调用方必须保证 `*mut UserProcess` 指向一个有效的 `UserProcess` 分配 (kmalloc)。
    // - 通过 USER_PROC_MANAGER.processes BTreeMap 持有的 NonNull 句柄都是有效的。
    #[derive(Clone, Copy)]
    pub struct UserProcRef(*mut UserProcess);

    impl UserProcRef {
        /// 从裸指针构造, 要求调用方提供 SAFETY 保证。
        ///
        /// # Safety
        /// - `ptr` 必须为非空, 指向有效 `UserProcess` 分配
        /// - 在 `UserProcRef` 存活期间, 不会被释放
        #[inline(always)]
        pub unsafe fn new_unchecked(ptr: *mut UserProcess) -> Self {
            Self(ptr)
        }

        #[allow(dead_code)] // 待进程诊断路径启用后使用。
        #[inline(always)]
        pub fn as_ptr(self) -> *mut UserProcess {
            self.0
        }

        /// 访问 pid 字段 (读写)
        #[inline(always)]
        pub fn pid(&self) -> u32 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).pid }
        }

        #[inline(always)]
        pub fn set_pid(&self, v: u32) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).pid = v;
            }
        }

        /// 访问 entry 字段 (读写)
        #[inline(always)]
        pub fn entry(&self) -> u64 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).entry }
        }

        #[inline(always)]
        pub fn set_entry(&self, v: u64) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).entry = v;
            }
        }

        /// 访问 create_time 字段 (读写, 待进程统计路径启用后使用)。
        #[inline(always)]
        #[allow(dead_code)] // 待进程统计路径启用后使用。
        pub fn create_time(&self) -> u64 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).create_time }
        }

        #[inline(always)]
        pub fn set_create_time(&self, v: u64) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).create_time = v;
            }
        }

        /// 访问 pwm/cr3/kernel_stack/user_stack/stack_bottom/state 原子字段
        #[inline(always)]
        pub fn load_pwm(&self) -> u64 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).pwm.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_pwm(&self, v: u64) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).pwm.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_cr3(&self) -> u64 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).cr3.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_cr3(&self, v: u64) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).cr3.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_kernel_stack(&self) -> u64 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).kernel_stack.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_kernel_stack(&self, v: u64) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).kernel_stack.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_user_stack(&self) -> u64 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).user_stack.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_user_stack(&self, v: u64) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).user_stack.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_stack_bottom(&self) -> u64 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).stack_bottom.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_stack_bottom(&self, v: u64) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).stack_bottom.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        #[allow(dead_code)] // 待进程状态查询路径启用后使用。
        pub fn load_state(&self) -> u32 {
            // SAFETY: `self` 由调用方保证为有效指针; 只读访问
            unsafe { (*self.0).state.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_state(&self, v: u32) {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            unsafe {
                (*self.0).state.store(v, Ordering::SeqCst);
            }
        }
    }

    /// 在 BTreeMap 中按 pid 索引得到的 NonNull 句柄转成安全引用。
    ///
    /// # Safety (内部)
    /// - `nn` 必须由 USER_PROC_MANAGER 持有, 指向有效 UserProcess 分配。
    pub fn deref_non_null(nn: NonNull<UserProcess>) -> &'static UserProcess {
        // SAFETY: nn is from USER_PROC_MANAGER BTreeMap, allocation outlives the manager.
        unsafe { &*nn.as_ptr() }
    }

    /// 获取当前活跃进程 (current: AtomicU64 → NonNull→ ref)。
    /// 待进程调度器完整集成后启用。
    #[allow(dead_code)] // 待进程调度器完整集成后启用。
    pub fn current_proc() -> Option<UserProcRef> {
        let cur = USER_PROC_MANAGER.current.load(Ordering::SeqCst);
        if cur == 0 {
            None
        } else {
            // SAFETY: cur > 0, 此前由 set_current 设为有效的 NonNull 指针。
            Some(unsafe { UserProcRef::new_unchecked(cur as *mut UserProcess) })
        }
    }

    #[allow(dead_code)] // 待进程调度器完整集成后启用。
    pub fn set_current_ref(r: Option<UserProcRef>) {
        if let Some(p) = r {
            USER_PROC_MANAGER
                .current
                .store(p.as_ptr() as u64, Ordering::SeqCst);
        } else {
            USER_PROC_MANAGER.current.store(0, Ordering::SeqCst);
        }
    }

    // === FFI 桥接安全包装 (mm/vmm 设备) ===

    /// 释放一个物理页 (来自内核栈)。
    ///
    /// # Safety (内部)
    /// - `phys` 必须为 `pmm_alloc_page` 返回的合法物理页基址。
    pub fn free_phys_page(phys: *mut u8) {
        // SAFETY: 物理页所有权从分配者转移给本 free。
        unsafe { pmm_free_page(phys) }
    }

    /// 销毁用户页表 (cr3)。
    ///
    /// # Safety (内部)
    /// - `cr3` 必须为 `vmm_create_user_page_table` 返回的合法页表基址。
    pub fn destroy_user_page_table(cr3: u64) {
        // SAFETY: cr3 是 vmm_create_user_page_table 创建的, 调用方负责所有权释放。
        unsafe { vmm_destroy_page_table(cr3) }
    }

    /// 查询用户页表中虚拟地址对应的物理地址。
    ///
    /// # Safety (内部)
    /// - `cr3` 必须是有效的页表基址。
    pub fn virt_to_phys(cr3: u64, vaddr: u64) -> u64 {
        // SAFETY: cr3 来自 user proc 的 cr3 字段, 已由 vmm_create_user_page_table 创建。
        unsafe { vmm_get_physical_in_table(cr3, vaddr) }
    }

    /// 创建用户页表。
    pub fn create_user_page_table() -> u64 {
        // SAFETY: FFI 调用, 内部保证返回非零 cr3 或 0 表示失败。
        unsafe { vmm_create_user_page_table() }
    }

    /// 分配多页连续物理页。
    ///
    /// # Safety (内部)
    /// - 调用方负责通过 `free_phys_pages` 释放。
    pub fn alloc_phys_pages(count: u64) -> *mut u8 {
        // SAFETY: 物理页分配, 调用方负责所有权。
        unsafe { pmm_alloc_pages(count) }
    }

    /// 释放多页连续物理页 (待批量页释放路径启用后使用)。
    #[allow(dead_code)] // 待批量页释放路径启用后使用。
    pub fn free_phys_pages(pages: *mut u8, count: u64) {
        for i in 0..count {
            raw::free_phys_page((pages as u64 + i * PAGE_SIZE) as *mut u8);
        }
    }

    /// 分配一页物理页 (单页, 待单页分配路径启用后使用)。
    #[allow(dead_code)] // 待单页分配路径启用后使用。
    pub fn alloc_phys_page() -> *mut u8 {
        // SAFETY: 物理页分配, 调用方负责所有权。
        unsafe { pmm_alloc_page() }
    }

    /// 在用户页表中建立映射。
    pub fn vmm_map_user_page(cr3: u64, vaddr: u64, paddr: u64, flags: u64) {
        // SAFETY: cr3 来自 user proc 的 cr3 字段, 已建立。
        unsafe {
            vmm_map_page_in_table(cr3, vaddr, paddr, flags);
            vmm_map_page(vaddr, paddr, flags);
            vmm_ensure_path_user(vaddr);
        }
    }

    /// 写一个 u8 到用户页表中的某个字节。
    pub fn write_user_byte(cr3: u64, off: usize, v: u8) {
        // SAFETY: vmm_get_physical_in_table 保证返回的物理页对应 vaddr, KERNEL_BASE 偏移后内核可访问。
        unsafe {
            let phys = vmm_get_physical_in_table(cr3, off as u64 & !(PAGE_SIZE - 1));
            if phys != 0 {
                let addr = (phys + KERNEL_BASE + (off as u64 & 0xFFF)) as *mut u8;
                *addr = v;
            }
        }
    }

    /// 写一个 u64 到用户页表中的某个偏移 (unaligned)。
    pub fn write_user_u64(cr3: u64, off: usize, v: u64) {
        // SAFETY: vmm_get_physical_in_table 保证返回的物理页对应 vaddr, KERNEL_BASE 偏移后内核可访问。
        unsafe {
            let phys = vmm_get_physical_in_table(cr3, off as u64 & !(PAGE_SIZE - 1));
            if phys != 0 {
                let ptr = (phys + KERNEL_BASE + (off as u64 & 0xFFF)) as *mut u64;
                core::ptr::write_unaligned(ptr, v);
            }
        }
    }

    /// 读一个字节 (从用户态指针) - 内部 unsafe 封装。
    pub fn read_byte_from_user_ptr(src: *const u8, j: usize) -> u8 {
        // SAFETY: 由调用方保证 src 在 [src, src+j+1) 区间内可读。
        unsafe { *src.add(j) }
    }

    /// 物理页零初始化 + 映射到用户页表。
    pub fn alloc_zeroed_user_page(cr3: u64, vaddr: u64, flags: u64) -> *mut u8 {
        // SAFETY: 物理页分配, 调用方负责所有权。
        let page = unsafe { pmm_alloc_page() };
        if page.is_null() {
            return page;
        }
        // SAFETY: page 来自 pmm_alloc_page, 大小为 PAGE_SIZE。
        unsafe { memset(page, 0, PAGE_SIZE) }
        raw::vmm_map_user_page(cr3, vaddr, page as u64, flags);
        page
    }

    /// 释放物理页 (用于 ELF 加载失败回滚)。
    pub fn free_phys_page_for_rollback(phys: u64) {
        // SAFETY: 物理页所有权从分配者转移给本 free。
        unsafe { pmm_free_page(phys as *mut u8) }
    }

    /// 从 ELF 文件复制 chunk 到用户物理页 (通过内核映射)。
    pub fn elf_chunk_copy(
        page_phys: u64,
        off_in_page: u64,
        elf_data: *const u8,
        src_off: usize,
        chunk: u64,
    ) {
        // SAFETY: 物理页 + KERNEL_BASE 偏移后内核可写, elf_data 区间内可读。
        unsafe {
            let dest = (page_phys + KERNEL_BASE + off_in_page) as *mut u8;
            let src = elf_data.add(src_off);
            memcpy(dest, src, chunk);
        }
    }

    /// 映射单个物理页到用户页表 (用于代码段加载)。
    pub fn map_code_page(cr3: u64, vaddr: u64, page_phys: u64) {
        // SAFETY: 物理页已分配, flags = R|X 简化形式。
        let flags = (PageFlags::PRESENT | PageFlags::USER).bits();
        // SAFETY: cr3 已建立, page_phys 来自 pmm_alloc_page。
        unsafe {
            vmm_map_page_in_table(cr3, vaddr, page_phys, flags);
            vmm_map_page(vaddr, page_phys, flags);
            vmm_ensure_path_user(vaddr);
        }
    }

    /// 用户进程代码页分配 + 清零。
    pub fn alloc_code_page() -> *mut u8 {
        // SAFETY: pmm_alloc_page 是 C-ABI 物理页分配器；返回的指针是
        // 物理地址 (本进程内用作内核虚拟地址 by HHDM)。
        let page = unsafe { pmm_alloc_page() };
        if !page.is_null() {
            // SAFETY: page 来自 pmm_alloc_page, 大小为 PAGE_SIZE。
            unsafe { memset(page, 0, PAGE_SIZE) }
        }
        page
    }

    /// 物理页 → 内核可写指针 (用于代码段 chunk 复制, 待 ELF 加载路径完善后使用)。
    #[allow(dead_code)] // 待 ELF 加载路径完善后使用。
    pub fn phys_to_kern_mut(phys: u64, off: u64) -> *mut u8 {
        (phys + KERNEL_BASE + off) as *mut u8
    }

    /// ELF 文件指针 + 偏移 (待 ELF 加载路径完善后使用)。
    #[allow(dead_code)] // 待 ELF 加载路径完善后使用。
    pub fn elf_ptr_at(elf_data: *const u8, off: usize) -> *const u8 {
        // SAFETY: 调用方保证 off 在 elf_size 范围内。
        unsafe { elf_data.add(off) }
    }

    /// 切换到用户页表 (aarch64 用户态进入前, 待 aarch64 进程切换启用后使用)。
    #[allow(dead_code)] // 待 aarch64 进程切换启用后使用。
    pub fn vmm_switch_to_user(cr3: u64) {
        // SAFETY: cr3 来自 user proc 的 cr3 字段, 已由 vmm_create_user_page_table 创建。
        unsafe { vmm_switch_page_table(cr3) }
    }

    /// 分配内存并清零 (类似 calloc)。
    pub fn alloc_zeroed(size: u64) -> *mut u8 {
        // SAFETY: kmalloc 由 kernel allocator 提供, 调用方负责释放。
        let ptr = unsafe { kmalloc(size) } as *mut u8;
        if !ptr.is_null() {
            // SAFETY: ptr 来自 kmalloc, 大小为 size, 清零区间 [ptr, ptr+size) 合法。
            unsafe { memset(ptr, 0, size) }
        }
        ptr
    }

    /// 分配并构造一个 `UserProcess` 内存, 清零后返回。
    ///
    /// # Arguments
    /// - `process`: 与本 `UserProcess` 镜像关联的权威 `Process` NonNull 句柄.
    ///              构造时写入 `UserProcess::process` 字段, 后续通过
    ///              `UserProcess::process()` 安全访问.
    ///
    /// # 返回
    /// 已清零的 `UserProcess` 裸指针; `process` 字段已正确指向传入的 `Process`.
    pub fn alloc_user_process(process: NonNull<Process>) -> Option<*mut UserProcess> {
        let size = core::mem::size_of::<UserProcess>() as u64;
        let ptr = raw::alloc_zeroed(size) as *mut UserProcess;
        if ptr.is_null() {
            None
        } else {
            // SAFETY: ptr 来自 alloc_zeroed, 大小为 size_of::<UserProcess>(), 区间合法.
            //         process NonNull 句柄由调用方保证有效 (INV-USER-PROC-2).
            unsafe {
                core::ptr::write(&mut (*ptr).process, process);
            }
            Some(ptr)
        }
    }

    /// 分配并清零一个 `Process` (用于 process table)。
    pub fn alloc_kernel_process() -> Option<*mut Process> {
        let size = core::mem::size_of::<Process>() as u64;
        let ptr = raw::alloc_zeroed(size) as *mut Process;
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }

    /// 释放 `alloc_kernel_process` 分配的 `Process` 内存 (回滚路径专用).
    ///
    /// # Safety (内部)
    /// - `kproc_ptr` 必须为 `alloc_kernel_process` 返回的合法指针, 且未再被使用.
    ///   调用后该指针失效, 不得再次解引用.
    /// - 仅用于 `UserProcManager::create()` 的失败回滚路径; 成功路径下应
    ///   将其所有权转移给 `PROCESS_TABLE.insert` 或 `destroy` 链路.
    pub fn free_kernel_process(kproc_ptr: *mut Process) {
        if kproc_ptr.is_null() {
            return;
        }
        // SAFETY: kproc_ptr 来自 alloc_kernel_process (基于 alloc_zeroed -> kmalloc).
        //         调用方保证此后不再访问该指针.
        unsafe {
            crate::kernel::framework::mm::kfree(kproc_ptr as *mut u8);
        }
    }

    /// 释放 `alloc_user_process` 分配的 `UserProcess` 镜像内存 (回滚路径专用).
    ///
    /// # Safety (内部)
    /// - `proc_ptr` 必须为 `alloc_user_process` 返回的合法指针, 且未再被使用.
    /// - 必须先于 `free_kernel_process` 调用 (LIFO 反序), 以避免
    ///   `UserProcess::process` NonNull 字段成为悬挂指针.
    pub fn free_user_process(proc_ptr: *mut UserProcess) {
        if proc_ptr.is_null() {
            return;
        }
        // SAFETY: proc_ptr 来自 alloc_user_process (基于 alloc_zeroed -> kmalloc).
        //         调用方保证此后不再访问该指针.
        unsafe {
            crate::kernel::framework::mm::kfree(proc_ptr as *mut u8);
        }
    }

    /// 从 PID/CR3 构造 UserProcRef 用于新创建进程。
    ///
    /// # Safety (内部)
    /// - `proc` 必须为 alloc_user_process 返回的合法指针, 拥有完整所有权。
    pub fn new_proc_ref(proc: *mut UserProcess) -> UserProcRef {
        // SAFETY: proc 来自 alloc_user_process, 指向有效 UserProcess 分配。
        unsafe { UserProcRef::new_unchecked(proc) }
    }

    /// 在已清零的 `Process` 内存上写入基本字段 (避免业务逻辑中的 `unsafe`)。
    ///
    /// # Safety (内部)
    /// - `kproc_ptr` 必须为 `alloc_kernel_process` 返回的合法指针, 已被清零。
    #[allow(clippy::too_many_arguments)]
    pub fn init_kernel_process_fields(
        kproc_ptr: *mut Process,
        pid: u32,
        pwm: u64,
        cr3: u64,
        kstack: u64,
        ustack: u64,
    ) {
        use crate::kernel::framework::proc::SchedPolicy;
        // SAFETY: kproc_ptr 来自 alloc_kernel_process, 已清零, 字段可被 ptr::write 覆盖。
        unsafe {
            core::ptr::write(&mut (*kproc_ptr).pid, ProcessId(pid));
            core::ptr::write(&mut (*kproc_ptr).pwm, AtomicU64::new(pwm));
            core::ptr::write(
                &mut (*kproc_ptr).state,
                AtomicU32::new(ProcessState::Ready as u32),
            );
            core::ptr::write(
                &mut (*kproc_ptr).priority,
                AtomicU32::new(ProcessPriority::Normal as u32),
            );
            core::ptr::write(&mut (*kproc_ptr).flags, AtomicU32::new(0));
            core::ptr::write(&mut (*kproc_ptr).parent, None);
            core::ptr::write(&mut (*kproc_ptr).cr3, AtomicU64::new(cr3));
            core::ptr::write(
                &mut (*kproc_ptr).kernel_stack,
                AtomicU64::new(kstack),
            );
            core::ptr::write(
                &mut (*kproc_ptr).user_stack,
                AtomicU64::new(ustack),
            );
            core::ptr::write(&mut (*kproc_ptr).exit_code, AtomicU32::new(0));
            core::ptr::write(&mut (*kproc_ptr).cpu_time, AtomicU64::new(0));
            core::ptr::write(&mut (*kproc_ptr).block_reason, AtomicU32::new(0));
            core::ptr::write(
                &mut (*kproc_ptr).sched_policy,
                AtomicU32::new(SchedPolicy::Normal as u32),
            );
            (*kproc_ptr).rt_priority.store(0, Ordering::SeqCst);
            (*kproc_ptr).session_id.store(0, Ordering::SeqCst);
            (*kproc_ptr).sleep_until.store(0, Ordering::SeqCst);
            // 零初始化包含 alloc 的字段 (Mutex/Vec/String), 保持有效空状态
            core::ptr::write_bytes(
                &mut (*kproc_ptr).name as *mut _ as *mut u8,
                0,
                core::mem::size_of::<Mutex<String>>(),
            );
            core::ptr::write_bytes(
                &mut (*kproc_ptr).children as *mut _ as *mut u8,
                0,
                core::mem::size_of::<Mutex<Vec<ProcessId>>>(),
            );
            core::ptr::write_bytes(
                &mut (*kproc_ptr).context as *mut _ as *mut u8,
                0,
                core::mem::size_of::<Mutex<ProcessContext>>(),
            );
            core::ptr::write_bytes(
                &mut (*kproc_ptr).fd_table as *mut _ as *mut u8,
                0,
                core::mem::size_of::<FdTable>(),
            );
        }
    }
}

use raw::UserProcRef;

/// 用户进程 FFI 桥接缓存 (Framekernel privilege wrapper)
///
/// # 设计: 单源真相 + FFI 镜像
///
/// QueenX 进程子系统维护**两个并行结构**:
/// - `Process` (在 `process.rs` 中, 权威单一源) — 全量进程描述符, 包含调度/
///   信号/文件系统/会话等所有元数据, 由 `PROCESS_TABLE` 管理.
/// - `UserProcess` (本结构, FFI 镜像) — 仅缓存进入 Ring 3 路径上**热访问**的
///   字段 (pid/pwm/cr3/kernel_stack/user_stack/state), 以及**独占**的 FFI 字段
///   (`entry`、`stack_bottom`、`create_time`).
///
/// # 字段分类
///
/// ## 共享字段 (与 `Process` 重叠, 同步方向: Process → UserProcess)
///
/// - `pid`           对应 `Process::pid`
/// - `pwm`           对应 `Process::pwm` (类型 `AtomicU64`)
/// - `cr3`           对应 `Process::cr3` (类型 `AtomicU64`)
/// - `kernel_stack`  对应 `Process::kernel_stack` (类型 `AtomicU64`)
/// - `user_stack`    对应 `Process::user_stack` (类型 `AtomicU64`)
/// - `state`         对应 `Process::state` (类型 `AtomicU32`)
///
/// 共享字段在 `create()` 完成后即与 `Process` 镜像同步; 运行期状态变更
/// 优先写 `Process`, 然后通过 `sync_from_process()` 推送到本镜像.
///
/// ## FFI 独占字段 (本结构独有, 不存在于 `Process`)
///
/// - `entry`         — asm 跳转入口, 由 `enter()` 读取并跳转
/// - `stack_bottom`  — 用户栈底地址, `setup_user_stack()` 计算依据
/// - `create_time`   — 进程创建时间戳, 调度/审计用
///
/// # 不变量 (INV-USER-PROC)
///
/// 1. **同步不变量**: 本结构共享字段值与 `Process` 对应字段**最终一致**.
///    同步通过 `sync_to_process()` / `sync_from_process()` 显式调用完成.
/// 2. **生命周期不变量**: `process` NonNull 指向的 `Process` 存活期 ≥ 本结构.
///    销毁本结构前必须先从 `USER_PROC_MANAGER.processes` 移除条目.
/// 3. **FFI 安全不变量**: `#[repr(C)]` 保持稳定的内存布局, 避免跨 FFI 边界时
///    Rust 端重新布局导致 C 端解析错误.
// SAFETY: UserProcess::process 字段存储 NonNull<Process> 句柄, 不持有所有权.
//         共享字段 (pid/pwm/cr3/kstack/ustack/state) 均为 Atomic* 或 u32, 满足 Send.
//         FFI 独占字段 (entry/stack_bottom/create_time) 均为 u64, 满足 Send.
// SAFETY: UserProcess 含裸指针, 但所有可变访问通过 USER_PROC_MANAGER 锁保护;
//         裸指针仅在锁内解引用, 不会跨线程无锁访问.
//         综合: UserProcess 可在线程间安全转移.
unsafe impl Send for UserProcess {}
// SAFETY: 同上, Sync 安全性由外部锁保证.
unsafe impl Sync for UserProcess {}
#[repr(C)]
pub struct UserProcess {
    /// ✅ 权威引用: 指向 `PROCESS_TABLE` 中对应的 `Process`.
    ///
    /// 持有 NonNull 而非裸指针, 表达"一定有值"的语义;
    /// 构造时强制调用方提供 `Process` 句柄, 杜绝悬垂.
    pub(crate) process: NonNull<Process>,

    // === 共享字段 (与 Process 镜像同步) ===
    /// `Process::pid.0` 的扁平缓存. 业务访问应走 `self.process().pid`.
    pub pid: u32,
    /// `Process::pwm` 镜像. 业务访问应走 `self.process().pwm`.
    pub pwm: AtomicU64,
    /// `Process::cr3` 镜像. 业务访问应走 `self.process().cr3`.
    pub cr3: AtomicU64,
    /// `Process::kernel_stack` 镜像. 业务访问应走 `self.process().kernel_stack`.
    pub kernel_stack: AtomicU64,
    /// `Process::user_stack` 镜像. 业务访问应走 `self.process().user_stack`.
    pub user_stack: AtomicU64,
    /// `Process::state` 镜像. 业务访问应走 `self.process().state`.
    pub state: AtomicU32,

    // === FFI 独占字段 ===
    /// asm 跳转入口地址 (由 `enter()` 读取并执行 `jmp entry`).
    pub entry: u64,
    /// 用户栈底虚拟地址, `setup_user_stack()` 据此计算 argv/envp 摆放位置.
    pub stack_bottom: AtomicU64,
    /// 进程创建时间戳 (ticks). 调度/审计用, 不属于 Process 状态.
    pub create_time: u64,
}

impl UserProcess {
    /// 获取权威 `Process` 引用.
    ///
    /// # Returns
    /// 对 `PROCESS_TABLE` 中存储的 `Process` 的 `&'static` 引用 (非空保证由
    /// NonNull 字段提供).
    pub fn process(&self) -> &Process {
        // SAFETY: UserProcess::process NonNull 字段的不变量 (INV-USER-PROC-2)
        // 保证其指向的 Process 在 UserProcess 存活期间有效.
        unsafe { self.process.as_ref() }
    }

    /// 从权威 `Process` 拉取共享字段, 同步到本镜像.
    ///
    /// 适用场景: 业务代码修改了 `Process` 字段, 需要刷新本镜像 (例如调度器
    /// 切换进程时, 将目标进程的 CR3 同步到 UserProcess 以便 enter() 读取).
    pub fn sync_from_process(&self) {
        let p = self.process();
        self.pwm.store(p.pwm.load(Ordering::SeqCst), Ordering::SeqCst);
        self.cr3.store(p.cr3.load(Ordering::SeqCst), Ordering::SeqCst);
        self.kernel_stack
            .store(p.kernel_stack.load(Ordering::SeqCst), Ordering::SeqCst);
        self.user_stack
            .store(p.user_stack.load(Ordering::SeqCst), Ordering::SeqCst);
        self.state.store(p.state.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    /// 将本镜像的共享字段推送到权威 `Process`.
    ///
    /// 适用场景: FFI 桥接层 (如 `user_proc_clone()`) 创建/修改了本镜像, 需
    /// 要将变更同步到 PROCESS_TABLE, 避免两侧脱节.
    pub fn sync_to_process(&self) {
        let p = self.process();
        p.pwm.store(self.pwm.load(Ordering::SeqCst), Ordering::SeqCst);
        p.cr3.store(self.cr3.load(Ordering::SeqCst), Ordering::SeqCst);
        p.kernel_stack
            .store(self.kernel_stack.load(Ordering::SeqCst), Ordering::SeqCst);
        p.user_stack
            .store(self.user_stack.load(Ordering::SeqCst), Ordering::SeqCst);
        p.state.store(self.state.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    /// 运行时不变量检查 (调试用).
    ///
    /// 校验 INV-USER-PROC-1: 本镜像共享字段与 Process 对应字段一致.
    /// 不一致则返回 false, 调用方可触发 `sync_from_process()` 修复.
    pub fn check_sync(&self) -> bool {
        let p = self.process();
        self.pid == p.pid.0
            && self.pwm.load(Ordering::SeqCst) == p.pwm.load(Ordering::SeqCst)
            && self.cr3.load(Ordering::SeqCst) == p.cr3.load(Ordering::SeqCst)
            && self.kernel_stack.load(Ordering::SeqCst) == p.kernel_stack.load(Ordering::SeqCst)
            && self.user_stack.load(Ordering::SeqCst) == p.user_stack.load(Ordering::SeqCst)
            && self.state.load(Ordering::SeqCst) == p.state.load(Ordering::SeqCst)
    }
}

pub struct UserProcManager {
    current: AtomicU64,
    // 使用 NonNull<UserProcess> 替代 *mut UserProcess,
    // 使 BTreeMap 自动实现 Send + Sync (NonNull: Send + Sync when T: Send + Sync)。
    processes: Mutex<alloc::collections::BTreeMap<u32, NonNull<UserProcess>>>,
}

// SAFETY: UserProcManager 始终通过静态 USER_PROC_MANAGER 访问.
// 所有变更都走 Mutex, NonNull 指针指向的 UserProcess 对象
// SAFETY: UserProcManager 含裸指针 HashMap, 但所有访问通过自身 Mutex 保护.
//         字段均为 Atomic* 或普通整数.
unsafe impl Send for UserProcManager {}
// SAFETY: 同上, 外部锁保证并发安全.
unsafe impl Sync for UserProcManager {}

impl UserProcManager {
    pub const fn new() -> Self {
        Self {
            current: AtomicU64::new(0),
            processes: Mutex::new(alloc::collections::BTreeMap::new()),
        }
    }

    pub fn init(&self) {}

    fn destroy(&self, proc: NonNull<UserProcess>, keep_kstack: bool) {
        // SAFETY: proc 是 NonNull<UserProcess>, 由 kmalloc 分配并插入
        // BTreeMap, 在 destroy 前始终有效.
        let proc_ref = raw::deref_non_null(proc);
        let cr3 = proc_ref.cr3.load(Ordering::SeqCst);
        if cr3 != 0 {
            raw::destroy_user_page_table(cr3);
        }
        if !keep_kstack {
            let kstack = proc_ref.kernel_stack.load(Ordering::SeqCst);
            if kstack != 0 {
                let kstack_base_virt = kstack - USER_KSTACK_SIZE;
                let kstack_base_phys = kstack_base_virt - KERNEL_BASE;
                for i in 0..(USER_KSTACK_SIZE / PAGE_SIZE) {
                    raw::free_phys_page((kstack_base_phys + i * PAGE_SIZE) as *mut u8);
                }
            }
        }
        let ustack = proc_ref.user_stack.load(Ordering::SeqCst);
        if ustack != 0 {
            let stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
            for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
                let svirt = stack_virt + USER_STACK_GUARD + i * PAGE_SIZE;
                let phys = raw::virt_to_phys(cr3, svirt);
                if phys != 0 {
                    raw::free_phys_page(phys as *mut u8);
                }
            }
        }
        let pid = proc_ref.pid;
        self.processes.lock().remove(&pid);
    }

    /// 销毁进程 (接受裸指针的兼容接口)
    fn destroy_raw(&self, proc: *mut UserProcess, keep_kstack: bool) {
        if let Some(nn) = NonNull::new(proc) {
            self.destroy(nn, keep_kstack);
        }
    }

    /// 获取进程裸指针 (向后兼容接口)。
    /// 内部存储为 NonNull, 转为 *mut 供外部调用。
    pub fn get(&self, pid: u32) -> Option<*mut UserProcess> {
        self.processes.lock().get(&pid).map(|n| n.as_ptr())
    }

    /// 通过闭包安全访问进程
    pub fn with_process<F, R>(&self, pid: u32, f: F) -> Option<R>
    where
        F: FnOnce(&UserProcess) -> R,
    {
        let processes = self.processes.lock();
        processes.get(&pid).map(|ptr| {
            // SAFETY: ptr 来自 BTreeMap 中的 NonNull<UserProcess>.
            // 进程存活于 manager 的整个生命周期; 持锁时
            // 进程永不被释放.
            f(raw::deref_non_null(*ptr))
        })
    }

    pub fn destroy_by_pid(&self, pid: u32) {
        // SAFETY: get returns *mut from NonNull which is never null.
        if let Some(proc) = self.processes.lock().get(&pid).copied() {
            self.destroy(proc, false);
        }
    }

    pub fn destroy_by_pid_no_kstack(&self, pid: u32) {
        if let Some(proc) = self.processes.lock().get(&pid).copied() {
            self.destroy(proc, true);
        }
    }

    /// 替换进程的用户地址空间 (execve 路径).
    ///
    /// 销毁旧页表和用户栈物理页, 然后将 CR3/entry/user_stack/stack_bottom
    /// 更新为新值. 不移除 BTreeMap 条目, 不释放内核栈, 不改变 PID —
    /// 保持 POSIX execve 语义 (PID 不变).
    pub fn replace_user_space(
        &self,
        pid: u32,
        new_cr3: u64,
        new_entry: u64,
        new_user_stack: u64,
        new_stack_bottom: u64,
    ) {
        let proc = match self.processes.lock().get(&pid).copied() {
            Some(p) => p,
            None => return,
        };
        // SAFETY: proc 来自 BTreeMap 中的 NonNull, 进程存活期间有效.
        let proc_ref = unsafe { UserProcRef::new_unchecked(proc.as_ptr()) };

        // 1. 销毁旧用户页表
        let old_cr3 = proc_ref.load_cr3();
        if old_cr3 != 0 {
            // 释放旧用户栈物理页 (必须在销毁页表前完成, 否则无法翻译虚拟地址)
            let old_stack_bottom = proc_ref.load_stack_bottom();
            if old_stack_bottom != 0 {
                for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
                    let svirt = old_stack_bottom + i * PAGE_SIZE;
                    let phys = raw::virt_to_phys(old_cr3, svirt);
                    if phys != 0 {
                        raw::free_phys_page(phys as *mut u8);
                    }
                }
            }
            raw::destroy_user_page_table(old_cr3);
        }

        // 2. 更新为新的地址空间
        proc_ref.store_cr3(new_cr3);
        proc_ref.set_entry(new_entry);
        proc_ref.store_user_stack(new_user_stack);
        proc_ref.store_stack_bottom(new_stack_bottom);

        // 3. 同步到权威 Process 结构
        // SAFETY: proc 来自 BTreeMap 中的 NonNull<UserProcess>, 其 process 字段
        // 指向 PROCESS_TABLE 中有效的 Process, 在进程存活期间有效.
        let kproc = unsafe { (*proc.as_ptr()).process.as_ptr() };
        // SAFETY: kproc 来自 UserProcess::process NonNull 字段, 在进程存活期间有效.
        unsafe {
            (*kproc).cr3.store(new_cr3, Ordering::SeqCst);
            (*kproc).user_stack.store(new_user_stack, Ordering::SeqCst);
        }
    }

    /// 从管理器中移除进程索引但不释放任何资源.
    /// 用于 execve: 新进程资源已转移到旧 PID, 仅需移除索引条目.
    pub fn detach_by_pid(&self, pid: u32) {
        self.processes.lock().remove(&pid);
    }

    pub fn create(&self, info: &UserProcInfo, pwm: u64) -> Option<*mut UserProcess> {
        // ✅ 单源真相: 优先分配权威 Process, 再分配 UserProcess 镜像并关联.
        //    此顺序保证 UserProcess::process NonNull 字段构造时即指向有效 Process.
        let kproc_ptr = raw::alloc_kernel_process()?;
        // SAFETY: alloc_kernel_process 成功时保证返回非空指针
        let kproc_nn = NonNull::new(kproc_ptr)?;

        // 分配并清零 UserProcess 内存, 关联权威 Process 句柄
        let proc_ptr = raw::alloc_user_process(kproc_nn)?;
        let proc = raw::new_proc_ref(proc_ptr);

        // 创建用户页表
        let cr3_val = raw::create_user_page_table();
        proc.store_cr3(cr3_val);
        if cr3_val == 0 {
            // 失败回滚 (DECISION-027): 页表创建失败, 必须释放已分配的
            // UserProcess + Process 内存. 顺序为 LIFO 反序: 先 UserProcess
            // (避免 process NonNull 字段成为悬挂), 再 Process.
            raw::free_user_process(proc_ptr);
            raw::free_kernel_process(kproc_ptr);
            return None;
        }

        // 分配用户栈
        let stack_pages = raw::alloc_phys_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
        if stack_pages.is_null() {
            // 失败回滚: 用户栈物理页分配失败, 销毁页表并释放结构内存.
            raw::destroy_user_page_table(cr3_val);
            raw::free_user_process(proc_ptr);
            raw::free_kernel_process(kproc_ptr);
            return None;
        }

        let stack_phys = stack_pages as u64;
        // ASLR: 随机化栈顶地址
        let aslr_stack_top = crate::kernel::framework::config::aslr_stack_top();
        let stack_virt = aslr_stack_top - USER_STACK_SIZE - USER_STACK_GUARD;

        for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
            let svirt = stack_virt + USER_STACK_GUARD + i * PAGE_SIZE;
            let sphys = stack_phys + i * PAGE_SIZE;
            raw::vmm_map_user_page(
                cr3_val,
                svirt,
                sphys,
                (PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER).bits(),
            );
        }

        proc.store_user_stack(aslr_stack_top);
        let initial_stack_bottom = aslr_stack_top - USER_STACK_SIZE;
        proc.store_stack_bottom(initial_stack_bottom);

        // 分配内核栈
        let kstack = raw::alloc_phys_pages(USER_KSTACK_SIZE / PAGE_SIZE);
        if kstack.is_null() {
            // 失败回滚: 内核栈物理页分配失败, 释放栈页+页表+结构内存.
            // 顺序仍为 LIFO 反序: 物理资源 → 镜像 → 权威结构.
            raw::free_phys_page(stack_pages);
            raw::destroy_user_page_table(cr3_val);
            raw::free_user_process(proc_ptr);
            raw::free_kernel_process(kproc_ptr);
            return None;
        }
        let kstack_top = kstack as u64 + KERNEL_BASE + USER_KSTACK_SIZE;
        proc.store_kernel_stack(kstack_top);
        crate::kernel::framework::proc::kernel_stack_write_canary(kstack_top);

        // ✅ PID 分配延后到所有内存/页表/栈资源就绪后:
        //   避免 `alloc_kernel_process` 或 `alloc_user_process` 失败时, 已分配
        //   的 PID 留在 next_pid 计数器中造成 PID 泄漏. 早期失败 (页表/栈分配)
        //   只回滚物理页与页表, 不需要回滚 PID.
        let pid = PROCESS_TABLE.allocate_pid()?;
        proc.set_pid(pid);
        proc.set_entry(info.entry);
        proc.store_pwm(pwm);
        proc.store_state(1);
        proc.set_create_time(crate::kernel::framework::timer::get_ticks());

        self.processes
            .lock()
            .insert(pid, NonNull::new(proc_ptr)?);

        // 在权威 Process 上写入基本字段 (与 UserProcess 镜像共享字段保持一致).
        raw::init_kernel_process_fields(
            kproc_ptr,
            pid,
            pwm,
            cr3_val,
            proc.load_kernel_stack(),
            proc.load_user_stack(),
        );

        // 插入 PROCESS_TABLE 完成权威注册.
        PROCESS_TABLE.insert(kproc_ptr);

        Some(proc_ptr)
    }

    pub fn enter(&self, proc: *mut UserProcess) {
        if proc.is_null() {
            return;
        }

        // SAFETY: proc 由调用方保证为非空、生命周期有效 (BTreeMap 中已存在)。
        let proc_ref = unsafe { UserProcRef::new_unchecked(proc) };
        self.current.store(proc as u64, Ordering::SeqCst);
        proc_ref.store_state(2);

        let kstack = proc_ref.load_kernel_stack();
        let rip_val = proc_ref.entry();
        let rsp_val = proc_ref.load_user_stack();
        let cr3 = proc_ref.load_cr3();
        let _ss_val = GDT_USER_DATA | 0x03;
        let _cs_val = GDT_USER_CODE | 0x03;
        let _rflags_val: u64 = 0x3202;

        crate::kernel::framework::cpu::arch::set_kernel_stack(kstack);

        // 更新 per-CPU 用户页表 CR3, 使 syscall/中断返回用户态时
        // 从 [gs:USER_PML4_OFF] 读取到正确的进程用户页表.
        // SAFETY: cr3 是当前进程的有效用户页表 PML4 物理地址;
        // 当前在调度器上下文, 独占访问 per-CPU 数据.
        unsafe {
            #[cfg(target_arch = "x86_64")]
            crate::kernel::framework::arch::gdt::gdt_set_user_cr3(cr3);
            let _ = cr3;
        }

        // 将 RSP0 栈页映射到用户页表 (添加 USER 位).
        // 用户态中断触发时 CPU 从 TSS 读取 RSP0 并切换到该栈,
        // 但内核大页映射没有 USER 位, 需要显式映射为 USER 可访问.
        {
            let rsp0_page = kstack & !(PAGE_SIZE - 1);
            let rsp0_phys = rsp0_page - crate::kernel::framework::mm::KERNEL_BASE as u64;
            crate::kernel::framework::mm::get_vmm().map_page_in_table(
                cr3,
                crate::kernel::framework::mm::VirtAddr(rsp0_page),
                crate::kernel::framework::mm::PhysAddr(rsp0_phys),
                crate::kernel::framework::mm::PageFlags::PRESENT
                    | crate::kernel::framework::mm::PageFlags::WRITABLE
                    | crate::kernel::framework::mm::PageFlags::USER,
            );
        }

        // SAFETY: enter_user 是平台特定的 arch 入口, 不会返回, 由调用方保证上下文有效。
        // user_cr3 传入用户页表物理地址, 由 enter_user 汇编在 iretq 前切换.
        unsafe {
            crate::arch!(enter_user(rip_val as usize, rsp_val as usize, 0, cr3, kstack));
        }
    }

    /// 将 argc/argv/envp 写入用户进程栈
    /// 返回设置后的新栈指针 (RSP)
    ///
    /// # Safety
    ///
    /// `pid` 对应一个活动进程. 调用方必须持有进程表锁.
    pub unsafe fn setup_user_stack(
        &self,
        proc: *mut UserProcess,
        argv: *const *const u8,
        argc: usize,
        _envp: *const *const u8,
        envc: usize,
    ) -> u64 {
        if proc.is_null() {
            return 0;
        }

        // SAFETY: proc 由调用方保证有效, 内部访问均经 UserProcRef 安全包装。
        let proc_ref = unsafe { UserProcRef::new_unchecked(proc) };
        let stack_top = proc_ref.load_user_stack();
        let cr3 = proc_ref.load_cr3();

        // 所需空间: argc(8) + argv 指针(8*(argc+1)) + envp 指针(8*(envc+1)) + 字符串
        let mut string_bytes: usize = 0;
        let mut arg_lens: alloc::vec::Vec<usize> = alloc::vec::Vec::new();

        if !argv.is_null() {
            for i in 0..argc {
                // SAFETY: argv 由调用方保证至少 argc 个有效指针。
                let s = unsafe { *argv.add(i) };
                if !s.is_null() {
                    let mut len: usize = 0;
                    // SAFETY: s 是 C 字符串, 不断读直到 NUL。
                    while unsafe { *s.add(len) } != 0 {
                        len += 1;
                    }
                    arg_lens.push(len + 1);
                    string_bytes += len + 1;
                } else {
                    arg_lens.push(1);
                    string_bytes += 1;
                }
            }
        }

        let ptr_count = 1 + (argc + 1) + (envc + 1); // argc(1) + argv(n+1) + envp(m+1)
        let total = ptr_count * 8 + string_bytes;
        // 确保 16 字节对齐
        let total = (total + 15) & !15;

        if total as u64 > stack_top - USER_STACK_TOP + USER_STACK_SIZE {
            return 0; // Stack overflow
        }

        let new_sp = stack_top - total as u64;
        let mut pos = new_sp as usize;

        // 写入 argc
        let argc_off = pos;
        pos += 8;
        // 跳过 argv 指针 (在字符串写完之后回填)
        let argv_start_off = pos;
        pos += (argc + 1) * 8;
        // 跳过 envp 指针
        let envp_start_off = pos;
        pos += (envc + 1) * 8;
        // 字符串区起始
        let strings_off = pos;

        // Write argc
        raw::write_user_u64(cr3, argc_off, argc as u64);

        // 写入 argv 字符串与指针
        let mut str_off = strings_off;
        for i in 0..argc {
            let abs_addr = str_off as u64;
            raw::write_user_u64(cr3, argv_start_off + i * 8, abs_addr);

            if !argv.is_null() && (i < argc) {
                // SAFETY: argv 由调用方保证, i < argc 有效。
                let src = unsafe { *argv.add(i) };
                let l = arg_lens[i];
                for j in 0..l {
                    let b = if src.is_null() {
                        0u8
                    } else {
                        raw::read_byte_from_user_ptr(src, j)
                    };
                    raw::write_user_byte(cr3, str_off + j, b);
                }
                str_off += l;
            }
        }
        // argv 末尾 NULL 哨兵
        raw::write_user_u64(cr3, argv_start_off + argc * 8, 0u64);

        // envp 指针 (暂时全填 NULL)
        for i in 0..(envc + 1) {
            raw::write_user_u64(cr3, envp_start_off + i * 8, 0u64);
        }

        // 更新进程栈指针
        proc_ref.store_user_stack(new_sp);
        new_sp
    }

    pub fn load_elf_from_memory(&self, elf_data: *const u8, elf_size: u64, pwm: u64) -> i32 {
        if elf_data.is_null() || elf_size < core::mem::size_of::<ElfHeader>() as u64 {
            return -1;
        }

        // P1-I-33: 委托给 elf::verify::verify_elf 单一来源, 避免解析方式不一致
        //
        // SAFETY: elf_data 区间已校验 (非空 + size >= header), verify_elf 内部仅读借用。
        let verified = match unsafe { super::elf::verify::verify_elf(elf_data, elf_size) } {
            Ok(v) => v,
            Err(_) => return -1,
        };

        // SAFETY: verify_elf 已通过校验, header 引用安全
        let header = unsafe { &*(elf_data as *const ElfHeader) };

        // PIE (ET_DYN) 支持: 检测 ELF 类型并计算 load_bias
        let is_pie = verified.is_pie;
        let load_bias: u64 = if is_pie {
            crate::kernel::framework::config::aslr_pie_base()
        } else {
            0
        };
        let entry = verified.entry + load_bias;

        // SAFETY: verify_elf 已通过校验, 后续 raw 指针访问 (header / phdr / 物理页) 集中此块
        unsafe {
            let info = UserProcInfo {
                entry,
                name: [0; 64],
                code_size: 0,
                code_data: core::ptr::null(),
            };

            let proc = match self.create(&info, pwm) {
                Some(p) => p,
                None => return -1,
            };

            // SAFETY: proc 由 create 返回, 生命周期由 UserProcManager 管理。
            let proc_ref = UserProcRef::new_unchecked(proc);
            let cr3 = proc_ref.load_cr3();

            // P1-I-32 修复: 改用栈上局部数组, 消除 RacyCell 静态分配器在 SMP 下
            // 多核 execve 并发的数据竞争. 8KB 临时缓冲在 USER_KSTACK_SIZE=16KB 上
            // 安全 (剩余 8KB 仍够 syscall handler 路径), 退出函数后自动释放, 无锁.
            // 仍按 phnum>256 截断保护, 单 PT_LOAD 段最大 1024 页.
            let mut allocated_pages = [0u64; 1024];
            let allocated_pages: &mut [u64] = &mut allocated_pages;
            let mut page_count: usize = 0;

            let phnum = header.e_phnum as usize;
            if phnum > 256 {
                self.destroy_raw(proc, false);
                return -1;
            }

            for i in 0..phnum {
                let phdr_size = core::mem::size_of::<ElfPhdr>() as u64;
                let phdr_offset = header.e_phoff + (i as u64) * header.e_phentsize as u64;
                if phdr_offset + phdr_size > elf_size {
                    self.destroy_raw(proc, false);
                    return -1;
                }
                let phdr = (elf_data.add(phdr_offset as usize)) as *const ElfPhdr;

                if (*phdr).p_type != PT_LOAD {
                    continue;
                }

                let vaddr_start = ((*phdr).p_vaddr + load_bias) & !(PAGE_SIZE - 1);
                let vaddr_end = ((*phdr).p_vaddr + (*phdr).p_memsz + load_bias + (PAGE_SIZE - 1)) & !(PAGE_SIZE - 1);
                let num_pages = (vaddr_end - vaddr_start) / PAGE_SIZE;

                for j in 0..num_pages {
                    let vaddr = vaddr_start + j * PAGE_SIZE;

                    let mut flags = PageFlags::PRESENT | PageFlags::USER;
                    if (*phdr).p_flags & 0x02 != 0 {
                        flags |= PageFlags::WRITABLE;
                    }
                    let flags = flags.bits();

                    // 在 aarch64 上, L2_DEVICE 中的 2MB BLOCK 描述符
                    // 会让 vmm_get_physical_in_table 对未映射的用户地址
                    // 返回非零. 跳过复用检查, 一律分配.
                    #[cfg(target_arch = "aarch64")]
                    let existing_phys: u64 = 0;
                    #[cfg(not(target_arch = "aarch64"))]
                    let existing_phys = raw::virt_to_phys(cr3, vaddr);

                    if existing_phys == 0 {
                        let page = raw::alloc_zeroed_user_page(cr3, vaddr, flags);
                        if page.is_null() {
                            for pi in 0..page_count {
                                raw::free_phys_page_for_rollback(allocated_pages[pi]);
                            }
                            self.destroy_raw(proc, false);
                            return -1;
                        }
                        if page_count < 1024 {
                            allocated_pages[page_count] = page as u64;
                            page_count += 1;
                        }
                    } else {
                        // 复用现有页, 记录下来
                        if page_count < 1024 {
                            allocated_pages[page_count] = existing_phys;
                            page_count += 1;
                        }
                    }
                }

                if (*phdr).p_filesz > 0 {
                    let file_offset_bytes = (*phdr).p_offset as usize;
                    let mut copied: u64 = 0;
                    let first_page_offset = (*phdr).p_vaddr & 0xFFF;
                    let start_idx = page_count.saturating_sub(num_pages as usize);

                    for j in 0..num_pages {
                        if copied >= (*phdr).p_filesz {
                            break;
                        }
                        let page_phys = allocated_pages[start_idx + j as usize];
                        if page_phys == 0 {
                            continue;
                        }

                        let off_in_page = if j == 0 { first_page_offset } else { 0 };
                        let max_in_page = PAGE_SIZE - off_in_page;
                        let remaining = (*phdr).p_filesz - copied;
                        let chunk = if max_in_page < remaining {
                            max_in_page
                        } else {
                            remaining
                        };
                        raw::elf_chunk_copy(
                            page_phys,
                            off_in_page,
                            elf_data,
                            file_offset_bytes + (copied as usize),
                            chunk,
                        );
                        copied += chunk;
                    }
                }
            }

            proc_ref.set_entry(entry);

            proc_ref.pid() as i32
        }
    }

    pub fn create_from_binary(&self, code: *const u8, code_size: u64, pwm: u64) -> i32 {
        let info = UserProcInfo {
            entry: USER_CODE_BASE,
            name: [0; 64],
            code_size,
            code_data: code,
        };

        let proc = match self.create(&info, pwm) {
            Some(p) => p,
            None => return -1,
        };

        // SAFETY: proc 由 create 返回, 生命周期由 UserProcManager 管理。
        let proc_ref = unsafe { UserProcRef::new_unchecked(proc) };
        let cr3 = proc_ref.load_cr3();
        let num_code_pages = code_size.div_ceil(PAGE_SIZE);

        for i in 0..num_code_pages {
            let page = raw::alloc_code_page();
            if page.is_null() {
                return -1;
            }

            let copy_size = if code_size - i * PAGE_SIZE > PAGE_SIZE {
                PAGE_SIZE
            } else {
                code_size - i * PAGE_SIZE
            };

            // SAFETY: page 来自 pmm_alloc_page, code 区间内可读。
            unsafe {
                memcpy(
                    page as *mut u8,
                    code.add((i * PAGE_SIZE) as usize),
                    copy_size,
                );
            }

            let vaddr = USER_CODE_BASE + i * PAGE_SIZE;
            raw::map_code_page(cr3, vaddr, page as u64);
        }

        proc_ref.pid() as i32
    }

    pub fn get_current(&self) -> Option<*mut UserProcess> {
        let current = self.current.load(Ordering::SeqCst);
        if current != 0 {
            Some(current as *mut UserProcess)
        } else {
            None
        }
    }

    pub fn set_current(&self, proc: Option<*mut UserProcess>) {
        if let Some(p) = proc {
            self.current.store(p as u64, Ordering::SeqCst);
        } else {
            self.current.store(0, Ordering::SeqCst);
        }
    }
}

pub static USER_PROC_MANAGER: UserProcManager = UserProcManager::new();

pub fn init() {
    USER_PROC_MANAGER.init();

    // 注册 memlock 限制查询回调, 解耦 mm→proc 依赖
    // SAFETY: get_memlock_limit 是 'static 函数指针, 在内核运行期间始终有效.
    unsafe {
        crate::kernel::framework::rlimit_query::register_memlock_limit(
            crate::kernel::framework::proc::get_memlock_limit,
        );
    }
}

/// 分配一个新的 PID（供 sys_fork 使用）
#[unsafe(no_mangle)]
pub extern "C" fn proc_alloc_pid() -> u32 {
    PROCESS_TABLE.allocate_pid().unwrap_or(0)
}

/// 克隆父进程的 UserProcess 给子进程（供 sys_fork 使用）
/// 子进程的 CR3 和内核栈已在 sys_fork 中分配好，此处仅创建 UserProcess 记录
#[unsafe(no_mangle)]
pub extern "C" fn user_proc_clone(parent_pid: u32, child_pid: u32) -> i32 {
    let parent_proc = match USER_PROC_MANAGER.get(parent_pid) {
        Some(p) => p,
        None => return -1,
    };

    let child_kernel_proc = match PROCESS_TABLE.get(child_pid) {
        Some(p) => match NonNull::new(p) {
            Some(nn) => nn,
            None => return -1,
        },
        None => return -1,
    };

    // SAFETY: parent_proc / child_kernel_proc 均来自管理器, 有效。
    unsafe {
        let parent_ref = UserProcRef::new_unchecked(parent_proc);
        // ✅ 关联权威 child_kernel_proc 句柄, 建立 UserProcess→Process 反向引用.
        let child_up = raw::alloc_user_process(child_kernel_proc).unwrap_or_default();
        if child_up.is_null() {
            return -1;
        }
        let child_ref = UserProcRef::new_unchecked(child_up);

        child_ref.set_pid(child_pid);
        child_ref.store_pwm(parent_ref.load_pwm());
        child_ref.store_cr3((*child_kernel_proc.as_ptr()).cr3.load(Ordering::SeqCst));
        child_ref.store_kernel_stack(
            (*child_kernel_proc.as_ptr()).kernel_stack.load(Ordering::SeqCst),
        );
        child_ref.store_user_stack(parent_ref.load_user_stack());
        child_ref.store_stack_bottom(parent_ref.load_stack_bottom());
        child_ref.set_entry(parent_ref.entry());
        child_ref.store_state(1);
        child_ref.set_create_time(crate::kernel::framework::timer::get_ticks());

        // ✅ 同步子进程镜像共享字段到权威 Process (INV-USER-PROC-1).
        (*child_up).sync_to_process();

        USER_PROC_MANAGER
            .processes
            .lock()
            .insert(child_pid, match NonNull::new(child_up) {
                Some(nn) => nn,
                None => {
                    klog_error!("user_proc_clone: 子进程指针为空");
                    return -1;
                }
            });
    }

    0
}

pub fn try_expand_user_stack(fault_addr: u64) -> bool {
    if fault_addr >= USER_STACK_TOP {
        return false;
    }
    if fault_addr < USER_STACK_EXPAND_LIMIT {
        return false;
    }

    let proc = match USER_PROC_MANAGER.get_current() {
        Some(p) => p,
        None => return false,
    };

    // SAFETY: proc 由管理器返回, 有效。
    let proc_ref = unsafe { UserProcRef::new_unchecked(proc) };
    let stack_bottom = proc_ref.load_stack_bottom();
    if fault_addr >= stack_bottom {
        return false;
    }

    let cr3 = proc_ref.load_cr3();
    if cr3 == 0 {
        return false;
    }

    let page_addr = fault_addr & !(PAGE_SIZE - 1);
    let pages_needed = (stack_bottom - page_addr) / PAGE_SIZE;

    for i in 0..pages_needed {
        let vaddr = page_addr + i * PAGE_SIZE;
        if vaddr >= stack_bottom {
            break;
        }

        if raw::virt_to_phys(cr3, vaddr) != 0 {
            continue;
        }

        let new_page = raw::alloc_zeroed_user_page(
            cr3,
            vaddr,
            (PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER).bits(),
        );
        if new_page.is_null() {
            return false;
        }
    }

    proc_ref.store_stack_bottom(page_addr);
    true
}
