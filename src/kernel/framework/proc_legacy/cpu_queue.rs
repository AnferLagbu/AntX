//! Per-CPU RunQueue — SMP 调度基础
//!
//! 在现有全局 MLFQ 之上添加 per-CPU 状态追踪：
//! - 每个 CPU 跟踪自己的 `current` PID
//! - per-CPU `need_reschedule` 标志
//! - 跨 CPU 重新调度 IPI (通过 SOftirq::Sched)
//!
//! ## 架构
//!
//! ```text
//! CPU 0                     CPU 1
//! schedule()               schedule()
//!   ├─ CpuQueue[0]          ├─ CpuQueue[1]
//!   │  current/need_resched  │  current/need_resched
//!   └─ SCHEDULER (global)   └─ SCHEDULER (global)
//!                                ↑
//!                          resched_ipi() → raise_softirq(Sched)
//! ```

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::types::Pid;

pub struct CpuQueue {
    pub current: AtomicU32,
    pub need_reschedule: AtomicBool,
    pub idle_pid: AtomicU32,
    pub online: AtomicBool,
}

// All fields (AtomicU32, AtomicBool) auto-implement Send + Sync.

impl CpuQueue {
    pub const fn new() -> Self {
        Self {
            current: AtomicU32::new(0),
            need_reschedule: AtomicBool::new(false),
            idle_pid: AtomicU32::new(0),
            online: AtomicBool::new(false),
        }
    }

    #[inline]
    pub fn get_current(&self) -> Option<Pid> {
        let pid = self.current.load(Ordering::Acquire);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }

    #[inline]
    pub fn set_current(&self, pid: Pid) {
        self.current.store(pid, Ordering::Release);
    }

    #[inline]
    pub fn set_need_reschedule(&self) {
        self.need_reschedule.store(true, Ordering::Release);
    }

    #[inline]
    pub fn take_need_reschedule(&self) -> bool {
        self.need_reschedule.swap(false, Ordering::AcqRel)
    }
}

struct CpuQueues {
    queues: UnsafeCell<[CpuQueue; crate::kernel::config::MAX_CPUS]>,
}

// SAFETY: CpuQueues wraps UnsafeCell<[CpuQueue; MAX_CPUS]>.
// Each CpuQueue[i] is only accessed by CPU i (per-CPU data).
// The caller must ensure CPU affinity when accessing queue entries.
unsafe impl Sync for CpuQueues {}

static CPU_QUEUES: CpuQueues = CpuQueues {
    queues: UnsafeCell::new([const { CpuQueue::new() }; crate::kernel::config::MAX_CPUS]),
};

pub fn cpu_queue(cpu_id: u32) -> &'static CpuQueue {
    let idx = cpu_id as usize % crate::kernel::config::MAX_CPUS;
    unsafe { &(&*CPU_QUEUES.queues.get())[idx] }
}

pub fn current_cpu_queue() -> &'static CpuQueue {
    let cpu_id = crate::kernel::smp::get_current_cpu();
    cpu_queue(cpu_id)
}

pub fn init_cpu_queue(cpu_id: u32, idle_pid: Pid) {
    let q = cpu_queue(cpu_id);
    q.idle_pid.store(idle_pid, Ordering::Release);
    q.current.store(idle_pid, Ordering::Release);
    q.online.store(true, Ordering::Release);
}

/// 向目标 CPU 发送重新调度 IPI
pub fn resched_cpu(target_cpu: u32) {
    let current = crate::kernel::smp::get_current_cpu();
    if target_cpu == current {
        current_cpu_queue().set_need_reschedule();
        return;
    }

    cpu_queue(target_cpu).set_need_reschedule();

    let target_apic_id = crate::kernel::smp::get_apic_id(target_cpu);
    if target_apic_id != 0xFFFF {
        crate::arch!(send_ipi(target_apic_id, 0xFE));
    }
}

/// IPI 重新调度入口 (由 IPI handler 调用，在目标 CPU 上执行)
/// 通过 softirq 延迟执行 schedule()
#[no_mangle]
pub extern "C" fn resched_ipi_handler() {
    crate::kernel::irq::raise_softirq(crate::kernel::irq::SoftirqVec::Sched);
}

/// 注册 softirq Sched handler (在 scheduler init 时调用)
pub fn register_sched_softirq() {
    crate::kernel::irq::open_softirq(crate::kernel::irq::SoftirqVec::Sched, sched_softirq_handler);
}

fn sched_softirq_handler() {
    let q = current_cpu_queue();
    if q.take_need_reschedule() {
        super::scheduler::SCHEDULER.schedule();
    }
}

#[no_mangle]
pub extern "C" fn cpq_init(cpu_id: u32, idle_pid: Pid) {
    init_cpu_queue(cpu_id, idle_pid);
}

#[no_mangle]
pub extern "C" fn cpq_resched_cpu(target_cpu: u32) {
    resched_cpu(target_cpu);
}
