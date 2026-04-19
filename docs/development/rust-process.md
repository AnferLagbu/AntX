# 进程管理 Rust 重写方案

## 一、概述

本文档描述将 AntX 进程管理模块从 C 语言重写为 Rust 的详细方案。进程管理涉及复杂的状态机和资源生命周期管理，Rust 的所有权系统和类型安全特性可以显著提高代码可靠性。

## 二、当前架构分析

### 2.1 现有模块

| 模块 | 文件 | 功能 |
|------|------|------|
| Process | `src/proc/process.c` | 进程创建、销毁、状态管理 |
| Scheduler | `src/proc/scheduler.c` | 进程调度、上下文切换 |
| Session | `src/proc/session.c` | 会话管理 |
| User Process | `src/proc/user_proc.c` | 用户态进程管理 |
| Context Switch | `src/proc/switch.asm` | 汇编上下文切换 |

### 2.2 现有问题

1. **状态机复杂**: 进程状态转换难以追踪
2. **资源泄漏**: 进程退出时资源可能未正确释放
3. **并发问题**: 多核环境下锁管理困难
4. **指针安全**: 大量裸指针操作

## 三、Rust 架构设计

### 3.1 核心类型定义

```rust
// src/proc/mod.rs

#![no_std]

use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};

pub type Pid = u32;
pub type Tid = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProcessId(Pid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(Tid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Created,
    Ready,
    Running,
    Blocked(BlockReason),
    Zombie,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    WaitingForIo,
    WaitingForChild,
    WaitingForSignal,
    Sleeping(u64),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessPriority {
    Idle = 0,
    Low = 1,
    Normal = 2,
    High = 3,
    RealTime = 4,
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessFlags {
    pub is_kernel: bool,
    pub is_traced: bool,
    pub is_stopped: bool,
}
```

### 3.2 进程结构定义

```rust
// src/proc/process.rs

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub struct Process {
    pub pid: ProcessId,
    pub state: Mutex<ProcessState>,
    pub priority: AtomicU32,
    pub flags: Mutex<ProcessFlags>,
    
    pub name: Mutex<String>,
    pub parent: Option<ProcessId>,
    pub children: Mutex<Vec<ProcessId>>,
    
    pub context: Mutex<ProcessContext>,
    pub memory: Arc<MemorySpace>,
    pub kernel_stack: KernelStack,
    
    pub exit_code: AtomicU32,
    pub cpu_time: AtomicU64,
    
    pub open_files: Mutex<BTreeMap<u32, Arc<dyn crate::fs::File>>>,
    pub working_dir: Mutex<String>,
    
    pub pwid: Pwid,
}

#[derive(Debug)]
pub struct ProcessContext {
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rflags: u64,
    pub cr3: u64,
    
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    
    pub cs: u16,
    pub ds: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub ss: u16,
}

impl ProcessContext {
    pub const fn new() -> Self {
        Self {
            rip: 0, rsp: 0, rbp: 0, rflags: 0x202, cr3: 0,
            rbx: 0, r12: 0, r13: 0, r14: 0, r15: 0,
            cs: 0x08, ds: 0x10, es: 0x10, fs: 0x10, gs: 0x10, ss: 0x10,
        }
    }
    
    pub fn set_user_mode(&mut self) {
        self.cs = 0x1B;
        self.ds = 0x23;
        self.es = 0x23;
        self.fs = 0x23;
        self.gs = 0x23;
        self.ss = 0x23;
        self.rflags = 0x202;
    }
}

pub struct KernelStack {
    bottom: *mut u8,
    top: *mut u8,
    size: usize,
}

impl KernelStack {
    pub fn new(size: usize) -> Option<Self> {
        unsafe {
            let bottom = crate::mm::pmm_alloc_pages(size / 4096) as *mut u8;
            if bottom.is_null() {
                return None;
            }
            
            Some(Self {
                bottom,
                top: bottom.add(size),
                size,
            })
        }
    }
    
    pub fn top(&self) -> u64 {
        self.top as u64
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        unsafe {
            crate::mm::pmm_free_pages(self.bottom as *mut u8, self.size / 4096);
        }
    }
}
```

### 3.3 内存空间管理

```rust
// src/proc/memory.rs

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use spin::Mutex;

pub struct MemorySpace {
    pub cr3: u64,
    pub regions: Mutex<BTreeMap<u64, MemoryRegion>>,
}

#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub start: u64,
    pub end: u64,
    pub permissions: MemoryPermissions,
    pub region_type: MemoryRegionType,
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub user: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum MemoryRegionType {
    Code,
    Data,
    Heap,
    Stack,
    Mmap,
    Vdso,
}

impl MemorySpace {
    pub fn new() -> Self {
        unsafe {
            let cr3 = crate::mm::vmm_create_page_table();
            
            Self {
                cr3,
                regions: Mutex::new(BTreeMap::new()),
            }
        }
    }
    
    pub fn map_region(&self, start: u64, size: u64, perms: MemoryPermissions) -> Result<(), MmError> {
        let mut regions = self.regions.lock();
        
        let region = MemoryRegion {
            start,
            end: start + size,
            permissions: perms,
            region_type: MemoryRegionType::Mmap,
        };
        
        // Map pages
        for addr in (start..start + size).step_by(4096) {
            unsafe {
                let phys = crate::mm::pmm_alloc_page();
                if phys.is_null() {
                    return Err(MmError::OutOfMemory);
                }
                
                let flags = self.permissions_to_flags(&perms);
                crate::mm::vmm_map_page(self.cr3, addr, phys as u64, flags);
            }
        }
        
        regions.insert(start, region);
        Ok(())
    }
    
    fn permissions_to_flags(&self, perms: &MemoryPermissions) -> u64 {
        let mut flags = 0;
        if perms.read { flags |= 0x01; }
        if perms.write { flags |= 0x02; }
        if perms.user { flags |= 0x04; }
        if !perms.execute { flags |= 0x8000000000000000; }
        flags
    }
}

impl Drop for MemorySpace {
    fn drop(&mut self) {
        unsafe {
            crate::mm::vmm_destroy_page_table(self.cr3);
        }
    }
}

#[derive(Debug)]
pub enum MmError {
    OutOfMemory,
    InvalidAddress,
    PermissionDenied,
}
```

### 3.4 调度器实现

```rust
// src/proc/scheduler.rs

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};

pub struct Scheduler {
    ready_queue: Mutex<VecDeque<Arc<Process>>>,
    current: Mutex<Option<Arc<Process>>>,
    all_processes: Mutex<Vec<Arc<Process>>>,
    
    next_pid: AtomicU32,
    running: AtomicBool,
    need_reschedule: AtomicBool,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: Mutex::new(VecDeque::new()),
            current: Mutex::new(None),
            all_processes: Mutex::new(Vec::new()),
            next_pid: AtomicU32::new(1),
            running: AtomicBool::new(false),
            need_reschedule: AtomicBool::new(false),
        }
    }
    
    pub fn create_process(&self, name: &str, parent: Option<ProcessId>) -> Arc<Process> {
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        
        let process = Arc::new(Process {
            pid: ProcessId(pid),
            state: Mutex::new(ProcessState::Created),
            priority: AtomicU32::new(ProcessPriority::Normal as u32),
            flags: Mutex::new(ProcessFlags {
                is_kernel: false,
                is_traced: false,
                is_stopped: false,
            }),
            name: Mutex::new(String::from(name)),
            parent,
            children: Mutex::new(Vec::new()),
            context: Mutex::new(ProcessContext::new()),
            memory: Arc::new(MemorySpace::new()),
            kernel_stack: KernelStack::new(65536).expect("Failed to allocate kernel stack"),
            exit_code: AtomicU32::new(0),
            cpu_time: AtomicU64::new(0),
            open_files: Mutex::new(BTreeMap::new()),
            working_dir: Mutex::new(String::from("/")),
            pwid: Pwid::default(),
        });
        
        self.all_processes.lock().push(process.clone());
        process
    }
    
    pub fn add(&self, process: Arc<Process>) {
        *process.state.lock() = ProcessState::Ready;
        self.ready_queue.lock().push_back(process);
    }
    
    pub fn schedule(&self) -> Option<Arc<Process>> {
        let mut ready = self.ready_queue.lock();
        
        if let Some(next) = ready.pop_front() {
            let mut current = self.current.lock();
            
            if let Some(ref prev) = *current {
                if *prev.state.lock() == ProcessState::Running {
                    *prev.state.lock() = ProcessState::Ready;
                    ready.push_back(prev.clone());
                }
            }
            
            *next.state.lock() = ProcessState::Running;
            *current = Some(next.clone());
            
            Some(next)
        } else {
            None
        }
    }
    
    pub fn current(&self) -> Option<Arc<Process>> {
        self.current.lock().clone()
    }
    
    pub fn block(&self, reason: BlockReason) {
        if let Some(current) = self.current() {
            *current.state.lock() = ProcessState::Blocked(reason);
            self.need_reschedule.store(true, Ordering::SeqCst);
        }
    }
    
    pub fn unblock(&self, pid: ProcessId) {
        let mut all = self.all_processes.lock();
        for process in all.iter() {
            if process.pid == pid {
                let mut state = process.state.lock();
                if let ProcessState::Blocked(_) = *state {
                    *state = ProcessState::Ready;
                    self.ready_queue.lock().push_back(process.clone());
                }
                break;
            }
        }
    }
    
    pub fn exit(&self, exit_code: u32) {
        if let Some(current) = self.current() {
            current.exit_code.store(exit_code, Ordering::SeqCst);
            *current.state.lock() = ProcessState::Zombie;
            
            // Notify parent
            if let Some(parent_pid) = current.parent {
                self.unblock(parent_pid);
            }
            
            self.need_reschedule.store(true, Ordering::SeqCst);
        }
    }
    
    pub fn yield_current(&self) {
        self.need_reschedule.store(true, Ordering::SeqCst);
    }
    
    pub fn should_reschedule(&self) -> bool {
        self.need_reschedule.swap(false, Ordering::SeqCst)
    }
}

// 全局调度器
pub static SCHEDULER: Scheduler = Scheduler::new();
```

### 3.5 上下文切换 (Rust + 汇编)

```rust
// src/proc/context_switch.rs

#[derive(Debug)]
#[repr(C)]
pub struct SwitchContext {
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rbp: u64,
    pub rip: u64,
}

extern "C" {
    fn context_switch_asm(from: *mut SwitchContext, to: *const SwitchContext);
}

pub unsafe fn context_switch(from: &mut SwitchContext, to: &SwitchContext) {
    context_switch_asm(from as *mut SwitchContext, to as *const SwitchContext);
}

pub fn do_switch(prev: &mut Process, next: &Process) {
    unsafe {
        // Save previous context
        let prev_ctx = prev.context.lock();
        let mut prev_switch = SwitchContext {
            rbx: prev_ctx.rbx,
            r12: prev_ctx.r12,
            r13: prev_ctx.r13,
            r14: prev_ctx.r14,
            r15: prev_ctx.r15,
            rbp: prev_ctx.rbp,
            rip: prev_ctx.rip,
        };
        
        // Switch page tables
        let next_cr3 = next.context.lock().cr3;
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) next_cr3,
            options(nostack, preserves_flags)
        );
        
        // Switch context
        let next_switch = SwitchContext {
            rbx: next.context.lock().rbx,
            r12: next.context.lock().r12,
            r13: next.context.lock().r13,
            r14: next.context.lock().r14,
            r15: next.context.lock().r15,
            rbp: next.context.lock().rbp,
            rip: next.context.lock().rip,
        };
        
        context_switch(&mut prev_switch, &next_switch);
    }
}
```

```nasm
; src/proc/switch.asm

global context_switch_asm

section .text
bits 64

context_switch_asm:
    ; Save callee-saved registers
    push rbx
    push r12
    push r13
    push r14
    push r15
    push rbp
    
    ; Save old stack pointer
    mov [rdi], rsp
    
    ; Load new stack pointer
    mov rsp, [rsi]
    
    ; Restore callee-saved registers
    pop rbp
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    
    ret
```

### 3.6 会话管理

```rust
// src/proc/session.rs

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

pub type SessionId = u32;

pub struct Session {
    pub sid: SessionId,
    pub leader: ProcessId,
    pub processes: Mutex<Vec<ProcessId>>,
    pub controlling_terminal: Mutex<Option<u32>>,
    pub foreground_group: Mutex<Option<ProcessGroupId>>,
}

pub type ProcessGroupId = u32;

pub struct ProcessGroup {
    pub pgid: ProcessGroupId,
    pub session: SessionId,
    pub processes: Mutex<Vec<ProcessId>>,
}

pub struct SessionManager {
    sessions: Mutex<BTreeMap<SessionId, Arc<Session>>>,
    groups: Mutex<BTreeMap<ProcessGroupId, Arc<ProcessGroup>>>,
    next_sid: AtomicU32,
    next_pgid: AtomicU32,
}

impl SessionManager {
    pub const fn new() -> Self {
        Self {
            sessions: Mutex::new(BTreeMap::new()),
            groups: Mutex::new(BTreeMap::new()),
            next_sid: AtomicU32::new(1),
            next_pgid: AtomicU32::new(1),
        }
    }
    
    pub fn create_session(&self, leader: ProcessId) -> Arc<Session> {
        let sid = self.next_sid.fetch_add(1, Ordering::SeqCst);
        
        let session = Arc::new(Session {
            sid,
            leader,
            processes: Mutex::new(vec![leader]),
            controlling_terminal: Mutex::new(None),
            foreground_group: Mutex::new(None),
        });
        
        self.sessions.lock().insert(sid, session.clone());
        session
    }
    
    pub fn create_process_group(&self, session: SessionId, leader: ProcessId) -> Arc<ProcessGroup> {
        let pgid = self.next_pgid.fetch_add(1, Ordering::SeqCst);
        
        let group = Arc::new(ProcessGroup {
            pgid,
            session,
            processes: Mutex::new(vec![leader]),
        });
        
        self.groups.lock().insert(pgid, group.clone());
        group
    }
    
    pub fn get_session(&self, sid: SessionId) -> Option<Arc<Session>> {
        self.sessions.lock().get(&sid).cloned()
    }
    
    pub fn get_process_group(&self, pgid: ProcessGroupId) -> Option<Arc<ProcessGroup>> {
        self.groups.lock().get(&pgid).cloned()
    }
}

pub static SESSION_MANAGER: SessionManager = SessionManager::new();
```

## 四、迁移计划

### 4.1 阶段一：基础设施 (1 周)

1. **配置 Rust 构建环境**
2. **创建进程核心类型**
3. **实现 C-Rust FFI 接口**

### 4.2 阶段二：进程管理 (1 周)

1. 实现 `Process` 结构
2. 实现 `MemorySpace` 管理
3. 实现进程创建/销毁

### 4.3 阶段三：调度器 (1 周)

1. 实现 `Scheduler` 结构
2. 实现调度算法
3. 集成上下文切换

### 4.4 阶段四：会话管理 (3 天)

1. 实现 `SessionManager`
2. 实现进程组管理

## 五、C-Rust 互操作

### 5.1 从 C 调用 Rust

```c
// src/include/proc_rust.h

#ifndef _PROC_RUST_H
#define _PROC_RUST_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

// 进程管理接口
uint32_t rust_proc_create(const char *name, uint32_t parent_pid);
void rust_proc_exit(uint32_t pid, uint32_t exit_code);
uint32_t rust_proc_get_current(void);
void rust_proc_yield(void);

// 调度器接口
void rust_sched_init(void);
void rust_sched_add(uint32_t pid);
void rust_sched_schedule(void);

// 会话管理接口
uint32_t rust_session_create(uint32_t leader_pid);
uint32_t rust_proc_group_create(uint32_t session_id, uint32_t leader_pid);

#ifdef __cplusplus
}
#endif

#endif // _PROC_RUST_H
```

### 5.2 Rust 导出函数

```rust
// src/proc/ffi.rs

use core::ffi::c_char;
use alloc::ffi::CString;

#[no_mangle]
pub extern "C" fn rust_proc_create(name: *const c_char, parent_pid: u32) -> u32 {
    let name_str = unsafe {
        CString::from_raw(name as *mut c_char)
            .to_string_lossy()
            .into_owned()
    };
    
    let parent = if parent_pid == 0 {
        None
    } else {
        Some(ProcessId(parent_pid))
    };
    
    let process = SCHEDULER.create_process(&name_str, parent);
    process.pid.0
}

#[no_mangle]
pub extern "C" fn rust_proc_exit(_pid: u32, exit_code: u32) {
    SCHEDULER.exit(exit_code);
}

#[no_mangle]
pub extern "C" fn rust_proc_get_current() -> u32 {
    SCHEDULER.current()
        .map(|p| p.pid.0)
        .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn rust_proc_yield() {
    SCHEDULER.yield_current();
}

#[no_mangle]
pub extern "C" fn rust_sched_add(pid: u32) {
    // Find process and add to ready queue
    let all = SCHEDULER.all_processes.lock();
    for process in all.iter() {
        if process.pid.0 == pid {
            SCHEDULER.add(process.clone());
            break;
        }
    }
}
```

## 六、测试策略

### 6.1 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_process_creation() {
        let process = SCHEDULER.create_process("test", None);
        assert_eq!(*process.state.lock(), ProcessState::Created);
    }
    
    #[test]
    fn test_scheduler_add() {
        let process = SCHEDULER.create_process("test", None);
        SCHEDULER.add(process.clone());
        
        let next = SCHEDULER.schedule();
        assert!(next.is_some());
        assert_eq!(next.unwrap().pid, process.pid);
    }
    
    #[test]
    fn test_process_state_transition() {
        let process = SCHEDULER.create_process("test", None);
        
        *process.state.lock() = ProcessState::Ready;
        assert_eq!(*process.state.lock(), ProcessState::Ready);
        
        *process.state.lock() = ProcessState::Running;
        assert_eq!(*process.state.lock(), ProcessState::Running);
        
        *process.state.lock() = ProcessState::Blocked(BlockReason::WaitingForIo);
        assert_eq!(*process.state.lock(), ProcessState::Blocked(BlockReason::WaitingForIo));
    }
}
```

## 七、预期收益

| 方面 | C 版本 | Rust 版本 |
|------|--------|-----------|
| 内存安全 | 手动管理，易出错 | 编译时保证 |
| 状态机 | 隐式，难追踪 | 枚举显式表达 |
| 资源管理 | 手动释放 | RAII 自动释放 |
| 并发安全 | 手动加锁 | 类型系统保证 |
| 错误处理 | 错误码 | Result<T, E> |

## 八、风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 上下文切换性能 | 高 | 保留汇编实现 |
| 内存分配器 | 高 | 统一内核分配器 |
| 调试困难 | 中 | 保留 C 版本参考 |
| 构建复杂度 | 中 | 使用 build.rs |

## 九、参考资源

- [Rust OSDev Community](https://rust-osdev.com/)
- [Writing an OS in Rust - Scheduling](https://os.phil-opp.com/async-await/)
- [Redox OS Process Management](https://gitlab.redox-os.org/redox-os/kernel)
- [Theseus OS Process](https://github.com/theseus-os/Theseus)
