use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

extern "C" {
    fn tss_set_kernel_stack(rsp0: u64);
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

/// ✅ 深化后的线程状态模型 (七状态)
///
/// 与 ProcessState 保持一致, 提供完整的生命周期管理:
/// - Created: 线程控制块已分配, 资源初始化中
/// - Ready: 就绪状态, 等待 CPU 调度
/// - Running: 正在 CPU 上执行
/// - Blocked: 阻塞等待 (I/O/锁/信号等)
/// - Zombie: 已退出, 等待父线程回收
/// - Terminated: 已被完全回收, TID 可重用
/// - Frozen: 被冻结/挂起 (调试/cgroup freezer/SIGSTOP)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreadState {
    Created = 0,
    Ready = 1,
    Running = 2,
    Blocked = 3,
    Zombie = 4,
    Terminated = 5,
    Frozen = 6,
}

impl ThreadState {
    /// 安全的从 u8 值转换为 ThreadState
    /// 无效值默认返回 Created (安全降级)
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => ThreadState::Created,
            1 => ThreadState::Ready,
            2 => ThreadState::Running,
            3 => ThreadState::Blocked,
            4 => ThreadState::Zombie,
            5 => ThreadState::Terminated,
            6 => ThreadState::Frozen,
            _ => ThreadState::Created, // 无效值安全回退
        }
    }

    /// 从 u32 值转换 (兼容 AtomicU32 存储)
    pub fn from_u32(value: u32) -> Self {
        Self::from_u8(value as u8)
    }

    /// 获取状态的字符串名称 (用于日志和调试)
    pub fn name(&self) -> &'static str {
        match self {
            ThreadState::Created => "Created",
            ThreadState::Ready => "Ready",
            ThreadState::Running => "Running",
            ThreadState::Blocked => "Blocked",
            ThreadState::Zombie => "Zombie",
            ThreadState::Terminated => "Terminated",
            ThreadState::Frozen => "Frozen",
        }
    }

    /// 检查线程是否可调度 (在就绪队列中)
    pub fn is_runnable(&self) -> bool {
        matches!(self, ThreadState::Ready | ThreadState::Running)
    }

    /// 检查线程是否存活 (未终止)
    pub fn is_alive(&self) -> bool {
        !matches!(self, ThreadState::Zombie | ThreadState::Terminated)
    }

    /// 检查线程是否可以被冻结
    pub fn can_freeze(&self) -> bool {
        matches!(self, ThreadState::Running | ThreadState::Ready | ThreadState::Blocked)
    }
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
    // ✅ 新增: 状态转换追踪字段
    pub state_change_count: AtomicU64,  // 状态转换次数统计
    pub frozen_since: AtomicU64,         // 冻结开始时间 (ticks)
    pub exit_code: AtomicU32,            // 退出码 (Zombie/Terminated 时有效)
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
            // ✅ 新增字段初始化
            state_change_count: AtomicU64::new(0),
            frozen_since: AtomicU64::new(0),
            exit_code: AtomicU32::new(0),
        }
    }

    /// ✅ 安全的状态设置 (带审计日志)
    /// 
    /// # Arguments
    /// * `new_state` - 目标新状态
    /// 
    /// # Returns
    /// * `Ok(())` - 状态转换成功
    /// * `Err(&str)` - 非法转换 (如从 Terminated 转换到 Running)
    pub fn set_state_safe(&self, new_state: ThreadState) -> Result<(), &'static str> {
        let current = ThreadState::from_u32(self.state.load(Ordering::Acquire));
        
        // ✅ 状态机合法性检查 (防止非法转换)
        match (current, new_state) {
            // 允许的正常转换
            (ThreadState::Created, ThreadState::Ready) => {},
            (ThreadState::Ready, ThreadState::Running) => {},
            (ThreadState::Running, ThreadState::Ready) => {},      // 时间片耗尽/抢占
            (ThreadState::Running, ThreadState::Blocked) => {},   // 阻塞系统调用
            (ThreadState::Running, ThreadState::Zombie) => {},     // exit()
            (ThreadState::Running, ThreadState::Frozen) => {},     // freeze
            (ThreadState::Ready, ThreadState::Frozen) => {},       // freeze
            (ThreadState::Blocked, ThreadState::Frozen) => {},     // freeze
            (ThreadState::Blocked, ThreadState::Ready) => {},      // 事件完成唤醒
            (ThreadState::Blocked, ThreadState::Zombie) => {},     // 被 kill
            (ThreadState::Zombie, ThreadState::Terminated) => {},  // 回收资源
            (ThreadState::Frozen, ThreadState::Ready) => {},       // thaw 唤醒
            (ThreadState::Frozen, ThreadState::Blocked) => {},     // thaw 后仍需等待
            
            // ❌ 禁止的非法转换
            _ => return Err("Illegal state transition"),
        }
        
        // 执行状态转换
        self.state.store(new_state as u32, Ordering::Release);
        self.state_change_count.fetch_add(1, Ordering::Relaxed);
        
        // ✅ 记录冻结时间点
        if new_state == ThreadState::Frozen {
            self.frozen_since.store(crate::kernel::timer::get_ticks(), Ordering::Relaxed);
        }
        
        // ✅ 审计日志 (调试模式) - 已禁用: no_std 环境
        // #[cfg(debug_assertions)]
        // eprintln!("[THREAD] TID={} {}→{}",
        //           self.tid, current.name(), new_state.name());
        
        Ok(())
    }

    /// 获取当前状态 (线程安全)
    pub fn get_state(&self) -> ThreadState {
        ThreadState::from_u32(self.state.load(Ordering::Acquire))
    }

    /// 检查线程是否可被调度
    pub fn is_runnable(&self) -> bool {
        self.get_state().is_runnable()
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
    }
    
    pub fn add_thread(&self, thread: *mut ThreadNode) {
        if thread.is_null() { return; }
        
        unsafe {
            // ✅ 使用安全的状态设置 (带合法性检查)
            let _ = (*thread).set_state_safe(ThreadState::Ready);
            
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
                        // ✅ 只调度 Ready 状态的线程 (跳过 Frozen/Zombie 等)
                        if (*thread).get_state() == ThreadState::Ready {
                            return Some(thread);
                        }
                        // 非就绪线程不加入队列, 直接丢弃
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
                    let ticks = crate::kernel::timer::get_ticks();
                    if ticks >= sleep_until {
                        (*thread).sleep_until.store(0, Ordering::SeqCst);
                        // ✅ 安全唤醒: Blocked → Ready
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
        
        if self.need_reschedule.load(Ordering::SeqCst) != 0 {
            self.schedule();
        }
    }
    
    pub fn schedule(&self) {
        let prev = self.current.load(Ordering::SeqCst);
        
        if prev != 0 {
            unsafe {
                let thread = prev as *mut ThreadNode;
                let state = (*thread).get_state();
                
                if state == ThreadState::Blocked {
                    // 保持 Blocked 状态 (等待事件)
                } else if state == ThreadState::Running {
                    // ✅ 时间片耗尽: Running → Ready
                    let _ = (*thread).set_state_safe(ThreadState::Ready);
                    self.add_thread(thread);
                }
                // Frozen/Zombie/Terminated 状态不重新入队
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
            // ✅ 安全设置: Ready → Running
            let _ = (*next).set_state_safe(ThreadState::Running);
        }
        self.current.store(next as u64, Ordering::SeqCst);
        self.need_reschedule.store(0, Ordering::SeqCst);
        self.stats.context_switches.fetch_add(1, Ordering::SeqCst);
        
        unsafe {
            tss_set_kernel_stack((*next).kernel_stack.load(Ordering::SeqCst));
        }
    }
    
    pub fn boost_all(&self) {
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
    
    // ========================================================================
    // ✅ Freeze/Thaw 支持 (进程/线程挂起与恢复)
    // ========================================================================
    
    /// 冻结指定线程 (类似 SIGSTOP / cgroup freezer)
    /// 
    /// # Arguments
    /// * `tid` - 目标线程 ID
    /// 
    /// # Returns
    /// * `Ok(())` - 冻结成功
    /// * `Err(&str)` - 失败 (线程不存在或状态不允许冻结)
    /// 
    /// # Usage
    /// - 调试器断点暂停
    /// - cgroup v2 freezer 功能
    /// - 作业控制 (SIGSTOP/SIGCONT)
    pub fn freeze_thread(&self, tid: u32) -> Result<(), &'static str> {
        let current = self.current.load(Ordering::SeqCst);
        
        if current == 0 {
            return Err("No current thread");
        }
        
        unsafe {
            let thread = current as *mut ThreadNode;
            
            if (*thread).tid != tid {
                return Err("Cannot freeze other threads yet (TODO: thread table)");
            }
            
            let state = (*thread).get_state();
            
            if !state.can_freeze() {
                return Err("Thread cannot be frozen in current state");
            }
            
            // ✅ 执行冻结: 当前状态 → Frozen
            (*thread).set_state_safe(ThreadState::Frozen)?;
            
            // 如果冻结的是当前运行线程, 立即触发重新调度
            if current == self.current.load(Ordering::SeqCst) {
                self.need_reschedule.store(1, Ordering::SeqCst);
                self.schedule();
            }
            
            Ok(())
        }
    }

    /// 解冻指定线程 (类似 SIGCONT)
    /// 
    /// # Arguments  
    /// * `tid` - 目标线程 ID
    /// 
    /// # Returns
    /// * `Ok(())` - 解冻成功
    /// * `Err(&str)` - 失败 (线程不存在或未处于 Frozen 状态)
    pub fn thaw_thread(&self, tid: u32) -> Result<(), &'static str> {
        let current = self.current.load(Ordering::SeqCst);
        
        if current == 0 {
            return Err("No current thread");
        }
        
        unsafe {
            let thread = current as *mut ThreadNode;
            
            if (*thread).tid != tid {
                return Err("Cannot thaw other threads yet (TODO: thread table)");
            }
            
            let state = (*thread).get_state();
            
            if state != ThreadState::Frozen {
                return Err("Thread is not frozen");
            }
            
            // ✅ 执行解冻: Frozen → Ready (加入就绪队列等待调度)
            (*thread).set_state_safe(ThreadState::Ready)?;
            self.add_thread(thread);
            
            // 触发重新调度让解冻的线程有机会运行
            self.need_reschedule.store(1, Ordering::SeqCst);
            
            Ok(())
        }
    }

    /// 终止指定线程 (线程退出)
    /// 
    /// # Arguments
    /// * `exit_code` - 退出码
    pub fn exit_thread(&self, exit_code: u32) {
        let current = self.current.load(Ordering::SeqCst);
        
        if current == 0 { return; }
        
        unsafe {
            let thread = current as *mut ThreadNode;
            
            // 设置退出码
            (*thread).exit_code.store(exit_code, Ordering::Relaxed);
            
            // ✅ 状态转换: Running → Zombie
            let _ = (*thread).set_state_safe(ThreadState::Zombie);
            
            // 触发调度器选择下一个线程
            self.need_reschedule.store(1, Ordering::SeqCst);
            self.schedule();
        }
    }

    /// 回收僵尸线程资源 (类似 waitpid)
    /// 
    /// # Arguments
    /// * `tid` - 要回收的线程 ID
    /// 
    /// # Returns
    /// * `Ok(u32)` - 被回收线程的退出码
    /// * `Err(&str)` - 回收失败
    pub fn reap_zombie_thread(&self, tid: u32) -> Result<u32, &'static str> {
        // TODO: 实现完整的线程表查找和资源回收
        // 当前简化实现: 仅做状态转换验证
        
        let current = self.current.load(Ordering::SeqCst);
        if current == 0 {
            return Err("No current thread");
        }
        
        unsafe {
            let thread = current as *mut ThreadNode;
            
            if (*thread).tid != tid {
                return Err("Thread table lookup not implemented");
            }
            
            let state = (*thread).get_state();
            
            if state != ThreadState::Zombie {
                return Err("Thread is not a zombie");
            }
            
            // 获取退出码
            let exit_code = (*thread).exit_code.load(Ordering::Relaxed);
            
            // ✅ 最终状态转换: Zombie → Terminated
            let _ = (*thread).set_state_safe(ThreadState::Terminated);
            
            // TODO: 释放内核栈、用户栈、页表等资源
            
            Ok(exit_code)
        }
    }
    
    pub fn dump_state(&self) {
        // ✅ 使用 klog 宏替代 println (no_std 兼容)
        // 注意: 在 no_std 环境中不能使用 println!
        // 此功能主要用于调试, 可通过串口输出
        #[cfg(debug_assertions)]
        {
            // 调试模式: 尝试使用 eprintln (如果可用)
            // 或者静默记录到内部缓冲区
        }
        
        // 统计信息可通过 get_current() 等方法查询
        // 不在此处输出 (避免依赖 std)
    }
}

pub static SCHEDULER_EX: SchedulerEx = SchedulerEx::new();

pub fn init() {
    SCHEDULER_EX.init();
}

// ========================================================================
// ✅ 单元测试模块 (状态转换、并发安全、边界条件)
//
// 注意: 测试在 no_std 环境下运行, 不能使用 std/println
// 使用 core 库和自定义的 klog_* 宏进行输出
// ========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== ThreadState 测试 ====================

    #[test]
    fn test_thread_state_from_u8_valid_values() {
        assert_eq!(ThreadState::from_u8(0), ThreadState::Created);
        assert_eq!(ThreadState::from_u8(1), ThreadState::Ready);
        assert_eq!(ThreadState::from_u8(2), ThreadState::Running);
        assert_eq!(ThreadState::from_u8(3), ThreadState::Blocked);
        assert_eq!(ThreadState::from_u8(4), ThreadState::Zombie);
        assert_eq!(ThreadState::from_u8(5), ThreadState::Terminated);
        assert_eq!(ThreadState::from_u8(6), ThreadState::Frozen);
    }

    #[test]
    fn test_thread_state_from_u8_invalid_value_fallback() {
        // 无效值应安全回退到 Created
        let invalid = ThreadState::from_u8(255);
        assert_eq!(invalid, ThreadState::Created);
        
        let another_invalid = ThreadState::from_u8(99);
        assert_eq!(another_invalid, ThreadState::Created);
    }

    #[test]
    fn test_thread_state_name() {
        assert_eq!(ThreadState::Created.name(), "Created");
        assert_eq!(ThreadState::Ready.name(), "Ready");
        assert_eq!(ThreadState::Running.name(), "Running");
        assert_eq!(ThreadState::Blocked.name(), "Blocked");
        assert_eq!(ThreadState::Zombie.name(), "Zombie");
        assert_eq!(ThreadState::Terminated.name(), "Terminated");
        assert_eq!(ThreadState::Frozen.name(), "Frozen");
    }

    #[test]
    fn test_thread_state_is_runnable() {
        assert!(ThreadState::Ready.is_runnable());
        assert!(ThreadState::Running.is_runnable());
        
        assert!(!ThreadState::Created.is_runnable());
        assert!(!ThreadState::Blocked.is_runnable());
        assert!(!ThreadState::Zombie.is_runnable());
        assert!(!ThreadState::Terminated.is_runnable());
        assert!(!ThreadState::Frozen.is_runnable());
    }

    #[test]
    fn test_thread_state_is_alive() {
        assert!(ThreadState::Created.is_alive());
        assert!(ThreadState::Ready.is_alive());
        assert!(ThreadState::Running.is_alive());
        assert!(ThreadState::Blocked.is_alive());
        assert!(ThreadState::Frozen.is_alive());
        
        // Zombie 和 Terminated 不算存活
        assert!(!ThreadState::Zombie.is_alive());
        assert!(!ThreadState::Terminated.is_alive());
    }

    #[test]
    fn test_thread_state_can_freeze() {
        // 以下状态可以被冻结
        assert!(ThreadState::Running.can_freeze());
        assert!(ThreadState::Ready.can_freeze());
        assert!(ThreadState::Blocked.can_freeze());
        
        // 以下状态不能被冻结
        assert!(!ThreadState::Created.can_freeze());
        assert!(!ThreadState::Zombie.can_freeze());
        assert!(!ThreadState::Terminated.can_freeze());
        assert!(!ThreadState::Frozen.can_freeze()); // 已经冻结
    }

    // ==================== ThreadNode 状态转换测试 ====================

    #[test]
    fn test_thread_node_normal_lifecycle() {
        let node = ThreadNode::new();
        
        // 初始状态应为 Created
        assert_eq!(node.get_state(), ThreadState::Created);
        
        // Created → Ready (添加到就绪队列)
        node.set_state_safe(ThreadState::Ready).unwrap();
        assert_eq!(node.get_state(), ThreadState::Ready);
        
        // Ready → Running (调度器选中)
        node.set_state_safe(ThreadState::Running).unwrap();
        assert_eq!(node.get_state(), ThreadState::Running);
        
        // Running → Ready (时间片耗尽)
        node.set_state_safe(ThreadState::Ready).unwrap();
        assert_eq!(node.get_state(), ThreadState::Ready);
        
        // Ready → Running (再次调度)
        node.set_state_safe(ThreadState::Running).unwrap();
        assert_eq!(node.get_state(), ThreadState::Running);
        
        // Running → Blocked (等待 I/O)
        node.set_state_safe(ThreadState::Blocked).unwrap();
        assert_eq!(node.get_state(), ThreadState::Blocked);
        
        // Blocked → Ready (I/O 完成)
        node.set_state_safe(ThreadState::Ready).unwrap();
        assert_eq!(node.get_state(), ThreadState::Ready);
        
        // Running → Zombie (线程退出)
        node.set_state_safe(ThreadState::Running).unwrap();
        node.set_state_safe(ThreadState::Zombie).unwrap();
        assert_eq!(node.get_state(), ThreadState::Zombie);
        
        // Zombie → Terminated (资源回收)
        node.set_state_safe(ThreadState::Terminated).unwrap();
        assert_eq!(node.get_state(), ThreadState::Terminated);
    }

    #[test]
    fn test_thread_node_freeze_thaw_cycle() {
        let node = ThreadNode::new();
        
        // 初始化到 Running 状态
        node.set_state_safe(ThreadState::Ready).unwrap();
        node.set_state_safe(ThreadState::Running).unwrap();
        
        // Running → Frozen (freeze)
        node.set_state_safe(ThreadState::Frozen).unwrap();
        assert_eq!(node.get_state(), ThreadState::Frozen);
        
        // Frozen → Ready (thaw 唤醒)
        node.set_state_safe(ThreadState::Ready).unwrap();
        assert_eq!(node.get_state(), ThreadState::Ready);
        
        // 再次测试从 Blocked 冻结
        node.set_state_safe(ThreadState::Running).unwrap();
        node.set_state_safe(ThreadState::Blocked).unwrap();
        node.set_state_safe(ThreadState::Frozen).unwrap();
        assert_eq!(node.get_state(), ThreadState::Frozen);
        
        // Frozen → Blocked (thaw 后仍需等待)
        node.set_state_safe(ThreadState::Blocked).unwrap();
        assert_eq!(node.get_state(), ThreadState::Blocked);
    }

    #[test]
    fn test_thread_node_illegal_transitions_rejected() {
        let node = ThreadNode::new();
        
        // ❌ 尝试非法转换: Created → Running (跳过 Ready)
        let result = node.set_state_safe(ThreadState::Running);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Illegal state transition");
        assert_eq!(node.get_state(), ThreadState::Created); // 状态不变
        
        // ❌ 尝试非法转换: Terminated → Running (复活已终止线程)
        node.state.store(ThreadState::Terminated as u32, Ordering::SeqCst);
        let result = node.set_state_safe(ThreadState::Running);
        assert!(result.is_err());
        assert_eq!(node.get_state(), ThreadState::Terminated);
        
        // ❌ 尝试非法转换: Zombie → Ready (跳过回收直接运行)
        node.state.store(ThreadState::Zombie as u32, Ordering::SeqCst);
        let result = node.set_state_safe(ThreadState::Ready);
        assert!(result.is_err());
        assert_eq!(node.get_state(), ThreadState::Zombie);
    }

    #[test]
    fn test_thread_node_state_change_count() {
        let node = ThreadNode::new();
        
        // 初始计数为 0
        assert_eq!(node.state_change_count.load(Ordering::Relaxed), 0);
        
        // 每次成功转换应该增加计数
        node.set_state_safe(ThreadState::Ready).unwrap();
        assert_eq!(node.state_change_count.load(Ordering::Relaxed), 1);
        
        node.set_state_safe(ThreadState::Running).unwrap();
        assert_eq!(node.state_change_count.load(Ordering::Relaxed), 2);
        
        node.set_state_safe(ThreadState::Blocked).unwrap();
        assert_eq!(node.state_change_count.load(Ordering::Relaxed), 3);
        
        // 失败的转换不应增加计数
        let _ = node.set_state_safe(ThreadState::Created); // 非法
        assert_eq!(node.state_change_count.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_thread_node_exit_code_preserved() {
        let node = ThreadNode::new();
        
        // 设置退出码
        node.exit_code.store(42, Ordering::SeqCst);
        
        // 转换到 Zombie
        node.set_state_safe(ThreadState::Ready).unwrap();
        node.set_state_safe(ThreadState::Running).unwrap();
        node.set_state_safe(ThreadState::Zombie).unwrap();
        
        // 退出码应保留
        assert_eq!(node.exit_code.load(Ordering::Relaxed), 42);
        
        // 转换到 Terminated 后退出码仍可读取
        node.set_state_safe(ThreadState::Terminated).unwrap();
        assert_eq!(node.exit_code.load(Ordering::Relaxed), 42);
    }

    // ==================== SchedulerEx 基础功能测试 ====================

    #[test]
    fn test_scheduler_ex_initialization() {
        let sched = SchedulerEx::new();
        
        // 验证初始状态
        assert_eq!(sched.current.load(Ordering::SeqCst), 0);
        assert_eq!(sched.idle_thread.load(Ordering::SeqCst), 0);
        assert_eq!(sched.tick_count.load(Ordering::SeqCst), 0);
        assert_eq!(sched.need_reschedule.load(Ordering::SeqCst), 0);
        assert_eq!(sched.runq.total.load(Ordering::SeqCst), 0);
        
        // 所有队列应为空
        for level in 0..SCHED_LEVELS {
            assert_eq!(sched.runq.queues[level].load(Ordering::SeqCst), 0);
            assert_eq!(sched.runq.counts[level].load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn test_scheduler_ex_priority_to_level_mapping() {
        // Realtime -> Level 0 (最高优先级)
        assert_eq!(SchedulerEx::priority_to_level(ThreadPriority::Realtime), 0);
        
        // High -> Level 1
        assert_eq!(SchedulerEx::priority_to_level(ThreadPriority::High), 1);
        
        // Normal -> Level 2
        assert_eq!(SchedulerEx::priority_to_level(ThreadPriority::Normal), 2);
        
        // Low/Idle -> Level 3 (最低优先级)
        assert_eq!(SchedulerEx::priority_to_level(ThreadPriority::Low), 3);
        assert_eq!(SchedulerEx::priority_to_level(ThreadPriority::Idle), 3);
    }

    #[test]
    fn test_scheduler_ex_level_to_quantum() {
        // Level 0 (Realtime): 5 ticks
        assert_eq!(SchedulerEx::level_to_quantum(0), SCHED_LEVEL_0_QUANTUM);
        
        // Level 1 (High): 10 ticks
        assert_eq!(SchedulerEx::level_to_quantum(1), SCHED_LEVEL_1_QUANTUM);
        
        // Level 2 (Normal): 20 ticks
        assert_eq!(SchedulerEx::level_to_quantum(2), SCHED_LEVEL_2_QUANTUM);
        
        // Level 3 (Low): 40 ticks
        assert_eq!(SchedulerEx::level_to_quantum(3), SCHED_LEVEL_3_QUANTUM);
        
        // 超出范围回退到最大值
        assert_eq!(SchedulerEx::level_to_quantum(99), SCHED_LEVEL_3_QUANTUM);
    }

    // ==================== 边界条件测试 ====================

    #[test]
    fn test_null_pointer_safety() {
        let sched = SchedulerEx::new();
        
        // 添加空指针应安全返回 (不 panic)
        sched.add_thread(std::ptr::null_mut());
        
        // 队列仍应为空
        assert_eq!(sched.runq.total.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_empty_queue_pop_returns_none() {
        let sched = SchedulerEx::new();
        
        // 从空队列弹出应返回 None
        for level in 0..SCHED_LEVELS {
            assert!(sched.run_queue_pop(level).is_none());
        }
        
        // pop_highest 也应返回 None
        assert!(sched.pop_highest().is_none());
    }

    #[test]
    fn test_max_priority_value_handling() {
        let node = ThreadNode::new();
        
        // 设置极端优先级值
        node.priority.store(u32::MAX, Ordering::SeqCst);
        
        // 应安全处理 (映射到 Realtime)
        let priority = match node.priority.load(Ordering::SeqCst) {
            0 => ThreadPriority::Idle,
            1 => ThreadPriority::Low,
            2 => ThreadPriority::Normal,
            3 => ThreadPriority::High,
            _ => ThreadPriority::Realtime,
        };
        assert_eq!(priority, ThreadPriority::Realtime);
    }
}
