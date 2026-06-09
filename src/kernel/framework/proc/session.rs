//! 会话 / 进程组 / 控制终端
//!
//! ## POSIX 语义
//!
//! - **会话 (session)**: 一组进程组的集合, 由 setsid() 创建, SID = 创建者 PID
//! - **进程组 (process group)**: 一组进程的集合, 用于信号广播 (kill 0 / kill -pgid)
//! - **控制终端 (controlling terminal)**: 每个会话最多一个, 前台进程组接收终端信号
//!
//! ## 实现策略
//!
//! - session_id 和 pgid 存储在 Process 结构体中 (AtomicU64 / AtomicU32)
//! - 控制终端信息存储在 Session 结构体中 (terminal 字段)
//! - fork 时子进程继承父进程的 session_id 和 pgid
//! - setsid: 创建新会话 + 新进程组, SID = PID, PGID = PID
//! - setpgid: 仅允许在同一会话内移动进程到已有进程组或创建新进程组
//!
//! ## 安全
//!
//! - 本模块属于 framework (TCB), 允许 unsafe
//! - 所有 Process 访问通过 PROCESS_TABLE.with_process 保护

use core::sync::atomic::Ordering;
use crate::kernel::framework::config::MAX_SESSIONS;
use crate::kernel::framework::proc::api::process_get_current_pid;
use crate::kernel::framework::proc::process::PROCESS_TABLE;
use crate::kernel::framework::sync::irq_spinlock::IrqSpinLock as Mutex;

extern "C" {
    fn klog_ffi_info(msg: *const u8);
}

fn log(s: &str) {
    // SAFETY: klog_ffi_info 接受有效 *const u8 指针
    unsafe {
        klog_ffi_info(s.as_ptr());
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

// ============================================================================
// Session 数据结构
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    Active = 0,
    Zombie = 1,
}

/// 会话结构体
///
/// 每个会话包含:
/// - session_id: 唯一标识 (= 创建者 PID)
/// - terminal: 控制终端设备号 (0 = 无控制终端)
/// - foreground_pgid: 前台进程组 ID
#[repr(C)]
pub struct Session {
    pub session_id: core::sync::atomic::AtomicU64,
    pub pwm: core::sync::atomic::AtomicU64,
    pub parent_sid: core::sync::atomic::AtomicU64,
    /// 控制终端设备号 (0 = 无)
    pub terminal: core::sync::atomic::AtomicU64,
    /// 前台进程组 ID
    pub foreground_pgid: core::sync::atomic::AtomicU32,
    pub create_time: core::sync::atomic::AtomicU64,
    pub state: core::sync::atomic::AtomicU32,
    pub process_list: core::sync::atomic::AtomicU64,
    pub process_count: core::sync::atomic::AtomicU32,
    pub next: core::sync::atomic::AtomicU64,
}

impl Session {
    pub const fn new() -> Self {
        Self {
            session_id: core::sync::atomic::AtomicU64::new(0),
            pwm: core::sync::atomic::AtomicU64::new(0),
            parent_sid: core::sync::atomic::AtomicU64::new(0),
            terminal: core::sync::atomic::AtomicU64::new(0),
            foreground_pgid: core::sync::atomic::AtomicU32::new(0),
            create_time: core::sync::atomic::AtomicU64::new(0),
            state: core::sync::atomic::AtomicU32::new(SessionState::Zombie as u32),
            process_list: core::sync::atomic::AtomicU64::new(0),
            process_count: core::sync::atomic::AtomicU32::new(0),
            next: core::sync::atomic::AtomicU64::new(0),
        }
    }
}

// ============================================================================
// SessionManager
// ============================================================================

pub struct SessionManager {
    session_table: Mutex<[Session; MAX_SESSIONS]>,
    next_session_id: core::sync::atomic::AtomicU64,
}

impl SessionManager {
    pub const fn new() -> Self {
        Self {
            session_table: Mutex::new([const { Session::new() }; MAX_SESSIONS]),
            next_session_id: core::sync::atomic::AtomicU64::new(1),
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

    /// 创建新会话, 返回 session_id
    pub fn create(&self, pwm: u64) -> Option<u64> {
        let idx = self.alloc_session()?;
        let sid = self.next_session_id.fetch_add(1, Ordering::SeqCst);

        let mut table = self.session_table.lock();
        let session = &mut table[idx];

        session.session_id.store(sid, Ordering::SeqCst);
        session.pwm.store(pwm, Ordering::SeqCst);
        session.parent_sid.store(0, Ordering::SeqCst);
        session.terminal.store(0, Ordering::SeqCst);
        session.foreground_pgid.store(0, Ordering::SeqCst);
        session.create_time.store(0, Ordering::SeqCst);
        session.state.store(SessionState::Active as u32, Ordering::SeqCst);
        session.process_list.store(0, Ordering::SeqCst);
        session.process_count.store(0, Ordering::SeqCst);
        session.next.store(0, Ordering::SeqCst);

        log("Session created: SID=");
        log_num(sid);
        log("\n");

        Some(sid)
    }

    /// 创建会话, SID = leader_pid (setsid 语义)
    pub fn create_with_sid(&self, leader_pid: u32, pwm: u64) -> Option<u64> {
        let idx = self.alloc_session()?;
        let sid = leader_pid as u64;

        let mut table = self.session_table.lock();
        let session = &mut table[idx];

        session.session_id.store(sid, Ordering::SeqCst);
        session.pwm.store(pwm, Ordering::SeqCst);
        session.parent_sid.store(0, Ordering::SeqCst);
        session.terminal.store(0, Ordering::SeqCst);
        session.foreground_pgid.store(leader_pid, Ordering::SeqCst);
        session.create_time.store(0, Ordering::SeqCst);
        session.state.store(SessionState::Active as u32, Ordering::SeqCst);
        session.process_list.store(0, Ordering::SeqCst);
        session.process_count.store(0, Ordering::SeqCst);
        session.next.store(0, Ordering::SeqCst);

        Some(sid)
    }

    pub fn destroy(&self, session_id: u64) {
        if let Some(idx) = self.find_session(session_id) {
            let mut table = self.session_table.lock();
            let session = &mut table[idx];

            session.session_id.store(0, Ordering::SeqCst);
            session.state.store(SessionState::Zombie as u32, Ordering::SeqCst);
            session.process_list.store(0, Ordering::SeqCst);
            session.process_count.store(0, Ordering::SeqCst);
            session.terminal.store(0, Ordering::SeqCst);
            session.foreground_pgid.store(0, Ordering::SeqCst);

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

    /// 设置会话的控制终端
    pub fn set_controlling_terminal(&self, session_id: u64, dev: u64) -> bool {
        if let Some(idx) = self.find_session(session_id) {
            let table = self.session_table.lock();
            let session = &table[idx];
            // 仅当会话尚无控制终端时才允许设置
            if session.terminal.load(Ordering::SeqCst) == 0 {
                session.terminal.store(dev, Ordering::SeqCst);
                return true;
            }
        }
        false
    }

    /// 获取会话的控制终端
    pub fn get_controlling_terminal(&self, session_id: u64) -> u64 {
        self.find_session(session_id)
            .map(|idx| {
                let table = self.session_table.lock();
                table[idx].terminal.load(Ordering::SeqCst)
            })
            .unwrap_or(0)
    }

    /// 释放会话的控制终端 (会话 leader 退出时)
    pub fn release_controlling_terminal(&self, session_id: u64) {
        if let Some(idx) = self.find_session(session_id) {
            let table = self.session_table.lock();
            table[idx].terminal.store(0, Ordering::SeqCst);
            table[idx].foreground_pgid.store(0, Ordering::SeqCst);
        }
    }

    /// 设置前台进程组
    pub fn set_foreground_pgid(&self, session_id: u64, pgid: u32) -> bool {
        if let Some(idx) = self.find_session(session_id) {
            let table = self.session_table.lock();
            table[idx].foreground_pgid.store(pgid, Ordering::SeqCst);
            return true;
        }
        false
    }

    /// 获取前台进程组
    pub fn get_foreground_pgid(&self, session_id: u64) -> u32 {
        self.find_session(session_id)
            .map(|idx| {
                let table = self.session_table.lock();
                table[idx].foreground_pgid.load(Ordering::SeqCst)
            })
            .unwrap_or(0)
    }
}

pub static SESSION_MANAGER: SessionManager = SessionManager::new();

pub fn init() {
    SESSION_MANAGER.init();
}

// ============================================================================
// 系统调用 TCB 入口
// ============================================================================

/// setsid — 创建新会话
///
/// POSIX 语义:
/// 1. 调用进程不能是进程组长 (pgid == pid → EPERM)
/// 2. 创建新会话, SID = PID
/// 3. 创建新进程组, PGID = PID
/// 4. 断开与原控制终端的关联
/// 5. 返回新 SID
pub fn proc_setsid() -> i64 {
    let pid = process_get_current_pid();
    if pid == 0 {
        return -1;
    }

    // 检查: 调用进程不能是进程组长
    let is_group_leader = PROCESS_TABLE
        .with_process(pid, |p| {
            let pgid = p.pgid.load(Ordering::SeqCst);
            // pgid 为 0 表示未初始化, 默认 pgid = pid
            let effective_pgid = if pgid == 0 { p.pid.0 } else { pgid };
            effective_pgid == pid
        })
        .unwrap_or(false);

    if is_group_leader {
        return -1; // EPERM
    }

    // 创建新会话, SID = PID
    if let Some(sid) = SESSION_MANAGER.create_with_sid(pid, 0) {
        // 更新进程的 session_id 和 pgid
        PROCESS_TABLE.with_process(pid, |p| {
            p.session_id.store(sid, Ordering::SeqCst);
            p.pgid.store(pid, Ordering::SeqCst);
        });
        sid as i64
    } else {
        -1 // EPERM (资源不足)
    }
}

/// getsid(pid) — 取会话 ID
///
/// - pid == 0: 当前进程的会话 ID
/// - pid > 0: 目标进程的会话 ID (需在同一会话)
pub fn proc_getsid(pid: i32) -> i64 {
    if pid < 0 {
        return -22; // -EINVAL
    }

    let target_pid = if pid == 0 {
        process_get_current_pid()
    } else {
        pid as u32
    };

    if target_pid == 0 {
        return -3; // -ESRCH
    }

    PROCESS_TABLE
        .with_process(target_pid, |p| {
            let sid = p.session_id.load(Ordering::SeqCst);
            // session_id 为 0 表示未初始化, 默认 sid = pid
            if sid == 0 {
                p.pid.0 as i64
            } else {
                sid as i64
            }
        })
        .unwrap_or(-3) // -ESRCH
}

/// setpgid(pid, pgid) — 设置进程组
///
/// POSIX 语义:
/// 1. pid == 0: 操作当前进程; pgid == 0: pgid = pid
/// 2. 目标进程必须是调用者自身或其子进程
/// 3. 目标进程和调用者必须在同一会话
/// 4. 若 pgid != 0 且 pgid != pid, 则该进程组必须已存在 (同会话内)
/// 5. 已执行 exec 的子进程不能被 setpgid
pub fn proc_setpgid(pid: i32, pgid: i32) -> i64 {
    if pid < 0 || pgid < 0 {
        return -22; // -EINVAL
    }

    let current_pid = process_get_current_pid();
    let target_pid = if pid == 0 { current_pid } else { pid as u32 };
    let new_pgid = if pgid == 0 { target_pid } else { pgid as u32 };

    if target_pid == 0 {
        return -3; // -ESRCH
    }

    // 获取当前进程的 session_id
    let current_sid = PROCESS_TABLE
        .with_process(current_pid, |p| p.session_id.load(Ordering::SeqCst))
        .unwrap_or(0);

    // 获取目标进程信息并验证
    let result = PROCESS_TABLE.with_process(target_pid, |p| {
        let target_sid = p.session_id.load(Ordering::SeqCst);

        // 必须在同一会话
        if current_sid != target_sid {
            return -1; // -EPERM
        }

        // 目标进程必须是调用者自身或其子进程
        // 简化: 允许自身和任何同会话进程 (严格 POSIX 需检查 parent)
        let is_self = p.pid.0 == current_pid;
        let is_child = p.parent.map(|ppid| ppid.0 == current_pid).unwrap_or(false);
        if !is_self && !is_child {
            return -1; // -EPERM
        }

        // 若 pgid 指定了一个已存在的进程组, 验证该组在同会话内
        if new_pgid != target_pid && new_pgid != 0 {
            let group_in_session = PROCESS_TABLE.with_process(new_pgid, |leader| {
                let leader_sid = leader.session_id.load(Ordering::SeqCst);
                leader_sid == current_sid
            }).unwrap_or(false);

            if !group_in_session {
                // 检查是否有任何进程在该 pgid 且同会话
                let mut found = false;
                PROCESS_TABLE.for_each(|proc| {
                    let pg = proc.pgid.load(Ordering::SeqCst);
                    let sid = proc.session_id.load(Ordering::SeqCst);
                    if pg == new_pgid && sid == current_sid {
                        found = true;
                    }
                    true
                });
                if !found {
                    return -22; // -EINVAL (进程组不存在)
                }
            }
        }

        // 设置 pgid
        p.pgid.store(new_pgid, Ordering::SeqCst);
        0
    });

    result.unwrap_or(-3) // -ESRCH
}

/// getpgid(pid) — 取进程组 ID
///
/// pid == 0: 当前进程
pub fn proc_getpgid(pid: i32) -> i64 {
    if pid < 0 {
        return -22; // -EINVAL
    }
    let target_pid = if pid == 0 {
        process_get_current_pid()
    } else {
        pid as u32
    };
    if target_pid == 0 {
        return -3; // -ESRCH
    }
    PROCESS_TABLE
        .with_process(target_pid, |proc| {
            let pgid = proc.pgid.load(Ordering::SeqCst);
            if pgid == 0 {
                proc.pid.0 as i64
            } else {
                pgid as i64
            }
        })
        .unwrap_or(-3)
}

/// 初始化新创建进程的 pgid (默认自成一组: pgid = pid)
///
/// 由 scheduler.create_process 内部调用.
pub fn proc_init_pgid(pid: u32) {
    PROCESS_TABLE.with_process(pid, |proc| {
        let cur = proc.pgid.load(Ordering::SeqCst);
        if cur == 0 {
            proc.pgid.store(pid, Ordering::SeqCst);
        }
    });
}

// ============================================================================
// 控制终端辅助
// ============================================================================

/// TIOCSCTTY — 设置控制终端
///
/// 仅会话 leader 可调用, 且会话尚无控制终端
pub fn sys_tiocsctty(fd: i32) -> i64 {
    let pid = process_get_current_pid();
    if pid == 0 {
        return -1;
    }

    // 获取当前进程的 session_id
    let sid = PROCESS_TABLE
        .with_process(pid, |p| p.session_id.load(Ordering::SeqCst))
        .unwrap_or(0);

    if sid == 0 {
        return -1; // EPERM: 无会话
    }

    // 检查调用者是否是会话 leader (SID == PID)
    if sid != pid as u64 {
        return -1; // EPERM: 非会话 leader
    }

    // 简化: fd 对应的设备号用 fd 值本身作为标识
    // 完整实现需要从 fd 获取 tty 设备号
    let dev = fd as u64;

    if SESSION_MANAGER.set_controlling_terminal(sid, dev) {
        0
    } else {
        -1 // EPERM: 已有控制终端
    }
}

/// 获取当前进程的控制终端设备号
pub fn get_controlling_terminal() -> u64 {
    let pid = process_get_current_pid();
    if pid == 0 {
        return 0;
    }
    let sid = PROCESS_TABLE
        .with_process(pid, |p| p.session_id.load(Ordering::SeqCst))
        .unwrap_or(0);
    if sid == 0 {
        return 0;
    }
    SESSION_MANAGER.get_controlling_terminal(sid)
}

/// 获取当前会话的前台进程组
pub fn get_foreground_pgid() -> u32 {
    let pid = process_get_current_pid();
    if pid == 0 {
        return 0;
    }
    let sid = PROCESS_TABLE
        .with_process(pid, |p| p.session_id.load(Ordering::SeqCst))
        .unwrap_or(0);
    if sid == 0 {
        return 0;
    }
    SESSION_MANAGER.get_foreground_pgid(sid)
}

/// tcsetpgrp — 设置前台进程组
///
/// 仅控制终端所在会话的进程可调用
pub fn sys_tcsetpgrp(fd: i32, pgid: i32) -> i64 {
    if pgid <= 0 {
        return -22; // -EINVAL
    }

    let pid = process_get_current_pid();
    if pid == 0 {
        return -1;
    }

    let sid = PROCESS_TABLE
        .with_process(pid, |p| p.session_id.load(Ordering::SeqCst))
        .unwrap_or(0);

    if sid == 0 {
        return -1; // EPERM
    }

    // 验证 pgid 对应的进程组在同会话内
    let mut found = false;
    PROCESS_TABLE.for_each(|proc| {
        let pg = proc.pgid.load(Ordering::SeqCst);
        let proc_sid = proc.session_id.load(Ordering::SeqCst);
        if pg == pgid as u32 && proc_sid == sid {
            found = true;
        }
        true
    });

    if !found {
        return -22; // -EINVAL
    }

    if SESSION_MANAGER.set_foreground_pgid(sid, pgid as u32) {
        0
    } else {
        -1
    }
}

/// tcgetpgrp — 获取前台进程组
pub fn sys_tcgetpgrp(_fd: i32) -> i64 {
    get_foreground_pgid() as i64
}

/// 向前台进程组发送信号 (终端驱动使用)
///
/// 当终端收到 SIGINT/SIGQUIT/SIGTSTP 时, 应调用此函数
/// 将信号发送给会话的前台进程组
pub fn signal_foreground_pgid(sig: u8) {
    let pgid = get_foreground_pgid();
    if pgid == 0 {
        return;
    }
    // 向整个进程组发送信号 (kill -pgid)
    crate::kernel::framework::proc::signal::do_signal_send_extended(-(pgid as i32), sig).ok();
}

/// 会话 leader 退出时释放控制终端
pub fn session_leader_exit(pid: u32) {
    let sid = PROCESS_TABLE
        .with_process(pid, |p| p.session_id.load(Ordering::SeqCst))
        .unwrap_or(0);

    if sid == 0 || sid != pid as u64 {
        return; // 不是会话 leader
    }

    // 向该会话的前台进程组发送 SIGHUP + SIGCONT
    let fg_pgid = SESSION_MANAGER.get_foreground_pgid(sid);
    if fg_pgid != 0 {
        crate::kernel::framework::proc::signal::do_signal_send_extended(-(fg_pgid as i32), 1).ok(); // SIGHUP
        crate::kernel::framework::proc::signal::do_signal_send_extended(-(fg_pgid as i32), 18).ok(); // SIGCONT
    }

    // 释放控制终端
    SESSION_MANAGER.release_controlling_terminal(sid);
}
