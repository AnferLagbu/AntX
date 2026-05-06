use core::sync::atomic::{AtomicU64, Ordering};

pub struct ThreadManager {
    current_thread: AtomicU64,
}

unsafe impl Send for ThreadManager {}
unsafe impl Sync for ThreadManager {}

impl ThreadManager {
    pub const fn new() -> Self {
        Self {
            current_thread: AtomicU64::new(0),
        }
    }

    pub fn init(&self) {}

    pub fn get_current_thread(&self) -> Option<u64> {
        let id = self.current_thread.load(Ordering::SeqCst);
        if id == 0 { None } else { Some(id) }
    }
}

pub static THREAD_MANAGER: ThreadManager = ThreadManager::new();

pub fn init() {
    THREAD_MANAGER.init();
}
