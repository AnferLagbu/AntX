use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::types::{ThreadState, ThreadPriority, SCHED_LEVEL_2_QUANTUM};

pub const MAX_THREADS: usize = 128;
pub const MAX_THREADS_PER_PROCESS: usize = 16;

/// ✅ 统一线程结构体 — 合并了 Thread 和 ThreadNode, 消除类型强转 UB
///
/// 字段分为三组:
/// 1. 调度器链表字段 (next/prev) — 供 SchedulerEx 环形双向链表使用
/// 2. 调度器记账字段 (priority/time_slice/cpu_time/sleep_until/state_change_count/frozen_since)
/// 3. 线程资源字段 (entry/kernel_stack/user_stack/cr3/context_ptr/rsp/cs/ss/rflags)
#[repr(C)]
pub struct Thread {
    // === 调度器链表 (SchedulerEx 环形双向链表) ===
    pub next: AtomicU64,
    pub prev: AtomicU64,

    // === 线程标识 ===
    pub tid: u32,
    pub pid: u32,

    // === 调度状态 ===
    pub state: AtomicU32,
    pub priority: AtomicU32,
    pub time_slice: AtomicU32,
    pub cpu_time: AtomicU64,
    pub sleep_until: AtomicU64,

    // === 内核/用户栈 ===
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,

    // === 页表 & 入口 ===
    pub cr3: AtomicU64,
    pub entry: u64,

    // === 上下文指针 (指向 ProcessContext 用于硬件切换) ===
    pub context_ptr: AtomicU64,

    // === Ring 3 上下文 (iretq 用的段选择子和栈) ===
    pub rsp: u64,
    pub cs: u64,
    pub ss: u64,
    pub rflags: u64,

    // === 退出 ===
    pub exit_code: AtomicU32,

    // === 状态追踪 ===
    pub state_change_count: AtomicU64,
    pub frozen_since: AtomicU64,
}

// SAFETY: Thread uses AtomicU64 for linked-list pointers and state tracking.
// Primitive fields (tid, pid, etc.) are written only during creation or
// under scheduler lock. No UnsafeCell without synchronization.
unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

impl Thread {
    pub fn new(tid: u32, pid: u32) -> Self {
        Self {
            next: AtomicU64::new(0),
            prev: AtomicU64::new(0),
            tid,
            pid,
            state: AtomicU32::new(ThreadState::Created as u32),
            priority: AtomicU32::new(ThreadPriority::Normal as u32),
            time_slice: AtomicU32::new(SCHED_LEVEL_2_QUANTUM),
            cpu_time: AtomicU64::new(0),
            sleep_until: AtomicU64::new(0),
            kernel_stack: AtomicU64::new(0),
            user_stack: AtomicU64::new(0),
            cr3: AtomicU64::new(0),
            entry: 0,
            context_ptr: AtomicU64::new(0),
            rsp: 0,
            cs: 0x08,
            ss: 0x10,
            rflags: 0x202,
            exit_code: AtomicU32::new(0),
            state_change_count: AtomicU64::new(0),
            frozen_since: AtomicU64::new(0),
        }
    }

    /// ✅ 安全的状态设置 (带合法性检查)
    pub fn set_state_safe(&self, new_state: ThreadState) -> Result<(), &'static str> {
        let current = ThreadState::from_u32(self.state.load(Ordering::Acquire));
        
        match (current, new_state) {
            (ThreadState::Created, ThreadState::Ready) => {},
            (ThreadState::Ready, ThreadState::Running) => {},
            (ThreadState::Running, ThreadState::Ready) => {},
            (ThreadState::Running, ThreadState::Blocked) => {},
            (ThreadState::Running, ThreadState::Zombie) => {},
            (ThreadState::Running, ThreadState::Frozen) => {},
            (ThreadState::Ready, ThreadState::Frozen) => {},
            (ThreadState::Blocked, ThreadState::Frozen) => {},
            (ThreadState::Blocked, ThreadState::Ready) => {},
            (ThreadState::Blocked, ThreadState::Zombie) => {},
            (ThreadState::Zombie, ThreadState::Terminated) => {},
            (ThreadState::Frozen, ThreadState::Ready) => {},
            (ThreadState::Frozen, ThreadState::Blocked) => {},
            _ => return Err("Illegal state transition"),
        }
        
        self.state.store(new_state as u32, Ordering::Release);
        self.state_change_count.fetch_add(1, Ordering::Relaxed);
        
        if new_state == ThreadState::Frozen {
            self.frozen_since.store(crate::kernel::timer::get_ticks(), Ordering::Relaxed);
        }
        
        Ok(())
    }

    pub fn get_state(&self) -> ThreadState {
        ThreadState::from_u32(self.state.load(Ordering::Acquire))
    }

    pub fn is_runnable(&self) -> bool {
        self.get_state().is_runnable()
    }

    pub fn is_alive(&self) -> bool {
        self.get_state().is_alive()
    }

    pub fn can_freeze(&self) -> bool {
        self.get_state().can_freeze()
    }
}

pub struct ThreadTable {
    threads: Mutex<[Option<*mut Thread>; MAX_THREADS]>,
    next_tid: AtomicU32,
}

use spin::Mutex;

// SAFETY: ThreadTable uses Mutex for the thread array and AtomicU32
// for next_tid. All mutations are serialized through the Mutex.
unsafe impl Send for ThreadTable {}
unsafe impl Sync for ThreadTable {}

impl ThreadTable {
    const fn new() -> Self {
        Self {
            threads: Mutex::new([None; MAX_THREADS]),
            next_tid: AtomicU32::new(1),
        }
    }

    pub fn allocate(&self) -> Option<u32> {
        let tid = self.next_tid.fetch_add(1, Ordering::SeqCst);
        if (tid as usize) < MAX_THREADS {
            Some(tid)
        } else {
            None
        }
    }

    pub fn insert(&self, thread: *mut Thread) -> bool {
        if thread.is_null() { return false; }
        let tid = unsafe { (*thread).tid };
        let mut table = self.threads.lock();
        if (tid as usize) < MAX_THREADS {
            table[tid as usize] = Some(thread);
            true
        } else {
            false
        }
    }

    pub fn get(&self, tid: u32) -> Option<*mut Thread> {
        if (tid as usize) >= MAX_THREADS { return None; }
        self.threads.lock()[tid as usize]
    }

    pub fn remove(&self, tid: u32) {
        if (tid as usize) < MAX_THREADS {
            self.threads.lock()[tid as usize] = None;
        }
    }
}

static THREAD_TABLE: ThreadTable = ThreadTable::new();

pub struct ThreadManager {
    current_thread: AtomicU64,
    thread_count: AtomicU32,
}

// SAFETY: ThreadManager uses only AtomicU64/AtomicU32 fields.
// All operations are lock-free atomic, safe for concurrent access.
unsafe impl Send for ThreadManager {}
unsafe impl Sync for ThreadManager {}

impl ThreadManager {
    pub const fn new() -> Self {
        Self {
            current_thread: AtomicU64::new(0),
            thread_count: AtomicU32::new(0),
        }
    }

    pub fn init(&self) {}

    pub fn create_thread(
        &self,
        pid: u32,
        entry: u64,
        user_stack: u64,
        kernel_stack: u64,
        cr3: u64,
    ) -> Option<u32> {
        let tid = THREAD_TABLE.allocate()?;

        let thread = unsafe { alloc::alloc::alloc(
            alloc::alloc::Layout::new::<Thread>()
        ) as *mut Thread };

        if thread.is_null() {
            return None;
        }

        unsafe {
            core::ptr::write(thread, Thread::new(tid, pid));
            (*thread).entry = entry;
            (*thread).user_stack.store(user_stack, Ordering::SeqCst);
            (*thread).kernel_stack.store(kernel_stack, Ordering::SeqCst);
            (*thread).cr3.store(cr3, Ordering::SeqCst);
            (*thread).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        }

        if !THREAD_TABLE.insert(thread) {
            unsafe { alloc::alloc::dealloc(
                thread as *mut u8,
                alloc::alloc::Layout::new::<Thread>()
            )};
            return None;
        }

        self.thread_count.fetch_add(1, Ordering::SeqCst);

        // ✅ 类型安全: Thread 现在包含调度器链表字段, 直接传入
        super::scheduler_ex::SCHEDULER_EX.add_thread(thread);

        Some(tid)
    }

    pub fn get_current_thread(&self) -> Option<u64> {
        let id = self.current_thread.load(Ordering::SeqCst);
        if id == 0 { None } else { Some(id) }
    }

    pub fn set_current(&self, tid: u32) {
        self.current_thread.store(tid as u64, Ordering::SeqCst);
    }

    pub fn get_thread(&self, tid: u32) -> Option<*mut Thread> {
        THREAD_TABLE.get(tid)
    }

    pub fn exit_current(&self, exit_code: u32) {
        if let Some(tid) = self.get_current_thread() {
            if let Some(thread) = THREAD_TABLE.get(tid as u32) {
                unsafe {
                    (*thread).exit_code.store(exit_code, Ordering::SeqCst);
                    (*thread).state.store(ThreadState::Zombie as u32, Ordering::SeqCst);
                }
            }
        }
    }

    pub fn count(&self) -> u32 {
        self.thread_count.load(Ordering::SeqCst)
    }
}

pub static THREAD_MANAGER: ThreadManager = ThreadManager::new();

pub fn init() {
    THREAD_MANAGER.init();
}