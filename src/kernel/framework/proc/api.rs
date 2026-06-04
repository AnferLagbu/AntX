//! 进程管理子系统 API 层
//!
//! 为内核其它模块提供进程/线程/调度的统一入口。
//!
//! ## 调用方契约
//! - `syscall::mod` —— fork/execve/exit/wait4/kill/getpid 等系统调用
//! - `syscall::mmap` —— mmap 通过 `process_get_current_pid` 获取当前进程
//! - `ipc::pipe/shm/signal` —— IPC 操作需关联当前进程 PID 和 PWM
//! - `barrier::recovery` —— 进程域纳入栏栈恢复
//! - `credo::session` —— 会话管理器注册/注销进程
//! - `fs::procfs` —— `/proc` 文件系统读取进程列表
//!
//! ## 安全约束
//! - `CURRENT_PROCESS_PTR` 用 AtomicU64 无锁读写,但 `C_CURRENT_PROCESS` 是 unsafe static mut
//! - `process_get_current()` 懒初始化 init 进程 (pid=1)
//! - `process_exit()` 必须在内核态调用,退出前切换到内核 CR3
//! - `PROCESS_TABLE` / `SCHEDULER` 均为全局单例,内部有锁保护
//!
//! ## 性能特征
//! - 进程查找: O(1) 哈希表
//! - 进程创建: O(N) PID 扫描 (N ≤ 65536)
//! - 上下文切换: asm stub, ~200 CPU cycles
//!
//! 设计目标:
//! - 隐藏内部实现细节(scheduler/scheduler_ex/process/thread)
//! - `#[no_mangle]` 提供稳定的跨模块符号名
//! - 错误路径返回 -1 / null,调用方按需检查
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::process::{Process, PROCESS_TABLE};
use super::scheduler::SCHEDULER;
use super::scheduler_ex::SCHEDULER_EX;
use super::session::SESSION_MANAGER;
use super::thread::THREAD_MANAGER;
use super::types::*;
use super::user_proc::{proc_alloc_pid, user_proc_clone, USER_PROC_MANAGER};
use crate::kernel::framework::lib::cstr::CStrExt;
use crate::kernel::framework::klog::klog_ffi_info;
use crate::kernel::framework::mm::api::{
    pmm_alloc_pages, pmm_free_pages, vmm_clone_user_page_table_cow, vmm_destroy_page_table,
    vmm_switch_page_table,
};
use crate::kernel::framework::timer::timer_get_ticks;

// === 特权层: 进程子系统裸指针/FFI 桥接集中地 ===
//
// 本子模块包含所有与 C ABI、裸指针 (进程表 entry) 以及 extern "C" FFI 交互
// 的 `unsafe` 代码。本模块的其余部分 (`api.rs` 顶层) 保持 100% 安全 Rust,
// 通过 `raw::*` 安全函数访问底层功能。
pub(crate) mod raw {
    use super::*;

    /// 从裸指针读取 `&Process`。
    ///
    /// # Safety (内部)
    /// - `ptr` 必须为非空, 指向有效 `Process` 实例
    pub fn process_ref<'a>(ptr: *const Process) -> &'a Process {
        // SAFETY: 调用方在 fork/exec 路径中保证 ptr 来自 PROCESS_TABLE
        unsafe { &*ptr }
    }

    /// 从裸指针读取 `&mut Process`。
    ///
    /// # Safety (内部)
    /// - `ptr` 必须为非空, 指向有效 `Process` 实例
    /// - 同一时刻只能存在一个可变引用
    pub fn process_ref_mut<'a>(ptr: *mut Process) -> &'a mut Process {
        // SAFETY: 调用方在 fork 路径中保证 `&mut` 唯一性
        unsafe { &mut *ptr }
    }

    /// 分配并构造一个 `Process` (用于 fork 创建子进程)。
    ///
    /// # Safety (内部)
    /// - 调用方在 fork 错误路径中负责 `dealloc_process` 释放。
    pub fn alloc_process(pid: u32, name: &str, parent: Option<ProcessId>) -> *mut Process {
        // SAFETY: alloc/dealloc 配对, 见 dealloc_process。
        unsafe {
            let layout = alloc::alloc::Layout::new::<Process>();
            let ptr = alloc::alloc::alloc(layout) as *mut Process;
            core::ptr::write(ptr, Process::new(pid, name, parent));
            ptr
        }
    }

    /// 释放 `alloc_process` 分配的内存。
    ///
    /// # Safety (内部)
    /// - `ptr` 必须由 `alloc_process` 产生且未被释放。
    #[allow(dead_code)]
    pub fn dealloc_process(ptr: *mut Process) {
        if !ptr.is_null() {
            // SAFETY: alloc/dealloc 配对。
            unsafe {
                let layout = alloc::alloc::Layout::new::<Process>();
                alloc::alloc::dealloc(ptr as *mut u8, layout);
            }
        }
    }

    /// 从裸指针 (Box 所有权) 还原 Box 并 drop。
    ///
    /// # Safety (内部)
    /// - `ptr` 必须由 `Box::into_raw` 产生且未被 drop。
    pub fn drop_boxed_process(ptr: *mut Process) {
        if !ptr.is_null() {
            // SAFETY: Box 所有权还原。
            unsafe { drop(alloc::boxed::Box::from_raw(ptr)) }
        }
    }

    /// 复制父进程内核栈内容到子进程。
    ///
    /// # Safety (内部)
    /// - `dst` 和 `src` 必须都是有效的、已映射的、可写的内核栈地址。
    /// - 区间 `[src, src+size)` 与 `[dst, dst+size)` 不得重叠。
    pub fn copy_kstack(dst: u64, src: u64, size: usize) {
        // SAFETY: 调用方在 fork 路径中保证栈映射有效且区间不重叠。
        unsafe { core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, size) }
    }

    /// 释放 `vmm_clone_user_page_table_cow` 产生的用户页表。
    ///
    /// # Safety (内部)
    /// - `cr3` 必须为 `vmm_clone_user_page_table_cow` 返回的非零物理地址。
    pub fn destroy_user_page_table(cr3: u64) {
        // SAFETY: cr3 由 vmm_clone_user_page_table_cow 创建。
        unsafe { vmm_destroy_page_table(cr3) }
    }

    /// 调用 `vmm_switch_page_table` (特权级页表切换)。
    ///
    /// # Safety (内部)
    /// - `cr3` 必须为有效的已建立物理页表基址。
    pub fn switch_page_table(cr3: u64) {
        // SAFETY: cr3 是从 vmm_clone_user_page_table_cow / kernel_pml4 获取的合法值。
        unsafe { vmm_switch_page_table(cr3) }
    }

    /// 调用 `vmm_clone_user_page_table_cow` (fork 路径, COW 共享页表)。
    ///
    /// # Safety (内部)
    /// - `parent_cr3` 必须是有效的、已建立的用户页表基址 (由 process.cr3 提供)。
    /// - 调用方在子进程使用完毕后, 通过 `destroy_user_page_table` 释放。
    pub fn clone_user_page_table_cow(parent_cr3: u64) -> u64 {
        // SAFETY: parent_cr3 来自 process.cr3, 已建立的页表。
        unsafe { vmm_clone_user_page_table_cow(parent_cr3) }
    }

    /// 通过 FFI 输出 info 级别日志。
    ///
    /// # Safety (内部, 由本模块封闭)
    /// - `msg` 必须为以 `\0` 结尾的有效 C 字符串。
    pub fn klog_info(msg: &[u8]) {
        // SAFETY: msg 来自上层调用, 上层 `api.rs` 中只传入静态字节串字面量。
        unsafe { klog_ffi_info(msg.as_ptr()) }
    }
}

static CURRENT_PROCESS_PTR: AtomicU64 = AtomicU64::new(0);
static INIT_PROCESS_CREATED: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
#[derive(Copy, Clone)]
struct CProcess {
    pid: u64,
    session_id: u64,
    parent_pid: u64,
    pwm: u64,
    state: u32,
    exit_code: u64,
    priority: i32,
    cpu_time: u64,
    start_time: u64,
    time_slice: u64,
}

impl CProcess {
    const fn zero() -> Self {
        Self {
            pid: 0,
            session_id: 0,
            parent_pid: 0,
            pwm: 0,
            state: 0,
            exit_code: 0,
            priority: 2,
            cpu_time: 0,
            start_time: 0,
            time_slice: 10,
        }
    }
}

use crate::kernel::framework::racy_cell::RacyCell;

static C_CURRENT_PROCESS: RacyCell<CProcess> = RacyCell::new(CProcess::zero());

#[no_mangle]
pub fn process_get_current() -> u64 {
    let ptr = CURRENT_PROCESS_PTR.load(Ordering::SeqCst);
    if ptr == 0 {
        if INIT_PROCESS_CREATED.load(Ordering::SeqCst) == 0 {
            create_init_process();
        }
        CURRENT_PROCESS_PTR.load(Ordering::SeqCst)
    } else {
        ptr
    }
}

fn create_init_process() {
    INIT_PROCESS_CREATED.store(1, Ordering::SeqCst);
    C_CURRENT_PROCESS.map_mut(|p| {
        p.pid = 1;
        p.state = 2;
        p.priority = 2;
        p.time_slice = 10;
    });
    // SAFETY: klog_ffi_info is unsafe extern "C". msg is a valid static byte slice.
    raw::klog_info(b"[PROC] Init process created (pid=1)");
    CURRENT_PROCESS_PTR.store(
        C_CURRENT_PROCESS.as_ptr() as u64,
        Ordering::SeqCst,
    );
}

#[no_mangle]
pub fn update_current_process_ptr(ptr: u64) {
    CURRENT_PROCESS_PTR.store(ptr, Ordering::SeqCst);
    if ptr != 0 {
        let proc_ptr = ptr as *const Process;
        // SAFETY: proc_ptr is a valid Process pointer from the process table.
        let proc = raw::process_ref(proc_ptr);
        let pwm_val = proc.get_pwm();
        let pid_val = proc.pid.0 as u64;
        C_CURRENT_PROCESS.map_mut(|p| {
            p.pid = pid_val;
            p.pwm = pwm_val;
        });
    }
}

#[no_mangle]
pub fn process_get_current_pid() -> u32 {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub fn process_get_by_pid(_pid: u32) -> u64 {
    if _pid as u64 == C_CURRENT_PROCESS.map(|p| p.pid) {
        C_CURRENT_PROCESS.as_ptr() as u64
    } else {
        PROCESS_TABLE.get(_pid).map(|p| p as u64).unwrap_or(0)
    }
}

#[no_mangle]
pub fn process_get_current_pwm() -> u64 {
    let pid = SCHEDULER.current().unwrap_or(0);
    if pid == 0 {
        return 0;
    }
    PROCESS_TABLE.with_process(pid, |p| p.get_pwm()).unwrap_or(0)
}

#[no_mangle]
pub fn process_get_pwm_by_pid(pid: u32) -> u64 {
    if pid == 0 {
        return 0;
    }
    PROCESS_TABLE.with_process(pid, |p| p.get_pwm()).unwrap_or(0)
}

#[no_mangle]
pub fn process_create(name: *const u8, parent_pid: Pid, pwm: u64) -> Pid {
    proc_create_internal(name, parent_pid, pwm)
}

#[no_mangle]
pub fn process_exit(exit_code: u32) {
    let current_pid = SCHEDULER.current().unwrap_or(0);
    if current_pid != 0 {
        let kernel_cr3 = crate::kernel::framework::mm::vmm::get_kernel_pml4();
        if kernel_cr3 != 0 {
            // SAFETY: kernel_cr3 是从 vmm::get_kernel_pml4() 获取的合法页表。
            raw::switch_page_table(kernel_cr3);
        }
        USER_PROC_MANAGER.destroy_by_pid_no_kstack(current_pid);
    }
    SCHEDULER.exit(exit_code);
}

#[no_mangle]
pub fn process_kill(pid: u32, exit_code: u32) {
    if pid == 0 {
        return;
    }

    let should_kill = PROCESS_TABLE
        .with_process(pid, |proc| {
            let state = proc.get_state();
            if state == ProcessState::Zombie || state == ProcessState::Terminated {
                false
            } else {
                proc.exit_code.store(exit_code, Ordering::SeqCst);
                let _ = proc.set_state_safe(ProcessState::Zombie);
                true
            }
        })
        .unwrap_or(false);

    if should_kill {
        SCHEDULER.unblock(pid);
        SCHEDULER.set_need_reschedule();
    }
}

#[no_mangle]
pub fn process_find_by_pid(pid: Pid) -> u64 {
    PROCESS_TABLE.get(pid).map(|p| p as u64).unwrap_or(0)
}

#[no_mangle]
pub fn proc_has_runnable() -> i32 {
    if SCHEDULER.has_any_runnable() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub fn thread_get_current() -> u64 {
    THREAD_MANAGER.get_current_thread().unwrap_or(0)
}

#[no_mangle]
pub fn scheduler_yield_ex() {
    SCHEDULER_EX.yield_current();
}

#[no_mangle]
pub fn scheduler_yield() {
    SCHEDULER.yield_current();
}

#[no_mangle]
pub fn scheduler_schedule() -> Pid {
    SCHEDULER.schedule().unwrap_or(0)
}

#[no_mangle]
pub fn scheduler_add(pid: Pid) {
    SCHEDULER.add(pid);
}

#[no_mangle]
pub fn wait_queue_init(_wq: *mut u8) {}

#[no_mangle]
pub fn wait_queue_add(_wq: *mut u8, _thread: u64) {}

#[no_mangle]
pub fn wait_queue_wake_one(_wq: *mut u8) {}

#[no_mangle]
pub fn wait_queue_wake_all(_wq: *mut u8) {}

#[no_mangle]
pub fn session_init() {
    SESSION_MANAGER.init();
}

#[no_mangle]
pub fn user_proc_init() {
    USER_PROC_MANAGER.init();
}

const ELF_MAX_SIZE: usize = 1024 * 1024;

#[no_mangle]
pub fn user_proc_load_elf(path: *const u8, pwm: u64) -> i32 {
    if path.is_null() {
        return -1;
    }

    let mut st: crate::kernel::framework::fs::vfs::types::VfsStat = crate::kernel::framework::fs::vfs::types::VfsStat::default();
    let stat_result = crate::kernel::framework::fs::vfs::api::vfs_stat(path, &mut st, pwm);
    if stat_result < 0 {
        return -1;
    }

    let file_size = st.size as u64;
    if file_size == 0 || file_size > ELF_MAX_SIZE as u64 {
        return -1;
    }

    let fd = crate::kernel::framework::fs::vfs::api::vfs_open(path, 0, pwm);
    if fd < 0 {
        return -1;
    }

    let pages = file_size.div_ceil(4096u64) as usize;
    let buffer = pmm_alloc_pages(pages);
    if buffer.is_null() {
        crate::kernel::framework::fs::vfs::api::vfs_close(fd as u32);
        return -1;
    }

    let bytes_read =
        crate::kernel::framework::fs::vfs::api::vfs_read(fd as u32, buffer as *mut u8, file_size as u32);

    crate::kernel::framework::fs::vfs::api::vfs_close(fd as u32);

    if bytes_read <= 0 {
        pmm_free_pages(buffer, pages);
        return -1;
    }

    let result =
        USER_PROC_MANAGER.load_elf_from_memory(buffer as *const u8, bytes_read as u64, pwm);

    pmm_free_pages(buffer, pages);

    result
}

#[no_mangle]
pub fn user_proc_load_elf_from_memory(
    elf_data: *const u8,
    elf_size: u64,
    pwm: u64,
) -> i32 {
    USER_PROC_MANAGER.load_elf_from_memory(elf_data, elf_size, pwm)
}

/// Set up argv/envp on user stack after ELF loading (for exec syscall)
#[no_mangle]
///
/// # Safety
///
/// `name` is a valid null-terminated C string. Process table has been initialized.
pub unsafe fn user_proc_setup_argv(
    pid: u32,
    argv: *const *const u8,
    argc: u32,
    envp: *const *const u8,
    envc: u32,
) -> i32 {
    let proc = match USER_PROC_MANAGER.get(pid) {
        Some(p) => p,
        None => return -1,
    };

    let sp = USER_PROC_MANAGER.setup_user_stack(proc, argv, argc as usize, envp, envc as usize);

    if sp == 0 {
        -1
    } else {
        0
    }
}

#[no_mangle]
pub fn user_proc_enter_by_pid(pid: u32) -> i32 {
    let (pid_val, pwm_val, state_val) = USER_PROC_MANAGER
        .with_process(pid, |proc| {
            (
                proc.pid as u64,
                proc.pwm.load(Ordering::SeqCst),
                proc.state.load(Ordering::SeqCst),
            )
        })
        .unwrap_or((0, 0, 0));

    if pid_val == 0 {
        return -1;
    }

    C_CURRENT_PROCESS.map_mut(|p| {
        p.pid = pid_val;
        p.pwm = pwm_val;
        p.state = state_val;
        p.parent_pid = 1;
    });

    if let Some(proc) = USER_PROC_MANAGER.get(pid) {
        USER_PROC_MANAGER.enter(proc);
        0
    } else {
        -1
    }
}

#[no_mangle]
pub fn launch_first_user_process() -> ! {
    crate::klog_boot_info!("[USER] Launching init process...");

    #[cfg(target_arch = "x86_64")]
    let bin = include_bytes!("../../../../build/user/init.bin");

    #[cfg(target_arch = "x86_64")]
    {
        let bin_ptr = bin.as_ptr();
        let bin_size = bin.len() as u64;

        if bin_size == 0 {
            crate::klog_err!(Boot, "[USER] init binary is empty");
            crate::kernel::framework::tests::qemu_exit(false);
        }

        let pid = USER_PROC_MANAGER.load_elf_from_memory(bin_ptr, bin_size, 0);
        if pid <= 0 {
            crate::klog_err!(Boot, "[USER] Failed to load init ELF, pid={}", pid);
            crate::kernel::framework::tests::qemu_exit(false);
        }

        let pid_u32 = pid as u32;

        C_CURRENT_PROCESS.map_mut(|p| {
            p.pid = pid_u32 as u64;
            p.pwm = 0;
            p.state = 2;
            p.parent_pid = 1;
        });

        SCHEDULER.add(pid_u32);

        crate::klog_boot_info!("[USER] Entering Ring 3 (init pid={})...", pid_u32);
        user_proc_enter_by_pid(pid_u32);
    }

    #[cfg(target_arch = "aarch64")]
    {
        // On aarch64, init.bin is an AArch64 ELF binary built from src/user/
        let bin = include_bytes!("../../../../build/user/init.bin");
        let bin_ptr = bin.as_ptr();
        let bin_size = bin.len() as u64;

        if bin_size == 0 {
            crate::klog_boot_info!("[USER] init ELF is empty");
            loop {
                crate::arch!(halt());
            }
        }

        let pid = USER_PROC_MANAGER.load_elf_from_memory(bin_ptr, bin_size, 0);
        if pid <= 0 {
            crate::klog_boot_info!("[USER] Failed to load init ELF");
            loop {
                crate::arch!(halt());
            }
        }

        let pid_u32 = pid as u32;

        C_CURRENT_PROCESS.map_mut(|p| {
            p.pid = pid_u32 as u64;
            p.pwm = 0;
            p.state = 2;
            p.parent_pid = 1;
        });

        SCHEDULER.add(pid_u32);

        crate::klog_boot_info!("[USER] Entering EL0 (init pid={})...", pid_u32);
        user_proc_enter_by_pid(pid_u32);
    }

    loop {
        crate::arch!(halt());
    }
}

#[no_mangle]
pub fn scheduler_tick() {
    SCHEDULER_EX.tick();
}

#[no_mangle]
pub fn scheduler_init() {
    super::scheduler::init();
    SCHEDULER_EX.init();
}

#[no_mangle]
pub fn process_init() {}

#[no_mangle]
pub fn thread_init() {
    super::thread::init();
}

#[no_mangle]
pub fn proc_create_internal(name: *const u8, parent_pid: Pid, pwm: u64) -> Pid {
    if name.is_null() {
        return 0;
    }

    let name_str = match name.as_kstr_opt() {
        Some(s) if !s.is_empty() => s,
        _ => return 0,
    };

    let parent = if parent_pid == 0 {
        None
    } else {
        Some(parent_pid)
    };

    SCHEDULER.create_process(name_str, parent, pwm).unwrap_or(0)
}

#[no_mangle]
pub fn scheduler_get_current_pwm() -> u64 {
    SCHEDULER.current()
        .and_then(|pid| PROCESS_TABLE.with_process(pid, |p| p.get_pwm()))
        .unwrap_or(0)
}

#[no_mangle]
pub fn scheduler_set_quota(pwm: u64, max_runtime: u64, period: u64) {
    SCHEDULER.set_quota(pwm, max_runtime, period);
}

#[no_mangle]
pub fn scheduler_remove_quota(pwm: u64) {
    SCHEDULER.remove_quota(pwm);
}

#[no_mangle]
pub fn scheduler_set_proc_limit(pwm: u64, max_procs: u32) {
    SCHEDULER.set_limit(pwm, max_procs);
}

#[no_mangle]
pub fn proc_exit_internal(exit_code: u32) {
    SCHEDULER.exit(exit_code);
}

#[no_mangle]
pub fn proc_get_current_pid_internal() -> Pid {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub fn proc_yield_internal() {
    SCHEDULER.yield_current();
}

#[no_mangle]
pub fn proc_block(reason: u32) {
    let block_reason = BlockReason::from_u8(reason as u8);
    SCHEDULER.block(block_reason);
}

#[no_mangle]
pub fn proc_unblock(pid: Pid) {
    SCHEDULER.unblock(pid);
}

#[no_mangle]
pub fn proc_set_priority(pid: Pid, priority: u32) -> i32 {
    if PROCESS_TABLE.with_process(pid, |p| {
        p.set_priority(ProcessPriority::from_u32(priority));
    }).is_some()
    {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub fn proc_get_state(pid: Pid) -> u32 {
    PROCESS_TABLE.with_process(pid, |p| p.get_state() as u32)
        .unwrap_or(ProcessState::Terminated as u32)
}

#[no_mangle]
pub fn sched_init_internal() {
    SCHEDULER.init();
}

#[no_mangle]
pub fn sched_add_internal(pid: Pid) {
    SCHEDULER.add(pid);
}

#[no_mangle]
pub fn sched_schedule_internal() -> Pid {
    SCHEDULER.schedule().unwrap_or(0)
}

#[no_mangle]
pub fn sched_should_reschedule() -> i32 {
    if SCHEDULER.should_reschedule() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub fn sched_set_current(pid: Pid) {
    SCHEDULER.set_current(pid);
}

#[no_mangle]
pub fn sched_get_current() -> Pid {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub fn proc_get_exit_code(pid: Pid) -> i32 {
    PROCESS_TABLE.with_process(pid, |p| p.exit_code.load(Ordering::SeqCst) as i32)
        .unwrap_or(-1)
}

#[no_mangle]
pub fn proc_is_initialized() -> i32 {
    if SCHEDULER.is_initialized() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub fn scheduler_get_time_slice() -> u64 {
    SCHEDULER.get_time_slice()
}

#[no_mangle]
pub fn scheduler_get_current_level() -> u32 {
    SCHEDULER.get_current_level()
}

#[no_mangle]
pub fn scheduler_tick_mlfq() {
    let cpu = crate::kernel::framework::smp::get_current_cpu() as usize;
    SCHEDULER.tick(cpu)
}

#[no_mangle]
pub fn scheduler_boost_priority() {
    SCHEDULER.boost_priority()
}

#[no_mangle]
pub fn scheduler_add_with_priority(pid: Pid, level: usize) {
    SCHEDULER.add_with_priority(pid, level)
}

#[no_mangle]
pub fn scheduler_add_rt_task(pid: Pid, rt_priority: u8, policy: u32) {
    use super::scheduler::SchedPolicy;
    SCHEDULER.add_rt_task(pid, rt_priority, SchedPolicy::from_u32(policy))
}

#[no_mangle]
pub fn scheduler_set_sched_policy(pid: Pid, policy: u32, rt_priority: u8) -> i32 {
    use super::scheduler::SchedPolicy;
    if SCHEDULER.set_sched_policy(pid, SchedPolicy::from_u32(policy), rt_priority) {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub fn scheduler_get_rt_count() -> usize {
    SCHEDULER.get_rt_count()
}

#[no_mangle]
pub fn proc_create_user(
    path: *const u8,
    argv: *const *const u8,
    argc: u32,
    pwm: u64,
) -> Pid {
    if path.is_null() {
        return 0;
    }

    let parent_pid = SCHEDULER.current().unwrap_or(0);
    let name_str = unsafe {
        // SAFETY: path is non-null C string from caller (C ABI contract).
        let cstr = core::ffi::CStr::from_ptr(path as *const core::ffi::c_char);
        cstr.to_str().unwrap_or("user")
    };

    let child_pid = SCHEDULER
        .create_process(
            name_str,
            if parent_pid != 0 {
                Some(parent_pid)
            } else {
                None
            },
            pwm,
        )
        .unwrap_or(0);
    if child_pid == 0 {
        return 0;
    }

    // Create session for the new user process
    if let Some(sid) = SESSION_MANAGER.create(pwm) {
        PROCESS_TABLE.with_process(child_pid, |proc| {
            proc.session_id.store(sid, Ordering::SeqCst);
        });
    }

    // Initialize per-process fd_table
    PROCESS_TABLE.with_process(child_pid, |proc| {
        proc.fd_table.init();
    });

    let load_result = user_proc_load_elf(path, pwm);
    if load_result < 0 {
        let pid = child_pid;
        let sid = PROCESS_TABLE
            .with_process(pid, |p| p.session_id.load(Ordering::SeqCst))
            .filter(|&s| s != 0);
        PROCESS_TABLE.remove_and_free(pid);
        if let Some(sid) = sid {
            SESSION_MANAGER.destroy(sid);
        }
        USER_PROC_MANAGER.destroy_by_pid(pid);
        return 0;
    }

    if !argv.is_null() && argc > 0 {
        let envp: *const *const u8 = core::ptr::null();
        unsafe {
            user_proc_setup_argv(child_pid, argv, argc, envp, 0);
        }
    }

    child_pid
}

#[no_mangle]
pub fn proc_exec_replace(path: *const u8, argv: *const *const u8, argc: u32) -> i32 {
    if path.is_null() {
        return -1;
    }

    let current_pid = SCHEDULER.current().unwrap_or(0);
    if current_pid == 0 {
        return -1;
    }

    let kernel_cr3 = crate::kernel::framework::mm::vmm::get_kernel_pml4();
    if kernel_cr3 != 0 {
        // SAFETY: kernel_cr3 是从 vmm::get_kernel_pml4() 获取的合法页表。
        raw::switch_page_table(kernel_cr3);
    }
    USER_PROC_MANAGER.destroy_by_pid_no_kstack(current_pid);
    PROCESS_TABLE.remove_and_free(current_pid);

    let pwm = scheduler_get_current_pwm();
    let new_pid = user_proc_load_elf(path, pwm);
    if new_pid < 0 {
        return -1;
    }

    let new_pid_u32 = new_pid as u32;

    if !argv.is_null() && argc > 0 {
        let envp: *const *const u8 = core::ptr::null();
        // SAFETY: argv 来自 C ABI 调用方, 由本函数 C ABI contract 保证。
        unsafe {
            user_proc_setup_argv(new_pid_u32, argv, argc, envp, 0);
        }
    }

    if USER_PROC_MANAGER.get(new_pid_u32).is_some() {
        let (pid_val, pwm_val, state_val) = USER_PROC_MANAGER
            .with_process(new_pid_u32, |proc| {
                (
                    proc.pid as u64,
                    proc.pwm.load(Ordering::SeqCst),
                    proc.state.load(Ordering::SeqCst),
                )
            })
            .unwrap_or((0, 0, 0));
        C_CURRENT_PROCESS.map_mut(|p| {
            p.pid = pid_val;
            p.pwm = pwm_val;
            p.state = state_val;
        });
    }

    user_proc_enter_by_pid(new_pid_u32);
    0
}

#[no_mangle]
pub fn proc_wait_child(pid: Pid) -> i32 {
    if pid == 0 {
        return -1;
    }

    let proc = PROCESS_TABLE.get(pid);
    if proc.is_none() {
        return -1;
    }

    let (state, code) = PROCESS_TABLE
        .with_process(pid, |process| {
            let state = process.get_state();
            if state == ProcessState::Zombie {
                let code = process.exit_code.load(Ordering::SeqCst) as i32;
                (state, Some(code))
            } else {
                (state, None)
            }
        })
        .unwrap_or((ProcessState::Terminated, None));

    if state == ProcessState::Zombie {
        PROCESS_TABLE.remove_and_free(pid);
        return code.unwrap_or(-1);
    }

    SCHEDULER.block(BlockReason::WaitingForChild);
    -2
}

#[no_mangle]
pub fn proc_sleep_ms(ms: u64) {
    // ✅ 修复: 阻塞式睡眠, 不再忙等 (Fix 6)
    if ms == 0 {
        return;
    }

    let pid = SCHEDULER.current().unwrap_or(0);
    if pid == 0 {
        return;
    }

    // 获取当前 tick 并计算到期时间
    let current_ticks = timer_get_ticks();
    // 假设每 tick = 10ms (100Hz), 转换 ms → ticks (最少 1 tick)
    let ticks_to_sleep = ms.div_ceil(10);
    if ticks_to_sleep == 0 {
        return;
    }

    let wakeup_at = current_ticks + ticks_to_sleep;

    // 设置 sleep_until 并阻塞进程
    PROCESS_TABLE.with_process(pid, |proc| {
        proc.sleep_until.store(wakeup_at, Ordering::SeqCst);
    });

    SCHEDULER.block(BlockReason::Sleeping);
    SCHEDULER.schedule();
}

/// ✅ fork 系统调用实现 (COW 共享物理页)
/// 父进程物理页标记只读 + 2 引用，子进程共享
/// 父进程返回 >0 (子进程 PID), 子进程返回 0
/// 失败返回 0
#[no_mangle]
pub fn sys_fork() -> Pid {
    let parent_pid = SCHEDULER.current().unwrap_or(0);
    if parent_pid == 0 {
        // SAFETY: klog_ffi_info is unsafe extern "C". msg is a valid static byte slice.
        raw::klog_info(b"[FORK] No current process\n\0");
        return 0;
    }

    // COW: 共享物理页, 双方标记只读
    let parent_cr3 = PROCESS_TABLE
        .with_process(parent_pid, |p| p.cr3.load(Ordering::SeqCst))
        .unwrap_or(0);
    if parent_cr3 == 0 {
        return 0;
    }

    // SAFETY: parent_cr3 来自 process.cr3, 已被 vmm_clone_user_page_table_cow 接受。
    let child_cr3 = raw::clone_user_page_table_cow(parent_cr3);
    if child_cr3 == 0 {
        // SAFETY: klog_ffi_info is unsafe extern "C".
        raw::klog_info(b"[FORK] COW page table clone failed\n\0");
        return 0;
    }

    // Allocate child PID
    let child_pid = proc_alloc_pid();
    if child_pid == 0 {
        // SAFETY: child_cr3 来自 vmm_clone_user_page_table_cow 成功返回。
        raw::destroy_user_page_table(child_cr3);
        return 0;
    }

    // Clone parent name
    let name_str = PROCESS_TABLE
        .with_process(parent_pid, |p| {
            let name = p.name.lock();
            alloc::string::String::clone(&*name)
        })
        .unwrap_or_default();
    let name_ref = name_str.as_str();

    // Create child Process
    // SAFETY: alloc_process 拥有 child 内存的所有权; 错误路径由 dealloc/drop 释放。
    let child_ptr = raw::alloc_process(child_pid, name_ref, Some(ProcessId(parent_pid)));
    let child = raw::process_ref_mut(child_ptr);

    // Copy parent properties to child
    let (parent_pwm, parent_sched_policy, parent_rt_priority) = PROCESS_TABLE
        .with_process(parent_pid, |p| {
            (
                p.pwm.load(Ordering::SeqCst),
                p.sched_policy.load(Ordering::SeqCst),
                p.rt_priority.load(Ordering::SeqCst),
            )
        })
        .unwrap_or((0, 0, 0));
    child.pwm.store(parent_pwm, Ordering::SeqCst);
    child.cr3.store(child_cr3, Ordering::SeqCst);
    child.sched_policy.store(parent_sched_policy, Ordering::SeqCst);
    child.rt_priority.store(parent_rt_priority, Ordering::SeqCst);

    // Add child to parent's children list
    PROCESS_TABLE.with_process(parent_pid, |p| {
        p.children.lock().push(ProcessId(child_pid));
    });

    // Allocate kernel stack for child
    if !child.allocate_kernel_stack() {
        // SAFETY: child_ptr 来自 alloc_process, 需要释放。
        raw::drop_boxed_process(child_ptr);
        // SAFETY: child_cr3 来自 vmm_clone_user_page_table_cow 成功返回。
        raw::destroy_user_page_table(child_cr3);
        return 0;
    }

    // Copy parent's kernel stack contents to child's kernel stack
    {
        let parent_kstack = PROCESS_TABLE
            .with_process(parent_pid, |p| p.kernel_stack.load(Ordering::SeqCst))
            .unwrap_or(0);
        let child_kstack = child.kernel_stack.load(Ordering::SeqCst);
        let stack_size: usize = 65536;
        // SAFETY: parent_kstack 与 child_kstack 都是已分配的内核栈, 区间不重叠。
        raw::copy_kstack(child_kstack, parent_kstack, stack_size);
        crate::kernel::framework::proc_tcb_legacy::process::kernel_stack_write_canary(child_kstack);
    }

    // Copy parent's ProcessContext to child's, but set RAX=0 for child
    let parent_ctx = PROCESS_TABLE
        .with_process(parent_pid, |p| *p.context.lock())
        .unwrap();
    {
        let mut child_ctx = child.context.lock();
        *child_ctx = parent_ctx;
        child_ctx.cr3 = child_cr3;
        child_ctx.rax = 0;
    }

    // Register child in process table
    PROCESS_TABLE.insert(child as *const Process as *mut Process);

    // Create UserProc for child
    if USER_PROC_MANAGER.get(parent_pid).is_some() {
        let clone_result = user_proc_clone(parent_pid, child_pid);
        if clone_result < 0 {
            PROCESS_TABLE.remove_and_free(child_pid);
            return 0;
        }
    }

    // Add child to scheduler
    let _ = child.set_state_safe(ProcessState::Ready);
    SCHEDULER.add_to_run_queue(child_pid);

    child_pid
}

#[no_mangle]
pub fn proc_get_ppid(pid: Pid) -> Pid {
    PROCESS_TABLE.with_process(pid, |p| p.parent.map(|p| p.0).unwrap_or(0))
        .unwrap_or(0)
}

#[no_mangle]
pub fn proc_set_pwm(pid: Pid, pwm: u64) -> i32 {
    if PROCESS_TABLE.with_process(pid, |p| p.pwm.store(pwm, Ordering::SeqCst)).is_some() {
        0
    } else {
        -1
    }
}
