use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

extern "C" {
    fn klog_ffi_info(msg: *const u8);
}

fn log(s: &str) {
    unsafe {
        klog_ffi_info(s.as_ptr());
    }
}

pub use crate::kernel::framework::config::MAX_SESSIONS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    Active = 0,
    Zombie = 1,
}

#[repr(C)]
pub struct Session {
    pub session_id: AtomicU64,
    pub pwm: AtomicU64,
    pub parent_sid: AtomicU64,
    pub terminal: AtomicU64,
    pub create_time: AtomicU64,
    pub state: AtomicU32,
    pub process_list: AtomicU64,
    pub process_count: AtomicU32,
    pub next: AtomicU64,
}

impl Session {
    pub const fn new() -> Self {
        Self {
            session_id: AtomicU64::new(0),
            pwm: AtomicU64::new(0),
            parent_sid: AtomicU64::new(0),
            terminal: AtomicU64::new(0),
            create_time: AtomicU64::new(0),
            state: AtomicU32::new(SessionState::Zombie as u32),
            process_list: AtomicU64::new(0),
            process_count: AtomicU32::new(0),
            next: AtomicU64::new(0),
        }
    }
}

pub struct SessionManager {
    session_table: Mutex<[Session; MAX_SESSIONS]>,
    next_session_id: AtomicU64,
}

// All fields (Mutex<[Session; N]>, AtomicU64) auto-implement Send + Sync.

impl SessionManager {
    pub const fn new() -> Self {
        Self {
            session_table: Mutex::new([const { Session::new() }; MAX_SESSIONS]),
            next_session_id: AtomicU64::new(1),
        }
    }

    pub fn init(&self) {
        log("Session manager initialized\n");
    }

    fn alloc_session(&self) -> Option<usize> {
        let table = self.session_table.lock();
        for i in 0..MAX_SESSIONS {
            if table[i].session_id.load(Ordering::SeqCst) == 0 {
                return Some(i);
            }
        }
        None
    }

    pub fn create(&self, pwm: u64) -> Option<u64> {
        let idx = self.alloc_session()?;
        let sid = self.next_session_id.fetch_add(1, Ordering::SeqCst);

        let mut table = self.session_table.lock();
        let session = &mut table[idx];

        session.session_id.store(sid, Ordering::SeqCst);
        session.pwm.store(pwm, Ordering::SeqCst);
        session.parent_sid.store(0, Ordering::SeqCst);
        session.terminal.store(0, Ordering::SeqCst);
        session.create_time.store(0, Ordering::SeqCst);
        session
            .state
            .store(SessionState::Active as u32, Ordering::SeqCst);
        session.process_list.store(0, Ordering::SeqCst);
        session.process_count.store(0, Ordering::SeqCst);
        session.next.store(0, Ordering::SeqCst);

        log("Session created: SID=");
        log_num(sid);
        log("\n");

        Some(sid)
    }

    pub fn destroy(&self, session_id: u64) {
        if let Some(idx) = self.find_session(session_id) {
            let mut table = self.session_table.lock();
            let session = &mut table[idx];

            session.session_id.store(0, Ordering::SeqCst);
            session
                .state
                .store(SessionState::Zombie as u32, Ordering::SeqCst);
            session.process_list.store(0, Ordering::SeqCst);
            session.process_count.store(0, Ordering::SeqCst);

            log("Session destroyed: SID=");
            log_num(session_id);
            log("\n");
        }
    }

    fn find_session(&self, session_id: u64) -> Option<usize> {
        let table = self.session_table.lock();
        for i in 0..MAX_SESSIONS {
            if table[i].session_id.load(Ordering::SeqCst) == session_id {
                return Some(i);
            }
        }
        None
    }

    pub fn get_session(&self, session_id: u64) -> Option<usize> {
        self.find_session(session_id)
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

pub static SESSION_MANAGER: SessionManager = SessionManager::new();

pub fn init() {
    SESSION_MANAGER.init();
}
