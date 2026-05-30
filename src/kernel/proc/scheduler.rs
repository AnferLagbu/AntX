use alloc::collections::VecDeque;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::types::*;
use super::process::{Process, PROCESS_TABLE};
use super::cfs::{CfsRunQueue, DlRunQueue, DeadlineParams, nice_to_weight, mlfq_level_to_nice,
    calc_vruntime_delta, cfs_should_preempt, NICE0_WEIGHT, DL_MAX_UTILIZATION_PCT,
    TARGET_LATENCY_TICKS, CFS_BOOST_INTERVAL_TICKS, LOAD_BALANCE_THRESHOLD};

macro_rules! klog_sched_warn {
    ($($arg:tt)*) => {
        $crate::klog_ffi!(klog_ffi_warn, $($arg)*)
    };
}

const MLFQ_LEVELS: usize = 4;
const TIME_SLICES: [u64; MLFQ_LEVELS] = [10, 20, 40, 80];

const RT_PRIORITY_MAX: u8 = 99;
const RT_TIME_SLICE: u64 = 5;
const RT_FIFO_WATCHDOG: u64 = 500;

pub struct PwidQuota {
    pub pwm: u64,
    pub used: bool,
    pub max_runtime: u64,
    pub period: u64,
    pub consumed: u64,
    pub next_reset: u64,
}

impl PwidQuota {
    const fn new() -> Self {
        Self { pwm: 0, used: false, max_runtime: 0, period: 0, consumed: 0, next_reset: 0 }
    }
}

const MAX_QUOTAS: usize = 32;

pub struct PwidLimit {
    pub pwm: u64,
    pub used: bool,
    pub max_procs: u32,
    pub current: u32,
}

const MAX_LIMITS: usize = 32;

pub static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal = 0,
    Fifo = 1,
    Rr = 2,
    Idle = 3,
    Deadline = 4,
}

impl SchedPolicy {
    pub fn from_u32(value: u32) -> Self {
        match value {
            0 => SchedPolicy::Normal,
            1 => SchedPolicy::Fifo,
            2 => SchedPolicy::Rr,
            3 => SchedPolicy::Idle,
            4 => SchedPolicy::Deadline,
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

const MAX_CPUS: usize = 256;

struct PerCpuSched {
    queues: [Mutex<VecDeque<Pid>>; MLFQ_LEVELS],
    rt_queue: Mutex<VecDeque<RtTaskInfo>>,
    cfs_rq: Mutex<CfsRunQueue>,
    dl_rq: Mutex<DlRunQueue>,
    current: AtomicU32,
    need_reschedule: AtomicBool,
    current_level: AtomicU32,
    time_remaining: AtomicU64,
    rt_running: AtomicBool,
    dl_running: AtomicBool,
    fifo_watchdog: AtomicU64,
}

// SAFETY: PerCpuSched is per-CPU data; each instance is only accessed by
// its owning CPU. Mutex<VecDeque> fields provide internal synchronization.
// Atomic fields are lock-free. The combination is safe to send/share
// across threads because mutation is always guarded.
unsafe impl Send for PerCpuSched {}
unsafe impl Sync for PerCpuSched {}

static PER_CPU_SCHED: [Mutex<Option<PerCpuSched>>; MAX_CPUS] = [
    const { Mutex::new(None) }; MAX_CPUS
];

pub fn init_per_cpu_sched(cpu_id: u32) {
    let idx = (cpu_id as usize) % MAX_CPUS;
    let mut guard = PER_CPU_SCHED[idx].lock();
    if guard.is_some() {
        return;
    }
    guard.replace(PerCpuSched {
        queues: [
            Mutex::new(VecDeque::new()),
            Mutex::new(VecDeque::new()),
            Mutex::new(VecDeque::new()),
            Mutex::new(VecDeque::new()),
        ],
        rt_queue: Mutex::new(VecDeque::new()),
        cfs_rq: Mutex::new(CfsRunQueue::new()),
        dl_rq: Mutex::new(DlRunQueue::new()),
        current: AtomicU32::new(0),
        need_reschedule: AtomicBool::new(false),
        current_level: AtomicU32::new(0),
        time_remaining: AtomicU64::new(TIME_SLICES[0]),
        rt_running: AtomicBool::new(false),
        dl_running: AtomicBool::new(false),
        fifo_watchdog: AtomicU64::new(0),
    });
}

#[inline]
fn per_cpu() -> &'static PerCpuSched {
    let cpu = crate::kernel::smp::get_current_cpu();
    let idx = (cpu as usize) % MAX_CPUS;
    {
        let guard = PER_CPU_SCHED[idx].lock();
        if guard.is_none() {
            drop(guard);
            init_per_cpu_sched(cpu);
        }
    }
    // SAFETY: PerCpuSched lives at a stable address within the static
    // PER_CPU_SCHED array. Once initialized (Some), it is never set
    // back to None, so the pointer remains valid for 'static.
    let guard = PER_CPU_SCHED[idx].lock();
    unsafe {
        let ptr = guard.as_ref().unwrap() as *const PerCpuSched;
        &*ptr
    }
}

#[inline]
fn per_cpu_for(cpu_id: u32) -> &'static PerCpuSched {
    let idx = (cpu_id as usize) % MAX_CPUS;
    {
        let guard = PER_CPU_SCHED[idx].lock();
        if guard.is_none() {
            drop(guard);
            init_per_cpu_sched(cpu_id);
        }
    }
    let guard = PER_CPU_SCHED[idx].lock();
    unsafe {
        let ptr = guard.as_ref().unwrap() as *const PerCpuSched;
        &*ptr
    }
}

pub struct Scheduler {
    quotas: Mutex<[PwidQuota; MAX_QUOTAS]>,
    limits: Mutex<[PwidLimit; MAX_LIMITS]>,
    initialized: AtomicBool,
}

// SAFETY: Scheduler uses Mutex for all mutable state (quotas, limits).
// AtomicBool for initialized flag is lock-free. All accesses are
// serialized through the Mutex, making cross-thread access safe.
unsafe impl Send for Scheduler {}
unsafe impl Sync for Scheduler {}

impl Scheduler {
    pub const fn new() -> Self {
        const QUOTA_ZERO: PwidQuota = PwidQuota::new();
        const LIMIT_ZERO: PwidLimit = PwidLimit { pwm: 0, used: false, max_procs: 0, current: 0 };
        Self {
            quotas: Mutex::new([QUOTA_ZERO; MAX_QUOTAS]),
            limits: Mutex::new([LIMIT_ZERO; MAX_LIMITS]),
            initialized: AtomicBool::new(false),
        }
    }
    
    pub fn init(&self) {
        init_per_cpu_sched(0);

        self.initialized.store(true, Ordering::SeqCst);

        let init_pid = self.create_process("init", None, 0);
        if let Some(pid) = init_pid {
            PROCESS_TABLE.with_process(pid, |proc| {
                let _ = proc.set_state_safe(ProcessState::Running);
                proc.set_priority(ProcessPriority::Normal);
            });
            self.set_current(pid);

            if let Some(process_ptr) = PROCESS_TABLE.get(pid) {
                unsafe {
                    extern "C" {
                        fn update_current_process_ptr(ptr: u64);
                    }
                    update_current_process_ptr(process_ptr as u64);
                }
            }
        }

        if per_cpu().need_reschedule.swap(false, Ordering::SeqCst) {
            self.schedule();
        }
    }
    
    pub fn create_process(&self, name: &str, parent: Option<Pid>, pwm: u64) -> Option<Pid> {
        let pid = PROCESS_TABLE.allocate_pid()?;

        // L4: per-PWM proc count limit
        if pwm != 0 {
            let mut limits = self.limits.lock();
            for l in limits.iter_mut() {
                if l.used && l.pwm == pwm {
                    if l.max_procs > 0 && l.current >= l.max_procs {
                        return None;
                    }
                    l.current += 1;
                    break;
                }
            }
        }
        
        let parent_id = parent.map(ProcessId);
        let process = alloc::boxed::Box::new(Process::new(pid, name, parent_id));
        process.set_pwm(pwm);
        
        let process_ptr = alloc::boxed::Box::into_raw(process);
        
        if !PROCESS_TABLE.insert(process_ptr) {
            unsafe { alloc::alloc::dealloc(process_ptr as *mut u8, alloc::alloc::Layout::new::<Process>()) };
            return None;
        }
        
        Some(pid)
    }
    
    pub fn add(&self, pid: Pid) {
        self.cfs_enqueue(pid);
    }

    pub fn add_with_priority(&self, pid: Pid, level: usize) {
        let nice = mlfq_level_to_nice(level);
        self.set_nice(pid, nice);
        self.cfs_enqueue(pid);
    }

    /// Set nice value and update the process's CFS weight.
    pub fn set_nice(&self, pid: Pid, nice: i8) {
        PROCESS_TABLE.with_process(pid, |proc| {
            let clamped = nice.clamp(-20, 19);
            let w = nice_to_weight(clamped);
            proc.nice.store(clamped as u32, Ordering::Release);
            proc.cfs_weight.store(w, Ordering::Release);
        });
    }

    /// Enqueue a process into the CFS run queue.
    fn cfs_enqueue(&self, pid: Pid) {
        let per_cpu = per_cpu();
        let vr = PROCESS_TABLE.with_process(pid, |p| {
            let _ = p.set_state_safe(ProcessState::Ready);
            let v = p.cfs_vruntime.load(Ordering::Acquire);
            let w = p.cfs_weight.load(Ordering::Acquire);
            p.cfs_on_rq.store(true, Ordering::Release);
            (v, w)
        });

        if let Some((vruntime, weight)) = vr {
            per_cpu.cfs_rq.lock().enqueue(pid, vruntime, weight);
        }
    }

    /// Set SCHED_DEADLINE parameters for a process.
    pub fn set_deadline_params(&self, pid: Pid, params: DeadlineParams) -> bool {
        if !params.is_valid() {
            return false;
        }
        let util = params.utilization_pct();
        if util > DL_MAX_UTILIZATION_PCT {
            return false;
        }
        PROCESS_TABLE.with_process(pid, |proc| {
            proc.set_sched_policy(SchedPolicy::Deadline);
            proc.dl_runtime.store(params.runtime, Ordering::Release);
            proc.dl_deadline.store(params.deadline, Ordering::Release);
            proc.dl_period.store(params.period, Ordering::Release);
            let now = TICK_COUNT.load(Ordering::Acquire);
            proc.dl_abs.store(now + params.deadline, Ordering::Release);
            proc.dl_remaining.store(params.runtime, Ordering::Release);
        });
        true
    }

    pub fn add_rt_task(&self, pid: Pid, rt_priority: u8, policy: SchedPolicy) {
        let priority = rt_priority.min(RT_PRIORITY_MAX);
        
        let mut rt_queue = per_cpu().rt_queue.lock();
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

    /// Pick a deadline task (EDF — earliest absolute deadline first).
    fn pick_deadline_task(&self) -> Option<Pid> {
        let per_cpu = per_cpu();
        let mut dl_rq = per_cpu.dl_rq.lock();
        if dl_rq.is_empty() {
            per_cpu.dl_running.store(false, Ordering::SeqCst);
            return None;
        }
        match dl_rq.pick_next() {
            Some((pid, dl_abs)) => {
                let alive = PROCESS_TABLE.with_process(pid, |p| {
                    p.get_state() != ProcessState::Zombie
                        && p.get_sched_policy() == SchedPolicy::Deadline
                }).unwrap_or(false);
                if alive {
                    per_cpu.dl_running.store(true, Ordering::SeqCst);
                    Some(pid)
                } else {
                    // pick_next() removed the task from the tree but
                    // preserved nr_running (same as CfsRunQueue).
                    // reinsert() puts it back without touching counters.
                    dl_rq.reinsert(pid, dl_abs);
                    per_cpu.dl_running.store(false, Ordering::SeqCst);
                    None
                }
            }
            None => {
                per_cpu.dl_running.store(false, Ordering::SeqCst);
                None
            }
        }
    }

    /// Pick a CFS task (minimum vruntime).
    fn pick_cfs_task(&self) -> Option<Pid> {
        let per_cpu = per_cpu();
        let mut cfs_rq = per_cpu.cfs_rq.lock();
        if cfs_rq.is_empty() {
            return None;
        }
        match cfs_rq.pick_next() {
            Some((pid, vr)) => {
                let schedulable = PROCESS_TABLE.with_process(pid, |p| {
                    let state = p.get_state();
                    let policy = p.get_sched_policy();
                    state != ProcessState::Blocked
                        && state != ProcessState::Zombie
                        && policy == SchedPolicy::Normal
                }).unwrap_or(false);
                if schedulable {
                    PROCESS_TABLE.with_process(pid, |p| {
                        p.cfs_on_rq.store(false, Ordering::Release);
                    });
                    Some(pid)
                } else {
                    // Task was removed from tree by pick_next() but is not
                    // schedulable (blocked/zombie/wrong policy). Re-insert
                    // it to avoid silently losing the task.
                    cfs_rq.update_curr(pid, vr);
                    None
                }
            }
            None => None,
        }
    }
    
    pub fn schedule(&self) -> Option<Pid> {
        let saved_flags = crate::arch!(interrupt_disable()) as u64;
        
        let per_cpu = per_cpu();
        let current_pid = per_cpu.current.load(Ordering::SeqCst);

        let mut next_pid = self.pick_deadline_task();

        // 2. RT (FIFO/RR) — preserved from MLFQ
        if next_pid.is_none() {
            per_cpu.dl_running.store(false, Ordering::SeqCst);

            let mut rt_queue = per_cpu.rt_queue.lock();
            
            while !rt_queue.is_empty() {
                let rt_task = match rt_queue.pop_front() {
                    Some(task) => task,
                    None => {
                        klog_sched_warn!("[SCHEDULER] RT queue race condition detected");
                        break;
                    }
                };
                let rt_pid = rt_task.pid;
                
                let alive = PROCESS_TABLE.with_process(rt_pid, |p| {
                    p.get_state() != ProcessState::Zombie
                }).unwrap_or(false);
                
                if !alive {
                    continue;
                }
                
                match rt_task.policy {
                    SchedPolicy::Fifo => {
                        next_pid = Some(rt_pid);
                        per_cpu.rt_running.store(true, Ordering::SeqCst);
                        per_cpu.fifo_watchdog.store(RT_FIFO_WATCHDOG, Ordering::SeqCst);
                        break;
                    }
                    SchedPolicy::Rr => {
                        next_pid = Some(rt_pid);
                        per_cpu.rt_running.store(true, Ordering::SeqCst);
                        let mut updated_rt = rt_task;
                        updated_rt.time_slice_remaining = RT_TIME_SLICE;
                        rt_queue.push_back(updated_rt);
                        break;
                    }
                    _ => {
                        self.cfs_enqueue(rt_pid);
                        per_cpu.rt_running.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            }
            
            if next_pid.is_none() {
                per_cpu.rt_running.store(false, Ordering::SeqCst);
            }
        }

        // 3. CFS (vruntime minimum) — replaces MLFQ for SCHED_NORMAL
        if next_pid.is_none() {
            next_pid = self.pick_cfs_task();
        }

        // 4. Load balance if nothing found locally
        if next_pid.is_none() && crate::kernel::smp::is_enabled() {
            self.load_balance();
            next_pid = self.pick_cfs_task();
        }

        let next = match next_pid {
            Some(pid) => pid,
            None => {
                if saved_flags & 0x200 != 0 {
                    crate::arch!(interrupt_enable());
                }
                return None;
            }
        };

        if next == current_pid {
            if saved_flags & 0x200 != 0 {
                crate::arch!(interrupt_enable());
            }
            return Some(next);
        }

        let prev_ptr = if current_pid != 0 {
            PROCESS_TABLE.get(current_pid)
        } else {
            None
        };

        let next_ptr = PROCESS_TABLE.get(next);

        if next_ptr.is_none() {
            if saved_flags & 0x200 != 0 {
                crate::arch!(interrupt_enable());
            }
            return None;
        }

        PROCESS_TABLE.with_process(next, |proc| {
            let _ = proc.set_state_safe(ProcessState::Running);
            let next_kernel_stack = proc.kernel_stack.load(Ordering::SeqCst);
            if next_kernel_stack != 0 {
                crate::kernel::cpu::arch::set_kernel_stack(next_kernel_stack);
            }
        });

        per_cpu.current.store(next, Ordering::SeqCst);

        super::scheduler_ex::SCHEDULER_EX.current.store(next as u64, Ordering::SeqCst);

        if let Some(next_ptr_raw) = next_ptr {
            unsafe { crate::kernel::proc::ffi::update_current_process_ptr(next_ptr_raw as u64); }
        }

        if let Some(user_proc) = super::user_proc::USER_PROC_MANAGER.get(next) {
            super::user_proc::USER_PROC_MANAGER.set_current(Some(user_proc));
        }

        // Re-enqueue the previous task
        if let Some(ref _prev_proc) = prev_ptr {
            let was_dl = per_cpu.dl_running.load(Ordering::SeqCst);
            let was_rt = per_cpu.rt_running.load(Ordering::SeqCst);

            if was_dl {
                let dl_info = PROCESS_TABLE.with_process(current_pid, |p| {
                    p.dl_abs.load(Ordering::Acquire)
                });
                if let Some(dl_abs) = dl_info {
                    // pick_next() preserved nr_running (same as CFS).
                    // reinsert() puts the task back into the tree
                    // without touching counters — the task was already
                    // counted at initial enqueue.
                    per_cpu.dl_rq.lock().reinsert(current_pid, dl_abs);
                }
            } else if was_rt {
                let rt_info = PROCESS_TABLE.with_process(current_pid, |p| {
                    (p.get_rt_priority(), p.get_sched_policy())
                });
                if let Some((rt_priority, policy)) = rt_info {
                    if policy != SchedPolicy::Fifo {
                        let mut rt_queue = per_cpu.rt_queue.lock();
                        let mut inserted = false;
                        for i in 0..rt_queue.len() {
                            if rt_queue[i].rt_priority < rt_priority {
                                rt_queue.insert(i, RtTaskInfo { pid: current_pid, rt_priority, policy, time_slice_remaining: RT_TIME_SLICE });
                                inserted = true;
                                break;
                            }
                        }
                        if !inserted {
                            rt_queue.push_back(RtTaskInfo { pid: current_pid, rt_priority, policy, time_slice_remaining: RT_TIME_SLICE });
                        }
                    }
                }
            } else {
                let (vr, _wt, _nice) = PROCESS_TABLE.with_process(current_pid, |p| {
                    (p.cfs_vruntime.load(Ordering::Acquire),
                     p.cfs_weight.load(Ordering::Acquire),
                     p.nice.load(Ordering::Acquire) as i8)
                }).unwrap_or((0, NICE0_WEIGHT, 0i8));
                per_cpu.cfs_rq.lock().update_curr(current_pid, vr);
                PROCESS_TABLE.with_process(current_pid, |p| {
                    p.cfs_on_rq.store(true, Ordering::Release);
                });
            }
            PROCESS_TABLE.with_process(current_pid, |p| {
                let _ = p.set_state_safe(ProcessState::Ready);
            });
        }

        let prev_ctx_ptr = prev_ptr.map_or(
            core::ptr::null_mut(),
            |p| {
                unsafe { &raw mut (*p).context as *mut Mutex<ProcessContext> }
            },
        );

        let next_ctx_ptr = next_ptr.map_or(
            core::ptr::null(),
            |p| {
                // SAFETY: next_ptr comes from PROCESS_TABLE.get() which returns a valid
                // pointer to a live Process. We only read the context field address.
                unsafe { &raw const (*p).context as *const Mutex<ProcessContext> }
            },
        );

        if !prev_ctx_ptr.is_null() {
            // SAFETY: Both pointers are valid and derived from live Process entries
            // in the PROCESS_TABLE. context_switch saves/restores register state.
            unsafe {
                let mut prev_ctx = (*prev_ctx_ptr).lock();
                let next_ctx = (*next_ctx_ptr).lock();
                crate::arch!(context_switch(
                    &mut *prev_ctx as *mut ProcessContext as *mut u8,
                    &*next_ctx as *const ProcessContext as *const u8
                ));
            }
        }

        Some(next)
    }

    pub fn current(&self) -> Option<Pid> {
        let pid = per_cpu().current.load(Ordering::SeqCst);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
    
    pub fn get_current_process(&self) -> Option<*mut Process> {
        let pid = per_cpu().current.load(Ordering::SeqCst);
        if pid == 0 {
            None
        } else {
            PROCESS_TABLE.get(pid)
        }
    }
    
    pub fn block(&self, reason: BlockReason) {
        let per_cpu = per_cpu();
        if let Some(pid) = self.current() {
            PROCESS_TABLE.with_process(pid, |proc| {
                let _ = proc.set_state_safe(ProcessState::Blocked);
                proc.block_reason.store(reason as u32, Ordering::SeqCst);
            });
            per_cpu.need_reschedule.store(true, Ordering::SeqCst);
        }
    }
    
    pub fn unblock(&self, pid: Pid) {
        let sched_policy = PROCESS_TABLE.with_process(pid, |proc| {
            let state = proc.get_state();
            if state != ProcessState::Blocked && state != ProcessState::Frozen {
                return None;
            }
            let _ = proc.set_state_safe(ProcessState::Ready);
            let policy = proc.get_sched_policy();
            if policy == SchedPolicy::Normal {
                proc.cfs_on_rq.store(true, Ordering::Release);
                let vr = proc.cfs_vruntime.load(Ordering::Acquire);
                let w = proc.cfs_weight.load(Ordering::Acquire);
                Some((policy, vr, w))
            } else if policy == SchedPolicy::Deadline {
                let dl_abs = proc.dl_abs.load(Ordering::Acquire);
                let runtime = proc.dl_runtime.load(Ordering::Acquire);
                let period = proc.dl_period.load(Ordering::Acquire);
                Some((policy, dl_abs, if period > 0 { (runtime * 100) / period } else { 0 }))
            } else {
                Some((policy, 0, 0))
            }
        }).flatten();

        match sched_policy {
            Some((SchedPolicy::Normal, vr, weight)) => {
                per_cpu().cfs_rq.lock().enqueue(pid, vr, weight);
            }
            Some((SchedPolicy::Fifo | SchedPolicy::Rr, _, _)) => {
                let (prio, pol) = PROCESS_TABLE.with_process(pid, |p| {
                    (p.get_rt_priority(), p.get_sched_policy())
                }).unwrap_or((0, SchedPolicy::Normal));

                let mut rt_q = per_cpu().rt_queue.lock();
                let mut inserted = false;
                for i in 0..rt_q.len() {
                    if rt_q[i].rt_priority < prio {
                        rt_q.insert(i, RtTaskInfo { pid, rt_priority: prio, policy: pol, time_slice_remaining: RT_TIME_SLICE });
                        inserted = true;
                        break;
                    }
                }
                if !inserted {
                    rt_q.push_back(RtTaskInfo { pid, rt_priority: prio, policy: pol, time_slice_remaining: RT_TIME_SLICE });
                }
            }
            Some((SchedPolicy::Deadline, dl_abs, util)) => {
                per_cpu().dl_rq.lock().enqueue(pid, dl_abs, util);
            }
            _ => {}
        }
    }
    
    pub fn exit(&self, exit_code: u32) {
        let per_cpu = per_cpu();
        if let Some(pid) = self.current() {
            let parent_pid_opt = PROCESS_TABLE.with_process(pid, |proc| {
                let pwm = proc.get_pwm();
                proc.exit_code.store(exit_code, Ordering::SeqCst);
                let _ = proc.set_state_safe(ProcessState::Zombie);
                self.dec_limit(pwm);
                proc.parent.map(|p| p.0)
            });

            if let Some(parent_pid) = parent_pid_opt.flatten() {
                self.unblock(parent_pid);
            }

            PROCESS_TABLE.with_process(pid, |proc| {
                let children: alloc::vec::Vec<Pid> =
                    proc.children.lock().iter().map(|c| c.0).collect();
                for child_pid in children {
                    PROCESS_TABLE.with_process_mut(child_pid, |child| {
                        let state = child.get_state();
                        if state == ProcessState::Zombie {
                            let _ = child.set_state_safe(ProcessState::Terminated);
                        } else {
                            child.parent = Some(ProcessId(1));
                        }
                    });
                    if PROCESS_TABLE.with_process(child_pid, |c| c.get_state() == ProcessState::Terminated).unwrap_or(false) {
                        PROCESS_TABLE.remove_and_free(child_pid);
                    }
                }
                proc.children.lock().clear();
            });
        }

        per_cpu.need_reschedule.store(true, Ordering::SeqCst);

        if self.schedule().is_none() {
            crate::arch!(outb(0xf4, (exit_code as u8).wrapping_shl(1) | 1));
            loop { crate::arch!(halt()); }
        }
    }
    
    pub fn yield_current(&self) {
        per_cpu().need_reschedule.store(true, Ordering::SeqCst);
        self.schedule();
    }

    pub fn set_need_reschedule(&self) {
        per_cpu().need_reschedule.store(true, Ordering::SeqCst);
    }
    
    pub fn should_reschedule(&self) -> bool {
        per_cpu().need_reschedule.swap(false, Ordering::SeqCst)
    }

    pub fn add_to_run_queue(&self, pid: Pid) {
        per_cpu().queues[0].lock().push_back(pid);
    }
    
    pub fn set_current(&self, pid: Pid) {
        per_cpu().current.store(pid, Ordering::SeqCst);

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
        let per_cpu = per_cpu();
        if !per_cpu.dl_rq.lock().is_empty() {
            return true;
        }
        if !per_cpu.rt_queue.lock().is_empty() {
            return true;
        }
        if !per_cpu.cfs_rq.lock().is_empty() {
            return true;
        }
        for l in 0..MLFQ_LEVELS {
            if !per_cpu.queues[l].lock().is_empty() {
                return true;
            }
        }
        false
    }

    /// Convenience: check if any runnable task exists (level 0 queues).
    pub fn has_any_runnable(&self) -> bool {
        self.has_runnable()
    }

    pub fn get_time_slice(&self) -> u64 {
        per_cpu().time_remaining.load(Ordering::SeqCst)
    }

    pub fn get_current_level(&self) -> u32 {
        per_cpu().current_level.load(Ordering::SeqCst)
    }

    pub fn tick(&self, cpu_id: usize) {
        let new_tick = TICK_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let per_cpu = per_cpu_for(cpu_id as u32);

        crate::kernel::barrier::RECOVERY_MANAGER.lock().tick(new_tick);
        crate::kernel::proc::oomd::OOMD.tick();
        crate::kernel::proc::scheduler_ex::SCHEDULER_EX.tick_accounting();

        // Periodic CFS boost — prevent vruntime starvation
        if new_tick % CFS_BOOST_INTERVAL_TICKS == 0 {
            let mut cfs_rq = per_cpu.cfs_rq.lock();
            cfs_rq.boost_all_vruntime();
        }

        // Periodic MLFQ boost — migrate long-wait tasks to level 0
        if new_tick % SCHED_BOOST_INTERVAL == 0 {
            self.boost_priority();
        }

        let current_pid = per_cpu.current.load(Ordering::SeqCst);
        if current_pid != 0 {
            // RT FIFO watchdog
            let is_rt = per_cpu.rt_running.load(Ordering::SeqCst);
            if is_rt && per_cpu.fifo_watchdog.load(Ordering::SeqCst) > 0 {
                let remaining = per_cpu.fifo_watchdog.fetch_sub(1, Ordering::SeqCst);
                if remaining <= 1 {
                    per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                    per_cpu.rt_running.store(false, Ordering::SeqCst);
                }
            }

            // Tick accounting: per-policy time tracking
            let is_dl = per_cpu.dl_running.load(Ordering::SeqCst);

            if is_dl {
                let (expired, should_replenish) = PROCESS_TABLE.with_process(current_pid, |p| {
                    let old_rem = p.dl_remaining.fetch_sub(1, Ordering::SeqCst);
                    let rem = old_rem - 1;
                    let expired = rem == 0;
                    let deadline = p.dl_deadline.load(Ordering::Acquire);
                    let dl_abs = p.dl_abs.load(Ordering::Acquire);
                    let now = TICK_COUNT.load(Ordering::Acquire);
                    let should_replenish = now >= dl_abs || (expired && now + deadline > dl_abs);
                    (expired, should_replenish)
                }).unwrap_or((true, false));

                if should_replenish {
                    PROCESS_TABLE.with_process(current_pid, |p| {
                        let runtime = p.dl_runtime.load(Ordering::Acquire);
                        let deadline = p.dl_deadline.load(Ordering::Acquire);
                        let now = TICK_COUNT.load(Ordering::Acquire);
                        p.dl_remaining.store(runtime, Ordering::Release);
                        p.dl_abs.store(now + deadline, Ordering::Release);
                    });
                }

                if expired || should_replenish {
                    per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                }
            } else if is_rt {
                let old_remaining = per_cpu.time_remaining.fetch_sub(1, Ordering::SeqCst);
                let new_remaining = old_remaining - 1;

                if new_remaining == 0 {
                    per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                    let policy = PROCESS_TABLE.with_process(current_pid, |proc| {
                        proc.get_sched_policy()
                    }).unwrap_or(SchedPolicy::Normal);

                    if policy == SchedPolicy::Fifo {
                        let old_watchdog = per_cpu.fifo_watchdog.fetch_sub(1, Ordering::SeqCst);
                        if old_watchdog - 1 == 0 {
                            per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                            crate::klog_crit!(Kernel, "[SCHEDULER] RT-FIFO watchdog triggered for pid={}", current_pid);
                        }
                    } else {
                        per_cpu.time_remaining.store(RT_TIME_SLICE, Ordering::SeqCst);
                    }
                }
            } else if current_pid != 0 {
                // CFS — update vruntime and check preemption.
                // IMPORTANT: Do NOT re-insert the running task into the tree here.
                // The running task stays out of the tree; it will be re-enqueued
                // only when it stops running (in schedule's re-enqueue path).
                let (should_preempt, should_yield) = {
                    let cfs_rq = per_cpu.cfs_rq.lock();
                    let vr = PROCESS_TABLE.with_process(current_pid, |p| {
                        let old_vr = p.cfs_vruntime.load(Ordering::Acquire);
                        let weight = p.cfs_weight.load(Ordering::Acquire);
                        let delta = calc_vruntime_delta(weight);
                        let new_vr = old_vr + delta;
                        p.cfs_vruntime.store(new_vr, Ordering::Release);
                        let sum = p.cfs_sum_exec_runtime.load(Ordering::Acquire);
                        p.cfs_sum_exec_runtime.store(sum + 1, Ordering::Release);
                        new_vr
                    }).unwrap_or(0);

                    let should_preempt = cfs_rq.nr_running > 0 && cfs_should_preempt(
                        vr,
                        cfs_rq.min_vruntime.load(Ordering::Acquire),
                        PROCESS_TABLE.with_process(current_pid, |p| {
                            p.cfs_weight.load(Ordering::Acquire)
                        }).unwrap_or(NICE0_WEIGHT),
                    );

                    let should_yield = vr > cfs_rq.min_vruntime.load(Ordering::Acquire) + TARGET_LATENCY_TICKS;

                    (should_preempt, should_yield)
                };

                if should_preempt || should_yield {
                    per_cpu.need_reschedule.store(true, Ordering::SeqCst);
                }
            }
        }

        // Sleep wakeup scan
        {
            let current_ticks = new_tick;
            let mut to_wake: [Pid; 8] = [0; 8];
            let mut wake_count = 0;
            for pid in 1..=255 {
                if wake_count >= 8 { break; }
                if pid == self.current().unwrap_or(0) { continue; }
                if let Some(proc) = PROCESS_TABLE.get(pid) {
                    unsafe {
                        let state = (*proc).get_state();
                        if state == ProcessState::Blocked {
                            let reason = (*proc).block_reason.load(Ordering::Relaxed);
                            if reason == BlockReason::Sleeping as u32 {
                                let until = (*proc).sleep_until.load(Ordering::SeqCst);
                                if until > 0 && current_ticks >= until {
                                    to_wake[wake_count] = pid;
                                    wake_count += 1;
                                }
                            }
                        }
                    }
                }
            }
            for i in 0..wake_count {
                self.unblock(to_wake[i]);
            }
        }

        // Zombie cleanup
        let socks_clean_interval: u64 = 1000;
        if new_tick % socks_clean_interval == 0 {
            let mut to_reap: [Pid; 16] = [0; 16];
            let mut reap_count = 0;
            for pid in 1..=255 {
                if reap_count >= 16 { break; }
                if let Some(proc) = PROCESS_TABLE.get(pid) {
                    unsafe {
                        if (*proc).get_state() == ProcessState::Zombie {
                            let parent_alive = (*proc).parent.map_or(true, |ppid| {
                                PROCESS_TABLE.get(ppid.0).map_or(false, |p| {
                                    let s = (*p).get_state();
                                    s != ProcessState::Zombie && s != ProcessState::Terminated
                                })
                            });
                            if !parent_alive || (*proc).parent == Some(ProcessId(1)) {
                                to_reap[reap_count] = pid;
                                reap_count += 1;
                            }
                        }
                    }
                }
            }
            for i in 0..reap_count {
                if let Some(proc) = PROCESS_TABLE.get(to_reap[i]) {
                    unsafe {
                        let _ = (*proc).set_state_safe(ProcessState::Terminated);
                    }
                }
                PROCESS_TABLE.remove_and_free(to_reap[i]);
            }
        }

        // Periodic load balance
        if new_tick % 64 == 0 {
            let local_load = self.total_runnable_for(crate::kernel::smp::get_current_cpu());
            if local_load < 2 {
                self.load_balance();
            }
        }

        if per_cpu.need_reschedule.swap(false, Ordering::SeqCst) {
            self.schedule();
        }
    }

    pub fn boost_priority(&self) {
        let per_cpu = per_cpu();
        for level in 1..MLFQ_LEVELS {
            let mut queue = per_cpu.queues[level].lock();
            while let Some(pid) = queue.pop_front() {
                per_cpu.queues[0].lock().push_back(pid);
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
        per_cpu().rt_queue.lock().len()
    }

    fn total_runnable_for(&self, cpu_id: u32) -> usize {
        let sched = per_cpu_for(cpu_id);
        let mut count = sched.cfs_rq.lock().nr_running as usize;
        count += sched.dl_rq.lock().nr_running as usize;
        count += sched.rt_queue.lock().len();
        for level in 0..MLFQ_LEVELS {
            count += sched.queues[level].lock().len();
        }
        // Count running task
        if sched.current.load(Ordering::SeqCst) != 0 {
            count += 1;
        }
        count
    }

    pub fn load_balance(&self) {
        let cpu_count = crate::kernel::smp::get_cpu_count();
        if cpu_count <= 1 {
            return;
        }

        let this_cpu = crate::kernel::smp::get_current_cpu();
        let local_weight = {
            let sched = per_cpu_for(this_cpu);
            sched.cfs_rq.lock().total_weight.load(Ordering::Acquire)
        };

        let mut max_weight: u64 = 0;
        let mut busiest_cpu: u32 = this_cpu;

        for cpu in 0..cpu_count {
            if cpu == this_cpu {
                continue;
            }
            let w = {
                let sched = per_cpu_for(cpu);
                sched.cfs_rq.lock().total_weight.load(Ordering::Acquire)
            };
            if w > max_weight {
                max_weight = w;
                busiest_cpu = cpu;
            }
        }

        // Check if the busiest CPU has significantly more load (weight-based)
        if max_weight < local_weight.saturating_add(LOAD_BALANCE_THRESHOLD) {
            return;
        }

        // Steal tasks from busiest CPU
        let mut tasks_to_migrate: [Pid; 4] = [0; 4];
        let mut count = 0;
        {
            let mut src_rq = per_cpu_for(busiest_cpu).cfs_rq.lock();
            for _ in 0..4 {
                match src_rq.pick_next() {
                    Some((pid, vr)) => {
                        let weight = PROCESS_TABLE.with_process(pid, |p| {
                            p.cfs_weight.load(Ordering::Acquire)
                        }).unwrap_or(NICE0_WEIGHT);
                        src_rq.dequeue(pid, vr, weight);
                        tasks_to_migrate[count] = pid;
                        count += 1;
                    }
                    None => break,
                }
            }
        }

        let mut dst_rq = per_cpu_for(this_cpu).cfs_rq.lock();
        for i in 0..count {
            let pid = tasks_to_migrate[i];
            let (vr, weight) = PROCESS_TABLE.with_process(pid, |p| {
                (p.cfs_vruntime.load(Ordering::Acquire),
                 p.cfs_weight.load(Ordering::Acquire))
            }).unwrap_or((0, NICE0_WEIGHT));
            dst_rq.enqueue(pid, vr, weight);
        }
    }
    
    /// Set CPU quota for a PWM. Caller must hold SYSTEM_CAP_QUOTA_ADMIN.
    pub fn set_quota(&self, pwm: u64, max_runtime: u64, period: u64) {
        let mut quotas = self.quotas.lock();
        let now = TICK_COUNT.load(Ordering::SeqCst);
        for q in quotas.iter_mut() {
            if q.used && q.pwm == pwm {
                q.max_runtime = max_runtime;
                q.period = period;
                q.consumed = 0;
                q.next_reset = now + period;
                return;
            }
        }
        for q in quotas.iter_mut() {
            if !q.used {
                q.used = true;
                q.pwm = pwm;
                q.max_runtime = max_runtime;
                q.period = period;
                q.consumed = 0;
                q.next_reset = now + period;
                return;
            }
        }
    }
    
    /// Remove CPU quota for a PWM
    pub fn remove_quota(&self, pwm: u64) {
        let mut quotas = self.quotas.lock();
        for q in quotas.iter_mut() {
            if q.used && q.pwm == pwm {
                q.used = false;
                q.pwm = 0;
                return;
            }
        }
    }
    
    /// Set process count limit for a PWM
    pub fn set_limit(&self, pwm: u64, max_procs: u32) {
        let mut limits = self.limits.lock();
        for l in limits.iter_mut() {
            if l.used && l.pwm == pwm {
                l.max_procs = max_procs;
                return;
            }
        }
        for l in limits.iter_mut() {
            if !l.used {
                l.used = true;
                l.pwm = pwm;
                l.max_procs = max_procs;
                l.current = 0;
                return;
            }
        }
    }
    
    /// Decrement proc count when a process exits (called from exit())
    fn dec_limit(&self, pwm: u64) {
        if pwm == 0 { return; }
        let mut limits = self.limits.lock();
        for l in limits.iter_mut() {
            if l.used && l.pwm == pwm && l.current > 0 {
                l.current -= 1;
                return;
            }
        }
    }
}

pub static SCHEDULER: Scheduler = Scheduler::new();

pub static SCHEDULER_READY: AtomicBool = AtomicBool::new(false);

pub fn init() {
    SCHEDULER.init();
    SCHEDULER_READY.store(true, Ordering::Release);
}