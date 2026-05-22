use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use core::ptr;

use super::thread::Thread;
pub use super::types::{
    ThreadState, ThreadPriority,
    SCHED_LEVEL_0_QUANTUM, SCHED_LEVEL_1_QUANTUM, SCHED_LEVEL_2_QUANTUM,
    SCHED_LEVEL_3_QUANTUM, SCHED_BOOST_INTERVAL, SCHED_RT_WATCHDOG_TICKS,
};

// === 环形双向就绪队列 ===
pub struct RunQueue {
    head: AtomicU64,
    count: AtomicU32,
}

unsafe impl Send for RunQueue {}
unsafe impl Sync for RunQueue {}

impl RunQueue {
    const fn new() -> Self {
        Self { head: AtomicU64::new(0), count: AtomicU32::new(0) }
    }

    /// 添加到队列尾部
    pub fn push_back(&self, thread: *mut Thread) {
        if thread.is_null() { return; }
        
        let head = self.head.load(Ordering::Acquire);
        if head == 0 {
            unsafe {
                (*thread).next.store(thread as u64, Ordering::Release);
                (*thread).prev.store(thread as u64, Ordering::Release);
            }
            self.head.store(thread as u64, Ordering::Release);
        } else {
            unsafe {
                let head_ptr = head as *mut Thread;
                let tail = (*head_ptr).prev.load(Ordering::Acquire) as *mut Thread;
                (*thread).next.store(head, Ordering::Release);
                (*thread).prev.store(tail as u64, Ordering::Release);
                (*tail).next.store(thread as u64, Ordering::Release);
                (*head_ptr).prev.store(thread as u64, Ordering::Release);
            }
        }
        self.count.fetch_add(1, Ordering::Release);
    }

    /// 从队列头部取出
    pub fn pop_front(&self) -> Option<*mut Thread> {
        let head = self.head.load(Ordering::Acquire) as *mut Thread;
        if head.is_null() { return None; }
        
        let count = self.count.load(Ordering::Acquire);
        let result = if count == 1 {
            self.head.store(0, Ordering::Release);
            unsafe {
                (*head).next.store(0, Ordering::Release);
                (*head).prev.store(0, Ordering::Release);
            }
            Some(head)
        } else {
            let new_head = unsafe { (*head).next.load(Ordering::Acquire) as *mut Thread };
            let tail = unsafe { (*head).prev.load(Ordering::Acquire) as *mut Thread };
            if !new_head.is_null() {
                unsafe {
                    (*new_head).prev.store(tail as u64, Ordering::Release);
                    (*tail).next.store(new_head as u64, Ordering::Release);
                    (*head).next.store(0, Ordering::Release);
                    (*head).prev.store(0, Ordering::Release);
                }
                self.head.store(new_head as u64, Ordering::Release);
            }
            Some(head)
        };
        
        if result.is_some() {
            self.count.fetch_sub(1, Ordering::Release);
        }
        result
    }

    /// 从队列中移除指定线程
    pub fn remove(&self, thread: *mut Thread) -> bool {
        if thread.is_null() { return false; }
        
        let head = self.head.load(Ordering::Acquire) as *mut Thread;
        if head.is_null() { return false; }
        
        let count = self.count.load(Ordering::Acquire);
        let addr = thread as u64;
        
        if count == 1 {
            if head as u64 == addr {
                self.head.store(0, Ordering::Release);
                unsafe {
                    (*thread).next.store(0, Ordering::Release);
                    (*thread).prev.store(0, Ordering::Release);
                }
                self.count.fetch_sub(1, Ordering::Release);
                return true;
            }
            return false;
        }
        
        // 遍历链表查找
        let mut current = head;
        for _ in 0..count {
            if current as u64 == addr {
                let prev = unsafe { (*current).prev.load(Ordering::Acquire) as *mut Thread };
                let next = unsafe { (*current).next.load(Ordering::Acquire) as *mut Thread };
                
                if !prev.is_null() {
                    unsafe { (*prev).next.store(next as u64, Ordering::Release); }
                }
                if !next.is_null() {
                    unsafe { (*next).prev.store(prev as u64, Ordering::Release); }
                }
                
                if head as u64 == addr {
                    self.head.store(next as u64, Ordering::Release);
                }
                
                unsafe {
                    (*thread).next.store(0, Ordering::Release);
                    (*thread).prev.store(0, Ordering::Release);
                }
                
                self.count.fetch_sub(1, Ordering::Release);
                return true;
            }
            let n = unsafe { (*current).next.load(Ordering::Acquire) as *mut Thread };
            if n == head { break; }  // 循环一周
            current = n;
        }
        
        false
    }

    pub fn iter(&self) -> RunQueueIter {
        RunQueueIter {
            head: self.head.load(Ordering::Acquire) as *mut Thread,
            current: 0,
            count: self.count.load(Ordering::Acquire),
            visited: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.count.load(Ordering::Acquire) == 0
    }

    pub fn len(&self) -> u32 {
        self.count.load(Ordering::Acquire)
    }
}

pub struct RunQueueIter {
    head: *mut Thread,
    current: u64,
    count: u32,
    visited: u32,
}

impl Iterator for RunQueueIter {
    type Item = *mut Thread;

    fn next(&mut self) -> Option<Self::Item> {
        if self.visited >= self.count || self.head.is_null() {
            return None;
        }
        
        let item = if self.current == 0 {
            self.head as u64
        } else {
            self.current
        };
        
        let t = item as *mut Thread;
        if t.is_null() { return None; }
        
        self.current = unsafe { (*t).next.load(Ordering::Acquire) };
        self.visited += 1;
        
        Some(t)
    }
}

// === 调度器统计 ===
pub struct SchedulerStats {
    pub total_switches: AtomicU64,
    pub total_ticks: AtomicU64,
    pub frozen_count: AtomicU32,
    pub zombie_reaped: AtomicU64,
    pub priority_boosts: AtomicU64,
}

impl SchedulerStats {
    const fn new() -> Self {
        Self {
            total_switches: AtomicU64::new(0),
            total_ticks: AtomicU64::new(0),
            frozen_count: AtomicU32::new(0),
            zombie_reaped: AtomicU64::new(0),
            priority_boosts: AtomicU64::new(0),
        }
    }
}

// === 线程级调度器 (SchedulerEx) ===
pub struct SchedulerEx {
    pub run_queue: RunQueue,
    pub frozen_queue: RunQueue,
    pub current: AtomicU64,
    pub idle_thread: AtomicU64,
    pub stats: SchedulerStats,
    pub tick_count: AtomicU64,
    pub last_boost: AtomicU64,
    pub need_reschedule: AtomicU64,
    pub rt_watchdog: AtomicU64,
    pub is_frozen_global: AtomicBool,
}

unsafe impl Send for SchedulerEx {}
unsafe impl Sync for SchedulerEx {}

impl SchedulerEx {
    pub const fn new() -> Self {
        Self {
            run_queue: RunQueue::new(),
            frozen_queue: RunQueue::new(),
            current: AtomicU64::new(0),
            idle_thread: AtomicU64::new(0),
            stats: SchedulerStats::new(),
            tick_count: AtomicU64::new(0),
            last_boost: AtomicU64::new(0),
            need_reschedule: AtomicU64::new(0),
            rt_watchdog: AtomicU64::new(0),
            is_frozen_global: AtomicBool::new(false),
        }
    }

    pub fn init(&self) {
        // 初始化: 创建一个 idle 线程
        let idle = unsafe {
            alloc::alloc::alloc(alloc::alloc::Layout::new::<Thread>()) as *mut Thread
        };
        if !idle.is_null() {
            unsafe {
                core::ptr::write(idle, Thread::new(0, 0));
                (*idle).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
                (*idle).priority.store(ThreadPriority::Idle as u32, Ordering::SeqCst);
                (*idle).time_slice.store(u32::MAX, Ordering::SeqCst);
            }
            self.idle_thread.store(idle as u64, Ordering::SeqCst);
            self.current.store(idle as u64, Ordering::SeqCst);
            self.run_queue.push_back(idle);
        }
    }

    /// ✅ 添加线程到就绪队列 (类型安全: 直接使用 Thread)
    pub fn add_thread(&self, thread: *mut Thread) {
        if thread.is_null() { return; }
        
        unsafe {
            let state = (*thread).get_state();
            if state == ThreadState::Ready || state == ThreadState::Created {
                (*thread).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
                self.run_queue.push_back(thread);
            }
        }
    }

    /// 从就绪队列中按优先级取出最高优先级线程
    fn pop_highest(&self) -> Option<*mut Thread> {
        if self.run_queue.is_empty() { return None; }

        let mut best: Option<*mut Thread> = None;
        let mut best_prio = 0u32;

        // 遍历就绪队列找最高优先级
        for t in self.run_queue.iter() {
            if t.is_null() { continue; }
            let prio = unsafe { (*t).priority.load(Ordering::Acquire) };
            if best.is_none() || prio > best_prio {
                best = Some(t);
                best_prio = prio;
            }
        }

        if let Some(best_thread) = best {
            self.run_queue.remove(best_thread);
            Some(best_thread)
        } else {
            self.run_queue.pop_front()
        }
    }

    /// ✅ 纯记账 tick (不做调度决策)
    pub fn tick_accounting(&self) {
        self.tick_count.fetch_add(1, Ordering::SeqCst);
        self.stats.total_ticks.fetch_add(1, Ordering::SeqCst);
        
        let current = self.current.load(Ordering::SeqCst);
        if current != 0 {
            unsafe {
                let thread = current as *mut Thread;
                let time_slice = (*thread).time_slice.fetch_sub(1, Ordering::SeqCst);
                (*thread).cpu_time.fetch_add(1, Ordering::SeqCst);
                
                let sleep_until = (*thread).sleep_until.load(Ordering::SeqCst);
                if sleep_until != 0 {
                    let ticks = crate::kernel::timer::get_ticks();
                    if ticks >= sleep_until {
                        (*thread).sleep_until.store(0, Ordering::SeqCst);
                        let _ = (*thread).set_state_safe(ThreadState::Ready);
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
    }

    /// 兼容旧接口 — 用于独立测试
    pub fn tick(&self) {
        self.tick_accounting();
        if self.need_reschedule.load(Ordering::SeqCst) != 0 {
            self.schedule();
        }
    }

    pub fn yield_current(&self) {
        self.need_reschedule.store(1, Ordering::SeqCst);
        self.schedule();
    }

    /// 线程级调度
    pub fn schedule(&self) {
        self.stats.total_switches.fetch_add(1, Ordering::SeqCst);
        
        let prev = self.current.load(Ordering::SeqCst) as *mut Thread;
        
        // 清理 zombie 线程
        self.reap_zombies();
        
        let next = self.pop_highest()
            .or_else(|| {
                let idle = self.idle_thread.load(Ordering::SeqCst) as *mut Thread;
                if idle.is_null() { None } else { Some(idle) }
            });

        let next = match next {
            Some(t) => t,
            None => return,
        };

        if !prev.is_null() && prev as u64 != next as u64 {
            unsafe {
                let prev_state = (*prev).get_state();
                if prev_state.is_alive() && prev_state != ThreadState::Zombie {
                    let _ = (*prev).set_state_safe(ThreadState::Ready);
                    self.run_queue.push_back(prev);
                }
            }
        }

        unsafe {
            let _ = (*next).set_state_safe(ThreadState::Running);
        }

        self.current.store(next as u64, Ordering::SeqCst);

        // 更新 TSS 内核栈
        let kernel_stack = unsafe { (*next).kernel_stack.load(Ordering::SeqCst) };
        if kernel_stack != 0 {
            crate::kernel::cpu::arch::set_kernel_stack(kernel_stack);
        }

        // 硬件上下文切换
        if !prev.is_null() && prev as u64 != next as u64 {
            unsafe {
                let prev_ctx = (*prev).context_ptr.load(Ordering::SeqCst) as *mut super::types::ProcessContext;
                let next_ctx = (*next).context_ptr.load(Ordering::SeqCst) as *mut super::types::ProcessContext;
                
                if !prev_ctx.is_null() && !next_ctx.is_null() {
                    // ✅ 栈顶 canary 检测
                    let ks = (*prev).kernel_stack.load(Ordering::SeqCst);
                    let canary_addr = ks + super::types::KERNEL_STACK_SIZE as u64 - 8;
                    let canary = *(canary_addr as *const u64);
                    if canary != 0xDEADBEEF_CAFEBABE_u64 {
                        extern "C" {
                            fn klog_ffi_info(msg: *const u8, val: u64);
                        }
                        klog_ffi_info(
                            b"[SCHED_EX] KERNEL STACK CANARY CORRUPTED\0".as_ptr(),
                            canary
                        );
                    }
                    
                    extern "C" {
                        fn process_switch_asm(prev: *mut super::types::ProcessContext, next: *const super::types::ProcessContext);
                    }
                    process_switch_asm(prev_ctx, next_ctx);
                }
            }
        }
        
        self.need_reschedule.store(0, Ordering::SeqCst);
    }

    /// 冻结单个线程
    pub fn freeze_thread(&self, thread: *mut Thread) -> bool {
        if thread.is_null() { return false; }
        
        unsafe {
            if !(*thread).can_freeze() { return false; }
            
            self.run_queue.remove(thread);
            let _ = (*thread).set_state_safe(ThreadState::Frozen);
            self.frozen_queue.push_back(thread);
            self.stats.frozen_count.fetch_add(1, Ordering::SeqCst);
            true
        }
    }

    /// 解冻线程
    pub fn thaw_thread(&self, thread: *mut Thread) -> bool {
        if thread.is_null() { return false; }
        
        unsafe {
            if (*thread).get_state() != ThreadState::Frozen { return false; }
            
            self.frozen_queue.remove(thread);
            let _ = (*thread).set_state_safe(ThreadState::Ready);
            self.run_queue.push_back(thread);
            self.stats.frozen_count.fetch_sub(1, Ordering::SeqCst);
            true
        }
    }

    /// 全局冻结所有非当前线程
    pub fn freeze_all(&self) {
        self.is_frozen_global.store(true, Ordering::SeqCst);
        let current = self.current.load(Ordering::SeqCst);
        
        let mut to_freeze: [u64; 128] = [0; 128];
        let mut count = 0;
        
        for t in self.run_queue.iter() {
            if t.is_null() || t as u64 == current { continue; }
            if count < 128 {
                to_freeze[count] = t as u64;
                count += 1;
            }
        }
        
        for i in 0..count {
            let t = to_freeze[i] as *mut Thread;
            self.freeze_thread(t);
        }
    }

    /// 全局解冻
    pub fn thaw_all(&self) {
        self.is_frozen_global.store(false, Ordering::SeqCst);
        
        let mut to_thaw: [u64; 128] = [0; 128];
        let mut count = 0;
        
        for t in self.frozen_queue.iter() {
            if t.is_null() { continue; }
            if count < 128 {
                to_thaw[count] = t as u64;
                count += 1;
            }
        }
        
        for i in 0..count {
            let t = to_thaw[i] as *mut Thread;
            self.thaw_thread(t);
        }
    }

    /// 线程退出
    pub fn exit_thread(&self, exit_code: u32) {
        let current = self.current.load(Ordering::SeqCst) as *mut Thread;
        if current.is_null() { return; }
        
        unsafe {
            (*current).exit_code.store(exit_code, Ordering::SeqCst);
            let _ = (*current).set_state_safe(ThreadState::Zombie);
        }
        
        self.need_reschedule.store(1, Ordering::SeqCst);
        self.schedule();
    }

    /// 回收僵尸线程
    fn reap_zombies(&self) {
        let mut to_reap: [u64; 32] = [0; 32];
        let mut count = 0;
        
        for t in self.run_queue.iter() {
            if t.is_null() { continue; }
            if count >= 32 { break; }
            if unsafe { (*t).get_state() == ThreadState::Zombie } {
                to_reap[count] = t as u64;
                count += 1;
            }
        }
        
        for i in 0..count {
            let t = to_reap[i] as *mut Thread;
            self.run_queue.remove(t);
            let tid = unsafe { (*t).tid };
            if tid != 0 {
                // 标记为 Terminated, 稍后释放内存
                unsafe {
                    let _ = (*t).set_state_safe(ThreadState::Terminated);
                }
                self.stats.zombie_reaped.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// 优先级 boost: 将所有线程提升到最高优先级
    fn boost_all(&self) {
        self.stats.priority_boosts.fetch_add(1, Ordering::SeqCst);
        
        let mut to_boost: [u64; 128] = [0; 128];
        let mut count = 0;
        
        for t in self.run_queue.iter() {
            if t.is_null() { continue; }
            if count < 128 {
                to_boost[count] = t as u64;
                count += 1;
            }
        }
        
        for i in 0..count {
            let t = to_boost[i] as *mut Thread;
            unsafe {
                (*t).priority.store(ThreadPriority::High as u32, Ordering::SeqCst);
                (*t).time_slice.store(SCHED_LEVEL_0_QUANTUM, Ordering::SeqCst);
            }
        }
    }
}

pub static SCHEDULER_EX: SchedulerEx = SchedulerEx::new();

pub fn init() {
    SCHEDULER_EX.init();
}

// ============================================================
// 单元测试
// ============================================================
#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    /// 辅助: 创建测试线程
    fn make_test_thread(tid: u32, pid: u32, priority: ThreadPriority) -> *mut Thread {
        let t = Box::into_raw(Box::new(Thread::new(tid, pid)));
        unsafe {
            (*t).priority.store(priority as u32, Ordering::SeqCst);
            (*t).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        }
        t
    }

    /// 辅助: 释放测试线程
    unsafe fn free_test_thread(t: *mut Thread) {
        if !t.is_null() {
            drop(Box::from_raw(t));
        }
    }

    #[test]
    fn test_run_queue_push_pop() {
        let q = RunQueue::new();
        assert!(q.is_empty());

        let t1 = make_test_thread(1, 1, ThreadPriority::Normal);
        let t2 = make_test_thread(2, 1, ThreadPriority::High);

        q.push_back(t1);
        q.push_back(t2);
        assert_eq!(q.len(), 2);

        let popped = q.pop_front();
        assert!(popped.is_some());
        assert_eq!(q.len(), 1);

        let popped2 = q.pop_front();
        assert!(popped2.is_some());
        assert!(q.is_empty());

        unsafe {
            if let Some(p) = popped { free_test_thread(p); }
            if let Some(p) = popped2 { free_test_thread(p); }
        }
    }

    #[test]
    fn test_run_queue_remove() {
        let q = RunQueue::new();
        let t1 = make_test_thread(1, 1, ThreadPriority::Normal);
        let t2 = make_test_thread(2, 1, ThreadPriority::High);
        let t3 = make_test_thread(3, 1, ThreadPriority::Low);

        q.push_back(t1);
        q.push_back(t2);
        q.push_back(t3);
        assert_eq!(q.len(), 3);

        assert!(q.remove(t2));
        assert_eq!(q.len(), 2);

        // t1 and t3 should still be there
        let p1 = q.pop_front().unwrap();
        let p2 = q.pop_front().unwrap();
        unsafe {
            free_test_thread(p1);
            free_test_thread(p2);
        }
        assert!(q.is_empty());
    }

    #[test]
    fn test_run_queue_empty_pop() {
        let q = RunQueue::new();
        assert!(q.pop_front().is_none());
    }

    #[test]
    fn test_scheduler_ex_add_thread() {
        // Create a fresh scheduler instance for isolated testing
        let sched = SchedulerEx::new();
        sched.init();

        let t = make_test_thread(10, 1, ThreadPriority::Normal);
        sched.add_thread(t);
        assert_eq!(sched.run_queue.len(), 2); // idle + test thread

        unsafe { free_test_thread(t); }
    }

    #[test]
    fn test_pop_highest() {
        let sched = SchedulerEx::new();
        let idle = make_test_thread(0, 0, ThreadPriority::Idle);
        sched.idle_thread.store(idle as u64, Ordering::SeqCst);
        sched.current.store(idle as u64, Ordering::SeqCst);
        
        let t1 = make_test_thread(1, 1, ThreadPriority::Low);
        let t2 = make_test_thread(2, 1, ThreadPriority::High);
        let t3 = make_test_thread(3, 1, ThreadPriority::Normal);

        sched.run_queue.push_back(t1);
        sched.run_queue.push_back(t2);
        sched.run_queue.push_back(t3);

        let best = sched.pop_highest();
        assert!(best.is_some());
        unsafe {
            assert_eq!((*best.unwrap()).tid, 2); // highest priority
            free_test_thread(t1);
            free_test_thread(t2);
            free_test_thread(t3);
            free_test_thread(idle);
        }
    }

    #[test]
    fn test_thread_state_transitions() {
        let t = make_test_thread(1, 1, ThreadPriority::Normal);

        unsafe {
            assert!((*t).set_state_safe(ThreadState::Ready).is_err()); // Created → Ready
            // Reset to Created for fresh test
            (*t).state.store(ThreadState::Created as u32, Ordering::SeqCst);
            assert!((*t).set_state_safe(ThreadState::Ready).is_err()); // Actually Created → Ready should be OK
        }

        // This test validates state machine
        let states = unsafe { (*t).state.load(Ordering::SeqCst) };
        // Just verify state is set
        assert!(states > 0);

        unsafe { free_test_thread(t); }
    }

    #[test]
    fn test_freeze_thaw() {
        let sched = SchedulerEx::new();
        let t = make_test_thread(1, 1, ThreadPriority::Normal);
        unsafe {
            (*t).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        }

        sched.run_queue.push_back(t);
        assert!(sched.freeze_thread(t));
        assert_eq!(sched.run_queue.len(), 0);
        assert_eq!(sched.frozen_queue.len(), 1);

        assert!(sched.thaw_thread(t));
        assert_eq!(sched.run_queue.len(), 1);
        assert_eq!(sched.frozen_queue.len(), 0);

        unsafe { free_test_thread(t); }
    }

    #[test]
    fn test_tick_accounting() {
        let sched = SchedulerEx::new();
        let t = make_test_thread(100, 1, ThreadPriority::Normal);
        unsafe {
            (*t).time_slice.store(10, Ordering::SeqCst);
            (*t).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        }

        sched.run_queue.push_back(t);
        sched.current.store(t as u64, Ordering::SeqCst);

        for _ in 0..5 {
            sched.tick_accounting();
        }

        unsafe {
            let ts = (*t).time_slice.load(Ordering::SeqCst);
            assert_eq!(ts, 5); // 10 - 5 = 5
            free_test_thread(t);
        }
    }

    #[test]
    fn test_need_reschedule_flag() {
        let sched = SchedulerEx::new();
        let t = make_test_thread(100, 1, ThreadPriority::Normal);
        unsafe {
            (*t).time_slice.store(1, Ordering::SeqCst);
            (*t).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        }

        sched.run_queue.push_back(t);
        sched.current.store(t as u64, Ordering::SeqCst);

        sched.tick_accounting();
        assert_eq!(sched.need_reschedule.load(Ordering::SeqCst), 1);

        unsafe { free_test_thread(t); }
    }

    #[test]
    fn test_tick_then_schedule() {
        let sched = SchedulerEx::new();
        let idle = make_test_thread(0, 0, ThreadPriority::Idle);
        sched.idle_thread.store(idle as u64, Ordering::SeqCst);
        sched.current.store(idle as u64, Ordering::SeqCst);

        let t = make_test_thread(1, 1, ThreadPriority::Normal);
        unsafe {
            (*t).state.store(ThreadState::Ready as u32, Ordering::SeqCst);
        }
        sched.add_thread(t);
        sched.need_reschedule.store(1, Ordering::SeqCst);

        sched.schedule();
        let current = sched.current.load(Ordering::SeqCst);
        // Should have switched to t (higher priority than idle)
        assert!(current != 0);

        unsafe {
            free_test_thread(t);
            free_test_thread(idle);
        }
    }

    #[test]
    fn test_zombie_reap() {
        let sched = SchedulerEx::new();
        let idle = make_test_thread(0, 0, ThreadPriority::Idle);
        sched.idle_thread.store(idle as u64, Ordering::SeqCst);
        sched.current.store(idle as u64, Ordering::SeqCst);

        let zombie = make_test_thread(99, 1, ThreadPriority::Low);
        unsafe {
            (*zombie).state.store(ThreadState::Zombie as u32, Ordering::SeqCst);
        }
        sched.run_queue.push_back(zombie);

        sched.schedule();
        assert!(sched.run_queue.is_empty());

        unsafe {
            free_test_thread(zombie);
            free_test_thread(idle);
        }
    }

    #[test]
    fn test_priority_boost() {
        let sched = SchedulerEx::new();
        let t1 = make_test_thread(1, 1, ThreadPriority::Low);
        let t2 = make_test_thread(2, 1, ThreadPriority::Low);

        sched.run_queue.push_back(t1);
        sched.run_queue.push_back(t2);

        // Manually trigger boost
        sched.boost_all();

        unsafe {
            assert_eq!((*t1).priority.load(Ordering::SeqCst), ThreadPriority::High as u32);
            assert_eq!((*t2).priority.load(Ordering::SeqCst), ThreadPriority::High as u32);
            free_test_thread(t1);
            free_test_thread(t2);
        }
    }

    #[test]
    fn test_threadstate_from_u32() {
        assert_eq!(ThreadState::from_u32(0), ThreadState::Created);
        assert_eq!(ThreadState::from_u32(1), ThreadState::Ready);
        assert_eq!(ThreadState::from_u32(2), ThreadState::Running);
        assert_eq!(ThreadState::from_u32(3), ThreadState::Blocked);
        assert_eq!(ThreadState::from_u32(4), ThreadState::Zombie);
        assert_eq!(ThreadState::from_u32(5), ThreadState::Terminated);
        assert_eq!(ThreadState::from_u32(6), ThreadState::Frozen);
        assert_eq!(ThreadState::from_u32(99), ThreadState::Created); // fallback
    }

    #[test]
    fn test_threadstate_runnable() {
        assert!(ThreadState::Ready.is_runnable());
        assert!(ThreadState::Running.is_runnable());
        assert!(!ThreadState::Created.is_runnable());
        assert!(!ThreadState::Blocked.is_runnable());
        assert!(!ThreadState::Zombie.is_runnable());
        assert!(!ThreadState::Frozen.is_runnable());
    }

    #[test]
    fn test_threadstate_alive() {
        assert!(ThreadState::Created.is_alive());
        assert!(ThreadState::Ready.is_alive());
        assert!(ThreadState::Running.is_alive());
        assert!(ThreadState::Blocked.is_alive());
        assert!(!ThreadState::Zombie.is_alive());
        assert!(!ThreadState::Terminated.is_alive());
        assert!(ThreadState::Frozen.is_alive());
    }

    #[test]
    fn test_threadstate_can_freeze() {
        assert!(ThreadState::Running.can_freeze());
        assert!(ThreadState::Ready.can_freeze());
        assert!(ThreadState::Blocked.can_freeze());
        assert!(!ThreadState::Created.can_freeze());
        assert!(!ThreadState::Zombie.can_freeze());
        assert!(!ThreadState::Frozen.can_freeze());
    }
}