#![allow(dead_code)]
use super::process::{FdTable, Process, PROCESS_TABLE};
use super::types::{ProcessContext, ProcessId, ProcessPriority, ProcessState};
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

#[cfg(target_arch = "x86_64")]
const KERNEL_BASE: u64 = 0xFFFF800000000000;
#[cfg(target_arch = "aarch64")]
const KERNEL_BASE: u64 = 0;

#[no_mangle]
pub static user_entry_cr3: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
pub static user_entry_target: AtomicU64 = AtomicU64::new(0);

extern "C" {
    fn pmm_alloc_page() -> *mut u8;
    fn pmm_alloc_pages(count: u64) -> *mut u8;
    fn pmm_free_page(page: *mut u8);
    fn vmm_create_user_page_table() -> u64;
    fn vmm_map_page_in_table(table: u64, vaddr: u64, paddr: u64, flags: u64);
    fn vmm_map_page(vaddr: u64, paddr: u64, flags: u64) -> i32;
    fn vmm_split_2mb_page(vaddr: u64) -> i32;
    fn vmm_ensure_path_user(vaddr: u64);
    fn vmm_switch_page_table(table: u64);
    fn vmm_destroy_page_table(cr3: u64);
    fn vmm_get_physical_in_table(table: u64, vaddr: u64) -> u64;
    fn memset(s: *mut u8, c: i32, n: u64);
    fn memcpy(dest: *mut u8, src: *const u8, n: u64);
    fn kmalloc(size: u64) -> *mut u8;
}

/// Page size in bytes.
///
/// 统一从 `config.rs` 引用以避免分散定义。
pub use crate::kernel::framework::config::{
    PAGE_SIZE, USER_STACK_SIZE, USER_STACK_GUARD, USER_STACK_TOP, USER_KSTACK_SIZE,
    USER_STACK_MAX_SIZE, USER_CODE_BASE,
};

/// 派生常量: 用户栈自动扩展的下界 (USER_STACK_TOP - USER_STACK_MAX_SIZE)
pub const USER_STACK_EXPAND_LIMIT: u64 = USER_STACK_TOP - USER_STACK_MAX_SIZE;

pub const PAGE_PRESENT: u64 = 1;
pub const PAGE_WRITABLE: u64 = 2;
pub const PAGE_USER: u64 = 4;

pub const GDT_USER_DATA: u64 = 0x18;
pub const GDT_USER_CODE: u64 = 0x20;

pub const PT_LOAD: u32 = 1;

#[repr(C)]
pub struct ElfHeader {
    pub magic: [u8; 4],
    pub class: u8,
    pub endian: u8,
    pub version: u8,
    pub os_abi: u8,
    pub abi_version: u8,
    pub padding: [u8; 7],
    pub e_type: u16,
    pub machine: u16,
    pub e_version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

#[repr(C)]
pub struct ElfPhdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

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

        #[allow(dead_code)]
        #[inline(always)]
        pub fn as_ptr(self) -> *mut UserProcess {
            self.0
        }

        /// 访问 pid 字段 (读写)
        #[inline(always)]
        pub fn pid(&self) -> u32 {
            unsafe { (*self.0).pid }
        }

        #[inline(always)]
        pub fn set_pid(&self, v: u32) {
            unsafe {
                (*self.0).pid = v;
            }
        }

        /// 访问 entry 字段 (读写)
        #[inline(always)]
        pub fn entry(&self) -> u64 {
            unsafe { (*self.0).entry }
        }

        #[inline(always)]
        pub fn set_entry(&self, v: u64) {
            unsafe {
                (*self.0).entry = v;
            }
        }

        /// 访问 create_time 字段 (读写)
        #[inline(always)]
        pub fn create_time(&self) -> u64 {
            unsafe { (*self.0).create_time }
        }

        #[inline(always)]
        pub fn set_create_time(&self, v: u64) {
            unsafe {
                (*self.0).create_time = v;
            }
        }

        /// 访问 pwm/cr3/kernel_stack/user_stack/stack_bottom/state 原子字段
        #[inline(always)]
        pub fn load_pwm(&self) -> u64 {
            unsafe { (*self.0).pwm.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_pwm(&self, v: u64) {
            unsafe {
                (*self.0).pwm.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_cr3(&self) -> u64 {
            unsafe { (*self.0).cr3.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_cr3(&self, v: u64) {
            unsafe {
                (*self.0).cr3.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_kernel_stack(&self) -> u64 {
            unsafe { (*self.0).kernel_stack.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_kernel_stack(&self, v: u64) {
            unsafe {
                (*self.0).kernel_stack.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_user_stack(&self) -> u64 {
            unsafe { (*self.0).user_stack.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_user_stack(&self, v: u64) {
            unsafe {
                (*self.0).user_stack.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_stack_bottom(&self) -> u64 {
            unsafe { (*self.0).stack_bottom.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_stack_bottom(&self, v: u64) {
            unsafe {
                (*self.0).stack_bottom.store(v, Ordering::SeqCst);
            }
        }

        #[inline(always)]
        pub fn load_state(&self) -> u32 {
            unsafe { (*self.0).state.load(Ordering::SeqCst) }
        }

        #[inline(always)]
        pub fn store_state(&self, v: u32) {
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
    ///
    /// # Safety (内部)
    /// - 必须持有 USER_PROC_MANAGER 锁或保证 current 不会并发改变。
    pub fn current_proc() -> Option<UserProcRef> {
        let cur = USER_PROC_MANAGER.current.load(Ordering::SeqCst);
        if cur == 0 {
            None
        } else {
            // SAFETY: cur > 0, 此前由 set_current 设为有效的 NonNull 指针。
            Some(unsafe { UserProcRef::new_unchecked(cur as *mut UserProcess) })
        }
    }

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

    /// 释放多页连续物理页。
    ///
    /// # Safety (内部)
    /// - `pages` 必须为 `alloc_phys_pages` 返回的合法物理页基址。
    pub fn free_phys_pages(pages: *mut u8, count: u64) {
        for i in 0..count {
            raw::free_phys_page((pages as u64 + i * PAGE_SIZE) as *mut u8);
        }
    }

    /// 分配一页物理页 (单页)。
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
            let phys = vmm_get_physical_in_table(cr3, off as u64 & !0xFFF);
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
            let phys = vmm_get_physical_in_table(cr3, off as u64 & !0xFFF);
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
        let flags = PAGE_PRESENT | PAGE_USER;
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

    /// 物理页 → 内核可写指针 (用于代码段 chunk 复制)。
    pub fn phys_to_kern_mut(phys: u64, off: u64) -> *mut u8 {
        (phys + KERNEL_BASE + off) as *mut u8
    }

    /// ELF 文件指针 + 偏移。
    pub fn elf_ptr_at(elf_data: *const u8, off: usize) -> *const u8 {
        // SAFETY: 调用方保证 off 在 elf_size 范围内。
        unsafe { elf_data.add(off) }
    }

    /// 切换到用户页表 (aarch64 用户态进入前)。
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
    pub fn alloc_user_process() -> Option<*mut UserProcess> {
        let size = core::mem::size_of::<UserProcess>() as u64;
        let ptr = raw::alloc_zeroed(size) as *mut UserProcess;
        if ptr.is_null() {
            None
        } else {
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
        use crate::kernel::framework::proc_tcb_legacy::scheduler::SchedPolicy;
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

#[repr(C)]
pub struct UserProcess {
    pub pid: u32,
    pub pwm: AtomicU64,
    pub cr3: AtomicU64,
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,
    pub stack_bottom: AtomicU64,
    pub entry: u64,
    pub state: AtomicU32,
    pub create_time: u64,
}

// All fields (u32, AtomicU64, AtomicU32, u64) are Send + Sync.
unsafe impl Send for UserProcess {}
unsafe impl Sync for UserProcess {}

pub struct UserProcManager {
    current: AtomicU64,
    // 使用 NonNull<UserProcess> 替代 *mut UserProcess,
    // 使 BTreeMap 自动实现 Send + Sync (NonNull: Send + Sync when T: Send + Sync)。
    processes: Mutex<alloc::collections::BTreeMap<u32, NonNull<UserProcess>>>,
}

// SAFETY: UserProcManager is always accessed via static USER_PROC_MANAGER.
// All mutations go through the Mutex, and the NonNull pointers target
// UserProcess objects whose fields are all Atomic* or plain integers.
unsafe impl Send for UserProcManager {}
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
        // SAFETY: proc is a NonNull<UserProcess> that was allocated via kmalloc
        // and inserted into the BTreeMap. It remains valid until destroyed.
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
            // SAFETY: ptr is a NonNull<UserProcess> from the BTreeMap.
            // The process lives for the lifetime of the manager; processes
            // are never freed while the lock is held.
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

    pub fn create(&self, info: &UserProcInfo, pwm: u64) -> Option<*mut UserProcess> {
        let pid = PROCESS_TABLE.allocate_pid()?;

        // 分配并清零 UserProcess 内存
        let proc_ptr = raw::alloc_user_process()?;
        let proc = raw::new_proc_ref(proc_ptr);

        // 创建用户页表
        let cr3_val = raw::create_user_page_table();
        proc.store_cr3(cr3_val);
        if cr3_val == 0 {
            return None;
        }

        // 分配用户栈
        let stack_pages = raw::alloc_phys_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
        if stack_pages.is_null() {
            raw::destroy_user_page_table(cr3_val);
            return None;
        }

        let stack_phys = stack_pages as u64;
        let stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;

        for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
            let svirt = stack_virt + USER_STACK_GUARD + i * PAGE_SIZE;
            let sphys = stack_phys + i * PAGE_SIZE;
            raw::vmm_map_user_page(
                cr3_val,
                svirt,
                sphys,
                PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER,
            );
        }

        proc.store_user_stack(USER_STACK_TOP);
        let initial_stack_bottom = USER_STACK_TOP - USER_STACK_SIZE;
        proc.store_stack_bottom(initial_stack_bottom);

        // 分配内核栈
        let kstack = raw::alloc_phys_pages(USER_KSTACK_SIZE / PAGE_SIZE);
        if kstack.is_null() {
            raw::free_phys_page(stack_pages);
            raw::destroy_user_page_table(cr3_val);
            return None;
        }
        let kstack_top = kstack as u64 + KERNEL_BASE + USER_KSTACK_SIZE;
        proc.store_kernel_stack(kstack_top);
        crate::kernel::framework::proc_tcb_legacy::process::kernel_stack_write_canary(kstack_top);

        proc.set_pid(pid);
        proc.set_entry(info.entry);
        proc.store_pwm(pwm);
        proc.store_state(1);
        proc.set_create_time(crate::kernel::framework::timer::get_ticks());

        self.processes
            .lock()
            .insert(pid, NonNull::new(proc_ptr).unwrap());

        // 分配并构造 Process (用于 process table)
        let kproc_ptr = raw::alloc_kernel_process()?;
        raw::init_kernel_process_fields(
            kproc_ptr,
            pid,
            pwm,
            cr3_val,
            proc.load_kernel_stack(),
            proc.load_user_stack(),
        );

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
        #[cfg(target_arch = "aarch64")]
        let cr3 = proc_ref.load_cr3();
        let _ss_val = GDT_USER_DATA | 0x03;
        let _cs_val = GDT_USER_CODE | 0x03;
        let _rflags_val: u64 = 0x3202;

        crate::kernel::framework::cpu::arch::set_kernel_stack(kstack);

        // On aarch64, TTBR0_EL1 must point to the user page table before
        // entering EL0. The user L0 table has kernel identity-mapped entries
        // copied, so kernel code and MMIO remain accessible after the switch.
        #[cfg(target_arch = "aarch64")]
        {
            raw::vmm_switch_to_user(cr3);
        }

        // SAFETY: enter_user 是平台特定的 arch 入口, 不会返回, 由调用方保证上下文有效。
        unsafe {
            crate::arch!(enter_user(rip_val as usize, rsp_val as usize, 0));
        }
    }

    /// Write argc/argv/envp to the user process stack
    /// Returns the new stack pointer (RSP) after setup
    ///
    /// # Safety
    ///
    /// `pid` corresponds to a live process. Process table lock must be held by caller.
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

        // Space needed: argc(8) + argv_ptrs(8*(argc+1)) + envp_ptrs(8*(envc+1)) + strings
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
        // Ensure 16-byte alignment
        let total = (total + 15) & !15;

        if total as u64 > stack_top - USER_STACK_TOP + USER_STACK_SIZE {
            return 0; // Stack overflow
        }

        let new_sp = stack_top - total as u64;
        let mut pos = new_sp as usize;

        // Write argc
        let argc_off = pos;
        pos += 8;
        // Skip argv ptrs (written after strings)
        let argv_start_off = pos;
        pos += (argc + 1) * 8;
        // Skip envp ptrs
        let envp_start_off = pos;
        pos += (envc + 1) * 8;
        // String area starts here
        let strings_off = pos;

        // Write argc
        raw::write_user_u64(cr3, argc_off, argc as u64);

        // Write argv strings + pointers
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
        // argv NULL terminator
        raw::write_user_u64(cr3, argv_start_off + argc * 8, 0u64);

        // envp pointers (all NULL for now)
        for i in 0..(envc + 1) {
            raw::write_user_u64(cr3, envp_start_off + i * 8, 0u64);
        }

        // Update process stack pointer
        proc_ref.store_user_stack(new_sp);
        new_sp
    }

    pub fn load_elf_from_memory(&self, elf_data: *const u8, elf_size: u64, pwm: u64) -> i32 {
        if elf_data.is_null() || elf_size < core::mem::size_of::<ElfHeader>() as u64 {
            return -1;
        }

        // SAFETY: elf_data 区间已校验 (非空 + size >= header), 内部访问通过 raw 包装。
        unsafe {
            let header = elf_data as *const ElfHeader;

            if (*header).magic[0] != 0x7F
                || (*header).magic[1] != b'E'
                || (*header).magic[2] != b'L'
                || (*header).magic[3] != b'F'
            {
                return -1;
            }

            // Accept ELF64 for both x86_64 (0x3E) and AArch64 (0xB7)
            if (*header).class != 2 || ((*header).machine != 0x3E && (*header).machine != 0xB7) {
                return -1;
            }

            let info = UserProcInfo {
                entry: (*header).entry,
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

            // Use static array to avoid 8KB stack allocation
            static ALLOCATED_PAGES: crate::kernel::framework::racy_cell::RacyCell<[u64; 1024]> =
                crate::kernel::framework::racy_cell::RacyCell::new([0; 1024]);
            let allocated_pages = ALLOCATED_PAGES.get_mut();
            let mut page_count: usize = 0;

            let phnum = (*header).phnum as usize;
            if phnum > 256 {
                self.destroy_raw(proc, false);
                return -1;
            }

            for i in 0..phnum {
                let phdr_size = core::mem::size_of::<ElfPhdr>() as u64;
                let phdr_offset = (*header).phoff + (i as u64) * (*header).phentsize as u64;
                if phdr_offset + phdr_size > elf_size {
                    self.destroy_raw(proc, false);
                    return -1;
                }
                let phdr = (elf_data.add(phdr_offset as usize)) as *const ElfPhdr;

                if (*phdr).p_type != PT_LOAD {
                    continue;
                }

                let vaddr_start = (*phdr).p_vaddr & !0xFFF;
                let vaddr_end = ((*phdr).p_vaddr + (*phdr).p_memsz + 0xFFF) & !0xFFF;
                let num_pages = (vaddr_end - vaddr_start) / PAGE_SIZE;

                for j in 0..num_pages {
                    let vaddr = vaddr_start + j * PAGE_SIZE;

                    let mut flags = PAGE_PRESENT | PAGE_USER;
                    if (*phdr).p_flags & 0x02 != 0 {
                        flags |= PAGE_WRITABLE;
                    }

                    // On aarch64, the 2MB BLOCK descriptors in L2_DEVICE cause
                    // vmm_get_physical_in_table to return non-zero for unmapped
                    // user addresses. Skip the reuse check and always allocate.
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
                        // Reuse existing page, record it
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

            proc_ref.set_entry((*header).entry);

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
}

/// 分配一个新的 PID（供 sys_fork 使用）
#[no_mangle]
pub extern "C" fn proc_alloc_pid() -> u32 {
    PROCESS_TABLE.allocate_pid().unwrap_or(0)
}

/// 克隆父进程的 UserProcess 给子进程（供 sys_fork 使用）
/// 子进程的 CR3 和内核栈已在 sys_fork 中分配好，此处仅创建 UserProcess 记录
#[no_mangle]
pub extern "C" fn user_proc_clone(parent_pid: u32, child_pid: u32) -> i32 {
    let parent_proc = match USER_PROC_MANAGER.get(parent_pid) {
        Some(p) => p,
        None => return -1,
    };

    let child_kernel_proc = match PROCESS_TABLE.get(child_pid) {
        Some(p) => p,
        None => return -1,
    };

    // SAFETY: parent_proc / child_kernel_proc 均来自管理器, 有效。
    unsafe {
        let parent_ref = UserProcRef::new_unchecked(parent_proc);
        let child_up = raw::alloc_user_process().unwrap_or_default();
        if child_up.is_null() {
            return -1;
        }
        let child_ref = UserProcRef::new_unchecked(child_up);

        child_ref.set_pid(child_pid);
        child_ref.store_pwm(parent_ref.load_pwm());
        child_ref.store_cr3((*child_kernel_proc).cr3.load(Ordering::SeqCst));
        child_ref.store_kernel_stack(
            (*child_kernel_proc).kernel_stack.load(Ordering::SeqCst),
        );
        child_ref.store_user_stack(parent_ref.load_user_stack());
        child_ref.store_stack_bottom(parent_ref.load_stack_bottom());
        child_ref.set_entry(parent_ref.entry());
        child_ref.store_state(1);
        child_ref.set_create_time(crate::kernel::framework::timer::get_ticks());

        USER_PROC_MANAGER
            .processes
            .lock()
            .insert(child_pid, NonNull::new(child_up).unwrap());
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
            PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER,
        );
        if new_page.is_null() {
            return false;
        }
    }

    proc_ref.store_stack_bottom(page_addr);
    true
}
