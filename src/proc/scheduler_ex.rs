use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

extern "C" {
    fn serial_putc(port: u16, c: i8);
    fn serial_puts(port: u16, s: *const i8);
    fn timer_get_ticks() -> u64;
    fn tss_set_kernel_stack(rsp0: u64);
}

fn log(s: &str) {
    unsafe {
        for c in s.bytes() {
            serial_putc(0x3F8, c as i8);
        }
    }
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

pub const SCHED_LEVELS: usize = 4;
pub const SCHED_LEVEL_0_QUANTUM: u32 = 5;
pub const SCHED_LEVEL_1_QUANTUM: u32 = 10;
pub const SCHED_LEVEL_2_QUANTUM: u32 = 20;
pub const SCHED_LEVEL_3_QUANTUM: u32 = 40;
pub const SCHED_BOOST_INTERVAL: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreadPriority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    Realtime = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreadState {
    Created = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
}

#[repr(C)]
pub struct ThreadNode {
    pub tid: u32,
    pub pid: u32,
    pub state: AtomicU32,
    pub priority: AtomicU32,
    pub time_slice: AtomicU32,
    pub cpu_time: AtomicU64,
    pub sleep_until: AtomicU64,
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,
    pub next: AtomicU64,
    pub prev: AtomicU64,
    pub context_ptr: AtomicU64,
    pub cr3: AtomicU64,
    pub entry: u64,
    pub rsp: u64,
    pub cs: u64,
    pub ss: u64,
    pub rflags: u64,
}

impl ThreadNode {
    pub const fn new() -> Self {
        Self {
            tid: 0,
            pid: 0,
            state: AtomicU32::new(ThreadState::Created as u32),
            priority: AtomicU32::new(ThreadPriority::Normal as u32),
            time_slice: AtomicU32::new(SCHED_LEVEL_2_QUANTUM),
            cpu_time: AtomicU64::new(0),
            sleep_until: AtomicU64::new(0),
            kernel_stack: AtomicU64::new(0),
            user_stack: AtomicU64::new(0),
            next: AtomicU64::new(0),
            prev: AtomicU64::new(0),
            context_ptr: AtomicU64::new(0),
            cr3: AtomicU64::new(0),
            entry: 0,
            rsp: 0,
            cs: 0x08,
            ss: 0x10,
            rflags: 0x202,
        }
    }
}

pub struct RunQueue {
    pub queues: [AtomicU64; SCHED_LEVELS],
    pub counts: [AtomicU32; SCHED_LEVELS],
    pub total: AtomicU32,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            queues: [const { AtomicU64::new(0) }; SCHED_LEVELS],
            counts: [const { AtomicU32::new(0) }; SCHED_LEVELS],
            total: AtomicU32::new(0),
        }
    }
}

pub struct SchedulerStats {
    pub context_switches: AtomicU64,
    pub total_ticks: AtomicU64,
}

impl SchedulerStats {
    pub const fn new() -> Self {
        Self {
            context_switches: AtomicU64::new(0),
            total_ticks: AtomicU64::new(0),
        }
    }
}

pub struct SchedulerEx {
    pub runq: RunQueue,
    pub current: AtomicU64,
    pub idle_thread: AtomicU64,
    pub tick_count: AtomicU64,
    pub last_boost: AtomicU64,
    pub need_reschedule: AtomicU32,
    pub stats: SchedulerStats,
}

unsafe impl Send for SchedulerEx {}
unsafe impl Sync for SchedulerEx {}

impl SchedulerEx {
    pub const fn new() -> Self {
        Self {
            runq: RunQueue::new(),
            current: AtomicU64::new(0),
            idle_thread: AtomicU64::new(0),
            tick_count: AtomicU64::new(0),
            last_boost: AtomicU64::new(0),
            need_reschedule: AtomicU32::new(0),
            stats: SchedulerStats::new(),
        }
    }
    
    fn priority_to_level(priority: ThreadPriority) -> usize {
        match priority {
            ThreadPriority::Realtime => 0,
            ThreadPriority::High => 1,
            ThreadPriority::Normal => 2,
            ThreadPriority::Low | ThreadPriority::Idle => 3,
        }
    }
    
    fn level_to_quantum(level: usize) -> u32 {
        match level {
            0 => SCHED_LEVEL_0_QUANTUM,
            1 => SCHED_LEVEL_1_QUANTUM,
            2 => SCHED_LEVEL_2_QUANTUM,
            _ => SCHED_LEVEL_3_QUANTUM,
        }
    }
    
    pub fn init(&self) {
        log("[SCHED] MLFQ scheduler initialized\n");
    }
    
    pub fn add_thread(&self, thread: *mut ThreadNode) {
        if thread.is_null() { return; }
        
        unsafe {
            (*thread).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
            
            let priority = match (*thread).priority.load(Ordering::SeqCst) {
                0 => ThreadPriority::Idle,
                1 => ThreadPriority::Low,
                2 => ThreadPriority::Normal,
                3 => ThreadPriority::High,
                _ => ThreadPriority::Realtime,
            };
            let level = Self::priority_to_level(priority);
            
            (*thread).time_slice.store(Self::level_to_quantum(level), Ordering::SeqCst);
            
            let head = self.runq.queues[level].load(Ordering::SeqCst);
            
            if head == 0 {
                (*thread).next.store(thread as u64, Ordering::SeqCst);
                (*thread).prev.store(thread as u64, Ordering::SeqCst);
                self.runq.queues[level].store(thread as u64, Ordering::SeqCst);
            } else {
                let head_ptr = head as *const ThreadNode;
                let tail = unsafe { (*head_ptr).prev.load(Ordering::SeqCst) };
                (*thread).prev.store(tail, Ordering::SeqCst);
                (*thread).next.store(head, Ordering::SeqCst);
                
                unsafe {
                    let tail_ptr = tail as *mut ThreadNode;
                    let head_ptr_mut = head as *mut ThreadNode;
                    (*tail_ptr).next.store(thread as u64, Ordering::SeqCst);
                    (*head_ptr_mut).prev.store(thread as u64, Ordering::SeqCst);
                }
            }
            
            self.runq.counts[level].fetch_add(1, Ordering::SeqCst);
            self.runq.total.fetch_add(1, Ordering::SeqCst);
            
            log("[SCHED] Added thread TID=");
            log_num((*thread).tid as u64);
            log(" to level ");
            log_num(level as u64);
            log("\n");
        }
    }
    
    fn run_queue_pop(&self, level: usize) -> Option<*mut ThreadNode> {
        if level >= SCHED_LEVELS { return None; }
        
        let head = self.runq.queues[level].load(Ordering::SeqCst);
        if head == 0 { return None; }
        
        let thread = head as *mut ThreadNode;
        
        unsafe {
            let next = (*thread).next.load(Ordering::SeqCst);
            let prev = (*thread).prev.load(Ordering::SeqCst);
            
            if next == head {
                self.runq.queues[level].store(0, Ordering::SeqCst);
            } else {
                (*(next as *mut ThreadNode)).prev.store(prev, Ordering::SeqCst);
                (*(prev as *mut ThreadNode)).next.store(next, Ordering::SeqCst);
                self.runq.queues[level].store(next, Ordering::SeqCst);
            }
            
            (*thread).next.store(0, Ordering::SeqCst);
            (*thread).prev.store(0, Ordering::SeqCst);
        }
        
        self.runq.counts[level].fetch_sub(1, Ordering::SeqCst);
        self.runq.total.fetch_sub(1, Ordering::SeqCst);
        
        Some(thread)
    }
    
    fn pop_highest(&self) -> Option<*mut ThreadNode> {
        for level in 0..SCHED_LEVELS {
            while self.runq.queues[level].load(Ordering::SeqCst) != 0 {
                if let Some(thread) = self.run_queue_pop(level) {
                    unsafe {
                        if (*thread).state.load(Ordering::SeqCst) == ThreadState::Ready as u32 {
                            return Some(thread);
                        }
                    }
                }
            }
        }
        None
    }
    
    pub fn tick(&self) {
        self.tick_count.fetch_add(1, Ordering::SeqCst);
        self.stats.total_ticks.fetch_add(1, Ordering::SeqCst);
        
        let current = self.current.load(Ordering::SeqCst);
        if current != 0 {
            unsafe {
                let thread = current as *mut ThreadNode;
                let time_slice = (*thread).time_slice.fetch_sub(1, Ordering::SeqCst);
                (*thread).cpu_time.fetch_add(1, Ordering::SeqCst);
                
                let sleep_until = (*thread).sleep_until.load(Ordering::SeqCst);
                if sleep_until != 0 {
                    let ticks = unsafe { timer_get_ticks() };
                    if ticks >= sleep_until {
                        (*thread).sleep_until.store(0, Ordering::SeqCst);
                        (*thread).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
                    }
                }
                
                if time_slice <= 1 {
                    self.need_reschedule.store(1, Ordering::SeqCst);
                }
            }
        }
        
        let tick_count = self.tick_count.load(Ordering::SeqCst);
        let last_boost = self.last_boost.load(Ordering::SeqCst);
        if tick_count - last_boost >= SCHED_BOOST_INTERVAL {
            self.boost_all();
            self.last_boost.store(tick_count, Ordering::SeqCst);
        }
        
        if self.need_reschedule.load(Ordering::SeqCst) != 0 {
            self.schedule();
        }
    }
    
    pub fn schedule(&self) {
        let prev = self.current.load(Ordering::SeqCst);
        
        if prev != 0 {
            unsafe {
                let thread = prev as *mut ThreadNode;
                let state = (*thread).state.load(Ordering::SeqCst);
                
                if state == ThreadState::Blocked as u32 {
                } else if state == ThreadState::Running as u32 {
                    let priority = (*thread).priority.load(Ordering::SeqCst);
                    let mut level = Self::priority_to_level(match priority {
                        0 => ThreadPriority::Idle,
                        1 => ThreadPriority::Low,
                        2 => ThreadPriority::Normal,
                        3 => ThreadPriority::High,
                        _ => ThreadPriority::Realtime,
                    });
                    if level < SCHED_LEVELS - 1 {
                        level += 1;
                    }
                    (*thread).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
                    self.add_thread(thread);
                }
            }
        }
        
        let next = match self.pop_highest() {
            Some(t) => t,
            None => {
                let idle = self.idle_thread.load(Ordering::SeqCst);
                if idle != 0 {
                    idle as *mut ThreadNode
                } else {
                    return;
                }
            }
        };
        
        unsafe {
            (*next).state.store(ThreadState::Running as u32, Ordering::SeqCst);
        }
        self.current.store(next as u64, Ordering::SeqCst);
        self.need_reschedule.store(0, Ordering::SeqCst);
        self.stats.context_switches.fetch_add(1, Ordering::SeqCst);
        
        log("[SCHED] Switch to TID=");
        log_num(unsafe { (*next).tid as u64 });
        log("\n");
        
        unsafe {
            tss_set_kernel_stack((*next).kernel_stack.load(Ordering::SeqCst));
        }
    }
    
    pub fn boost_all(&self) {
        log("[SCHED] Priority boost\n");
        
        for level in 1..SCHED_LEVELS {
            while self.runq.queues[level].load(Ordering::SeqCst) != 0 {
                if let Some(thread) = self.run_queue_pop(level) {
                    self.add_thread(thread);
                }
            }
        }
    }
    
    pub fn get_current(&self) -> Option<*mut ThreadNode> {
        let current = self.current.load(Ordering::SeqCst);
        if current != 0 {
            Some(current as *mut ThreadNode)
        } else {
            None
        }
    }
    
    pub fn yield_current(&self) {
        self.need_reschedule.store(1, Ordering::SeqCst);
        self.schedule();
    }
    
    pub fn dump_state(&self) {
        log("=== Scheduler State ===\n");
        log("Current TID: ");
        if let Some(thread) = self.get_current() {
            unsafe {
                log_num((*thread).tid as u64);
            }
        } else {
            log("none");
        }
        log("\n");
        
        log("Run queues:\n");
        for i in 0..SCHED_LEVELS {
            log("  Level ");
            log_num(i as u64);
            log(": ");
            log_num(self.runq.counts[i].load(Ordering::SeqCst) as u64);
            log(" threads\n");
        }
        
        log("Context switches: ");
        log_num(self.stats.context_switches.load(Ordering::SeqCst));
        log("\n");
    }
}

pub static SCHEDULER_EX: SchedulerEx = SchedulerEx::new();

pub fn init() {
    SCHEDULER_EX.init();
}
