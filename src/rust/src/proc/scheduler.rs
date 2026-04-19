use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::types::*;
use super::process::{Process, PROCESS_TABLE};

extern "C" {
    fn serial_putc(port: u16, c: i8);
}

fn log(s: &str) {
    unsafe {
        for c in s.bytes() {
            serial_putc(0x3F8, c as i8);
        }
    }
}

fn log_num(n: u32) {
    let mut buf = [0u8; 12];
    let mut num = n;
    let mut i = 11;
    
    if num == 0 {
        log("0");
        return;
    }
    
    while num > 0 {
        buf[i] = (num % 10) as u8 + b'0';
        num /= 10;
        i -= 1;
    }
    
    let s = core::str::from_utf8(&buf[i + 1..]).unwrap_or("?");
    log(s);
}

pub struct Scheduler {
    ready_queue: Mutex<VecDeque<Pid>>,
    current: AtomicU32,
    all_ready: Mutex<Vec<Pid>>,
    need_reschedule: AtomicBool,
    initialized: AtomicBool,
}

unsafe impl Send for Scheduler {}
unsafe impl Sync for Scheduler {}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: Mutex::new(VecDeque::new()),
            current: AtomicU32::new(0),
            all_ready: Mutex::new(Vec::new()),
            need_reschedule: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
        }
    }
    
    pub fn init(&self) {
        self.initialized.store(true, Ordering::SeqCst);
        log("[SCHED] Rust scheduler initialized\n");
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
        
        log("[SCHED] Created process: ");
        log_num(pid);
        log("\n");
        
        Some(pid)
    }
    
    pub fn add(&self, pid: Pid) {
        self.ready_queue.lock().push_back(pid);
        self.all_ready.lock().push(pid);
        
        log("[SCHED] Added process ");
        log_num(pid);
        log("\n");
    }
    
    pub fn schedule(&self) -> Option<Pid> {
        let mut ready = self.ready_queue.lock();
        let current_pid = self.current.load(Ordering::SeqCst);
        
        let mut next_pid: Option<Pid> = None;
        let mut skipped: Vec<Pid> = Vec::new();
        
        while let Some(pid) = ready.pop_front() {
            if let Some(process) = PROCESS_TABLE.get(pid) {
                unsafe {
                    let state = (*process).get_state();
                    if state != ProcessState::Blocked && state != ProcessState::Zombie {
                        next_pid = Some(pid);
                        break;
                    } else {
                        skipped.push(pid);
                    }
                }
            } else {
                next_pid = Some(pid);
                break;
            }
        }
        
        for pid in skipped {
            ready.push_back(pid);
        }
        
        if let Some(next_pid) = next_pid {
            if current_pid != 0 && current_pid != next_pid {
                if let Some(process) = PROCESS_TABLE.get(current_pid) {
                    unsafe {
                        let state = (*process).get_state();
                        if state != ProcessState::Blocked && state != ProcessState::Zombie {
                            ready.push_back(current_pid);
                        }
                    }
                } else {
                    ready.push_back(current_pid);
                }
            }
            
            self.current.store(next_pid, Ordering::SeqCst);
            
            log("[SCHED] Scheduled process ");
            log_num(next_pid);
            log("\n");
            
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
                    self.ready_queue.lock().push_back(pid);
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
    }
    
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }
}

pub static SCHEDULER: Scheduler = Scheduler::new();

pub fn init() {
    SCHEDULER.init();
}
