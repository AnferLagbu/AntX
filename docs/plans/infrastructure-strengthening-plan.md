# QX 内核强基工程实施计划

**文档版本**: v1.0  
**创建日期**: 2026-05-03  
**状态**: 待审批  
**预计工期**: 8 周 (10-13 天 P0 + 20-27 天 P1 + 28-39 天 P2)

---

## 📋 文档目录

1. [执行摘要](#执行摘要)
2. [当前状态评估](#当前状态评估)
3. [强基工程优先级矩阵](#强基工程优先级矩阵)
4. [P0 级工程详细规划](#p0-级工程详细规划)
5. [P1 级工程详细规划](#p1-级工程详细规划)
6. [P2 级工程详细规划](#p2-级工程详细规划)
7. [8周实施路线图](#8周实施路线图)
8. [验收标准与测试方案](#验收标准与测试方案)
9. [风险管理与应对策略](#风险管理与应对策略)
10. [资源需求与依赖关系](#资源需求与依赖关系)
11. [附录：技术细节参考](#附录技术细节参考)

---

## 执行摘要

### 背景

QX 内核经过前期开发，已具备以下基础设施：

✅ **已就绪的核心能力**:
- 内存管理: kmalloc/PMM/VMM + 大页支持 (2MB/1GB)
- 进程调度: MLFQ 调度器 + 进程生命周期管理
- 文件系统: VFS 抽象层 + HvFS 混合架构 + 多种 FS 实现
- 中断系统: IDT/ISR/IRQ 完整框架 (Page Fault 已修复)
- CPU 驱动: AMD64 特性检测 + MSR 管理 + TSC 基准
- 多核支持: SMP 框架 (AP 启动/IPI/Barrier 同步)

❌ **关键缺失的基础设施**:
- **零并发保护** (Spinlock/Mutex/Atomic) → 多核不安全
- **零 Slab 分配器** → 小对象效率低下
- **零设备驱动框架** (PCI/DMA) → 无法扩展硬件支持
- **IPC 不完整** → 进程间通信受限

### 核心问题

> **当前内核在单核环境下可以稳定运行，但一旦启用 SMP 或尝试加载复杂子系统（如 lwIP、GUI、数据库），将面临严重的并发安全和性能问题。**

### 计划目标

通过 **8 周系统性强基工程建设**，使 QX 内核达到：

1. ✅ **SMP 安全**: 所有共享数据结构受并发原语保护
2. ✅ **高性能内存分配**: Slab Allocator 支持高效小对象缓存
3. ✅ **可扩展驱动模型**: PCI 总线 + DMA Engine + Driver Framework
4. ✅ **完整的 IPC**: Pipe/ShM/Semaphore/Message Queue 全部实现
5. ✅ **生产级稳定性**: OOM Handler + 安全框架 + 性能优化

### 投资回报预期

| 投入 | 产出 | ROI |
|------|------|-----|
| 10-13 天 (P0) | 内核不再崩溃，可安全使用多核 | ⭐⭐⭐⭐⭐ |
| 20-27 天 (P1) | 可加载网卡/GPU 驱动，支持 lwIP | ⭐⭐⭐⭐ |
| 28-39 天 (P2) | 生产级性能，可运行复杂应用 | ⭐⭐⭐ |

---

## 当前状态评估

### 内核成熟度评分 (2026-05-03)

| 子系统 | 成熟度 | 评分 | 关键缺陷 |
|--------|--------|------|---------|
| 内存管理 | ⚠️ 功能完整但线程不安全 | **7.5/10** | 无锁、无 Slab、无 OOM |
| 进程调度 | ✅ 相当成熟 | **8.5/10** | 缺实时性保证 |
| 文件系统 | ✅ 设计优秀但需缓存优化 | **7/10** | 无 dentry/inode cache |
| 中断系统 | ✅ 已修复致命问题 | **8/10** | 中断共享机制待完善 |
| CPU/SMP | ✅ 框架就绪但未验证 | **7/10** | 需真实硬件测试 |
| 设备驱动 | ❌ 几乎空白 | **2/10** | 无 PCI/DMA/Driver Model |
| 并发原语 | ❌ 完全缺失 | **0/10** | **致命缺陷!** |
| IPC | ⚠️ 接口定义但实现不全 | **3/10** | 仅消息队列基础 |
| 安全机制 | ⚠️ PWID 存在但不完整 | **4/10** | 缺 Capability/ACL |

**综合评分: 5.6/10 (及格线边缘)**

### 关键依赖链缺失分析

```
高层应用 (lwIP/GUI/DB)
    │
    ├── 需要: Slab Allocator ← ❌ 缺失
    │       │
    │       └── 需要: Spinlock ← ❌ 缺失 (根因!)
    │
    ├── 需要: IPC (Pipe/ShM) ← ⚠️ 不完整
    │       │
    │       └── 需要: Mutex ← ❌ 缺失
    │
    └── 需要: 设备驱动 (网卡/GPU) ← ❌ 缺失
            │
            ├── 需要: PCI Bus ← ❌ 缺失
            └── 需要: DMA Engine ← ❌ 缺失
```

**结论**: 并发原语 (Spinlock/Mutex) 是所有后续工作的**前置依赖**，必须最先解决。

---

## 强基工程优先级矩阵

### P0 级: 不解决就无法继续 (本周必做)

| # | 工程名称 | 当前状态 | 阻塞范围 | 工作量 | 累计时间 |
|---|---------|---------|---------|--------|----------|
| **P0-1** | **Spinlock + Atomic 操作库** | 0% ❌ | 所有共享状态、SMP、lwIP | 2-3天 | Day 1-3 |
| **P0-2** | **Mutex (睡眠锁)** | 0% ❌ | 用户同步、IPC、文件锁 | 3-4天 | Day 4-7 |
| **P0-3** | **Slab Allocator v1.0** | 0% ❌ | lwIP、文件系统、驱动对象 | 3-4天 | Day 8-11 |
| **P0-4** | **保护现有代码 (kmalloc/PMM/VMM)** | 需重构 | 系统稳定性 | 2天 | Day 12-13 |

**P0 总计: 10-13 天**

### P1 级: 影响功能完整性 (下周开始)

| # | 工程名称 | 当前状态 | 解锁能力 | 工作量 | 累计时间 |
|---|---------|---------|---------|--------|----------|
| **P1-1** | **PCI 总线驱动框架** | 0% ❌ | 网卡/GPU/USB/NVMe | 4-5天 | Week 3 |
| **P1-2** | **DMA Engine (一致性DMA)** | 0% ❌ | 高性能 I/O、网络、存储 | 3-4天 | Week 3-4 |
| **P1-3** | **中断共享机制完善** | 20% ⚠️ | 多设备同 IRQ、PCI MSI | 2-3天 | Week 4 |
| **P1-4** | **Driver Model (bus/device/driver)** | 0% ❌ | 动态驱动加载、热插拔 | 5-7天 | Week 4-5 |
| **P1-5** | **IPC 实现完善化** | 30% ⚠️ | Shell 管道、进程协作 | 4-5天 | Week 5 |
| **P1-6** | **OOM Handler** | 0% ❌ | 系统稳定性、长时间运行 | 2-3天 | Week 5-6 |

**P1 总计: 20-27 天**

### P2 级: 增强体验和性能 (本月完成)

| # | 工程名称 | 当前状态 | 价值提升 | 工作量 | 累计时间 |
|---|---------|---------|---------|--------|----------|
| **P2-1** | **Read-Write Lock** | 0% ❌ | 文件系统并发读性能 | 2-3天 | Week 6 |
| **P2-2** | **Workqueue (工作队列)** | 0% ❌ | 中断底半处理质量 | 3-4天 | Week 6-7 |
| **P2-3** | **Completion (完成量)** | 0% ❌ | 异步 I/O 同步原语 | 2天 | Week 7 |
| **P2-4** | **Timer Wheel (时间轮)** | 基础版 ⚠️ | 大量定时器场景优化 | 3-4天 | Week 7 |
| **P2-5** | **VFS Cache Layer** | 0% ❌ | 文件操作速度提升 10-100x | 5-7天 | Week 7-8 |
| **P2-6** | **Security Framework (Capability)** | PWID基础 ⚠️ | 权限控制、安全策略 | 5-7天 | Week 8 |
| **P2-7** | **Memory Cgroup-lite** | 0% ❌ | 进程内存限制、资源隔离 | 3-4天 | Week 8 |
| **P2-8** | **RCU (Read-Copy-Update)** | 0% ❌ | 无锁读取场景 (高级) | 5-7天 | Week 9+ |

**P2 总计: 28-39 天** (可选，按需实施)

---

## P0 级工程详细规划

### P0-1: Spinlock + Atomic 操作库

#### 目标
提供 x86_64 架构下的高效自旋锁和原子操作原语，作为所有并发保护的基石。

#### 技术规格

```c
/* ====== src/include/spinlock.h ====== */

/**
 * @brief 自旋锁结构体
 *
 * 使用 x86 的 xchg 或 lock bts 指令实现。
 */
typedef struct spinlock {
    volatile int locked;   /* 0=未锁定, 1=已锁定 */
#ifdef CONFIG_DEBUG_SPINLOCK
    const char *name;      /* 锁名称 (调试用) */
    void *owner;           /* 持有者 (CPU ID) */
    uint64_t acquire_time; /* 获取时间戳 */
#endif
} spinlock_t;

/* 初始化宏 */
#define SPINLOCK_INIT(name) \
    { .locked = 0 }

#define DEFINE_SPINLOCK(name) \
    spinlock_t name = SPINLOCK_INIT(name)

/* 核心接口 */
void spin_init(spinlock_t *lock);
void spin_lock(spinlock_t *lock);        /* 阻塞式获取 */
void spin_unlock(spinlock_t *lock);      /* 释放锁 */
int  spin_trylock(spinlock_t *lock);     /* 非阻塞尝试 */
int  spin_is_locked(spinlock_t *lock);   /* 查询状态 */

/* 调试接口 */
void spin_lock_debug(spinlock_t *lock, const char *file, int line);
#define spin_lock(l) spin_lock_debug(l, __FILE__, __LINE__)

/* ====== src/include/atomic.h ====== */

/* 原子类型 */
typedef struct {
    volatile int counter;
} atomic_t;

typedef struct {
    volatile long counter;
} atomic_long_t;

/* 原子读写 */
static inline int atomic_read(atomic_t *v);
static inline void atomic_set(atomic_t *v, int i);

/* 原子算术运算 */
static inline void atomic_inc(atomic_t *v);
static inline void atomic_dec(atomic_t *v);
static inline int atomic_add_return(int i, atomic_t *v);
static inline int atomic_sub_return(int i, atomic_t *v);

/* 原子位操作 */
static inline void atomic_or(int mask, atomic_t *v);
static inline void atomic_and(int mask, atomic_t *v);

/* CAS (Compare-And-Swap) */
static inline int atomic_cmpxchg(atomic_t *v, int old_val, int new_val);
static inline int atomic_try_add(atomic_t *v, int delta);

/* 内存屏障 */
static inline void barrier(void);
static inline void smp_mb(void);   /* Full memory barrier */
static inline void smp_rmb(void);  /* Read barrier */
static inline void smp_wmb(void);  /* Write barrier */

/* ====== src/include/rwlock.h ====== */

typedef struct rwlock {
    spinlock_t lock;        /* 保护内部状态 */
    int readers;            /* 当前读者数 */
    int writer;             /* 写者标志 (0/1) */
    int pending_writers;    /* 等待的写者数 */
} rwlock_t;

#define RWLOCK_INIT { .lock = SPINLOCK_INIT, .readers = 0, .writer = 0 }

void rwlock_init(rwlock_t *rw);
void read_lock(rwlock_t *rw);
void read_unlock(rwlock_t *rw);
void write_lock(rwlock_t *rw);
void write_unlock(rwlock_t *rw);
int  write_trylock(rwlock_t *rw);
```

#### 实现要点 (x86_64)

```asm
/* spin_lock 实现 (使用 xchg 指令) */
spin_lock:
    movl $1, %eax          # 要写入的值 (locked=1)
    xchgl %eax, (%rdi)     # 原子交换
    test %eax, %eax         # 检查旧值
    jz .L_acquired          # 如果旧值=0, 获取成功
.L_spin:
    pause                   # 提示 CPU 正在自旋 (降低功耗)
    movl $1, %eax
    xchgl %eax, (%rdi)
    test %eax, %eax
    jnz .L_spin             # 继续等待
.L_acquired:
    ret

/* atomic_cmpxchg 实现 (使用 lock cmpxchgl) */
atomic_cmpxchg:
    movl %esi, %eax         # old_val -> EAX
    lock cmpxchgl %edx, (%rdi)  # 比较 EAX 与 (*v), 相等则写入 new_val
    movl %eax, %eax         # 返回旧值
    ret
```

#### 测试用例

```c
/* test_spinlock.c */
static int test_spinlock_basic(void) {
    spinlock_t lock = SPINLOCK_INIT;
    
    TEST_ASSERT(!spin_is_locked(&lock));
    
    spin_lock(&lock);
    TEST_ASSERT(spin_is_locked(&lock));
    
    spin_unlock(&lock);
    TEST_ASSERT(!spin_is_locked(&lock));
    
    return TEST_PASS;
}

static int test_spinlock_trylock(void) {
    spinlock_t lock = SPINLOCK_INIT;
    
    spin_lock(&lock);
    TEST_ASSERT(spin_trylock(&lock) == 0);  /* 应该失败 */
    spin_unlock(&lock);
    TEST_ASSERT(spin_trylock(&lock) != 0);  /* 应该成功 */
    spin_unlock(&lock);
    
    return TEST_PASS;
}

/* test_atomic.c */
static int test_atomic_inc_dec(void) {
    atomic_t counter = ATOMIC_INIT(0);
    
    for (int i = 0; i < 1000; i++) {
        atomic_inc(&counter);
    }
    TEST_ASSERT_EQ(atomic_read(&counter), 1000);
    
    for (int i = 0; i < 500; i++) {
        atomic_dec(&counter);
    }
    TEST_ASSERT_EQ(atomic_read(&counter), 500);
    
    return TEST_PASS;
}
```

#### 交付物清单

- [ ] `src/include/spinlock.h` - 自旋锁接口定义
- [ ] `src/include/atomic.h` - 原子操作接口
- [ ] `src/include/rwlock.h` - 读写锁接口
- [ ] `src/kernel/spinlock.c` - 自旋锁实现
- [ ] `src/kernel/atomic.c` - 原子操作辅助函数
- [ ] `src/kernel/rwlock.c` - 读写锁实现
- [ ] `src/kernel/tests/test_spinlock.c` - 单元测试
- [ ] `src/kernel/tests/test_atomic.c` - 原子操作测试
- [ ] Makefile 更新（添加新文件编译）

#### 验收标准

- [ ] Spinlock 在单核下正确工作（基本获取/释放）
- [ ] Atomic 操作在多线程下结果正确（+/- 10000次无竞争）
- [ ] RWLock 允许多读者并行，互斥写者
- [ ] 所有测试在 `make test-quick` 中通过
- [ ] 无死锁情况发生

---

### P0-2: Mutex (睡眠锁)

#### 目标
提供基于等待队列的互斥锁，支持进程睡眠/唤醒语义，用于用户空间同步。

#### 技术规格

```c
/* ====== src/include/mutex.h ====== */

typedef struct mutex {
    spinlock_t wait_lock;     /* 保护等待队列 */
    struct task_struct *owner;/* 当前持有者 */
    struct list_head wait_queue; /* 等待此锁的进程列表 */
#ifdef CONFIG_DEBUG_MUTEX
    const char *name;
    int depth;                /* 嵌套深度 (可重入 mutex) */
#endif
} mutex_t;

#define MUTEX_INIT(name) \
    { .wait_lock = SPINLOCK_INIT, .owner = NULL }

#define DEFINE_MUTEX(name) \
    mutex_t name = MUTEX_INIT(name)

void mutex_init(mutex_t *mutex);
void mutex_lock(mutex_t *mutex);      /* 可能睡眠 */
void mutex_unlock(mutex_t *mutex);    /* 唤醒等待者 */
int  mutex_trylock(mutex_t *mutex);   /* 非阻塞版本 */
int  mutex_is_locked(mutex_t *mutex);

/* 可重入版本 (用于递归锁定场景) */
void mutex_lock_nested(mutex_t *mutex, unsigned int subclass);
```

#### 实现架构

```
mutex_lock() 流程:

1. 尝试快速路径 (trylock):
   if (owner == NULL) { owner = current; return; }  // O(1)

2. 慢速路径 (需要睡眠):
   a. spin_lock(&wait_lock);
   b. 将 current 加入 wait_queue;
   c. set_task_state(current, TASK_UNINTERRUPTIBLE);
   d. spin_unlock(&wait_lock);
   e. schedule();  // 让出 CPU

3. 被唤醒后重新检查:
   goto step 1 (再次尝试获取)
```

#### 与 Spinlock 的区别

| 特性 | Spinlock | Mutex |
|------|----------|-------|
| **适用上下文** | 中断处理、原子操作 | 进程上下文 |
| **阻塞行为** | 忙等 (CPU 循环) | 睡眠 (让出 CPU) |
| **持有时间** | 极短 (<1μs) | 较长 (可能 ms 级) |
| **可中断性** | 不可中断 | 可被信号中断 |
| **优先级反转** | 可能发生 | 需要优先级继承 (PI) |
| **典型用途** | 保护数据结构 | 保护临界区代码段 |

#### 交付物清单

- [ ] `src/include/mutex.h` - 互斥锁接口
- [ ] `src/kernel/mutex.c` - 互斥锁实现 (依赖 wait_queue)
- [ ] `src/kernel/tests/test_mutex.c` - 测试用例
- [ ] 文档更新

#### 验收标准

- [ ] Mutex 在单进程中正确序列化访问
- [ ] Mutex 支持嵌套锁定 (depth tracking)
- [ ] 优先级继承 (Priority Inheritance) 可选实现
- [ ] 无死锁、无饥饿现象

---

### P0-3: Slab Allocator v1.0

#### 目标
实现类似 Linux Slab 的高效小对象缓存分配器，专门针对频繁分配/释放的固定大小内核对象进行优化。

#### 架构设计

```
Slab Allocator 层次结构:

┌─────────────────────────────────────┐
│           用户 API 层               │
│  kmem_cache_create / alloc / free   │
├─────────────────────────────────────┤
│         Cache Manager 层            │
│  kmem_cache_t (per-object-type)     │
│  ├── Common Slab (通用缓存)         │
│  ├── DMA Slab (DMA 对齐缓存)        │
│  └── Specialized Slab (专用缓存)    │
├─────────────────────────────────────┤
│           Slab 层                   │
│  struct slab (连续物理页组)         │
│  ├── Full (已满)                    │
│  ├── Partial (部分使用)             │
│  └── Empty (空闲)                   │
├─────────────────────────────────────┤
│         Object 层                   │
│  固定大小对象的数组                 │
│  ├── 构造函数 (ctor)                │
│  └── 析构函数 (dtor)                │
└─────────────────────────────────────┘
         ↑ 底层依赖
    PMM (物理页) + Spinlock (并发保护)
```

#### 数据结构设计

```c
/* ====== src/include/slab.h ====== */

/* Slab 标志位 */
#define SLAB_HWCACHE_ALIGN   0x00002000UL  /* 缓存行对齐 */
#define SLAB_PANIC           0x00040000UL  /* 分配失败时 panic */
#define SLAB_RECLAIM_ACCOUNT 0x00020000UL  /* 记录到 slab 可回收 */

/* 对象状态 */
#define OBJECT_FREE    0  /* 空闲 */
#define OBJECT_ACTIVE  1  /* 已分配 */

typedef struct kmem_cache {
    /* Cache 元信息 */
    const char *name;              /* 名称 (如 "inode_cache") */
    size_t object_size;            /* 单个对象大小 */
    size_t align;                  /* 对齐要求 */
    size_t size;                   /* 实际大小 (含 coloring) */
    
    /* Slab 链表 */
    struct list_head slabs_full;    /* 完全使用的 slab */
    struct list_head slabs_partial; /* 部分使用的 slab */
    struct list_head slabs_free;    /* 完全空闲的 slab */
    
    unsigned int num;              /* 每个 slab 中的对象数 */
    unsigned int gfporder;         /* 分配的 2^gfporder 页 */
    
    /* 构造/析构函数 */
    void (*ctor)(void *obj);        /* 构造函数 (可选) */
    void (*dtor)(void *obj);        /* 析构函数 (可选) */
    
    /* 统计信息 */
    atomic_t active_objs;          /* 活跃对象数 */
    atomic_t num_objs;             /* 总对象数 */
    
    /* Per-CPU 缓存 (减少锁竞争) */
    struct array_cache **cpu_cache; /* 每个CPU一个本地缓存 */
    unsigned int batchcount;        /* 批量填充/回收数量 */
    
    /* 并发保护 */
    spinlock_t lock;
    
    /* DMA 相关 */
    int is_dma;                     /* 是否 DMA 兼容 */
} kmem_cache_t;

/* Slab 结构 (一组连续物理页) */
typedef struct slab {
    struct list_head list;          /* 链接到 cache 的 slabs_* */
    kmem_cache_t *cache;            /* 所属 cache */
    void *s_mem;                    /* 第一个对象的起始地址 */
    unsigned int inuse;             /* 已使用对象数 */
    unsigned int free;              /* 下一个空闲对象索引 */
} slab_t;

/* ====== 核心 API ====== */

/* 创建/销毁 Cache */
kmem_cache_t* kmem_cache_create(const char *name, size_t size,
                                 size_t align, 
                                 void (*ctor)(void*), void (*dtor)(void*));
void kmem_cache_destroy(kmem_cache_t *cache);

/* 分配/释放对象 */
void* kmem_cache_alloc(kmem_cache_t *cache);
void* kmem_cache_zalloc(kmem_cache_t *cache);  /* Zeroed alloc */
void  kmem_cache_free(kmem_cache_t *cache, void *obj);

/* 通用 Slab (预定义常用大小的 caches) */
extern kmem_cache_t *size_cache[16];  /* 16B, 32B, ..., 128KB */

void* slab_malloc(size_t size);
void  slab_free(void *ptr);

/* DMA Slab */
kmem_cache_t* kmem_cache_create_dma(const char *name, size_t size);
void* dma_alloc_coherent(size_t size, dma_addr_t *dma_handle);
void  dma_free_coherent(size_t size, void *vaddr, dma_addr_t dma_handle);
```

#### 性能目标

| 对象大小 | 目标分配延迟 | 目标吞吐量 | 对比 kmalloc 提升 |
|---------|------------|-----------|----------------|
| 32B (dentry) | <100ns | >10M ops/s | **10-50x** |
| 64B (inode) | <120ns | >8M ops/s | **15-30x** |
| 256B (file) | <150ns | >5M ops/s | **8-20x** |
| 1KB (skb) | <300ns | >3M ops/s | **5-10x** |

#### 交付物清单

- [ ] `src/include/slab.h` - Slab 接口定义
- [ ] `src/mm/slab.c` - Slab Allocator 实现 (~800 行)
- [ ] `src/mm/slab_dma.c` - DMA Slab 变体
- [ ] `src/mm/tests/test_slab.c` - 性能基准测试
- [ ] Makefile 更新

#### 验收标准

- [ ] 创建 inode_cache (64B) 并分配/释放 10000 次，无泄漏
- [ ] Per-CPU 缓存生效 (多核下锁竞争降低 90%)
- [ ] 支持 ctor/dtor 回调
- [ ] DMA Slab 返回的地址满足 DMA 对齐要求
- [ ] 内存碎片率 < 5%

---

### P0-4: 保护现有代码 (kmalloc/PMM/VMM)

#### 目标
将新实现的 Spinlock 和 Slab 集成到现有的内存管理子系统中，使其线程安全。

#### 重构清单

| 文件 | 需添加的保护 | 锁类型 | 影响范围 |
|------|------------|--------|---------|
| `src/mm/kmalloc.c` | `free_list`, `heap_allocated`, `heap_current` | Spinlock | 所有动态内存分配 |
| `src/mm/pmm.c` | `bitmap[]`, `mem_info` | Spinlock | 物理页分配 |
| `src/mm/vmm.c` | `kernel_pml4` 及页表修改 | Spinlock | 地址映射 |
| `src/proc/process.c` | `process_table[]`, `pid_bitmap` | Spinlock/RWLock | 进程创建/销毁 |
| `src/proc/scheduler.c` | `run_queues[]`, `current` | Spinlock | 调度决策 |
| `src/fs/vfs/vfs.rs` | superblock list, inode cache | RWLock | 文件系统操作 |

#### 示例改造 (kmalloc.c)

```c
// 改造前:
void* kmalloc(uint64_t size) {
    // ... 直接操作全局变量 ...
    free_list->free = 0;  // ❌ 竞争条件!
}

// 改造后:
static DEFINE_SPINLOCK(kmalloc_lock);

void* kmalloc(uint64_t size) {
    spin_lock(&kmalloc_lock);  // 🔒 加锁
    
    // ... 安全地操作全局变量 ...
    free_list->free = 0;  // ✅ 安全
    
    spin_unlock(&kmalloc_lock);  // 🔓 解锁
    
    return result;
}
```

#### 交付物清单

- [ ] 所有 `.c` 文件的并发保护补丁
- [ ] 回归测试套件更新
- [ ] 性能基准对比报告 (加锁前 vs 加锁后)

#### 验收标准

- [ ] `make test-comprehensive` 全部通过 (41/42 测试)
- [ ] SMP 模式下 (`make test-cpu-host`) 无崩溃
- [ ] 性能下降 < 15% (锁开销可控)
- [ ] 无死锁检测 (使用 lockdep 工具或手动审查)

---

## P1 级工程详细规划

### P1-1: PCI 总线驱动框架

#### 目标
实现 PCI 总线枚举、配置空间访问、BAR 映射和中断路由，为网卡/GPU/USB 等设备提供硬件抽象层。

#### 核心功能

```c
/* ====== src/include/pci.h ====== */

/* PCI 配置空间寄存器偏移 */
#define PCI_VENDOR_ID       0x00
#define PCI_DEVICE_ID       0x02
#define PCI_COMMAND         0x04
#define PCI_STATUS          0x06
#define PCI_CLASS_REVISION  0x08
#define PCI_BAR0            0x10  /* Base Address Register 0 */
#define PCI_INTERRUPT_LINE  0x3C
#define PCI_CAPABILITY_LIST 0x34

/* PCI 命令寄存器位 */
#define PCI_COMMAND_IO      0x01
#define PCI_COMMAND_MEMORY  0x02
#define PCI_COMMAND_MASTER  0x04

struct pci_device_id {
    uint32_t vendor, device;
    uint32_t subvendor, subdevice;
    uint32_t class_code, class_mask;
    unsigned long driver_data;
};

struct pci_driver {
    struct list_head node;
    const char *name;
    const struct pci_device_id *id_table;
    int (*probe)(struct pci_dev *dev);
    void (*remove)(struct pci_dev *dev);
};

struct pci_dev {
    struct list_head bus_list;
    struct pci_driver *driver;
    
    uint8_t  bus, devfn;       /* 总线号, 设备/功能号 */
    uint16_t vendor, device;   /* 厂商/设备ID */
    uint8_t  irq;              /* 中断线 */
    
    struct resource bar[6];    /* BAR 资源 */
    uint8_t  msi_cap;          /* MSI 能力偏移 */
    uint8_t  msix_cap;         /* MSI-X 能力偏移 */
    
    void *driver_data;         /* 驱动私有数据 */
};

/* PCI API */
void pci_init(void);                              /* 扫描所有 PCI 总线 */
int pci_register_driver(struct pci_driver *drv);   /* 注册驱动 */
void pci_unregister_driver(struct pci_driver *drv);
uint32_t pci_read_config_dword(struct pci_dev *dev, int where);
void pci_write_config_dword(struct pci_dev *dev, int where, uint32_t val);
int pci_enable_device(struct pci_dev *dev);        /* 使能设备 (IO/MEM/IRQ) */
void pci_disable_device(struct pci_dev *dev);
int pci_request_region(struct pci_dev *dev, int bar, const char *name);
void pci_release_region(structpci_dev *dev, int bar);
void* pci_iomap(struct pci_dev *dev, int bar, unsigned long maxlen);
void pci_iounmap(struct pci_dev *dev, void *addr);
```

#### 实现步骤

1. **PCI 配置空间访问** (I/O端口 0xCF8/0xCFC 或 MMCONFIG)
2. **总线枚举** (遍历 Bus 0, Device 0-31, Function 0-7)
3. **BAR 解析** (识别 IO/MEM 类型, 大小, 可预取)
4. **中断映射** (INTx → IRQ, MSI, MSI-X)
5. **驱动匹配** (根据 Vendor/Device/Class 匹配)

#### 交付物

- [ ] `src/include/pci.h` - PCI 接口
- [ ] `src/drivers/pci/pci.c` - PCI 核心实现
- [ ] `src/drivers/pci/pci_ids.h` - 已知设备ID表
- [ ] 测试工具: `lspci` 命令

---

## 8周实施路线图

### Week 1: 并发基础 (Day 1-5)

```
Day 1 (Mon): Spinlock v1.0
  ☐ 创建 spinlock.h/atomic.h/rwlock.h
  ☐ 实现 x86_64 汇编原语 (spin_lock/unlock, atomic_*)
  ☐ 编写单元测试 (test_spinlock.c, test_atomic.c)
  ☐ 验证: make test 通过

Day 2 (Tue): Spinlock 调试增强
  ☐ 添加死锁检测 (lock ordering validation)
  ☐ 添加 ownership tracking
  ☐ 性能基准测试 (lock/unlock 延迟测量)
  ☐ 集成到现有简单模块 (如 serial.c)

Day 3 (Wed): Mutex 实现
  ☐ 设计 wait_queue 基础结构
  ☐ 实现 mutex_lock/unlock (sleep/wake)
  ☐ 处理嵌套锁定和优先级继承 (PI)
  ☐ 编写 test_mutex.c

Day 4 (Thu): Mutex 测试与集成
  ☐ 多进程互斥测试
  ☐ 死锁场景测试 (ABBA)
  ☐ 饥饿测试 (writer starvation)
  ☐ 文档编写

Day 5 (Fri): 回顾与调整
  ☐ Code Review 所有并发原语
  ☐ 性能回归测试 (确保无显著退化)
  ☐ 更新 Makefile 和文档
  ☐ Week 1 总结会议
```

### Week 2: 内存管理强化 (Day 6-10)

```
Day 6-8 (Mon-Wed): Slab Allocator
  ☐ 实现 kmem_cache_create/destroy
  ☐ 实现 kmem_cache_alloc/free (slab 管理算法)
  ☐ 实现 Per-CPU 本地缓存
  ☐ 实现 ctor/dtor 回调
  ☐ 编写 test_slab.c (压力测试 + 泄漏检测)

Day 9 (Thu): Slab DMA 变体
  ☐ 实现 DMA Slab (对齐约束)
  ☐ dma_alloc_coherent/free
  ☐ 与 PCI BAR 集成准备

Day 10 (Fri): 保护现有代码
  ☐ 为 kmalloc.c 添加 spinlock
  ☐ 为 pmm.c 添加 bitmap lock
  ☐ 为 vmm.c 添加 page_table_lock
  ☐ 回归测试: make test-comprehensive
```

### Week 3-4: 设备驱动基础设施

```
Week 3:
  ☐ PCI 总线驱动 (pci.c)
  ☐ DMA Engine (dma.c)
  ☐ 中断共享机制完善
  
Week 4:
  ☐ Driver Model (bus/device/driver 三元组)
  ☐ IPC 完善 (pipe/shm/semaphore)
  ☐ OOM Handler
```

### Week 5-8: 性能与体验

```
Week 5-6:
  ☐ Read-Write Lock
  ☐ Workqueue
  ☐ Timer Wheel
  ☐ Completion

Week 7-8:
  ☐ VFS Cache Layer
  ☐ Security Framework
  ☐ Memory Cgroup
  ☐ 最终集成测试与调优
```

---

## 验收标准与测试方案

### P0 验收 checklist

- [ ] **Spinlock**: 
  - [ ] 单核正确性: 10000 次 lock/unlock 无异常
  - [ ] trylock 语义正确
  - [ ] 无死锁 (lockdep 检测通过)
  - [ ] 性能: lock 延迟 < 50ns (uncontended)

- [ ] **Mutex**:
  - [ ] 进程间互斥正确
  - [ ] sleep/wake 语义正确
  - [ ] 支持信号中断
  - [ ] 无饥饿 (fairness 保证)

- [ ] **Slab**:
  - [ ] 创建 10 个不同大小的 cache
  - [ ] 每个分配/释放 10000 次
  - [ ] 内存泄漏检测: 0 bytes leaked
  - [ ] 性能: 比 kmalloc 快 10x+

- [ ] **现有代码保护**:
  - [ ] 所有测试通过 (41/42)
  - [ ] SMP 模式稳定运行 1h+
  - [ ] 性能下降 < 15%

### 自动化测试命令

```bash
# P0 阶段验证
make test-spinlock          # Spinlock 单元测试
make test-mutex             # Mutex 单元测试
make test-slab              # Slab 性能测试
make test-concurrency       # 并发压力测试 (SMP)
make test-regression        # 回归测试 (确保无破坏)

# P1 阶段验证
make test-pci               # PCI 枚举测试
make test-dma               # DMA 一致性测试
make test-ipc               # IPC 完整性测试
make test-driver-model      # 驱动模型测试

# 综合验证
make test-comprehensive     # 全量测试 (必须全绿!)
make test-cpu-host          # 真实硬件多核测试
make test-stress            # 长时间压力测试 (1h+)
```

---

## 风险管理与应对策略

### 高风险项

| 风险 | 概率 | 影响 | 缓解措施 |
|------|------|------|---------|
| **Spinlock 实现错误导致死锁** | 中 | 致命 | 使用 lockdep 工具 + 代码审查 |
| **Slab 碎片严重** | 低 | 性能 | 实现 slab 合并/reclaim 机制 |
| **Mutex 优先级反转** | 中 | 实时性 | 实现 Priority Inheritance (PI) |
| **PCI 驱动兼容性问题** | 高 | 功能受限 | 先支持 e1000 (Intel) + RTL8139 (Realtek) |
| **OOM Handler 过于激进** | 中 | 用户体验差 | 可配置阈值 + 日志记录 |

### 应急预案

如果某项 P0 工程延期超过 2 天:

1. **降级方案**: 使用简化版 (如去掉 PI 的 Mutex)
2. **替代方案**: 临时禁用 SMP (回退到单核模式)
3. **并行加速**: 增加人手或延长工作时间

---

## 资源需求与依赖关系

### 外部依赖

| 依赖项 | 版本要求 | 用途 | 获取方式 |
|--------|---------|------|---------|
| QEMU | >= 6.0 | 虚拟化测试环境 | 系统包管理器 |
| GCC | >= 10.0 | C11/C17 支持 | 系统包管理器 |
| NASM/YASM | >= 2.0 | 汇编代码编译 | 系统包管理器 |
| 多核 CPU | >= 2 cores | SMP 测试 | 物理机或 -smp N 参数 |

### 内部依赖关系图

```
P0-1 (Spinlock) ──→ P0-2 (Mutex) ──→ P1-5 (IPC)
       │                                   │
       ├─→ P0-3 (Slab) ──→ P1-2 (DMA)    │
       │         │                         │
       └─→ P0-4 (保护代码) ──→ P1-1 (PCI)│
                                      │    │
P0 全部完成 ──────────────────────────┘    │
                                           ↓
                                    后续所有工作
```

---

## 附录: 技术细节参考

### A. x86_64 内存屏障指令

```asm
/* Full barrier (compiler + CPU) */
mfence  /* StoreLoad barrier (最强) */
sfence  /* StoreStore barrier */
lfence  /* LoadLoad barrier */

/* Compiler barrier only (no CPU instruction) */
/* Used in: barrier(), smp_mb(), etc. */
__asm__ volatile("" ::: "memory");
```

### B. Spinlock 性能优化技巧

1. **Pause 指令**: 在自旋循环中插入 `pause` 降低功耗
2. **Exponential Backoff**: 失败后增加等待时间
3. **MCS Lock**: 减少缓存一致性流量 (高级)
4. **Ticket Lock**: 保证 FIFO 公平性

### C. Slab Coloring 技术

为防止 false sharing，Slab 中每个对象起始地址添加随机偏移 (coloring):

```
Slab Layout (with coloring):

[Object 0] [pad] [Object 1] [pad] [Object 2]
^--color=0       ^--color=1       ^--color=2
```

### D. 参考 Linux 实现

- `include/linux/spinlock.h`
- `kernel/locking/spinlock.c`
- `mm/slab.c` 或 `mm/slub.c`
- `drivers/pci/pci.c`

---

## 文档维护记录

| 版本 | 日期 | 作者 | 变更说明 |
|------|------|------|---------|
| v1.0 | 2026-05-03 | AI Assistant | 初始版本创建 |

---

**下一步行动**: 审批本计划后，立即开始 **P0-1: Spinlock + Atomic** 实施。
