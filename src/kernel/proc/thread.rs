//! Thread Management System
//!
//! Implements a 1:1 threading model (like Linux) where each thread is a
//! separate kernel scheduling entity (Process) that may share address space
//! with other threads in the same thread group.
//!
//! # Thread Model
//!
//! - Threads within a process share: CR3 (page tables), file descriptors, heap
//! - Each thread has its own: kernel stack, user stack, register state, TID
//! - The "thread group leader" is the first thread (PID == TID)
//! - Thread creation uses clone() with CLONE_VM flag
//!
//! # Clone Flags
//!
//! - CLONE_VM:     Share address space (threads)
//! - CLONE_FS:     Share filesystem info
//! - CLONE_FILES:  Share file descriptor table
//! - CLONE_SIGHAND:Share signal handlers

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use alloc::vec;
use alloc::format;
use alloc::vec::Vec;
use spin::Mutex;

use super::types::*;
use super::process::{Process, PROCESS_TABLE};

pub const CLONE_VM: u64 = 0x00000100;
pub const CLONE_FS: u64 = 0x00000200;
pub const CLONE_FILES: u64 = 0x00000400;
pub const CLONE_SIGHAND: u64 = 0x00000800;
pub const CLONE_THREAD: u64 = 0x00010000;

const MAX_THREADS_PER_PROCESS: usize = 128;

macro_rules! klog_thread {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_info, $($arg)*)
    };
}

#[repr(C)]
pub struct ThreadInfo {
    pub tid: Tid,
    pub pid: Pid,
    pub flags: u64,
    pub is_main: bool,
}

struct ThreadGroup {
    leader_pid: Pid,
    threads: Vec<Tid>,
    shared_cr3: AtomicU64,
    ref_count: AtomicU32,
}

static THREAD_GROUPS: Mutex<Vec<ThreadGroup>> = Mutex::new(Vec::new());

pub fn thread_create(parent_pid: Pid, clone_flags: u64, entry: u64, stack: u64, arg: u64) -> Option<Tid> {
    let parent = PROCESS_TABLE.get(parent_pid)?;

    let child_pid = PROCESS_TABLE.allocate_pid()?;

    let child_process = unsafe {
        let layout = alloc::alloc::Layout::new::<Process>();
        let ptr = alloc::alloc::alloc(layout) as *mut Process;
        if ptr.is_null() {
            PROCESS_TABLE.free_pids.lock().push_back(child_pid);
            return None;
        }

        let parent_ref = &*parent;
        ptr.write(Process::new(child_pid, &format!("{}-t{}", parent_ref.name.lock().as_str(), child_pid), Some(ProcessId(parent_pid))));

        if clone_flags & CLONE_VM != 0 {
            let parent_cr3 = parent_ref.cr3.load(Ordering::SeqCst);
            (*ptr).cr3.store(parent_cr3, Ordering::SeqCst);

            increment_thread_group_ref(parent_pid);
        } else {
            if !(*ptr).allocate_user_space() {
                alloc::alloc::dealloc(ptr as *mut u8, alloc::alloc::Layout::new::<Process>());
                PROCESS_TABLE.free_pids.lock().push_back(child_pid);
                return None;
            }
        }

        if !(*ptr).allocate_kernel_stack() {
            alloc::alloc::dealloc(ptr as *mut u8, alloc::alloc::Layout::new::<Process>());
            PROCESS_TABLE.free_pids.lock().push_back(child_pid);
            return None;
        }

        (*ptr).set_state_safe(ProcessState::Ready).ok();

        let kstack_top = (*ptr).kernel_stack.load(Ordering::SeqCst);
        let _kstack_base = kstack_top - KERNEL_STACK_SIZE as u64;

        unsafe {
            let rsp = kstack_top - 8;

            core::ptr::write(rsp as *mut u64, entry);
            let rsp = rsp - 8;
            core::ptr::write(rsp as *mut u64, arg);
            let rsp = rsp - 8;
            core::ptr::write(rsp as *mut u64, stack);
            let rsp = rsp - 8;
            core::ptr::write(rsp as *mut u64, 0);

            (*ptr).kernel_rsp.store(rsp, Ordering::SeqCst);
        }

        let parent_children = &parent_ref.children;
        parent_children.lock().push(ProcessId(child_pid));

        ptr
    };

    if !PROCESS_TABLE.insert(child_process) {
        unsafe {
            alloc::alloc::dealloc(child_process as *mut u8, alloc::alloc::Layout::new::<Process>());
        }
        PROCESS_TABLE.free_pids.lock().push_back(child_pid);
        return None;
    }

    if clone_flags & CLONE_VM != 0 {
        add_thread_to_group(parent_pid, child_pid);
    } else {
        create_thread_group(child_pid);
    }

    if clone_flags & CLONE_THREAD != 0 {
        unsafe {
            (*child_process).flags.fetch_or(
                super::types::ProcessFlags::IS_KERNEL.bits(),
                Ordering::SeqCst
            );
        }
    }

    klog_thread!("[THREAD] Created TID={} in PID={}, flags=0x{:X}", child_pid, parent_pid, clone_flags);

    Some(child_pid)
}

pub fn thread_exit(tid: Tid, exit_code: i32) -> bool {
    let process = match PROCESS_TABLE.get(tid) {
        Some(p) => p,
        None => return false,
    };

    unsafe {
        (*process).exit_code.store(exit_code as u32, Ordering::SeqCst);
        let _ = (*process).set_state_safe(ProcessState::Zombie);
    }

    let is_shared_vm = is_thread_in_group(tid);

    if is_shared_vm {
        let ref_count = decrement_thread_group_ref(tid);
        if ref_count == 0 {
            unsafe {
                (*process).cr3.store(0, Ordering::SeqCst);
            }
        } else {
            unsafe {
                (*process).cr3.store(0, Ordering::SeqCst);
            }
        }
    }

    remove_thread_from_group(tid);

    klog_thread!("[THREAD] TID={} exited with code {}", tid, exit_code);
    true
}

pub fn thread_join(tid: Tid) -> Option<i32> {
    let process = PROCESS_TABLE.get(tid)?;

    let state = unsafe { (*process).get_state() };
    if state != ProcessState::Zombie {
        return None;
    }

    let exit_code = unsafe { (*process).exit_code.load(Ordering::SeqCst) as i32 };

    if let Some(_ptr) = PROCESS_TABLE.remove(tid) {
        unsafe {
            alloc::alloc::dealloc(_ptr as *mut u8, alloc::alloc::Layout::new::<Process>());
        }
    }

    Some(exit_code)
}

pub fn thread_get_info(tid: Tid) -> Option<ThreadInfo> {
    let process = PROCESS_TABLE.get(tid)?;
    unsafe {
        Some(ThreadInfo {
            tid,
            pid: (*process).parent.map(|p| p.0).unwrap_or(tid),
            flags: 0,
            is_main: !is_thread_in_group(tid) || is_thread_group_leader(tid),
        })
    }
}

pub fn thread_get_count(pid: Pid) -> usize {
    let groups = THREAD_GROUPS.lock();
    for group in groups.iter() {
        if group.leader_pid == pid {
            return group.threads.len();
        }
    }
    1
}

fn create_thread_group(leader_pid: Pid) {
    let mut groups = THREAD_GROUPS.lock();
    groups.push(ThreadGroup {
        leader_pid,
        threads: vec![leader_pid],
        shared_cr3: AtomicU64::new(0),
        ref_count: AtomicU32::new(1),
    });
}

fn add_thread_to_group(leader_pid: Pid, tid: Tid) {
    let mut groups = THREAD_GROUPS.lock();
    for group in groups.iter_mut() {
        if group.leader_pid == leader_pid {
            group.threads.push(tid);
            group.ref_count.fetch_add(1, Ordering::SeqCst);
            return;
        }
    }
    groups.push(ThreadGroup {
        leader_pid,
        threads: vec![leader_pid, tid],
        shared_cr3: AtomicU64::new(0),
        ref_count: AtomicU32::new(2),
    });
}

fn remove_thread_from_group(tid: Tid) {
    let mut groups = THREAD_GROUPS.lock();
    for i in 0..groups.len() {
        let group = &mut groups[i];
        if let Some(pos) = group.threads.iter().position(|&t| t == tid) {
            group.threads.swap_remove(pos);
            if group.threads.is_empty() {
                groups.remove(i);
            }
            return;
        }
    }
}

fn is_thread_in_group(tid: Tid) -> bool {
    let groups = THREAD_GROUPS.lock();
    for group in groups.iter() {
        if group.threads.contains(&tid) {
            return true;
        }
    }
    false
}

fn is_thread_group_leader(tid: Tid) -> bool {
    let groups = THREAD_GROUPS.lock();
    for group in groups.iter() {
        if group.leader_pid == tid {
            return true;
        }
    }
    false
}

fn increment_thread_group_ref(pid: Pid) {
    let groups = THREAD_GROUPS.lock();
    for group in groups.iter() {
        if group.leader_pid == pid || group.threads.contains(&pid) {
            group.ref_count.fetch_add(1, Ordering::SeqCst);
            return;
        }
    }
}

fn decrement_thread_group_ref(tid: Tid) -> u32 {
    let groups = THREAD_GROUPS.lock();
    for group in groups.iter() {
        if group.threads.contains(&tid) {
            return group.ref_count.fetch_sub(1, Ordering::SeqCst);
        }
    }
    0
}

pub struct ThreadManager {
    next_tid: AtomicU32,
}

unsafe impl Send for ThreadManager {}
unsafe impl Sync for ThreadManager {}

impl ThreadManager {
    pub const fn new() -> Self {
        Self {
            next_tid: AtomicU32::new(1),
        }
    }

    pub fn init(&self) {
        create_thread_group(0);
        klog_thread!("[THREAD] ThreadManager initialized");
    }

    pub fn create_thread(&self, parent_pid: Pid, flags: u64, entry: u64, stack: u64, arg: u64) -> Option<Tid> {
        thread_create(parent_pid, flags, entry, stack, arg)
    }

    pub fn exit_thread(&self, tid: Tid, exit_code: i32) -> bool {
        thread_exit(tid, exit_code)
    }

    pub fn join_thread(&self, tid: Tid) -> Option<i32> {
        thread_join(tid)
    }

    pub fn get_thread_count(&self, pid: Pid) -> usize {
        thread_get_count(pid)
    }

    pub fn get_current_thread(&self) -> Option<u64> {
        let pid = super::scheduler::SCHEDULER.current()?;
        PROCESS_TABLE.get(pid).map(|p| p as u64)
    }
}

pub static THREAD_MANAGER: ThreadManager = ThreadManager::new();

pub fn init() {
    THREAD_MANAGER.init();
}

#[no_mangle]
pub extern "C" fn thread_manager_init() { init(); }
#[no_mangle]
pub extern "C" fn thread_create_c(parent_pid: u32, flags: u64, entry: u64, stack: u64, arg: u64) -> u32 {
    thread_create(parent_pid, flags, entry, stack, arg).unwrap_or(0)
}
#[no_mangle]
pub extern "C" fn thread_exit_c(tid: u32, exit_code: i32) -> bool {
    thread_exit(tid, exit_code)
}
#[no_mangle]
pub extern "C" fn thread_join_c(tid: u32) -> i32 {
    thread_join(tid).unwrap_or(-1)
}
