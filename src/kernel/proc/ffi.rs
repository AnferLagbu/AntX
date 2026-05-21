use core::ffi::c_char;
use core::sync::atomic::{AtomicU64, AtomicU32, Ordering};

use super::types::*;
use super::scheduler::SCHEDULER;
use super::thread::THREAD_MANAGER;
use super::scheduler_ex::SCHEDULER_EX;
use super::session::SESSION_MANAGER;
use super::user_proc::USER_PROC_MANAGER;
use super::process::{Process, PROCESS_TABLE};

extern "C" {
    fn vmm_get_physical_in_table(table: u64, vaddr: u64) -> u64;
}

static CURRENT_PROCESS_PTR: AtomicU64 = AtomicU64::new(0);
static INIT_PROCESS_CREATED: AtomicU32 = AtomicU32::new(0);

#[repr(C)]
struct CProcess {
    pid: u64,
    session_id: u64,
    parent_pid: u64,
    pwid: u64,
    state: u32,
    exit_code: u64,
    priority: i32,
    cpu_time: u64,
    start_time: u64,
    time_slice: u64,
}

static mut C_CURRENT_PROCESS: CProcess = CProcess {
    pid: 0,
    session_id: 0,
    parent_pid: 0,
    pwid: 0,
    state: 0,
    exit_code: 0,
    priority: 2,
    cpu_time: 0,
    start_time: 0,
    time_slice: 10,
};

#[no_mangle]
pub extern "C" fn process_get_current() -> u64 {
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
    unsafe {
        C_CURRENT_PROCESS.pid = 1;
        C_CURRENT_PROCESS.state = 2;
        C_CURRENT_PROCESS.priority = 2;
        C_CURRENT_PROCESS.time_slice = 10;

        extern "C" {
            fn klog_ffi_info(msg: *const u8);
        }

        let msg = b"[PROC] Init process created (pid=1)";
        unsafe { klog_ffi_info(msg.as_ptr()); }
    }
    CURRENT_PROCESS_PTR.store(unsafe { &C_CURRENT_PROCESS as *const CProcess as u64 }, Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn update_current_process_ptr(ptr: u64) {
    CURRENT_PROCESS_PTR.store(ptr, Ordering::SeqCst);
    if ptr != 0 {
        let proc_ptr = ptr as *const Process;
        unsafe {
            C_CURRENT_PROCESS.pid = (*proc_ptr).pid.0 as u64;
            C_CURRENT_PROCESS.pwid = (*proc_ptr).get_pwid();
        }
    }
}

#[no_mangle]
pub extern "C" fn process_get_current_pid() -> u32 {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn process_get_by_pid(_pid: u32) -> u64 {
    // Return current C process pointer if pid matches, otherwise 0
    unsafe {
        if _pid as u64 == C_CURRENT_PROCESS.pid {
            &C_CURRENT_PROCESS as *const CProcess as u64
        } else {
            PROCESS_TABLE.get(_pid).map(|p| p as u64).unwrap_or(0)
        }
    }
}

#[no_mangle]
pub extern "C" fn process_create(name: *const c_char, parent_pid: Pid, pwid: u64) -> Pid {
    proc_create_internal(name, parent_pid, pwid)
}

#[no_mangle]
pub extern "C" fn process_exit(exit_code: u32) {
    let current_pid = SCHEDULER.current().unwrap_or(0);
    if current_pid != 0 {
        USER_PROC_MANAGER.destroy_by_pid(current_pid);
    }
    SCHEDULER.exit(exit_code);
}

#[no_mangle]
pub extern "C" fn process_kill(pid: u32, exit_code: u32) {
    // ✅ 修复: 杀指定 PID, 而非当前进程
    if pid == 0 { return; }
    
    if let Some(proc) = PROCESS_TABLE.get(pid) {
        unsafe {
            let state = (*proc).get_state();
            if state == ProcessState::Zombie || state == ProcessState::Terminated {
                return; // already dead
            }
            (*proc).exit_code.store(exit_code, Ordering::SeqCst);
            let _ = (*proc).set_state_safe(ProcessState::Zombie);
        }
        
        // 如果目标正在阻塞, 唤醒它使其能立即调度到并退出
        SCHEDULER.unblock(pid);
        // 触发重新调度
        SCHEDULER.set_need_reschedule();
    }
}

#[no_mangle]
pub extern "C" fn process_find_by_pid(pid: Pid) -> u64 {
    PROCESS_TABLE.get(pid).map(|p| p as u64).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn proc_has_runnable() -> i32 {
    if SCHEDULER.has_runnable() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn thread_get_current() -> u64 {
    THREAD_MANAGER.get_current_thread().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn scheduler_yield_ex() {
    SCHEDULER_EX.yield_current();
}

#[no_mangle]
pub extern "C" fn scheduler_yield() {
    SCHEDULER.yield_current();
}

#[no_mangle]
pub extern "C" fn scheduler_schedule() -> Pid {
    SCHEDULER.schedule().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn scheduler_add(pid: Pid) {
    SCHEDULER.add(pid);
}

#[no_mangle]
pub extern "C" fn wait_queue_init(_wq: *mut u8) {
}

#[no_mangle]
pub extern "C" fn wait_queue_add(_wq: *mut u8, _thread: u64) {
}

#[no_mangle]
pub extern "C" fn wait_queue_wake_one(_wq: *mut u8) {
}

#[no_mangle]
pub extern "C" fn wait_queue_wake_all(_wq: *mut u8) {
}

#[no_mangle]
pub extern "C" fn session_init() {
    SESSION_MANAGER.init();
}

#[no_mangle]
pub extern "C" fn user_proc_init() {
    USER_PROC_MANAGER.init();
}

const ELF_MAX_SIZE: usize = 1024 * 1024;

extern "C" {
    fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void;
}

#[no_mangle]
pub extern "C" fn user_proc_load_elf(path: *const c_char, pwid: u64) -> i32 {
    if path.is_null() {
        return -1;
    }

    let mut st: crate::kernel::fs::vfs::types::VfsStat = unsafe { core::mem::zeroed() };
    let stat_result = unsafe { crate::kernel::fs::vfs::ffi::vfs_stat(path, &mut st, pwid) };
    if stat_result < 0 {
        return -1;
    }

    let file_size = st.size as u64;
    if file_size == 0 || file_size > ELF_MAX_SIZE as u64 {
        return -1;
    }

    let fd = unsafe { crate::kernel::fs::vfs::ffi::vfs_open(path, 0, pwid) };
    if fd < 0 {
        return -1;
    }

    let pages = (file_size + 4096u64 - 1) / 4096u64;
    let buffer = unsafe { pmm_alloc_pages(pages) };
    if buffer.is_null() {
        unsafe { crate::kernel::fs::vfs::ffi::vfs_close(fd as u32) };
        return -1;
    }

    let bytes_read = unsafe {
        crate::kernel::fs::vfs::ffi::vfs_read(fd as u32, buffer as *mut u8, file_size as u32)
    };

    unsafe { crate::kernel::fs::vfs::ffi::vfs_close(fd as u32) };

    if bytes_read <= 0 {
        extern "C" { fn pmm_free_pages(addr: *mut core::ffi::c_void, count: u64); }
        unsafe { pmm_free_pages(buffer as *mut core::ffi::c_void, pages as u64) };
        return -1;
    }

    let result = USER_PROC_MANAGER.load_elf_from_memory(buffer as *const u8, bytes_read as u64, pwid);

    extern "C" { fn pmm_free_pages(addr: *mut core::ffi::c_void, count: u64); }
    unsafe { pmm_free_pages(buffer as *mut core::ffi::c_void, pages as u64) };

    result
}

#[no_mangle]
pub extern "C" fn user_proc_load_elf_from_memory(elf_data: *const u8, elf_size: u64, pwid: u64) -> i32 {
    USER_PROC_MANAGER.load_elf_from_memory(elf_data, elf_size, pwid)
}

/// Set up argv/envp on user stack after ELF loading (for exec syscall)
#[no_mangle]
pub unsafe extern "C" fn user_proc_setup_argv(
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
    
    let sp = USER_PROC_MANAGER.setup_user_stack(
        proc,
        argv,
        argc as usize,
        envp,
        envc as usize,
    );
    
    if sp == 0 { -1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn user_proc_enter_by_pid(pid: u32) -> i32 {
    if let Some(proc) = USER_PROC_MANAGER.get(pid) {
        unsafe {
            C_CURRENT_PROCESS.pid = (*proc).pid as u64;
            C_CURRENT_PROCESS.pwid = (*proc).pwid.load(Ordering::SeqCst);
            C_CURRENT_PROCESS.state = (*proc).state.load(Ordering::SeqCst);
            C_CURRENT_PROCESS.parent_pid = 1;
        }
        USER_PROC_MANAGER.enter(proc);
        0
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn launch_first_user_process() -> ! {
    crate::klog_boot_info!("[USER] Launching init process...");

    let bin = include_bytes!("../../../build/user/init.bin");

    unsafe {
        let bin_ptr = bin.as_ptr();
        let bin_size = bin.len() as u64;

        if bin_size == 0 {
            crate::klog_err!(Boot, "[USER] init binary is empty");
            crate::kernel::tests::qemu_exit(false);
        }

        let pid = USER_PROC_MANAGER.load_elf_from_memory(bin_ptr, bin_size, 0);
        if pid <= 0 {
            crate::klog_err!(Boot, "[USER] Failed to load init ELF, pid={}", pid);
            crate::kernel::tests::qemu_exit(false);
        }

        let pid_u32 = pid as u32;

        C_CURRENT_PROCESS.pid = pid_u32 as u64;
        C_CURRENT_PROCESS.pwid = 0;
        C_CURRENT_PROCESS.state = 2;
        C_CURRENT_PROCESS.parent_pid = 1;

        SCHEDULER.add(pid_u32);

        crate::klog_boot_info!("[USER] Entering Ring 3 (init pid={})...", pid_u32);
        user_proc_enter_by_pid(pid_u32);
    }

    loop { crate::arch!(halt()); }
}

#[no_mangle]
pub extern "C" fn scheduler_tick() {
    SCHEDULER_EX.tick();
}

#[no_mangle]
pub extern "C" fn scheduler_init() {
    super::scheduler::init();
    SCHEDULER_EX.init();
}

#[no_mangle]
pub extern "C" fn process_init() {
    super::process::init();
}

#[no_mangle]
pub extern "C" fn thread_init() {
    super::thread::init();
}

#[no_mangle]
pub extern "C" fn proc_create_internal(name: *const c_char, parent_pid: Pid, pwid: u64) -> Pid {
    if name.is_null() {
        return 0;
    }
    
    let name_str = unsafe {
        const MAX_NAME_LEN: usize = 256;
        let len = (0..MAX_NAME_LEN).find(|&i| *name.add(i) == 0).unwrap_or(MAX_NAME_LEN);
        let slice = core::slice::from_raw_parts(name as *const u8, len);
        match core::str::from_utf8(slice) {
            Ok(s) => s,
            Err(_) => return 0,
        }
    };
    
    let parent = if parent_pid == 0 {
        None
    } else {
        Some(parent_pid)
    };
    
    SCHEDULER.create_process(name_str, parent, pwid).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn scheduler_get_current_pwid() -> u64 {
    if let Some(pid) = SCHEDULER.current() {
        if let Some(process) = PROCESS_TABLE.get(pid) {
            return unsafe { (*process).get_pwid() };
        }
    }
    0
}

#[no_mangle]
pub extern "C" fn scheduler_set_quota(pwid: u64, max_runtime: u64, period: u64) {
    SCHEDULER.set_quota(pwid, max_runtime, period);
}

#[no_mangle]
pub extern "C" fn scheduler_remove_quota(pwid: u64) {
    SCHEDULER.remove_quota(pwid);
}

#[no_mangle]
pub extern "C" fn scheduler_set_proc_limit(pwid: u64, max_procs: u32) {
    SCHEDULER.set_limit(pwid, max_procs);
}

#[no_mangle]
pub extern "C" fn proc_exit_internal(exit_code: u32) {
    SCHEDULER.exit(exit_code);
}

#[no_mangle]
pub extern "C" fn proc_get_current_pid_internal() -> Pid {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn proc_yield_internal() {
    SCHEDULER.yield_current();
}

#[no_mangle]
pub extern "C" fn proc_block(reason: u32) {
    let block_reason = BlockReason::from_u8(reason as u8);
    SCHEDULER.block(block_reason);
}

#[no_mangle]
pub extern "C" fn proc_unblock(pid: Pid) {
    SCHEDULER.unblock(pid);
}

#[no_mangle]
pub extern "C" fn proc_set_priority(pid: Pid, priority: u32) -> i32 {
    use super::process::PROCESS_TABLE;
    
    if let Some(process) = PROCESS_TABLE.get(pid) {
        unsafe {
            (*process).set_priority(ProcessPriority::from_u32(priority));
        }
        0
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn proc_get_state(pid: Pid) -> u32 {
    use super::process::PROCESS_TABLE;
    
    if let Some(process) = PROCESS_TABLE.get(pid) {
        unsafe {
            (*process).get_state() as u32
        }
    } else {
        ProcessState::Terminated as u32
    }
}

#[no_mangle]
pub extern "C" fn sched_init_internal() {
    SCHEDULER.init();
}

#[no_mangle]
pub extern "C" fn sched_add_internal(pid: Pid) {
    SCHEDULER.add(pid);
}

#[no_mangle]
pub extern "C" fn sched_schedule_internal() -> Pid {
    SCHEDULER.schedule().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn sched_should_reschedule() -> i32 {
    if SCHEDULER.should_reschedule() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn sched_set_current(pid: Pid) {
    SCHEDULER.set_current(pid);
}

#[no_mangle]
pub extern "C" fn sched_get_current() -> Pid {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn proc_get_exit_code(pid: Pid) -> i32 {
    use super::process::PROCESS_TABLE;
    
    if let Some(process) = PROCESS_TABLE.get(pid) {
        unsafe {
            (*process).exit_code.load(core::sync::atomic::Ordering::SeqCst) as i32
        }
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn proc_is_initialized() -> i32 {
    if SCHEDULER.is_initialized() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn scheduler_get_time_slice() -> u64 {
    SCHEDULER.get_time_slice()
}

#[no_mangle]
pub extern "C" fn scheduler_get_current_level() -> u32 {
    SCHEDULER.get_current_level()
}

#[no_mangle]
pub extern "C" fn scheduler_tick_mlfq() {
    SCHEDULER.tick()
}

#[no_mangle]
pub extern "C" fn scheduler_boost_priority() {
    SCHEDULER.boost_priority()
}

#[no_mangle]
pub extern "C" fn scheduler_add_with_priority(pid: Pid, level: usize) {
    SCHEDULER.add_with_priority(pid, level)
}

#[no_mangle]
pub extern "C" fn scheduler_add_rt_task(pid: Pid, rt_priority: u8, policy: u32) {
    use super::scheduler::SchedPolicy;
    SCHEDULER.add_rt_task(pid, rt_priority, SchedPolicy::from_u32(policy))
}

#[no_mangle]
pub extern "C" fn scheduler_set_sched_policy(pid: Pid, policy: u32, rt_priority: u8) -> i32 {
    use super::scheduler::SchedPolicy;
    if SCHEDULER.set_sched_policy(pid, SchedPolicy::from_u32(policy), rt_priority) {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn scheduler_get_rt_count() -> usize {
    SCHEDULER.get_rt_count()
}

#[no_mangle]
pub extern "C" fn proc_create_user(path: *const c_char, argv: *const *const u8, argc: u32, pwid: u64) -> Pid {
    if path.is_null() { return 0; }

    let parent_pid = SCHEDULER.current().unwrap_or(0);
    let name_str = unsafe {
        let cstr = core::ffi::CStr::from_ptr(path);
        cstr.to_str().unwrap_or("user")
    };

    let child_pid = SCHEDULER.create_process(name_str, if parent_pid != 0 { Some(parent_pid) } else { None }, pwid).unwrap_or(0);
    if child_pid == 0 { return 0; }

    // Create session for the new user process
    if let Some(sid) = SESSION_MANAGER.create(pwid) {
        if let Some(proc) = PROCESS_TABLE.get(child_pid) {
            unsafe { (*proc).session_id.store(sid, Ordering::SeqCst); }
        }
    }

    // Initialize per-process fd_table
    if let Some(proc) = PROCESS_TABLE.get(child_pid) {
        unsafe { (*proc).fd_table.init(); }
    }

    let load_result = user_proc_load_elf(path, pwid);
    if load_result < 0 {
        let pid = child_pid;
        // 保存 session_id 在释放之前
        let sid = PROCESS_TABLE.get(pid)
            .map(|p| unsafe { (*p).session_id.load(Ordering::SeqCst) });
        PROCESS_TABLE.remove_and_free(pid);
        if let Some(sid) = sid.and_then(|s| if s != 0 { Some(s) } else { None }) {
            SESSION_MANAGER.destroy(sid);
        }
        USER_PROC_MANAGER.destroy_by_pid(pid as u32);
        return 0;
    }

    if !argv.is_null() && argc > 0 {
        let envp: *const *const u8 = core::ptr::null();
        unsafe { user_proc_setup_argv(child_pid, argv, argc, envp, 0); }
    }

    child_pid
}

#[no_mangle]
pub extern "C" fn proc_exec_replace(path: *const c_char, argv: *const *const u8, argc: u32) -> i32 {
    if path.is_null() { return -1; }

    let current_pid = SCHEDULER.current().unwrap_or(0);
    if current_pid == 0 { return -1; }

    USER_PROC_MANAGER.destroy_by_pid(current_pid);
    PROCESS_TABLE.remove_and_free(current_pid);

    let pwid = scheduler_get_current_pwid();
    let new_pid = user_proc_load_elf(path, pwid);
    if new_pid < 0 { return -1; }

    let new_pid_u32 = new_pid as u32;

    if !argv.is_null() && argc > 0 {
        let envp: *const *const u8 = core::ptr::null();
        unsafe { user_proc_setup_argv(new_pid_u32, argv, argc, envp, 0); }
    }

    if let Some(proc) = USER_PROC_MANAGER.get(new_pid_u32) {
        unsafe {
            C_CURRENT_PROCESS.pid = (*proc).pid as u64;
            C_CURRENT_PROCESS.pwid = (*proc).pwid.load(Ordering::SeqCst);
            C_CURRENT_PROCESS.state = (*proc).state.load(Ordering::SeqCst);
        }
    }

    user_proc_enter_by_pid(new_pid_u32);
    0
}

#[no_mangle]
pub extern "C" fn proc_wait_child(pid: Pid) -> i32 {
    if pid == 0 { return -1; }

    let proc = PROCESS_TABLE.get(pid);
    if proc.is_none() { return -1; }

    let process = unsafe { &*proc.unwrap() };
    let state = process.get_state();
    if state == ProcessState::Zombie {
        let code = process.exit_code.load(Ordering::SeqCst) as i32;
        // ✅ 修复内存泄漏: 回收 Zombie 子进程 PCB
        PROCESS_TABLE.remove_and_free(pid);
        return code;
    }

    SCHEDULER.block(BlockReason::WaitingForChild);
    -2
}

#[no_mangle]
pub extern "C" fn proc_sleep_ms(ms: u64) {
    // ✅ 修复: 阻塞式睡眠, 不再忙等 (Fix 6)
    if ms == 0 { return; }
    
    let pid = SCHEDULER.current().unwrap_or(0);
    if pid == 0 { return; }
    
    // 获取当前 tick 并计算到期时间
    extern "C" { fn timer_get_ticks() -> u64; }
    let current_ticks = unsafe { timer_get_ticks() };
    // 假设每 tick = 10ms (100Hz), 转换 ms → ticks (最少 1 tick)
    let ticks_to_sleep = (ms + 9) / 10;
    if ticks_to_sleep == 0 { return; }
    
    let wakeup_at = current_ticks + ticks_to_sleep;
    
    // 设置 sleep_until 并阻塞进程
    if let Some(proc) = PROCESS_TABLE.get(pid) {
        unsafe {
            (*proc).sleep_until.store(wakeup_at, Ordering::SeqCst);
        }
    }
    
    SCHEDULER.block(BlockReason::Sleeping);
    SCHEDULER.schedule();
}

/// ✅ fork 系统调用实现 (Fix 7)
/// 深拷贝进程地址空间, 创建子进程并从同一位置继续执行
/// 父进程返回 >0 (子进程 PID), 子进程返回 0
/// 失败返回 0
#[no_mangle]
pub extern "C" fn sys_fork() -> Pid {
    let parent_pid = SCHEDULER.current().unwrap_or(0);
    if parent_pid == 0 {
        unsafe {
            extern "C" { fn klog_ffi_info(msg: *const u8); }
            klog_ffi_info(b"[FORK] No current process\n\0".as_ptr());
        }
        return 0;
    }
    
    let parent_ptr = match PROCESS_TABLE.get(parent_pid) {
        Some(p) => p,
        None => { return 0; }
    };
    
    let parent = unsafe { &*parent_ptr };
    
    // Clone page table
    let parent_cr3 = parent.cr3.load(Ordering::SeqCst);
    extern "C" { fn vmm_clone_user_page_table(parent_pml4: u64) -> u64; }
    let child_cr3 = unsafe { vmm_clone_user_page_table(parent_cr3) };
    if child_cr3 == 0 {
        unsafe {
            extern "C" { fn klog_ffi_info(msg: *const u8); }
            klog_ffi_info(b"[FORK] Page table clone failed\n\0".as_ptr());
        }
        return 0;
    }
    
    // Allocate child PID
    extern "C" { fn proc_alloc_pid() -> Pid; }
    let child_pid = unsafe { proc_alloc_pid() };
    if child_pid == 0 {
        extern "C" { fn vmm_destroy_page_table(pml4: u64); }
        unsafe { vmm_destroy_page_table(child_cr3); }
        return 0;
    }
    
    // Clone parent name
    let parent_name = unsafe { parent.name.lock() };
    let name_str = alloc::string::String::clone(&*parent_name);
    drop(parent_name);
    let name_ref = name_str.as_str();
    
    // Create child Process
    let child = unsafe {
        let layout = alloc::alloc::Layout::new::<Process>();
        let ptr = alloc::alloc::alloc(layout) as *mut Process;
        core::ptr::write(ptr, Process::new(child_pid, name_ref, Some(ProcessId(parent_pid))));
        &mut *ptr
    };
    
    // Copy remaining parent properties
    child.pwid.store(parent.pwid.load(Ordering::SeqCst), Ordering::SeqCst);
    child.cr3.store(child_cr3, Ordering::SeqCst);
    child.sched_policy.store(parent.sched_policy.load(Ordering::SeqCst), Ordering::SeqCst);
    child.rt_priority.store(parent.rt_priority.load(Ordering::SeqCst), Ordering::SeqCst);
    
    // Add child to parent's children list
    parent.children.lock().push(ProcessId(child_pid));
    
    // Allocate kernel stack for child
    if !child.allocate_kernel_stack() {
        unsafe {
            drop(alloc::boxed::Box::from_raw(child as *mut Process));
            extern "C" { fn vmm_destroy_page_table(pml4: u64); }
            vmm_destroy_page_table(child_cr3);
        }
        return 0;
    }
    
    // Copy parent's kernel stack contents to child's kernel stack
    {
        let parent_kstack = parent.kernel_stack.load(Ordering::SeqCst);
        let child_kstack = child.kernel_stack.load(Ordering::SeqCst);
        let stack_size: usize = 65536;
        unsafe {
            core::ptr::copy_nonoverlapping(
                parent_kstack as *const u8,
                child_kstack as *mut u8,
                stack_size,
            );
        }
        crate::kernel::proc::process::kernel_stack_write_canary(child_kstack);
    }
    
    // Copy parent's ProcessContext to child's, but set RAX=0 for child
    {
        let parent_ctx = parent.context.lock();
        let mut child_ctx = child.context.lock();
        *child_ctx = *parent_ctx;
        child_ctx.cr3 = child_cr3;
        child_ctx.rax = 0;
    }
    
    // Register child in process table
    PROCESS_TABLE.insert(child as *const Process as *mut Process);
    
    // Create UserProc for child
    if let Some(parent_up) = USER_PROC_MANAGER.get(parent_pid) {
        extern "C" { fn user_proc_clone(parent_pid: Pid, child_pid: Pid) -> i32; }
        let clone_result = unsafe { user_proc_clone(parent_pid, child_pid) };
        if clone_result < 0 {
            PROCESS_TABLE.remove_and_free(child_pid);
            return 0;
        }
    }
    
    // Add child to scheduler
    let _ = unsafe { (*child).set_state_safe(ProcessState::Ready) };
    SCHEDULER.add_to_run_queue(child_pid);
    
    child_pid
}

#[no_mangle]
pub extern "C" fn proc_get_ppid(pid: Pid) -> Pid {
    let proc = PROCESS_TABLE.get(pid);
    if let Some(p) = proc {
        unsafe { (*p).parent.map(|p| p.0).unwrap_or(0) }
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn proc_set_pwid(pid: Pid, pwid: u64) -> i32 {
    let proc = PROCESS_TABLE.get(pid);
    if let Some(p) = proc {
        unsafe { (*p).pwid.store(pwid, Ordering::SeqCst); }
        0
    } else {
        -1
    }
}
