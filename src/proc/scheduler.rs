use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::types::*;
use super::process::{Process, PROCESS_TABLE};

const MLFQ_LEVELS: usize = 4;
const TIME_SLICES: [u64; MLFQ_LEVELS] = [10, 20, 40, 80];

pub struct Scheduler {
    queues: [Mutex<VecDeque<Pid>>; MLFQ_LEVELS],
    current: AtomicU32,
    all_ready: Mutex<Vec<Pid>>,
    need_reschedule: AtomicBool,
    initialized: AtomicBool,
    current_level: AtomicU32,
    time_remaining: AtomicU64,
}

unsafe impl Send for Scheduler {}
unsafe impl Sync for Scheduler {}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            queues: [
                Mutex::new(VecDeque::new()),
                Mutex::new(VecDeque::new()),
                Mutex::new(VecDeque::new()),
                Mutex::new(VecDeque::new()),
            ],
            current: AtomicU32::new(0),
            all_ready: Mutex::new(Vec::new()),
            need_reschedule: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            current_level: AtomicU32::new(0),
            time_remaining: AtomicU64::new(TIME_SLICES[0]),
        }
    }
    
    pub fn init(&self) {
        self.initialized.store(true, Ordering::SeqCst);

        let init_pid = self.create_process("init", None);
        if let Some(pid) = init_pid {
            if let Some(process_ptr) = PROCESS_TABLE.get(pid) {
                unsafe {
                    (*process_ptr).set_state(ProcessState::Running);
                    (*process_ptr).set_priority(ProcessPriority::Normal);
                }
                self.set_current(pid);

                unsafe {
                    extern "C" {
                        fn update_current_process_ptr(ptr: u64);
                    }
                    update_current_process_ptr(process_ptr as u64);
                }
            }
        }
    }
    
    pub fn create_process(&self, name: &str, parent: Option<Pid>) -> Option<Pid> {
        let pid = PROCESS_TABLE.allocate_pid()?;
        
        let parent_id = parent.map(ProcessId);
        let process = alloc::boxed::Box::new(Process::new(pid, name, parent_id));
        
        let process_ptr = alloc::boxed::Box::into_raw(process);
        
        if !PROCESS_TABLE.insert(process_ptr) {
            unsafe { alloc::alloc::dealloc(process_ptr as *mut u8, alloc::alloc::Layout::new::<Process>()) };
            return None;
        }
        
        Some(pid)
    }
    
    pub fn add(&self, pid: Pid) {
        self.queues[0].lock().push_back(pid);
        self.all_ready.lock().push(pid);
    }

    pub fn add_with_priority(&self, pid: Pid, level: usize) {
        if level < MLFQ_LEVELS {
            self.queues[level].lock().push_back(pid);
            self.all_ready.lock().push(pid);
        }
    }
    
    pub fn schedule(&self) -> Option<Pid> {
        let current_pid = self.current.load(Ordering::SeqCst);
        let mut next_pid: Option<Pid> = None;

        for level in 0..MLFQ_LEVELS {
            let mut queue = self.queues[level].lock();

            while let Some(pid) = queue.pop_front() {
                if let Some(process) = PROCESS_TABLE.get(pid) {
                    unsafe {
                        let state = (*process).get_state();
                        if state != ProcessState::Blocked && state != ProcessState::Zombie {
                            next_pid = Some(pid);
                            self.current_level.store(level as u32, Ordering::SeqCst);
                            self.time_remaining.store(TIME_SLICES[level], Ordering::SeqCst);
                            break;
                        } else {
                            queue.push_back(pid);
                        }
                    }
                } else {
                    next_pid = Some(pid);
                    self.current_level.store(level as u32, Ordering::SeqCst);
                    self.time_remaining.store(TIME_SLICES[level], Ordering::SeqCst);
                    break;
                }
            }

            if next_pid.is_some() {
                break;
            }
        }

        if current_pid != 0 && current_pid != next_pid.unwrap_or(0) {
            if let Some(process) = PROCESS_TABLE.get(current_pid) {
                unsafe {
                    let state = (*process).get_state();
                    if state == ProcessState::Running {
                        let level = (self.current_level.load(Ordering::SeqCst) as usize + 1).min(MLFQ_LEVELS - 1);
                        self.queues[level].lock().push_back(current_pid);
                        (*process).set_state(ProcessState::Ready);
                    }
                }
            }
        }

        if let Some(next_pid) = next_pid {
            self.current.store(next_pid, Ordering::SeqCst);

            if let Some(process_ptr) = PROCESS_TABLE.get(next_pid) {
                unsafe {
                    extern "C" {
                        fn update_current_process_ptr(ptr: u64);
                    }
                    update_current_process_ptr(process_ptr as u64);

                    (*process_ptr).set_state(ProcessState::Running);
                }
            }

            Some(next_pid)
        } else {
            None
        }
    }

    pub fn current(&self) -> Option<Pid> {
        let pid = self.current.load(Ordering::SeqCst);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
    
    pub fn get_current_process(&self) -> Option<*mut Process> {
        let pid = self.current.load(Ordering::SeqCst);
        if pid == 0 {
            None
        } else {
            PROCESS_TABLE.get(pid)
        }
    }
    
    pub fn block(&self, reason: BlockReason) {
        if let Some(pid) = self.current() {
            if let Some(process) = PROCESS_TABLE.get(pid) {
                unsafe {
                    (*process).set_state(ProcessState::Blocked);
                    (*process).block_reason.store(reason as u32, Ordering::SeqCst);
                }
            }
            self.need_reschedule.store(true, Ordering::SeqCst);
        }
    }
    
    pub fn unblock(&self, pid: Pid) {
        if let Some(process) = PROCESS_TABLE.get(pid) {
            unsafe {
                let state = (*process).get_state();
                if state == ProcessState::Blocked {
                    (*process).set_state(ProcessState::Ready);
                    let boost_level = 0usize;
                    self.queues[boost_level].lock().push_back(pid);
                }
            }
        }
    }
    
    pub fn exit(&self, exit_code: u32) {
        if let Some(pid) = self.current() {
            if let Some(process) = PROCESS_TABLE.get(pid) {
                unsafe {
                    (*process).exit_code.store(exit_code, Ordering::SeqCst);
                    (*process).set_state(ProcessState::Zombie);
                    
                    if let Some(parent_pid) = (*process).parent {
                        self.unblock(parent_pid.0);
                    }
                }
            }
            self.need_reschedule.store(true, Ordering::SeqCst);
        }
    }
    
    pub fn yield_current(&self) {
        self.need_reschedule.store(true, Ordering::SeqCst);
    }
    
    pub fn should_reschedule(&self) -> bool {
        self.need_reschedule.swap(false, Ordering::SeqCst)
    }
    
    pub fn set_current(&self, pid: Pid) {
        self.current.store(pid, Ordering::SeqCst);

        if let Some(process_ptr) = PROCESS_TABLE.get(pid) {
            unsafe {
                extern "C" {
                    fn update_current_process_ptr(ptr: u64);
                }
                update_current_process_ptr(process_ptr as u64);
            }
        }
    }
    
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }

    pub fn has_runnable(&self) -> bool {
        for level in 0..MLFQ_LEVELS {
            let queue = self.queues[level].lock();
            if !queue.is_empty() {
                return true;
            }
        }
        false
    }

    pub fn get_time_slice(&self) -> u64 {
        self.time_remaining.load(Ordering::SeqCst)
    }

    pub fn get_current_level(&self) -> u32 {
        self.current_level.load(Ordering::SeqCst)
    }

    pub fn tick(&self) {
        let remaining = self.time_remaining.fetch_sub(1, Ordering::SeqCst);
        if remaining <= 1 {
            self.need_reschedule.store(true, Ordering::SeqCst);
            self.time_remaining.store(TIME_SLICES[self.current_level.load(Ordering::SeqCst) as usize], Ordering::SeqCst);
        }
    }

    pub fn boost_priority(&self) {
        for level in 1..MLFQ_LEVELS {
            let mut queue = self.queues[level].lock();
            while let Some(pid) = queue.pop_front() {
                self.queues[0].lock().push_back(pid);
            }
        }
    }
}

pub static SCHEDULER: Scheduler = Scheduler::new();

pub fn init() {
    SCHEDULER.init();
}
