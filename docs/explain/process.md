# 进程管理子系统

> 进程生命周期管理与调度

---

## 🎯 概述

AntX的进程管理子系统负责：
- 进程创建与销毁
- 进程调度
- 上下文切换
- 进程间通信

---

## 📦 进程控制块 (PCB)

```rust
pub struct Process {
    pub pid: u32,                    // 进程ID
    pub pwid: u64,                   // 特权工作负载ID
    pub state: ProcessState,         // 进程状态
    pub priority: u8,                // 优先级（0-255）
    pub time_slice: u64,             // 剩余时间片
    
    // 内存管理
    pub cr3: PhysAddr,               // 页表基址
    pub kernel_stack: VirtAddr,      // 内核栈
    pub user_stack: VirtAddr,        // 用户栈
    
    // CPU上下文
    pub context: CpuContext,         // 寄存器状态
    
    // 文件描述符
    pub fds: [Option<FileDescriptor>; MAX_FDS],
    
    // 统计信息
    pub cpu_time: u64,               // CPU时间
    pub start_time: u64,             // 启动时间
}

pub enum ProcessState {
    Ready,       // 就绪
    Running,     // 运行
    Blocked,     // 阻塞
    Zombie,      // 僵尸
}
```

---

## 🔄 调度器

### 调度策略

- **优先级调度**: 0最高，255最低
- **时间片轮转**: 同优先级进程轮转
- **抢占式**: 高优先级进程可抢占

### 调度器结构

```rust
pub struct Scheduler {
    run_queues: [Vec<Arc<Process>>; 256], // 优先级队列
    current: AtomicPtr<Process>,          // 当前进程
    timer_ticks: AtomicU64,               // 时钟滴答
}

impl Scheduler {
    /// 添加进程到就绪队列
    pub fn add_process(&mut self, process: Arc<Process>)
    
    /// 选择下一个运行的进程
    pub fn schedule(&mut self) -> Option<Arc<Process>>
    
    /// 时钟中断处理
    pub fn tick(&mut self)
}
```

---

## 🔀 上下文切换

### 保存上下文

```asm
; 保存当前进程上下文
save_context:
    push rax
    push rbx
    push rcx
    ; ... 保存所有寄存器
    mov [rdi + context_offset], rsp
    ret
```

### 恢复上下文

```asm
; 恢复目标进程上下文
restore_context:
    mov rsp, [rsi + context_offset]
    pop r15
    pop r14
    pop r13
    ; ... 恢复所有寄存器
    iretq
```

---

## 📊 进程生命周期

```
创建 → 就绪 ↔ 运行 → 僵尸 → 销毁
         ↓      ↓
         ← 阻塞 ←
```

---

**最后更新**: 2026-05-18
