# QueenX 进程与线程机制

> 本文档描述 QueenX 内核的进程与线程实现，包括多级反馈队列调度器。

## 一、设计概述

QueenX 采用现代操作系统设计理念，将**进程**作为资源分配的基本单位，将**线程**作为调度的基本单位。

### 1.1 核心概念

| 概念 | 说明 |
|------|------|
| **进程 (Process)** | 资源分配的基本单位，包含地址空间、文件描述符、权限等 |
| **线程 (Thread)** | 调度的基本单位，包含执行上下文、栈、优先级等 |
| **任务 (Task)** | 进程或线程的统称 |

### 1.2 架构图

```
┌─────────────────────────────────────────────────────────────┐
│                        进程 (Process)                        │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                    地址空间 (CR3)                     │   │
│  │  • 代码段  • 数据段  • 堆  • 栈                        │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                    资源                              │   │
│  │  • 文件描述符表  • PWID  • 工作目录                   │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐                   │
│  │ 线程 1   │ │ 线程 2   │ │ 线程 3   │  ...              │
│  │ (主线程) │ │          │ │          │                   │
│  └──────────┘ └──────────┘ └──────────┘                   │
└─────────────────────────────────────────────────────────────┘
```

## 二、数据结构

### 2.1 线程结构体 (`struct thread`)

```c
struct thread {
    tid_t tid;                    // 线程 ID
    pid_t pid;                    // 所属进程 ID
    
    enum thread_state state;      // 线程状态
    enum thread_priority priority;// 优先级
    enum block_reason block_reason;// 阻塞原因
    
    uint64_t kernel_stack;        // 内核栈
    uint64_t user_stack;          // 用户栈
    
    struct cpu_context context;   // CPU 上下文
    
    uint64_t cpu_time;            // CPU 时间
    int32_t time_slice;           // 剩余时间片
    
    void *tls_base;               // 线程本地存储
    void *entry_point;            // 入口点
    void *entry_arg;              // 入口参数
    
    struct thread *next;          // 调度队列链表
    struct thread *prev;
    struct thread *process_next;  // 进程线程链表
};
```

### 2.2 进程结构体 (`struct process`)

```c
struct process {
    pid_t pid;                    // 进程 ID
    pid_t parent_pid;             // 父进程 ID
    uint64_t pwid;                // PWID 权限标识
    
    char name[64];                // 进程名
    
    uint64_t cr3;                 // 页表基址
    
    struct thread *main_thread;   // 主线程
    struct thread *thread_list;   // 线程链表
    uint32_t thread_count;        // 线程数
    
    struct process *parent;       // 父进程
    struct process *children;     // 子进程链表
    struct process *sibling;      // 兄弟进程链表
    
    int exit_code;                // 退出码
    
    char cwd[256];                // 当前工作目录
    char root[256];               // 根目录
    
    uint64_t umask;               // 文件权限掩码
    
    struct {
        int stdin_fd;             // 标准输入
        int stdout_fd;            // 标准输出
        int stderr_fd;            // 标准错误
    } stdio;
};
```

## 三、线程状态

### 3.1 状态转换图

```
                    ┌─────────────┐
                    │   CREATED   │
                    └──────┬──────┘
                           │ thread_create()
                           ▼
                    ┌─────────────┐
           ┌───────│    READY    │◄───────┐
           │       └──────┬──────┘        │
           │              │ schedule()    │
           │              ▼               │
           │       ┌─────────────┐        │
           │       │   RUNNING   │────────┘
           │       └──────┬──────┘  time_slice=0
           │              │
           │              │ block()
           │              ▼
           │       ┌─────────────┐
           │       │   BLOCKED   │
           │       └──────┬──────┘
           │              │ unblock()
           │              ▼
           └──────►┌─────────────┐
                   │   READY     │
                   └─────────────┘
                           │
                           │ exit()
                           ▼
                    ┌─────────────┐
                    │   ZOMBIE    │
                    └─────────────┘
```

### 3.2 状态说明

| 状态 | 说明 |
|------|------|
| `THREAD_CREATED` | 线程刚创建，未初始化 |
| `THREAD_READY` | 就绪状态，等待调度 |
| `THREAD_RUNNING` | 正在运行 |
| `THREAD_BLOCKED` | 阻塞状态，等待事件 |
| `THREAD_ZOMBIE` | 僵尸状态，等待父进程回收 |

## 四、调度器

### 4.1 多级反馈队列调度 (MLFQ)

QueenX 采用**多级反馈队列调度算法**，提供良好的响应时间和吞吐量平衡。

```
┌─────────────────────────────────────────────────────────────┐
│                    调度器优先级队列                          │
├─────────────────────────────────────────────────────────────┤
│  Level 0 (最高优先级) - 时间片: 2ms                          │
│  ┌─────┬─────┬─────┬─────┐                                  │
│  │ T1  │ T2  │ T3  │ ... │  ← 实时/高优先级任务             │
│  └─────┴─────┴─────┴─────┘                                  │
├─────────────────────────────────────────────────────────────┤
│  Level 1 - 时间片: 4ms                                       │
│  ┌─────┬─────┬─────┬─────┐                                  │
│  │ T4  │ T5  │ ... │     │  ← 高优先级任务                   │
│  └─────┴─────┴─────┴─────┘                                  │
├─────────────────────────────────────────────────────────────┤
│  Level 2 - 时间片: 8ms                                       │
│  ┌─────┬─────┬─────┬─────┐                                  │
│  │ T6  │ T7  │ ... │     │  ← 普通任务                       │
│  └─────┴─────┴─────┴─────┘                                  │
├─────────────────────────────────────────────────────────────┤
│  Level 3 (最低优先级) - 时间片: 16ms                         │
│  ┌─────┬─────┬─────┬─────┐                                  │
│  │ T8  │ T9  │ ... │     │  ← 后台任务                       │
│  └─────┴─────┴─────┴─────┘                                  │
└─────────────────────────────────────────────────────────────┘
```

### 4.2 调度规则

1. **优先级规则**：高优先级队列中的线程先运行
2. **时间片规则**：每个队列有不同的时间片，低优先级队列时间片更长
3. **降级规则**：用完时间片的线程降级到下一级队列
4. **提升规则**：定期将所有线程提升到最高优先级（防止饥饿）

### 4.3 优先级定义

```c
enum thread_priority {
    PRIORITY_IDLE = 0,      // 空闲线程
    PRIORITY_LOW = 8,       // 低优先级
    PRIORITY_NORMAL = 16,   // 普通优先级
    PRIORITY_HIGH = 24,     // 高优先级
    PRIORITY_REALTIME = 31  // 实时优先级
};
```

## 五、等待队列

### 5.1 数据结构

```c
struct wait_queue {
    struct thread *head;     // 等待线程链表头
    uint32_t count;          // 等待线程数
};
```

### 5.2 操作函数

| 函数 | 说明 |
|------|------|
| `wait_queue_init()` | 初始化等待队列 |
| `wait_queue_add()` | 将线程加入等待队列 |
| `wait_queue_wake_one()` | 唤醒一个等待线程 |
| `wait_queue_wake_all()` | 唤醒所有等待线程 |
| `wait_queue_wait()` | 当前线程等待 |

## 六、API 参考

### 6.1 线程管理

```c
// 创建线程
struct thread *thread_create(pid_t pid, void (*entry)(void *), void *arg, 
                             enum thread_priority priority);

// 创建用户态线程
struct thread *thread_create_user(pid_t pid, uint64_t entry, 
                                  uint64_t stack_top, 
                                  enum thread_priority priority);

// 线程退出
void thread_exit(struct thread *thread, int exit_code);

// 线程阻塞/唤醒
void thread_block(struct thread *thread, enum block_reason reason);
void thread_unblock(struct thread *thread);

// 线程让出 CPU
void thread_yield(void);

// 线程睡眠
void thread_sleep(uint64_t ms);

// 获取当前线程
struct thread *thread_get_current(void);
```

### 6.2 进程管理

```c
// 创建进程
struct process *process_create_ex(const char *name, pid_t parent_pid, 
                                  uint64_t pwid);

// 进程退出
void process_exit_ex(struct process *proc, int exit_code);

// 获取进程
struct process *process_get_by_pid(pid_t pid);
pid_t process_get_current_pid(void);
```

### 6.3 调度器

```c
// 初始化调度器
void scheduler_init_ex(void);

// 添加/移除线程
void scheduler_add_thread(struct thread *thread);
void scheduler_remove_thread(struct thread *thread);

// 调度
void scheduler_tick_ex(void);      // 时钟中断调用
void scheduler_schedule_ex(void);  // 执行调度
void scheduler_yield_ex(void);     // 主动让出

// 优先级管理
int scheduler_set_thread_priority(tid_t tid, enum thread_priority priority);
enum thread_priority scheduler_get_thread_priority(tid_t tid);

// 调试
void scheduler_dump_state(void);
```

## 七、文件位置

| 文件 | 说明 |
|------|------|
| `src/include/thread.h` | 线程/进程数据结构定义 |
| `src/include/scheduler_ex.h` | 调度器接口定义 |
| `src/proc/thread.c` | 线程/进程管理实现 |
| `src/proc/scheduler_ex.c` | 调度器实现 |

---

*最后更新: 2026-04-19*
