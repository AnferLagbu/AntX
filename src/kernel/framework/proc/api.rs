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
use super::user_proc::{user_proc_clone, USER_PROC_MANAGER};
pub use super::user_proc::proc_alloc_pid;
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

/// TD-10: 当前 CPU 是否处于内核态 (syscall / 中断 / 异常).
///
/// - 0: 用户态 (正常运行)
/// - 1: 内核态
///
/// 单一全局变量, 单核模型. 调度器每 tick 在 `tick_accounting` 读取,
/// syscall dispatch 入口设 1, 出口设 0.
static CURRENT_IN_KERN: AtomicU64 = AtomicU64::new(0);

/// TD-10: 设置当前 CPU 是否处于内核态.
#[no_mangle]
pub fn proc_set_in_kern(v: u32) {
    CURRENT_IN_KERN.store(v as u64, Ordering::SeqCst);
}

/// TD-10: 读取当前 CPU 是否处于内核态.
#[no_mangle]
pub fn proc_get_in_kern() -> u32 {
    CURRENT_IN_KERN.load(Ordering::SeqCst) as u32
}

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

/// 检查指定 PID 的进程是否存在.
#[inline]
pub fn process_exists(pid: u32) -> bool {
    PROCESS_TABLE.get(pid).is_some()
}

/// 尝试增加进程引用计数, 返回是否成功.
#[inline]
pub fn process_try_inc_ref(pid: u32) -> bool {
    PROCESS_TABLE.try_inc_ref(pid)
}

/// 减少进程引用计数, 若计数归零则释放.
#[inline]
pub fn process_dec_ref(pid: u32) {
    PROCESS_TABLE.dec_ref_and_maybe_free(pid);
}

/// 在持有引用期间读取进程的 CR3 页表基址, 返回 None 表示进程不存在或 cr3 为 0.
///
/// 调用方必须先调用 `process_try_inc_ref(pid)` 成功后再调用此函数,
/// 使用完毕后调用 `process_dec_ref(pid)`.
pub fn process_get_cr3(pid: u32) -> Option<u64> {
    PROCESS_TABLE.with_process(pid, |proc| proc.cr3.load(Ordering::SeqCst))
        .filter(|&c| c != 0)
}

/// 读取进程的 PWM (凭证标识), 返回 None 表示进程不存在.
pub fn process_get_pwm(pid: u32) -> Option<u64> {
    PROCESS_TABLE.with_process(pid, |proc| proc.get_pwm())
}

/// 设置进程的信号 pending 位.
///
/// 调用方必须先调用 `process_try_inc_ref(pid)` 成功后再调用此函数,
/// 使用完毕后调用 `process_dec_ref(pid)`.
pub fn process_signal_pending_set(pid: u32, sig: u32) {
    PROCESS_TABLE.with_process_mut(pid, |proc| {
        proc.signal_pending_set(sig);
    });
}

/// 对指定进程执行只读闭包操作, 返回闭包结果.
///
/// 通过公共 api 层访问进程, 避免直接引用 `proc::process::PROCESS_TABLE`.
/// 返回 None 表示进程不存在.
pub fn process_with<F, R>(pid: u32, f: F) -> Option<R>
where
    F: FnOnce(&super::process::Process) -> R,
{
    PROCESS_TABLE.with_process(pid, f)
}

/// 对指定进程执行可变闭包操作, 返回闭包结果.
///
/// 返回 None 表示进程不存在.
pub fn process_with_mut<F, R>(pid: u32, f: F) -> Option<R>
where
    F: FnOnce(&mut super::process::Process) -> R,
{
    PROCESS_TABLE.with_process_mut(pid, f)
}

/// 遍历所有进程, 对每个进程执行闭包.
///
/// 闭包返回 false 时停止遍历.
pub fn process_for_each<F>(f: F)
where
    F: FnMut(&super::process::Process) -> bool,
{
    PROCESS_TABLE.for_each(f);
}

/// 获取进程的原始指针 (用于需要直接访问进程的场景).
///
/// 返回 None 表示进程不存在.
/// 调用方必须确保进程在访问期间不会被释放.
pub fn process_get_raw(pid: u32) -> Option<*const super::process::Process> {
    PROCESS_TABLE.get(pid).map(|p| p as *const _)
}

/// 释放子进程 PCB (wait4 回收).
///
/// 仅在进程状态为 Zombie 时调用.
pub fn process_remove_and_free(pid: u32) {
    PROCESS_TABLE.remove_and_free(pid);
}

/// 将进程注册到进程表.
///
/// 用于 clone 等需要手动创建进程后注册的场景.
pub fn process_insert(process: *mut super::process::Process) -> bool {
    PROCESS_TABLE.insert(process)
}

/// 写入内核栈金丝雀值.
pub fn kernel_stack_write_canary(stack_top: u64) {
    super::process::kernel_stack_write_canary(stack_top);
}

/// 解除进程阻塞 (加入就绪队列).
pub fn scheduler_unblock(pid: u32) {
    SCHEDULER.unblock(pid);
}

/// 将进程加入就绪队列.
pub fn scheduler_add_to_run_queue(pid: u32) {
    SCHEDULER.add_to_run_queue(pid);
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
        // 释放该进程持有的所有文件锁
        crate::kernel::framework::fs::vfs::flock::flock_release_pid(current_pid);
        crate::kernel::framework::fs::vfs::flock::posix_lock_release_pid(current_pid);

        let kernel_cr3 = crate::kernel::framework::mm::vmm::get_kernel_pml4();
        if kernel_cr3 != 0 {
            // SAFETY: kernel_cr3 是从 vmm::get_kernel_pml4() 获取的合法页表。
            raw::switch_page_table(kernel_cr3);
        }
        USER_PROC_MANAGER.destroy_by_pid_no_kstack(current_pid);
    }
    SCHEDULER.exit(exit_code);
}

/// 阻塞当前进程 (用于 futex wait / 等待 I/O 等)
///
/// 将当前进程状态设为 Blocked, 并设置阻塞原因.
/// 下次调度时会切换到其他就绪进程.
#[no_mangle]
pub fn process_block(pid: u32) {
    use super::types::BlockReason;
    if pid == 0 {
        return;
    }
    // 仅阻塞当前正在运行的进程
    let current_pid = SCHEDULER.current().unwrap_or(0);
    if pid != current_pid {
        return;
    }
    SCHEDULER.block(BlockReason::FutexWait);
    // 触发调度, 让出 CPU
    SCHEDULER.schedule();
}

/// 解除进程阻塞 (用于 futex wake / I/O 完成等)
///
/// 将指定进程从 Blocked 状态恢复为 Ready, 允许调度器重新调度它.
#[no_mangle]
pub fn process_unblock(pid: u32) {
    if pid == 0 {
        return;
    }
    SCHEDULER.unblock(pid);
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

// ============================================================================
// init 启动状态查询 (供 services 包装)
// ============================================================================

/// init 启动状态: 0=未启动, 1=initramfs 解压中, 2=init ELF 加载中, 3=已 Ring 3 进入
static INIT_STATUS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// 获取 init 启动状态
pub fn init_launch_status() -> u32 {
    INIT_STATUS.load(core::sync::atomic::Ordering::Acquire)
}

/// 由 launch_first_user_process 内部设置
fn set_init_status(s: u32) {
    INIT_STATUS.store(s, core::sync::atomic::Ordering::Release);
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

/// 在 ELF 加载完成后, 在用户栈上建立 argv/envp (供 exec 系统调用使用)
#[no_mangle]
///
/// # Safety
///
/// `name` 是合法的 C 字符串 (以 NUL 结尾). 进程表已初始化.
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

    // 1. 挂载 ramfs 为根文件系统
    let mount_result = crate::kernel::framework::fs::vfs::api::vfs_mount(
        b"/\0".as_ptr(),
        b"ramfs\0".as_ptr(),
    );
    if mount_result < 0 {
        crate::klog_boot_info!("[USER] Warning: ramfs mount on / failed ({})", mount_result);
    }

    // 2. 解压 initramfs (如果启用 feature "initramfs")
    //    使用: cargo build --features initramfs
    //    需要在 build/user/initramfs.cpio 放置 cpio newc 归档
    set_init_status(1);
    #[cfg(all(target_arch = "x86_64", feature = "initramfs"))]
    {
        let initramfs = include_bytes!("../../../../build/user/initramfs.cpio");
        if initramfs.len() > 0 {
            // SAFETY: 调用方保证指针/类型有效 (详见上下文)
            let result = unsafe {
                crate::kernel::framework::fs::initramfs::unpack(
                    initramfs.as_ptr(),
                    initramfs.len(),
                )
            };
            match result {
                Ok(count) => {
                    crate::klog_boot_info!("[USER] initramfs: {} files unpacked", count);
                    set_init_status(2);
                    // 尝试从 /init 执行
                    let pid = user_proc_load_elf(b"/init\0".as_ptr(), 0);
                    if pid > 0 {
                        let pid_u32 = pid as u32;
                        C_CURRENT_PROCESS.map_mut(|p| {
                            p.pid = pid_u32 as u64;
                            p.pwm = 0;
                            p.state = 2;
                            p.parent_pid = 1;
                        });
                        SCHEDULER.add(pid_u32);
                        set_init_status(3);
                        crate::klog_boot_info!("[USER] Entering Ring 3 (init from /init, pid={})...", pid_u32);
                        user_proc_enter_by_pid(pid_u32);
                    }
                    crate::klog_boot_info!("[USER] /init not found, falling back to init.bin");
                }
                Err(e) => {
                    crate::klog_boot_info!("[USER] initramfs unpack failed: {}", e);
                }
            }
        }

        // 回退: 直接加载内嵌的 init.bin
        let bin = include_bytes!("../../../../build/user/init.bin");
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
        // 挂载 ramfs
        let _ = crate::kernel::framework::fs::vfs::api::vfs_mount(
            b"/\0".as_ptr(),
            b"ramfs\0".as_ptr(),
        );

        // 在 aarch64 上, init.bin 是由 src/user/ 编译出的 AArch64 ELF 二进制
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

    // 注册 tick 查询回调, 解耦 barrier→proc::scheduler 依赖
    // SAFETY: get_tick 是 'static 函数指针, 在内核运行期间始终有效.
    unsafe {
        crate::kernel::framework::tick_query::register_tick_query(
            crate::kernel::framework::proc::scheduler::get_tick,
        );
    }
    // D2: 初始化 cgroup 子系统
    super::cgroup::cgroup_init();
    // D3: 初始化 NUMA 拓扑 (UMA 回退, 后续接入 ACPI SRAT)
    crate::kernel::framework::mm::numa::numa_init(
        crate::kernel::framework::mm::api::pmm_get_total_pages() * 4096,
        crate::kernel::framework::config::MAX_CPUS as u32,
    );
    // D4: 初始化 eBPF 子系统
    crate::kernel::framework::debug::ebpf::bpf_init();
    // D5: 初始化电源管理子系统
    crate::kernel::framework::driver::power::pm_init(
        crate::kernel::framework::config::MAX_CPUS as u32,
    );
    // D6: 初始化安全启动 + TPM (移至 credo_init, 消除 proc→credo 依赖)
    crate::kernel::framework::credo::credo_init();
    // D7: 初始化 CET (Shadow Stack)
    crate::kernel::framework::arch::shadow_stack::cet_init();
    // D8: 初始化 Tickless (NO_HZ)
    crate::kernel::framework::timer::tickless::tickless_init(
        crate::kernel::framework::config::MAX_CPUS as u32,
    );
    // D9: 初始化 NTP/PTP 时钟同步
    crate::kernel::framework::timer::time_sync::timesync_init();
    // D10: 初始化 kexec
    crate::kernel::framework::driver::kexec::kexec_init();
    // D11: 初始化 UEFI (0 = 无 UEFI 固件, 实际由 bootloader 传入)
    crate::kernel::framework::driver::uefi::uefi_init(0);
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
    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
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

    // 初始化每进程的 fd_table
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
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
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

    let pwm = scheduler_get_current_pwm();

    // P0-I-31 修复: transactional execve — 先在临时进程中加载并验证新 ELF,
    // 验证通过后将新地址空间转移到当前 PID, 保持 POSIX execve 语义 (PID 不变).
    //
    // 旧版"先摧毁再加载"假设 ELF/FS/分配永不失败, 一旦失败调度器指向
    // 已释放 PID → panic (UAF). 此前修复改为"双进程替换"但引入 PID 变更,
    // 违反 POSIX execve 语义 (父进程 wait() 匹配失败).
    //
    // 阶段 1: 加载新 ELF (分配临时 UserProc/PID).
    //         任一环节失败 → 返回 -1, 原进程完整保留.
    let new_pid = user_proc_load_elf(path, pwm);
    if new_pid < 0 {
        return -1;
    }

    let new_pid_u32 = new_pid as u32;

    // 阶段 2: 读取新进程的地址空间信息 (CR3/entry/stack)
    let new_addr_space = USER_PROC_MANAGER.with_process(new_pid_u32, |proc| {
        (
            proc.cr3.load(Ordering::SeqCst),
            proc.entry,
            proc.user_stack.load(Ordering::SeqCst),
            proc.stack_bottom.load(Ordering::SeqCst),
        )
    });

    let (new_cr3, new_entry, new_user_stack, new_stack_bottom) = match new_addr_space {
        Some(info) => info,
        None => {
            // 新进程加载成功但无法读取, 清理并失败
            USER_PROC_MANAGER.destroy_by_pid(new_pid_u32);
            PROCESS_TABLE.remove_and_free(new_pid_u32);
            return -1;
        }
    };

    // 阶段 3: 切换到内核页表, 替换当前进程的用户地址空间
    let kernel_cr3 = crate::kernel::framework::mm::vmm::get_kernel_pml4();
    if kernel_cr3 != 0 {
        // SAFETY: kernel_cr3 是从 vmm::get_kernel_pml4() 获取的合法页表。
        raw::switch_page_table(kernel_cr3);
    }

    // 原子替换: 销毁旧用户空间 + 更新为新地址空间, PID 不变
    USER_PROC_MANAGER.replace_user_space(
        current_pid,
        new_cr3,
        new_entry,
        new_user_stack,
        new_stack_bottom,
    );

    // 阶段 4: 移除临时新进程 (资源已转移到当前 PID, 仅移除索引)
    USER_PROC_MANAGER.detach_by_pid(new_pid_u32);
    PROCESS_TABLE.remove_and_free(new_pid_u32);

    // 5. 设置 argv/envp
    if !argv.is_null() && argc > 0 {
        let envp: *const *const u8 = core::ptr::null();
        // SAFETY: argv 来自 C ABI 调用方, 由本函数 C ABI contract 保证。
        unsafe {
            user_proc_setup_argv(current_pid, argv, argc, envp, 0);
        }
    }

    // 5a. I-48: 重置信号状态 (execve 后信号处理 = 默认)
    crate::kernel::framework::proc::signal::reset_signal_state_on_exec(current_pid);

    // 6. 同步当前进程信息
    C_CURRENT_PROCESS.map_mut(|p| {
        p.pid = current_pid as u64;
    });

    // 7. 进入当前 PID (使用新地址空间)
    user_proc_enter_by_pid(current_pid);
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

    // 克隆父进程名
    let name_str = PROCESS_TABLE
        .with_process(parent_pid, |p| {
            let name = p.name.lock();
            alloc::string::String::clone(&*name)
        })
        .unwrap_or_default();
    let name_ref = name_str.as_str();

    // 创建子进程 Process
    // SAFETY: alloc_process 拥有 child 内存的所有权; 错误路径由 dealloc/drop 释放。
    let child_ptr = raw::alloc_process(child_pid, name_ref, Some(ProcessId(parent_pid)));
    let child = raw::process_ref_mut(child_ptr);

    // 复制父进程属性到子进程
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

    // 继承父进程 rlimit 表
    {
        let parent_rlimit = PROCESS_TABLE
            .with_process(parent_pid, |p| p.rlimit_table.lock().clone())
            .unwrap_or_default();
        *child.rlimit_table.lock() = parent_rlimit;
    }

    // 继承父进程 session_id 和 pgid
    {
        let (parent_sid, parent_pgid) = PROCESS_TABLE
            .with_process(parent_pid, |p| {
                (
                    p.session_id.load(core::sync::atomic::Ordering::SeqCst),
                    p.pgid.load(core::sync::atomic::Ordering::SeqCst),
                )
            })
            .unwrap_or((0, 0));
        child.session_id.store(parent_sid, core::sync::atomic::Ordering::SeqCst);
        // 子进程继承父进程的 pgid (POSIX 语义)
        let effective_pgid = if parent_pgid == 0 { parent_pid } else { parent_pgid };
        child.pgid.store(effective_pgid, core::sync::atomic::Ordering::SeqCst);
        // P1 #14: 继承父进程 stack canary (fork 后父子 canary 相同)
        {
            let parent_canary = PROCESS_TABLE
                .with_process(parent_pid, |p| p.stack_canary.load(Ordering::SeqCst))
                .unwrap_or(0);
            child.stack_canary.store(parent_canary, Ordering::SeqCst);
        }
        // C7: 继承父进程 Seccomp 过滤器 (fork 后子进程拥有独立副本)
        {
            let parent_seccomp = PROCESS_TABLE
                .with_process(parent_pid, |p| {
                    let mode = p.seccomp.get_mode();
                    let no_new_privs = p.seccomp.is_no_new_privs();
                    let filters = p.seccomp.filters.lock();
                    (mode, no_new_privs, filters.clone())
                })
                .unwrap_or((crate::kernel::framework::proc::seccomp::SeccompMode::Disabled, false, alloc::vec::Vec::new()));
            child.seccomp.mode.store(parent_seccomp.0 as u8, Ordering::SeqCst);
            child.seccomp.no_new_privs.store(parent_seccomp.1 as u8, Ordering::SeqCst);
            *child.seccomp.filters.lock() = parent_seccomp.2;
        }
        // D1: 继承父进程 Namespace 集合 (fork 默认共享)
        {
            let parent_ns = PROCESS_TABLE
                .with_process(parent_pid, |p| {
                    crate::kernel::framework::proc::namespace::NamespaceSet::fork_from(&p.namespaces.lock())
                })
                .unwrap_or_else(crate::kernel::framework::proc::namespace::NamespaceSet::new_init);
            *child.namespaces.lock() = parent_ns;
        }
        // D2: 继承父进程 cgroup ID
        {
            let parent_cg = PROCESS_TABLE
                .with_process(parent_pid, |p| p.cgroup_id.load(core::sync::atomic::Ordering::Acquire))
                .unwrap_or(0);
            child.cgroup_id.store(parent_cg, core::sync::atomic::Ordering::Release);
            // 将子进程加入 cgroup 的进程列表
            if crate::kernel::framework::proc::cgroup::cgroup_is_initialized() {
                let sub = crate::kernel::framework::proc::cgroup::cgroup_subsystem();
                if let Some(cg) = sub.find(parent_cg) {
                    cg.attach_proc(child_pid);
                }
            }
        }
        // D3: 继承父进程 NUMA 策略
        {
            let parent_policy = PROCESS_TABLE
                .with_process(parent_pid, |p| {
                    let policy = p.numa_policy.lock();
                    let mode = *policy.mode.lock();
                    let mask = *policy.nodemask.lock();
                    (mode, mask)
                })
                .unwrap_or((crate::kernel::framework::mm::numa::NumaPolicy::Default, 0));
            let child_policy = child.numa_policy.lock();
            *child_policy.mode.lock() = parent_policy.0;
            *child_policy.nodemask.lock() = parent_policy.1;
        }
    }

    // 将子进程加入父进程的子进程列表
    PROCESS_TABLE.with_process(parent_pid, |p| {
        p.children.lock().push(ProcessId(child_pid));
    });

    // 为子进程分配内核栈
    if !child.allocate_kernel_stack() {
        // SAFETY: child_ptr 来自 alloc_process, 需要释放。
        raw::drop_boxed_process(child_ptr);
        // SAFETY: child_cr3 来自 vmm_clone_user_page_table_cow 成功返回。
        raw::destroy_user_page_table(child_cr3);
        return 0;
    }

    // 复制父进程内核栈内容到子进程
    {
        let parent_kstack = PROCESS_TABLE
            .with_process(parent_pid, |p| p.kernel_stack.load(Ordering::SeqCst))
            .unwrap_or(0);
        let child_kstack = child.kernel_stack.load(Ordering::SeqCst);
        let stack_size: usize = 65536;
        // SAFETY: parent_kstack 与 child_kstack 都是已分配的内核栈, 区间不重叠。
        raw::copy_kstack(child_kstack, parent_kstack, stack_size);
        crate::kernel::framework::proc::process::kernel_stack_write_canary(child_kstack);
    }

    // 复制父进程 ProcessContext 到子进程, 但把 RAX 置 0
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

    // 为子进程创建 UserProc
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

// ============================================================================
// times / alarm / setitimer / getitimer — POSIX 进程时间与定时器 API
// ============================================================================

/// 累加当前进程的 user/sys 时间 (调度器每 tick 调用, in_kern 区分用户/内核).
/// Framekernel 调度器在 `tick_accounting` 中会调用此函数; 若调度器未启用统计,
/// 则 user_time/sys_time 一直为 0, times() 返回 0 是真实结果而非占位.
#[no_mangle]
pub fn proc_account_tick(in_kern: u32) {
    let pid = CURRENT_PROCESS_PTR.load(Ordering::SeqCst);
    if pid == 0 {
        return;
    }
    PROCESS_TABLE.with_process(pid as Pid, |p| {
        p.tick_count.fetch_add(1, Ordering::SeqCst);
        if in_kern != 0 {
            p.sys_time.fetch_add(1, Ordering::SeqCst);
        } else {
            p.user_time.fetch_add(1, Ordering::SeqCst);
        }
    });
}

/// 取得进程的 user/sys 时间 (jiffies 累积), services 层写入 tms.
/// 真实实现: 通过 PROCESS_TABLE.with_process 读 Atomic; 若 process 不存在返 -1.
#[no_mangle]
pub fn proc_get_times(pid: u32, out_user: *mut u64, out_sys: *mut u64) -> i32 {
    if out_user.is_null() || out_sys.is_null() {
        return -1;
    }
    let res = PROCESS_TABLE.with_process(pid as Pid, |p| {
        (p.user_time.load(Ordering::SeqCst), p.sys_time.load(Ordering::SeqCst))
    });
    match res {
        Some((u, s)) => {
            // SAFETY: 调用方保证指针有效 (services/syscall 上下文).
            unsafe {
                core::ptr::write_unaligned(out_user, u);
                core::ptr::write_unaligned(out_sys, s);
            }
            0
        }
        None => -1,
    }
}

/// 取得当前进程启动时刻 jiffies.
#[no_mangle]
pub fn proc_get_start_jiffies(pid: u32) -> u64 {
    PROCESS_TABLE
        .with_process(pid as Pid, |p| p.start_jiffies.load(Ordering::SeqCst))
        .unwrap_or(0)
}

/// 标记进程启动时刻 (process_create 后调用).
#[no_mangle]
pub fn proc_set_start_jiffies(pid: u32, j: u64) {
    PROCESS_TABLE.with_process(pid as Pid, |p| {
        p.start_jiffies.store(j, Ordering::SeqCst);
    });
}

/// alarm(seconds) — 设置 alarm 剩余秒数对应的 jiffies 到期时刻.
/// 返回旧剩余时间 (秒).
#[no_mangle]
pub fn proc_alarm(pid: u32, seconds: u32) -> u32 {
    let hz = crate::kernel::framework::timer::get_frequency() as u64;
    if hz == 0 {
        return 0;
    }
    let now = crate::kernel::framework::timer::get_ticks();
    let prev_remaining = PROCESS_TABLE
        .with_process(pid as Pid, |p| {
            let deadline = p.alarm_deadline.load(Ordering::SeqCst);
            let prev = if deadline == 0 || now >= deadline {
                0u32
            } else {
                ((deadline - now) / hz) as u32
            };
            // 新 deadline
            if seconds == 0 {
                p.alarm_deadline.store(0, Ordering::SeqCst);
            } else {
                p.alarm_deadline
                    .store(now + (seconds as u64) * hz, Ordering::SeqCst);
            }
            p.alarm_prev_remaining.store(prev as u64, Ordering::SeqCst);
            prev
        })
        .unwrap_or(0);
    prev_remaining
}

/// 调度器 tick 时检查 alarm 是否到期; 到期则触发 SIGALRM 并清零.
/// 返回 1 表示有 alarm 触发 (供调度器唤醒/投递信号).
#[no_mangle]
pub fn proc_check_alarm(pid: u32) -> i32 {
    let now = crate::kernel::framework::timer::get_ticks();
    let triggered = PROCESS_TABLE
        .with_process(pid as Pid, |p| {
            let d = p.alarm_deadline.load(Ordering::SeqCst);
            if d != 0 && now >= d {
                p.alarm_deadline.store(0, Ordering::SeqCst);
                true
            } else {
                false
            }
        })
        .unwrap_or(false);
    if triggered {
        // 14 = SIGALRM
        let _ = crate::kernel::framework::proc::signal::do_signal_send(pid as Pid, 14);
        1
    } else {
        0
    }
}

/// setitimer(ITIMER_REAL, new, old) — Framekernel 只实现 ITIMER_REAL.
#[no_mangle]
pub fn proc_setitimer_real(
    pid: u32,
    new_seconds: u64,
    new_interval: u64,
    out_old_seconds: *mut u64,
    out_old_remaining: *mut u64,
) -> i32 {
    let hz = crate::kernel::framework::timer::get_frequency() as u64;
    if hz == 0 {
        return -1;
    }
    let now = crate::kernel::framework::timer::get_ticks();
    let result: i32 = 0;
    PROCESS_TABLE.with_process(pid as Pid, |p| {
        // 旧值回填
        if !out_old_seconds.is_null() {
            let d = p.itimer_real_deadline.load(Ordering::SeqCst);
            let rem = if d == 0 || now >= d {
                0u64
            } else {
                (d - now) / hz
            };
            // SAFETY: 调用方保证指针有效.
            unsafe {
                core::ptr::write_unaligned(out_old_seconds, p.itimer_real_interval.load(Ordering::SeqCst) / hz);
                core::ptr::write_unaligned(out_old_remaining, rem);
            }
        }
        // 新值
        if new_seconds == 0 {
            p.itimer_real_deadline.store(0, Ordering::SeqCst);
        } else {
            p.itimer_real_deadline
                .store(now + new_seconds * hz, Ordering::SeqCst);
        }
        p.itimer_real_interval.store(new_interval * hz, Ordering::SeqCst);
        p.itimer_real_remaining.store(new_seconds * hz, Ordering::SeqCst);
    });
    result
}

/// getitimer(ITIMER_REAL, value) — 读取当前 ITIMER_REAL 剩余.
#[no_mangle]
pub fn proc_getitimer_real(pid: u32, out_remaining_seconds: *mut u64) -> i32 {
    if out_remaining_seconds.is_null() {
        return -1;
    }
    let hz = crate::kernel::framework::timer::get_frequency() as u64;
    let now = crate::kernel::framework::timer::get_ticks();
    let res = PROCESS_TABLE.with_process(pid as Pid, |p| {
        let d = p.itimer_real_deadline.load(Ordering::SeqCst);
        if d == 0 || hz == 0 || now >= d {
            0u64
        } else {
            (d - now) / hz
        }
    });
    let rem = res.unwrap_or(0);
    // SAFETY: 调用方保证指针有效.
    unsafe {
        core::ptr::write_unaligned(out_remaining_seconds, rem);
    }
    0
}

/// 调度器 tick 时检查 itimer_real 是否到期, 到期则触发 SIGALRM 并按 interval 重置.
#[no_mangle]
pub fn proc_check_itimer_real(pid: u32) -> i32 {
    let now = crate::kernel::framework::timer::get_ticks();
    let mut triggered = false;
    PROCESS_TABLE.with_process(pid as Pid, |p| {
        let d = p.itimer_real_deadline.load(Ordering::SeqCst);
        if d != 0 && now >= d {
            let interval_ticks = p.itimer_real_interval.load(Ordering::SeqCst);
            if interval_ticks > 0 {
                // 周期性: 重新设置 deadline = now + interval
                p.itimer_real_deadline
                    .store(now + interval_ticks, Ordering::SeqCst);
                p.itimer_real_remaining.store(interval_ticks, Ordering::SeqCst);
            } else {
                // 一次性: 关闭
                p.itimer_real_deadline.store(0, Ordering::SeqCst);
                p.itimer_real_remaining.store(0, Ordering::SeqCst);
            }
            triggered = true;
        }
    });
    if triggered {
        let _ = crate::kernel::framework::proc::signal::do_signal_send(pid as Pid, 14);
        1
    } else {
        0
    }
}

/// POSIX getrusage(who, rusage) — 写回进程/子进程 user/sys 时间.
/// 真实实现: 写两段 timeval {sec, usec} 到用户缓冲:
///   ru_utime: 进程用户态 CPU 时间 (user_time jiffies -> sec/usec)
///   ru_stime: 进程内核态 CPU 时间 (sys_time jiffies -> sec/usec)
///   ru_maxrss 等其余字段写 0.
/// 缓冲布局 (Linux x86_64):
/// - 偏移 0:  `ru_utime.tv_sec`  (i64)
/// - 偏移 8:  `ru_utime.tv_usec` (i64)
/// - 偏移 16: `ru_stime.tv_sec`  (i64)
/// - 偏移 24: `ru_stime.tv_usec` (i64)
/// - 偏移 32 起: 其他 16 个 long, 写 0.
#[no_mangle]
pub fn proc_get_rusage(pid: u32, who: i32, out: *mut u8, out_len: u64) -> i32 {
    if out.is_null() || out_len < 32 {
        return -1;
    }
    // who: 0=RUSAGE_SELF, 1=RUSAGE_CHILDREN, 2=RUSAGE_THREAD
    // (值含义与 Linux 一致)
    if who != 0 && who != 1 && who != 2 {
        return -1;
    }
    let hz = crate::kernel::framework::timer::get_frequency() as u64;
    if hz == 0 {
        return -1;
    }
    let (user, sys) = if who == 0 {
        PROCESS_TABLE
            .with_process(pid as Pid, |p| {
                (
                    p.user_time.load(Ordering::SeqCst),
                    p.sys_time.load(Ordering::SeqCst),
                )
            })
            .unwrap_or((0, 0))
    } else {
        // who == 1 (子进程) 或 RUSAGE_THREAD, 暂未实现
        (0, 0)
    };
    let ut_sec = (user / hz) as i64;
    let ut_usec = ((user % hz) * 1_000_000 / hz) as i64;
    let st_sec = (sys / hz) as i64;
    let st_usec = ((sys % hz) * 1_000_000 / hz) as i64;

    // SAFETY: out 至少 32 字节可写, who 合法.
    unsafe {
        core::ptr::write_unaligned(out as *mut i64, ut_sec);
        core::ptr::write_unaligned(out.add(8) as *mut i64, ut_usec);
        core::ptr::write_unaligned(out.add(16) as *mut i64, st_sec);
        core::ptr::write_unaligned(out.add(24) as *mut i64, st_usec);
        // 剩余字节清 0
        let tail = (out_len as usize).saturating_sub(32);
        if tail > 0 {
            core::ptr::write_bytes(out.add(32), 0u8, tail);
        }
    }
    0
}
