use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use super::scheduler_ex::ThreadState;

pub const MAX_THREADS: usize = 128;
pub const MAX_THREADS_PER_PROCESS: usize = 16;

pub struct Thread {
    pub tid: u32,
    pub pid: u32,
    pub state: AtomicU32,
    pub priority: AtomicU32,
    pub time_slice: AtomicU64,
    pub cpu_time: AtomicU64,
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,
    pub cr3: AtomicU64,
    pub entry: u64,
    pub exit_code: AtomicU32,
    pub context_ptr: AtomicU64,
}

unsafe impl Send for Thread {}
unsafe impl Sync for Thread {}

impl Thread {
    pub fn new(tid: u32, pid: u32) -> Self {
        Self {
            tid,
            pid,
            state: AtomicU32::new(ThreadState::Created as u32),
            priority: AtomicU32::new(2),
            time_slice: AtomicU64::new(20),
            cpu_time: AtomicU64::new(0),
            kernel_stack: AtomicU64::new(0),
            user_stack: AtomicU64::new(0),
            cr3: AtomicU64::new(0),
            entry: 0,
            exit_code: AtomicU32::new(0),
            context_ptr: AtomicU64::new(0),
        }
    }
}

pub struct ThreadTable {
    threads: Mutex<[Option<*mut Thread>; MAX_THREADS]>,
    next_tid: AtomicU32,
}

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

        // Also add to SchedulerEx for thread-level scheduling
        super::scheduler_ex::SCHEDULER_EX.add_thread(thread as *mut super::scheduler_ex::ThreadNode);

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
