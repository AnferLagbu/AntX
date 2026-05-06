use core::ffi::c_char;
use core::sync::atomic::{AtomicU64, AtomicU32, Ordering};

use super::types::*;
use super::scheduler::SCHEDULER;
use super::thread::THREAD_MANAGER;
use super::scheduler_ex::SCHEDULER_EX;
use super::session::SESSION_MANAGER;
use super::user_proc::USER_PROC_MANAGER;
use super::process::PROCESS_TABLE;

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
pub extern "C" fn process_create(name: *const c_char, parent_pid: Pid) -> Pid {
    proc_create_internal(name, parent_pid)
}

#[no_mangle]
pub extern "C" fn process_exit(exit_code: u32) {
    SCHEDULER.exit(exit_code);
}

#[no_mangle]
pub extern "C" fn process_kill(pid: u32, exit_code: u32) {
    // Set exit code and exit the process
    SCHEDULER.exit(exit_code);
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
    fn pmm_alloc_pages(count: u64) -> *mut u8;
}

#[no_mangle]
pub extern "C" fn user_proc_load_elf(path: *const c_char, pwid: u64) -> i32 {
    if path.is_null() {
        return -1;
    }
    
    let fd = unsafe { crate::fs::vfs::ffi::vfs_open(path, 0, pwid) };
    if fd < 0 {
        return -1;
    }
    
    let buffer = unsafe { pmm_alloc_pages((ELF_MAX_SIZE / 4096) as u64) };
    if buffer.is_null() {
        unsafe { crate::fs::vfs::ffi::vfs_close(fd as u32) };
        return -1;
    }
    
    let bytes_read = unsafe { 
        crate::fs::vfs::ffi::vfs_read(fd as u32, buffer, ELF_MAX_SIZE as u32) 
    };
    
    unsafe { crate::fs::vfs::ffi::vfs_close(fd as u32) };
    
    if bytes_read <= 0 {
        extern "C" { fn pmm_free_pages(addr: *mut u8, count: u64); }
        unsafe { pmm_free_pages(buffer, (ELF_MAX_SIZE / 4096) as u64) };
        return -1;
    }
    
    USER_PROC_MANAGER.load_elf_from_memory(buffer, bytes_read as u64, pwid)
}

#[no_mangle]
pub extern "C" fn user_proc_load_elf_from_memory(elf_data: *const u8, elf_size: u64, pwid: u64) -> i32 {
    USER_PROC_MANAGER.load_elf_from_memory(elf_data, elf_size, pwid)
}

#[no_mangle]
pub extern "C" fn user_proc_enter_by_pid(pid: u32) -> i32 {
    if let Some(proc) = USER_PROC_MANAGER.get(pid) {
        unsafe {
            C_CURRENT_PROCESS.pid = (*proc).pid as u64;
            C_CURRENT_PROCESS.pwid = (*proc).pwid.load(Ordering::SeqCst);
            C_CURRENT_PROCESS.state = (*proc).state.load(Ordering::SeqCst);
            C_CURRENT_PROCESS.parent_pid = 1; // init is parent
        }
        USER_PROC_MANAGER.enter(proc);
        0
    } else {
        -1
    }
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
pub extern "C" fn proc_create_internal(name: *const c_char, parent_pid: Pid) -> Pid {
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
    
    SCHEDULER.create_process(name_str, parent).unwrap_or(0)
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
