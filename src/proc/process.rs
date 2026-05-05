use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::types::*;
use super::scheduler::{SchedPolicy};

extern "C" {
    fn pmm_alloc_pages(count: u64) -> *mut u8;
    fn vmm_create_user_page_table() -> u64;
    fn vmm_destroy_page_table(cr3: u64);
}

pub struct Process {
    pub pid: ProcessId,
    pub state: AtomicU32,
    pub priority: AtomicU32,
    pub flags: AtomicU32,
    
    pub name: Mutex<String>,
    pub parent: Option<ProcessId>,
    pub children: Mutex<Vec<ProcessId>>,
    
    pub context: Mutex<ProcessContext>,
    pub cr3: AtomicU64,
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,
    
    pub exit_code: AtomicU32,
    pub cpu_time: AtomicU64,
    
    pub block_reason: AtomicU32,
    
    pub sched_policy: AtomicU32,
    pub rt_priority: AtomicU32,
}

unsafe impl Send for Process {}
unsafe impl Sync for Process {}

impl Process {
    pub fn new(pid: Pid, name: &str, parent: Option<ProcessId>) -> Self {
        Self {
            pid: ProcessId(pid),
            state: AtomicU32::new(ProcessState::Created as u32),
            priority: AtomicU32::new(ProcessPriority::Normal as u32),
            flags: AtomicU32::new(0),
            name: Mutex::new(String::from(name)),
            parent,
            children: Mutex::new(Vec::new()),
            context: Mutex::new(ProcessContext::new()),
            cr3: AtomicU64::new(0),
            kernel_stack: AtomicU64::new(0),
            user_stack: AtomicU64::new(0),
            exit_code: AtomicU32::new(0),
            cpu_time: AtomicU64::new(0),
            block_reason: AtomicU32::new(BlockReason::Unknown as u32),
            sched_policy: AtomicU32::new(SchedPolicy::Normal as u32),
            rt_priority: AtomicU32::new(0),
        }
    }
    
    pub fn allocate_kernel_stack(&self) -> bool {
        unsafe {
            let stack = pmm_alloc_pages((KERNEL_STACK_SIZE / 4096) as u64);
            if stack.is_null() {
                return false;
            }
            self.kernel_stack.store(stack.add(KERNEL_STACK_SIZE) as u64, Ordering::SeqCst);
            true
        }
    }
    
    pub fn allocate_user_space(&self) -> bool {
        unsafe {
            let cr3 = vmm_create_user_page_table();
            if cr3 == 0 {
                return false;
            }
            self.cr3.store(cr3, Ordering::SeqCst);
            true
        }
    }
    
    pub fn get_state(&self) -> ProcessState {
        ProcessState::from_u8(self.state.load(Ordering::SeqCst) as u8)
    }
    
    pub fn set_state(&self, state: ProcessState) {
        self.state.store(state as u32, Ordering::SeqCst);
    }
    
    pub fn get_priority(&self) -> ProcessPriority {
        ProcessPriority::from_u32(self.priority.load(Ordering::SeqCst))
    }
    
    pub fn set_priority(&self, priority: ProcessPriority) {
        self.priority.store(priority as u32, Ordering::SeqCst);
    }
    
    pub fn is_kernel(&self) -> bool {
        let flags = self.flags.load(Ordering::SeqCst);
        (flags & ProcessFlags::IS_KERNEL.bits()) != 0
    }
    
    pub fn set_kernel(&self, is_kernel: bool) {
        let mut flags = self.flags.load(Ordering::SeqCst);
        if is_kernel {
            flags |= ProcessFlags::IS_KERNEL.bits();
        } else {
            flags &= !ProcessFlags::IS_KERNEL.bits();
        }
        self.flags.store(flags, Ordering::SeqCst);
    }
    
    pub fn get_sched_policy(&self) -> SchedPolicy {
        SchedPolicy::from_u32(self.sched_policy.load(Ordering::SeqCst))
    }
    
    pub fn set_sched_policy(&self, policy: SchedPolicy) {
        self.sched_policy.store(policy as u32, Ordering::SeqCst);
    }
    
    pub fn get_rt_priority(&self) -> u8 {
        self.rt_priority.load(Ordering::SeqCst) as u8
    }
    
    pub fn set_rt_priority(&self, priority: u8) {
        self.rt_priority.store(priority as u32, Ordering::SeqCst);
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let cr3 = self.cr3.load(Ordering::SeqCst);
        if cr3 != 0 {
            unsafe {
                vmm_destroy_page_table(cr3);
            }
        }
    }
}

pub struct ProcessTable {
    processes: Mutex<[Option<usize>; MAX_PROCESSES]>,
    next_pid: AtomicU32,
}

unsafe impl Send for ProcessTable {}
unsafe impl Sync for ProcessTable {}

impl ProcessTable {
    pub const fn new() -> Self {
        Self {
            processes: Mutex::new([None; MAX_PROCESSES]),
            next_pid: AtomicU32::new(1),
        }
    }
    
    pub fn allocate_pid(&self) -> Option<Pid> {
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        if pid as usize >= MAX_PROCESSES {
            None
        } else {
            Some(pid)
        }
    }
    
    pub fn insert(&self, process: *mut Process) -> bool {
        let mut table = self.processes.lock();
        let pid = unsafe { (*process).pid.0 as usize };
        if pid >= MAX_PROCESSES {
            return false;
        }
        table[pid] = Some(process as usize);
        true
    }
    
    pub fn get(&self, pid: Pid) -> Option<*mut Process> {
        let table = self.processes.lock();
        if pid as usize >= MAX_PROCESSES {
            return None;
        }
        table[pid as usize].map(|addr| addr as *mut Process)
    }
    
    pub fn remove(&self, pid: Pid) -> Option<*mut Process> {
        let mut table = self.processes.lock();
        if pid as usize >= MAX_PROCESSES {
            return None;
        }
        table[pid as usize].take().map(|addr| addr as *mut Process)
    }
}

pub static PROCESS_TABLE: ProcessTable = ProcessTable::new();

pub fn init() {
}
