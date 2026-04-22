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

#[no_mangle]
pub extern "C" fn process_get_current() -> u64 {
    CURRENT_PROCESS_PTR.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn process_get_current_pid() -> u32 {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn process_get_by_pid(_pid: u32) -> u64 {
    0
}

#[no_mangle]
pub extern "C" fn process_create(name: *const c_char, parent_pid: Pid) -> Pid {
    rust_proc_create(name, parent_pid)
}

#[no_mangle]
pub extern "C" fn process_exit(exit_code: u32) {
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
    THREAD_MANAGER.get_current_thread().map(|_| 1).unwrap_or(0) as u64
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

#[no_mangle]
pub extern "C" fn user_proc_load_elf(_path: *const c_char, _pwid: u64) -> i32 {
    -1
}

#[no_mangle]
pub extern "C" fn user_proc_load_elf_from_memory(elf_data: *const u8, elf_size: u64, pwid: u64) -> i32 {
    USER_PROC_MANAGER.load_elf_from_memory(elf_data, elf_size, pwid)
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
pub extern "C" fn rust_proc_create(name: *const c_char, parent_pid: Pid) -> Pid {
    if name.is_null() {
        return 0;
    }
    
    let name_str = unsafe {
        let len = (0..).find(|&i| *name.add(i) == 0).unwrap_or(0);
        let slice = core::slice::from_raw_parts(name as *const u8, len);
        core::str::from_utf8_unchecked(slice)
    };
    
    let parent = if parent_pid == 0 {
        None
    } else {
        Some(parent_pid)
    };
    
    SCHEDULER.create_process(name_str, parent).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn rust_proc_exit(exit_code: u32) {
    SCHEDULER.exit(exit_code);
}

#[no_mangle]
pub extern "C" fn rust_proc_get_current() -> Pid {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn rust_proc_yield() {
    SCHEDULER.yield_current();
}

#[no_mangle]
pub extern "C" fn rust_proc_block(reason: u32) {
    let block_reason = BlockReason::from_u8(reason as u8);
    SCHEDULER.block(block_reason);
}

#[no_mangle]
pub extern "C" fn rust_proc_unblock(pid: Pid) {
    SCHEDULER.unblock(pid);
}

#[no_mangle]
pub extern "C" fn rust_proc_set_priority(pid: Pid, priority: u32) -> i32 {
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
pub extern "C" fn rust_proc_get_state(pid: Pid) -> u32 {
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
pub extern "C" fn rust_sched_init() {
    SCHEDULER.init();
}

#[no_mangle]
pub extern "C" fn rust_sched_add(pid: Pid) {
    use super::process::PROCESS_TABLE;
    
    if PROCESS_TABLE.get(pid).is_none() {
        let name = alloc::format!("proc_{}", pid);
        let _ = SCHEDULER.create_process(&name, None);
    }
    SCHEDULER.add(pid);
}

#[no_mangle]
pub extern "C" fn rust_sched_schedule() -> Pid {
    SCHEDULER.schedule().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn rust_sched_should_reschedule() -> i32 {
    if SCHEDULER.should_reschedule() { 1 } else { 0 }
}

#[no_mangle]
pub extern "C" fn rust_sched_set_current(pid: Pid) {
    SCHEDULER.set_current(pid);
}

#[no_mangle]
pub extern "C" fn rust_sched_get_current() -> Pid {
    SCHEDULER.current().unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn rust_proc_get_exit_code(pid: Pid) -> i32 {
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
pub extern "C" fn rust_proc_is_initialized() -> i32 {
    if SCHEDULER.is_initialized() { 1 } else { 0 }
}
