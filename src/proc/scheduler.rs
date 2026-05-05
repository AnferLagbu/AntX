use alloc::collections::VecDeque;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::types::*;
use super::process::{Process, PROCESS_TABLE};

const MLFQ_LEVELS: usize = 4;
const TIME_SLICES: [u64; MLFQ_LEVELS] = [10, 20, 40, 80];

const RT_PRIORITY_MAX: u8 = 99;
const RT_TIME_SLICE: u64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal = 0,
    Fifo = 1,
    Rr = 2,
    Idle = 3,
}

impl SchedPolicy {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => SchedPolicy::Normal,
            1 => SchedPolicy::Fifo,
            2 => SchedPolicy::Rr,
            3 => SchedPolicy::Idle,
            _ => SchedPolicy::Normal,
        }
    }
}

pub struct RtTaskInfo {
    pub pid: Pid,
    pub rt_priority: u8,
    pub policy: SchedPolicy,
    pub time_slice_remaining: u64,
}

pub struct Scheduler {
    queues: [Mutex<VecDeque<Pid>>; MLFQ_LEVELS],
    rt_queue: Mutex<VecDeque<RtTaskInfo>>,
    current: AtomicU32,
    all_ready: Mutex<Vec<Pid>>,
    need_reschedule: AtomicBool,
    initialized: AtomicBool,
    current_level: AtomicU32,
    time_remaining: AtomicU64,
    rt_running: AtomicBool,
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
            rt_queue: Mutex::new(VecDeque::new()),
            current: AtomicU32::new(0),
            all_ready: Mutex::new(Vec::new()),
            need_reschedule: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            current_level: AtomicU32::new(0),
            time_remaining: AtomicU64::new(TIME_SLICES[0]),
            rt_running: AtomicBool::new(false),
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

    pub fn add_rt_task(&self, pid: Pid, rt_priority: u8, policy: SchedPolicy) {
        let priority = rt_priority.min(RT_PRIORITY_MAX);
        
        let mut rt_queue = self.rt_queue.lock();
        let mut inserted = false;
        
        for i in 0..rt_queue.len() {
            if rt_queue[i].rt_priority < priority {
                rt_queue.insert(i, RtTaskInfo {
                    pid,
                    rt_priority: priority,
                    policy,
                    time_slice_remaining: RT_TIME_SLICE,
                });
                inserted = true;
                break;
            }
        }
        
        if !inserted {
            rt_queue.push_back(RtTaskInfo {
                pid,
                rt_priority: priority,
                policy,
                time_slice_remaining: RT_TIME_SLICE,
            });
        }
    }
    
    pub fn schedule(&self) -> Option<Pid> {
        let current_pid = self.current.load(Ordering::SeqCst);
        let mut next_pid: Option<Pid> = None;

        {
            let mut rt_queue = self.rt_queue.lock();
            
            if !rt_queue.is_empty() {
                let rt_task = rt_queue.pop_front().unwrap();
                let rt_pid = rt_task.pid;
                
                match rt_task.policy {
                    SchedPolicy::Fifo => {
                        next_pid = Some(rt_pid);
                        self.rt_running.store(true, Ordering::SeqCst);
                    }
                    SchedPolicy::Rr => {
                        next_pid = Some(rt_pid);
                        self.rt_running.store(true, Ordering::SeqCst);
                        let mut updated_rt = rt_task;
                        updated_rt.time_slice_remaining = RT_TIME_SLICE;
                        rt_queue.push_back(updated_rt);
                    }
                    _ => {
                        self.queues[0].lock().push_back(rt_pid);
                        self.rt_running.store(false, Ordering::SeqCst);
                    }
                }
            } else {
                self.rt_running.store(false, Ordering::SeqCst);
            }
        }

        if next_pid.is_none() {
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
        }

        if current_pid != 0 && current_pid != next_pid.unwrap_or(0) {
            if let Some(process) = PROCESS_TABLE.get(current_pid) {
                unsafe {
                    let state = (*process).get_state();
                    if state == ProcessState::Running {
                        let is_rt = self.rt_running.load(Ordering::SeqCst);
                        
                        if is_rt {
                            let mut rt_queue = self.rt_queue.lock();
                            let rt_priority = (*process).get_rt_priority();
                            let policy = (*process).get_sched_policy();
                            
                            if policy != SchedPolicy::Fifo {
                                let mut inserted = false;
                                for i in 0..rt_queue.len() {
                                    if rt_queue[i].rt_priority < rt_priority {
                                        rt_queue.insert(i, RtTaskInfo {
                                            pid: current_pid,
                                            rt_priority,
                                            policy,
                                            time_slice_remaining: RT_TIME_SLICE,
                                        });
                                        inserted = true;
                                        break;
                                    }
                                }
                                if !inserted {
                                    rt_queue.push_back(RtTaskInfo {
                                        pid: current_pid,
                                        rt_priority,
                                        policy,
                                        time_slice_remaining: RT_TIME_SLICE,
                                    });
                                }
                            }
                        } else {
                            let level = (self.current_level.load(Ordering::SeqCst) as usize + 1).min(MLFQ_LEVELS - 1);
                            self.queues[level].lock().push_back(current_pid);
                        }
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
                    
                    let is_rt = (*process).get_sched_policy() == SchedPolicy::Fifo || 
                                 (*process).get_sched_policy() == SchedPolicy::Rr;
                    
                    if is_rt {
                        let rt_priority = (*process).get_rt_priority();
                        let policy = (*process).get_sched_policy();
                        self.add_rt_task(pid, rt_priority, policy);
                    } else {
                        let boost_level = 0usize;
                        self.queues[boost_level].lock().push_back(pid);
                    }
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
        {
            let rt_queue = self.rt_queue.lock();
            if !rt_queue.is_empty() {
                return true;
            }
        }
        
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
        let is_rt = self.rt_running.load(Ordering::SeqCst);
        
        if is_rt {
            let remaining = self.time_remaining.fetch_sub(1, Ordering::SeqCst);
            if remaining <= 1 {
                self.need_reschedule.store(true, Ordering::SeqCst);
                self.time_remaining.store(RT_TIME_SLICE, Ordering::SeqCst);
            }
        } else {
            let remaining = self.time_remaining.fetch_sub(1, Ordering::SeqCst);
            if remaining <= 1 {
                self.need_reschedule.store(true, Ordering::SeqCst);
                self.time_remaining.store(TIME_SLICES[self.current_level.load(Ordering::SeqCst) as usize], Ordering::SeqCst);
            }
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
    
    pub fn set_sched_policy(&self, pid: Pid, policy: SchedPolicy, rt_priority: u8) -> bool {
        if let Some(process) = PROCESS_TABLE.get(pid) {
            unsafe {
                (*process).set_sched_policy(policy);
                (*process).set_rt_priority(rt_priority.min(RT_PRIORITY_MAX));
            }
            true
        } else {
            false
        }
    }
    
    pub fn get_rt_count(&self) -> usize {
        self.rt_queue.lock().len()
    }
}

pub static SCHEDULER: Scheduler = Scheduler::new();

pub fn init() {
    SCHEDULER.init();
}