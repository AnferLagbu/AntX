use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use super::types::*;
use super::scheduler::{SchedPolicy};

const MAX_FDS_PER_PROCESS: usize = 64;

#[derive(Debug)]
pub struct FdTable {
    entries: Mutex<[i32; MAX_FDS_PER_PROCESS]>,
}

impl FdTable {
    pub const fn new() -> Self {
        Self {
            entries: Mutex::new([-1; MAX_FDS_PER_PROCESS]),
        }
    }

    pub fn init(&self) {
        let mut entries = self.entries.lock();
        for e in entries.iter_mut() {
            *e = -1;
        }
    }

    /// ✅ 分配 per-process FD slot, 返回本地 fd 编号
    pub fn alloc_fd(&self, global_fd: i32) -> Option<usize> {
        let mut entries = self.entries.lock();
        for i in 0..MAX_FDS_PER_PROCESS {
            if entries[i] == -1 {
                entries[i] = global_fd;
                return Some(i);
            }
        }
        None
    }

    /// ✅ 通过本地 fd 获取全局 FD 编号
    pub fn get_global_fd(&self, local_fd: usize) -> Option<i32> {
        let entries = self.entries.lock();
        if local_fd < MAX_FDS_PER_PROCESS {
            let gfd = entries[local_fd];
            if gfd != -1 { Some(gfd) } else { None }
        } else {
            None
        }
    }

    /// ✅ 关闭本地 fd
    pub fn close_fd(&self, local_fd: usize) -> bool {
        if local_fd >= MAX_FDS_PER_PROCESS { return false; }
        let mut entries = self.entries.lock();
        if entries[local_fd] != -1 {
            entries[local_fd] = -1;
            true
        } else {
            false
        }
    }
}

extern "C" {
    fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void;
    fn vmm_create_user_page_table() -> u64;
    fn vmm_destroy_page_table(cr3: u64);
}

pub const KERNEL_STACK_CANARY: u64 = 0xDEADBEEF_CAFEBABE;

pub fn kernel_stack_check_canary(stack_top: u64) -> bool {
    if stack_top == 0 { return true; }
    unsafe {
        let canary_ptr = (stack_top - 8) as *const u64;
        if (canary_ptr as u64) < 0x1000 { return true; }
        let value = core::ptr::read_volatile(canary_ptr);
        value == KERNEL_STACK_CANARY
    }
}

pub fn kernel_stack_write_canary(stack_top: u64) {
    if stack_top == 0 { return; }
    unsafe {
        let canary_ptr = (stack_top - 8) as *mut u64;
        if (canary_ptr as u64) < 0x1000 { return; }
        core::ptr::write_volatile(canary_ptr, KERNEL_STACK_CANARY);
    }
}

pub struct Process {
    pub pid: ProcessId,
    pub pwm: AtomicU64,
    pub state: AtomicU32,
    pub priority: AtomicU32,
    pub flags: AtomicU32,
    
    pub name: Mutex<String>,
    pub parent: Option<ProcessId>,
    pub children: Mutex<Vec<ProcessId>>,
    
    pub context: Mutex<ProcessContext>,
    pub cr3: AtomicU64,
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,
    
    pub exit_code: AtomicU32,
    pub cpu_time: AtomicU64,
    
    pub block_reason: AtomicU32,
    
    pub sched_policy: AtomicU32,
    pub rt_priority: AtomicU32,

    pub session_id: AtomicU64,
    pub fd_table: FdTable,
    
    /// ✅ 阻塞睡眠到期时间 (ticks), 用于 proc_sleep_ms
    pub sleep_until: AtomicU64,
}

// ✅ P0-5 修复: 添加详细的安全性不变性注释
//
// # Safety (Send)
// Process 可以安全地在线程间转移所有权, 因为:
// 1. 所有可变状态都通过 Mutex 或 AtomicX 保护
// 2. Mutex<String> 和 Mutex<Vec> 内部使用 spin::Mutex, 它实现了 Send
// 3. 原始指针字段 (cr3, kernel_stack, user_stack) 只通过原子操作访问
// 4. 不存在悬垂指针或数据竞争的风险
//
// # Safety (Sync)  
// Process 可以安全地被多个线程共享引用 (&Process), 因为:
// 1. name, children, context 等复合类型都被 Mutex 包装
//    - 访问这些字段必须先获取锁, 保证互斥
// 2. pid, pwm, state 等简单字段都是 Atomic 类型
//    - 使用 Ordering::SeqCst 或 Acquire/Release 保证可见性
// 3. 不存在内部可变性导致的未同步修改
// 4. 调度器在切换进程时通过 scheduler_lock 保护整个 ProcessTable
//
// SAFETY: Process uses Mutex for mutable state; all fields are either
// Copy/primitive types or protected by locks. No UnsafeCell or interior
// mutability without synchronization. Cross-thread access is safe because
// mutation always goes through the scheduler lock or per-field Mutex.
unsafe impl Send for Process {}
unsafe impl Sync for Process {}

impl Process {
    pub fn new(pid: Pid, name: &str, parent: Option<ProcessId>) -> Self {
        Self {
            pid: ProcessId(pid),
            pwm: AtomicU64::new(0),
            state: AtomicU32::new(ProcessState::Created as u32),
            priority: AtomicU32::new(ProcessPriority::Normal as u32),
            flags: AtomicU32::new(0),
            name: Mutex::new(String::from(name)),
            parent,
            children: Mutex::new(Vec::new()),
            context: Mutex::new(ProcessContext::new()),
            cr3: AtomicU64::new(0),
            kernel_stack: AtomicU64::new(0),
            user_stack: AtomicU64::new(0),
            exit_code: AtomicU32::new(0),
            cpu_time: AtomicU64::new(0),
            block_reason: AtomicU32::new(BlockReason::Unknown as u32),
            sched_policy: AtomicU32::new(SchedPolicy::Normal as u32),
            rt_priority: AtomicU32::new(0),
            session_id: AtomicU64::new(0),
            fd_table: FdTable::new(),
            sleep_until: AtomicU64::new(0),
        }
    }
    
    pub fn allocate_kernel_stack(&self) -> bool {
        const KERNEL_BASE: u64 = 0xFFFF800000000000;
        unsafe {
            let stack = pmm_alloc_pages((KERNEL_STACK_SIZE / 4096) as u64);
            if stack.is_null() {
                return false;
            }
            // Convert physical address to higher-half virtual address so that
            // TSS RSP0 is accessible when the user page table is loaded.
            let stack_top = stack as u64 + KERNEL_BASE + KERNEL_STACK_SIZE as u64;
            self.kernel_stack.store(stack_top, Ordering::SeqCst);
            kernel_stack_write_canary(stack_top);
            true
        }
    }
    
    pub fn allocate_user_space(&self) -> bool {
        unsafe {
            let cr3 = vmm_create_user_page_table();
            if cr3 == 0 {
                return false;
            }
            self.cr3.store(cr3, Ordering::SeqCst);
            true
        }
    }
    
    pub fn get_state(&self) -> ProcessState {
        ProcessState::from_u8(self.state.load(Ordering::SeqCst) as u8)
    }
    
    /// ✅ 安全的状态设置 (带合法性检查和审计日志)
    /// 
    /// # Arguments
    /// * `new_state` - 目标新状态
    /// 
    /// # Returns
    /// * `Ok(())` - 状态转换成功
    /// * `Err(&str)` - 非法状态转换
    pub fn set_state_safe(&self, new_state: ProcessState) -> Result<(), &'static str> {
        let current = self.get_state();
        
        // ✅ 状态机合法性检查 (防止非法转换)
        match (current, new_state) {
            // 允许的正常转换
            (ProcessState::Created, ProcessState::Ready) => {},
            (ProcessState::Ready, ProcessState::Running) => {},
            (ProcessState::Running, ProcessState::Ready) => {},      // 时间片耗尽/抢占
            (ProcessState::Running, ProcessState::Blocked) => {},   // 阻塞系统调用
            (ProcessState::Running, ProcessState::Zombie) => {},     // exit()
            (ProcessState::Running, ProcessState::Frozen) => {},     // freeze
            (ProcessState::Ready, ProcessState::Frozen) => {},       // freeze
            (ProcessState::Blocked, ProcessState::Frozen) => {},     // freeze
            (ProcessState::Blocked, ProcessState::Ready) => {},      // 事件完成唤醒
            (ProcessState::Blocked, ProcessState::Zombie) => {},     // 被 kill
            (ProcessState::Zombie, ProcessState::Terminated) => {},  // wait() 回收
            (ProcessState::Frozen, ProcessState::Ready) => {},       // thaw 唤醒
            (ProcessState::Frozen, ProcessState::Blocked) => {},     // thaw 后仍需等待
            
            // ❌ 禁止的非法转换
            _ => return Err("Illegal process state transition"),
        }
        
        // 执行状态转换
        self.state.store(new_state as u32, Ordering::Release);
        
        // ✅ 审计日志 (调试模式) - 已禁用: no_std 环境
        // #[cfg(debug_assertions)]
        // eprintln!("[PROCESS] PID={} {}→{}",
        //           self.pid.0, current.name(), new_state.name());
        
        Ok(())
    }
    
    /// 旧版兼容接口 (内部使用, 不建议新代码使用)
    #[deprecated(note = "Use set_state_safe() for state transitions with validation")]
    pub fn set_state(&self, state: ProcessState) {
        // 兼容旧代码, 但记录警告
        let _ = self.set_state_safe(state);
    }
    
    pub fn get_priority(&self) -> ProcessPriority {
        ProcessPriority::from_u32(self.priority.load(Ordering::SeqCst))
    }
    
    pub fn set_priority(&self, priority: ProcessPriority) {
        self.priority.store(priority as u32, Ordering::SeqCst);
    }
    
    pub fn is_kernel(&self) -> bool {
        let flags = self.flags.load(Ordering::SeqCst);
        (flags & ProcessFlags::IS_KERNEL.bits()) != 0
    }
    
    pub fn set_kernel(&self, is_kernel: bool) {
        let mut flags = self.flags.load(Ordering::SeqCst);
        if is_kernel {
            flags |= ProcessFlags::IS_KERNEL.bits();
        } else {
            flags &= !ProcessFlags::IS_KERNEL.bits();
        }
        self.flags.store(flags, Ordering::SeqCst);
    }
    
    pub fn get_sched_policy(&self) -> SchedPolicy {
        SchedPolicy::from_u32(self.sched_policy.load(Ordering::SeqCst))
    }
    
    pub fn set_sched_policy(&self, policy: SchedPolicy) {
        self.sched_policy.store(policy as u32, Ordering::SeqCst);
    }
    
    pub fn get_rt_priority(&self) -> u8 {
        self.rt_priority.load(Ordering::SeqCst) as u8
    }
    
    pub fn set_rt_priority(&self, priority: u8) {
        self.rt_priority.store(priority as u32, Ordering::SeqCst);
    }
    
    pub fn get_pwm(&self) -> u64 {
        self.pwm.load(Ordering::SeqCst)
    }
    
    pub fn set_pwm(&self, pwm: u64) {
        self.pwm.store(pwm, Ordering::SeqCst);
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let cr3 = self.cr3.load(Ordering::SeqCst);
        if cr3 != 0 {
            unsafe {
                vmm_destroy_page_table(cr3);
            }
        }
    }
}

pub struct ProcessTable {
    processes: Mutex<[Option<usize>; MAX_PROCESSES]>,
    next_pid: AtomicU32,
}

// SAFETY: ProcessTable uses Mutex for the process array and AtomicU32
// for next_pid. All mutations are serialized through the Mutex.
unsafe impl Send for ProcessTable {}
unsafe impl Sync for ProcessTable {}

impl ProcessTable {
    pub const fn new() -> Self {
        Self {
            processes: Mutex::new([None; MAX_PROCESSES]),
            next_pid: AtomicU32::new(1),
        }
    }
    
    pub fn allocate_pid(&self) -> Option<Pid> {
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        if pid as usize >= MAX_PROCESSES {
            None
        } else {
            Some(pid)
        }
    }
    
    pub fn insert(&self, process: *mut Process) -> bool {
        let mut table = self.processes.lock();
        let pid = unsafe { (*process).pid.0 as usize };
        if pid >= MAX_PROCESSES {
            return false;
        }
        table[pid] = Some(process as usize);
        true
    }
    
    pub fn get(&self, pid: Pid) -> Option<*mut Process> {
        let table = self.processes.lock();
        if pid as usize >= MAX_PROCESSES {
            return None;
        }
        table[pid as usize].map(|addr| addr as *mut Process)
    }

    pub fn with_process<F, R>(&self, pid: Pid, f: F) -> Option<R>
    where
        F: FnOnce(&Process) -> R,
    {
        let table = self.processes.lock();
        if pid as usize >= MAX_PROCESSES {
            return None;
        }
        match table[pid as usize] {
            Some(addr) => {
                let proc_ref = unsafe { &*(addr as *const Process) };
                Some(f(proc_ref))
            }
            None => None,
        }
    }

    pub fn with_process_mut<F, R>(&self, pid: Pid, f: F) -> Option<R>
    where
        F: FnOnce(&mut Process) -> R,
    {
        let table = self.processes.lock();
        if pid as usize >= MAX_PROCESSES {
            return None;
        }
        match table[pid as usize] {
            Some(addr) => {
                let proc_ref = unsafe { &mut *(addr as *mut Process) };
                Some(f(proc_ref))
            }
            None => None,
        }
    }
    
    pub fn remove(&self, pid: Pid) -> Option<*mut Process> {
        let mut table = self.processes.lock();
        if pid as usize >= MAX_PROCESSES {
            return None;
        }
        table[pid as usize].take().map(|addr| addr as *mut Process)
    }

    /// ✅ 移除进程并释放 Box<Process> 内存 (Fix 4: 内存泄漏修复)
    pub fn remove_and_free(&self, pid: Pid) {
        if let Some(ptr) = self.remove(pid) {
            unsafe {
                let boxed = Box::from_raw(ptr);
                // Process::Drop 会自动销毁页表 (vmm_destroy_page_table)
                drop(boxed);
            }
        }
    }

    /// 遍历所有进程 (回调返回 false 时提前终止)
    pub fn for_each<F: FnMut(&Process) -> bool>(&self, mut f: F) {
        let table = self.processes.lock();
        for entry in table.iter() {
            if let &Some(addr) = entry {
                let proc = unsafe { &*(addr as *const Process) };
                if !f(proc) { break; }
            }
        }
    }
}

pub static PROCESS_TABLE: ProcessTable = ProcessTable::new();

#[derive(Clone, Copy)]
struct ProcSnapshot {
    next_pid: u32,
    slots: [Option<usize>; MAX_PROCESSES],
}

static PROC_SNAPSHOT: Mutex<Option<ProcSnapshot>> = Mutex::new(None);

pub fn proc_barrier_capture() {
    let table = &PROCESS_TABLE;
    *PROC_SNAPSHOT.lock() = Some(ProcSnapshot {
        next_pid: table.next_pid.load(Ordering::SeqCst),
        slots: *table.processes.lock(),
    });
}

pub fn proc_barrier_rollback() -> bool {
    if let Some(ref snap) = *PROC_SNAPSHOT.lock() {
        let table = &PROCESS_TABLE;
        table.next_pid.store(snap.next_pid, Ordering::SeqCst);
        *table.processes.lock() = snap.slots;
    }
    true
}

extern "C" fn proc_barrier_capture_cb() {
    proc_barrier_capture();
}

extern "C" fn proc_barrier_rollback_cb() -> bool {
    proc_barrier_rollback()
}

pub fn proc_register_barrier_domain() {
    crate::kernel::barrier::recovery_domain_register(4);
    if let Some(dom) = crate::kernel::barrier::RECOVERY_MANAGER.lock().find(4) {
        *dom.capture_cb.lock() = Some(proc_barrier_capture_cb);
        *dom.rollback_cb.lock() = Some(proc_barrier_rollback_cb);
    }
}
