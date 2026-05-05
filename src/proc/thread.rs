use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

extern "C" {
    fn klog_ffi_info(msg: *const u8);
    fn pmm_alloc_pages(count: u64) -> *mut u8;
    fn pmm_free_page(page: *mut u8);
    fn vmm_create_user_page_table() -> u64;
    fn timer_get_ticks() -> u64;
}

fn log(s: &str) {
    unsafe { klog_ffi_info(s.as_ptr()); }
}

fn log_num(n: u64) {
    if n == 0 {
        log("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut num = n;
    let mut i = 19;
    while num > 0 {
        buf[i] = (num % 10) as u8 + b'0';
        num /= 10;
        i -= 1;
    }
    let s = core::str::from_utf8(&buf[i + 1..]).unwrap_or("?");
    log(s);
}

pub const MAX_THREADS: usize = 256;
pub const MAX_PROCESSES: usize = 64;
pub const KERNEL_STACK_SIZE: usize = 8192;
pub const USER_STACK_SIZE: usize = 65536;
pub const PAGE_SIZE: usize = 4096;
pub const DEFAULT_TIME_SLICE: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreadState {
    Created = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
    Exited = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BlockReason {
    None = 0,
    Waiting = 1,
    Sleeping = 2,
    Waitpid = 3,
    Io = 4,
    Unknown = 255,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreadPriority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Realtime = 4,
}

#[repr(C)]
pub struct CpuContext {
    pub r15: u64, pub r14: u64, pub r13: u64, pub r12: u64,
    pub r11: u64, pub r10: u64, pub r9: u64, pub r8: u64,
    pub rbp: u64, pub rdi: u64, pub rsi: u64, pub rdx: u64,
    pub rcx: u64, pub rbx: u64, pub rax: u64,
    pub rip: u64, pub cs: u64, pub rflags: u64,
    pub rsp: u64, pub ss: u64, pub cr3: u64,
}

impl CpuContext {
    pub const fn new() -> Self {
        Self {
            r15: 0, r14: 0, r13: 0, r12: 0,
            r11: 0, r10: 0, r9: 0, r8: 0,
            rbp: 0, rdi: 0, rsi: 0, rdx: 0,
            rcx: 0, rbx: 0, rax: 0,
            rip: 0, cs: 0x08, rflags: 0x202,
            rsp: 0, ss: 0x10, cr3: 0,
        }
    }
}

pub type Tid = u32;
pub type Pid = u32;

#[repr(C)]
pub struct Thread {
    pub tid: Tid,
    pub pid: Pid,
    pub state: AtomicU32,
    pub priority: AtomicU32,
    pub block_reason: AtomicU32,
    
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,
    pub user_stack_base: u64,
    pub user_stack_size: u64,
    
    pub context: Mutex<CpuContext>,
    
    pub cpu_time: AtomicU64,
    pub start_time: u64,
    pub sleep_until: AtomicU64,
    pub time_slice: AtomicU32,
    
    pub tls_base: u64,
    pub entry_point: u64,
    pub entry_arg: u64,
    
    pub exit_code: AtomicU32,
    pub wait_tid: Tid,
    
    pub next: AtomicU64,
    pub prev: AtomicU64,
    pub process_next: AtomicU64,
}

impl Thread {
    pub const fn new() -> Self {
        Self {
            tid: 0,
            pid: 0,
            state: AtomicU32::new(ThreadState::Created as u32),
            priority: AtomicU32::new(ThreadPriority::Normal as u32),
            block_reason: AtomicU32::new(BlockReason::None as u32),
            kernel_stack: AtomicU64::new(0),
            user_stack: AtomicU64::new(0),
            user_stack_base: 0,
            user_stack_size: 0,
            context: Mutex::new(CpuContext::new()),
            cpu_time: AtomicU64::new(0),
            start_time: 0,
            sleep_until: AtomicU64::new(0),
            time_slice: AtomicU32::new(DEFAULT_TIME_SLICE),
            tls_base: 0,
            entry_point: 0,
            entry_arg: 0,
            exit_code: AtomicU32::new(0),
            wait_tid: 0,
            next: AtomicU64::new(0),
            prev: AtomicU64::new(0),
            process_next: AtomicU64::new(0),
        }
    }
}

#[repr(C)]
pub struct Process {
    pub pid: Pid,
    pub parent_pid: Pid,
    pub pwid: AtomicU64,
    pub session_id: AtomicU64,
    
    pub name: [u8; 64],
    pub cr3: AtomicU64,
    
    pub main_thread: AtomicU64,
    pub thread_list: AtomicU64,
    pub thread_count: AtomicU32,
    
    pub parent: AtomicU64,
    pub children: AtomicU64,
    pub sibling: AtomicU64,
    
    pub cwd: [u8; 256],
    pub root: [u8; 256],
    
    pub exit_code: AtomicU32,
    pub exit_status: AtomicU32,
    
    pub start_time: u64,
    pub cpu_time: AtomicU64,
    
    pub umask: AtomicU32,
    
    pub stdin_fd: AtomicU32,
    pub stdout_fd: AtomicU32,
    pub stderr_fd: AtomicU32,
}

impl Process {
    pub const fn new() -> Self {
        Self {
            pid: 0,
            parent_pid: 0,
            pwid: AtomicU64::new(0),
            session_id: AtomicU64::new(0),
            name: [0; 64],
            cr3: AtomicU64::new(0),
            main_thread: AtomicU64::new(0),
            thread_list: AtomicU64::new(0),
            thread_count: AtomicU32::new(0),
            parent: AtomicU64::new(0),
            children: AtomicU64::new(0),
            sibling: AtomicU64::new(0),
            cwd: [0; 256],
            root: [0; 256],
            exit_code: AtomicU32::new(0),
            exit_status: AtomicU32::new(0),
            start_time: 0,
            cpu_time: AtomicU64::new(0),
            umask: AtomicU32::new(0o22),
            stdin_fd: AtomicU32::new(0),
            stdout_fd: AtomicU32::new(1),
            stderr_fd: AtomicU32::new(2),
        }
    }
}

pub struct WaitQueue {
    head: AtomicU64,
    count: AtomicU32,
}

impl WaitQueue {
    pub const fn new() -> Self {
        Self {
            head: AtomicU64::new(0),
            count: AtomicU32::new(0),
        }
    }
}

pub struct ThreadManager {
    thread_table: Mutex<[Thread; MAX_THREADS]>,
    process_table: Mutex<[Process; MAX_PROCESSES]>,
    current_thread: AtomicU64,
    next_tid: AtomicU32,
    next_pid: AtomicU32,
}

unsafe impl Send for ThreadManager {}
unsafe impl Sync for ThreadManager {}

impl ThreadManager {
    pub const fn new() -> Self {
        Self {
            thread_table: Mutex::new([const { Thread::new() }; MAX_THREADS]),
            process_table: Mutex::new([const { Process::new() }; MAX_PROCESSES]),
            current_thread: AtomicU64::new(0),
            next_tid: AtomicU32::new(1),
            next_pid: AtomicU32::new(1),
        }
    }
    
    pub fn init(&self) {
        log("[THREAD] Thread system initialized\n");
    }
    
    fn alloc_thread(&self) -> Option<usize> {
        let table = self.thread_table.lock();
        for i in 0..MAX_THREADS {
            if table[i].state.load(Ordering::SeqCst) == ThreadState::Created as u32 
                && table[i].tid == 0 {
                return Some(i);
            }
        }
        None
    }
    
    fn alloc_process(&self) -> Option<usize> {
        let table = self.process_table.lock();
        for i in 0..MAX_PROCESSES {
            if table[i].pid == 0 {
                return Some(i);
            }
        }
        None
    }
    
    pub fn create_thread(&self, pid: Pid, entry: u64, arg: u64, priority: ThreadPriority) -> Option<Tid> {
        let idx = self.alloc_thread()?;
        let proc_idx = self.find_process(pid)?;
        
        let tid = self.next_tid.fetch_add(1, Ordering::SeqCst);
        
        let mut table = self.thread_table.lock();
        let thread = &mut table[idx];
        
        thread.tid = tid;
        thread.pid = pid;
        thread.state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        thread.priority.store(priority as u32, Ordering::SeqCst);
        thread.block_reason.store(BlockReason::None as u32, Ordering::SeqCst);
        
        unsafe {
            let stack = pmm_alloc_pages((KERNEL_STACK_SIZE / PAGE_SIZE) as u64);
            if stack.is_null() {
                return None;
            }
            thread.kernel_stack.store(stack.add(KERNEL_STACK_SIZE) as u64, Ordering::SeqCst);
        }
        
        let proc_table = self.process_table.lock();
        let proc = &proc_table[proc_idx];
        thread.context.lock().cr3 = proc.cr3.load(Ordering::SeqCst);
        thread.context.lock().rip = entry;
        thread.context.lock().rsp = thread.kernel_stack.load(Ordering::SeqCst);
        drop(proc_table);
        
        thread.entry_point = entry;
        thread.entry_arg = arg;
        thread.time_slice.store(DEFAULT_TIME_SLICE, Ordering::SeqCst);
        
        log("[THREAD] Created thread TID=");
        log_num(tid as u64);
        log(" PID=");
        log_num(pid as u64);
        log("\n");
        
        Some(tid)
    }
    
    pub fn create_user_thread(&self, pid: Pid, entry: u64, stack_top: u64, priority: ThreadPriority) -> Option<Tid> {
        let idx = self.alloc_thread()?;
        let proc_idx = self.find_process(pid)?;
        
        let tid = self.next_tid.fetch_add(1, Ordering::SeqCst);
        
        let mut table = self.thread_table.lock();
        let thread = &mut table[idx];
        
        thread.tid = tid;
        thread.pid = pid;
        thread.state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        thread.priority.store(priority as u32, Ordering::SeqCst);
        
        unsafe {
            let stack = pmm_alloc_pages((KERNEL_STACK_SIZE / PAGE_SIZE) as u64);
            if stack.is_null() {
                return None;
            }
            thread.kernel_stack.store(stack.add(KERNEL_STACK_SIZE) as u64, Ordering::SeqCst);
        }
        
        thread.user_stack.store(stack_top, Ordering::SeqCst);
        thread.user_stack_base = stack_top - USER_STACK_SIZE as u64;
        thread.user_stack_size = USER_STACK_SIZE as u64;
        
        let proc_table = self.process_table.lock();
        let proc = &proc_table[proc_idx];
        let mut ctx = thread.context.lock();
        ctx.cr3 = proc.cr3.load(Ordering::SeqCst);
        ctx.rip = entry;
        ctx.rsp = stack_top;
        ctx.cs = 0x1B;
        ctx.ss = 0x23;
        drop(proc_table);
        
        log("[THREAD] Created user thread TID=");
        log_num(tid as u64);
        log("\n");
        
        Some(tid)
    }
    
    pub fn thread_exit(&self, tid: Tid, exit_code: i32) {
        let idx = match self.find_thread(tid) {
            Some(i) => i,
            None => return,
        };
        
        let mut table = self.thread_table.lock();
        let thread = &mut table[idx];
        
        thread.exit_code.store(exit_code as u32, Ordering::SeqCst);
        thread.state.store(ThreadState::Zombie as u32, Ordering::SeqCst);
        
        let kernel_stack = thread.kernel_stack.load(Ordering::SeqCst);
        if kernel_stack != 0 {
            unsafe {
                for i in 0..(KERNEL_STACK_SIZE / PAGE_SIZE) {
                    pmm_free_page((kernel_stack - KERNEL_STACK_SIZE as u64 + (i * PAGE_SIZE) as u64) as *mut u8);
                }
            }
            thread.kernel_stack.store(0, Ordering::SeqCst);
        }
        
        log("[THREAD] Thread exited TID=");
        log_num(tid as u64);
        log("\n");
    }
    
    pub fn thread_block(&self, tid: Tid, reason: BlockReason) {
        if let Some(idx) = self.find_thread(tid) {
            let table = self.thread_table.lock();
            table[idx].state.store(ThreadState::Blocked as u32, Ordering::SeqCst);
            table[idx].block_reason.store(reason as u32, Ordering::SeqCst);
        }
    }
    
    pub fn thread_unblock(&self, tid: Tid) {
        if let Some(idx) = self.find_thread(tid) {
            let table = self.thread_table.lock();
            let state = table[idx].state.load(Ordering::SeqCst);
            if state == ThreadState::Blocked as u32 {
                table[idx].state.store(ThreadState::Ready as u32, Ordering::SeqCst);
                table[idx].block_reason.store(BlockReason::None as u32, Ordering::SeqCst);
            }
        }
    }
    
    fn find_thread(&self, tid: Tid) -> Option<usize> {
        let table = self.thread_table.lock();
        for i in 0..MAX_THREADS {
            if table[i].tid == tid {
                return Some(i);
            }
        }
        None
    }
    
    fn find_process(&self, pid: Pid) -> Option<usize> {
        let table = self.process_table.lock();
        for i in 0..MAX_PROCESSES {
            if table[i].pid == pid {
                return Some(i);
            }
        }
        None
    }
    
    pub fn create_process(&self, name: &str, parent_pid: Pid, pwid: u64) -> Option<Pid> {
        let idx = self.alloc_process()?;
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        
        let mut table = self.process_table.lock();
        let proc = &mut table[idx];
        
        proc.pid = pid;
        proc.parent_pid = parent_pid;
        proc.pwid.store(pwid, Ordering::SeqCst);
        
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(63);
        proc.name[..len].copy_from_slice(&name_bytes[..len]);
        proc.name[len] = 0;
        
        let cr3 = unsafe { vmm_create_user_page_table() };
        if cr3 == 0 {
            proc.pid = 0;
            return None;
        }
        proc.cr3.store(cr3, Ordering::SeqCst);
        
        proc.cwd[0] = b'/';
        proc.cwd[1] = 0;
        proc.root[0] = b'/';
        proc.root[1] = 0;
        
        proc.start_time = unsafe { timer_get_ticks() };
        
        log("[PROCESS] Created PID=");
        log_num(pid as u64);
        log(" name=");
        log(name);
        log("\n");
        
        Some(pid)
    }
    
    pub fn get_current_thread(&self) -> Option<usize> {
        let ptr = self.current_thread.load(Ordering::SeqCst);
        if ptr == 0 {
            None
        } else {
            Some(ptr as usize)
        }
    }
    
    pub fn set_current_thread(&self, idx: usize) {
        self.current_thread.store(idx as u64, Ordering::SeqCst);
    }
}

pub static THREAD_MANAGER: ThreadManager = ThreadManager::new();

pub fn init() {
    THREAD_MANAGER.init();
}
