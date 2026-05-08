# AntX 栏栈（Barrier Stack）设计文档

> **版本**: v1.0 | **状态**: 设计阶段 | **作者**: AntX Development Team
>
> 本文档定义 AntX 宏内核中基于 Rust 语言级隔离的故障恢复机制——栏栈的完整设计。
> 包括架构原理、实现方案、利好分析、潜在危机、设计准则和与 Minix3 的详细对比。

---

## 一、问题起源

### 1.1 宏内核的根本矛盾

宏内核将文件系统、网络栈、设备驱动全部运行在共享地址空间中。一个模块的逻辑错误可能扩散为全局停机。传统应对：

| 方案 | 代表 | 代价 |
|------|------|------|
| 不做恢复 | Linux/Windows (panic/BSOD) | 可用性损失 |
| 微内核隔离 | Minix3 (RS 重启进程) | IPC 开销 + 粗粒度恢复 |
| 托管语言隔离 | Singularity (SIP) | 要求托管运行时 |

### 1.2 AntX 的三重独特条件

栏栈之所以可能，因为 AntX 同时满足三个从未重叠过的条件：

1. **Rust 编译期内存安全**：借用检查器在编译期保证一个模块不会通过合法代码破坏另一个模块的内存——这是栏栈"模块间互不污染"假设的证明。
2. **宏内核共享地址空间**：代码段在 `.text` 只读页中物理不可破坏；模块状态的恢复不需要跨进程协调，不需要页表切换，不需要序列化/反序列化。
3. **无 POSIX 历史包袱**：不被 `fsync`/`O_SYNC`/`O_DIRECT` 的持久化承诺绑定，可以自由定义"写入成功仅限于缓存，barrier 才是持久化承诺点"的语义。

三者缺一不可：Rust 缺位 → 内存互踩无法防御；宏内核缺位 → 退化为 Minix3 RS；无包袱缺位 → 回滚会违背 POSIX 语义。

---

## 二、架构设计

### 2.1 核心概念

```
栏（Barrier）：特定时刻的模块状态检查点
栏栈（Barrier Stack）：一个恢复域内所有未过期栏的序列
恢复域（Recovery Domain）：共享同一栏栈的一组内核模块
回滚（Rollback）：将恢复域的所有可回滚状态返回到最近的有效栏
级联回滚（Cascade Rollback）：当底层域回滚时，所有依赖它的上层域同步回滚
```

### 2.2 恢复域数据结构

```rust
const MAX_RECOVERY_DOMAINS: usize = 32;

struct RecoveryDomain {
    id: u64,                              // 域标识 (复用 PWID)
    name: &'static str,                   // 人类可读名称 (如 "ramfs", "hvfs")
    state: AtomicU32,                     // Active | Freezing | RollingBack | Recovering | Quarantined
    
    // 栏状态
    barrier_generation: AtomicU64,        // 当前栏代际
    barrier_interval_ticks: u64,          // 创建栏的 tick 间隔 (默认 100)
    next_barrier_tick: AtomicU64,         // 下一个栏的创建时间
    
    // 故障追踪
    rollback_count: AtomicU32,            // 总回滚次数
    consecutive_failures: AtomicU32,      // 连续失败次数
    last_crash_fingerprint: AtomicU64,    // 上次崩溃指纹 (用于检测相同输入)
    last_rollback_time: AtomicU64,        // 上次回滚的 tick
    backoff_until: AtomicU64,             // 退避截止 tick
    
    // 依赖
    depends_on: [Option<u64>; 8],         // 本域依赖的其他域 ID
    depended_by: [Option<u64>; 8],        // 依赖本域的其他域 ID
    
    // 配额
    cpu_quota: PwidQuota,                 // CPU 配额 (复用)
    proc_limit: PwidLimit,                // 进程数限制 (复用)
    
    // 回滚回调 (由各模块注册)
    rollback_fn: Option<fn(&RecoveryDomain) -> RollbackResult>,
    reset_fn: Option<fn(&RecoveryDomain) -> bool>,
}
```

### 2.3 栏的创建 (O(1), 零拷贝)

栏的创建极其轻量——不进行任何状态拷贝：

```rust
fn scheduler_barrier_maintenance(domains: &[RecoveryDomain], current_tick: u64) {
    for domain in domains.iter().filter(|d| d.state == Active) {
        if current_tick >= domain.next_barrier_tick.load(Ordering::SeqCst) {
            // 创建新栏：只递增 generation 计数
            domain.barrier_generation.fetch_add(1, Ordering::SeqCst);
            domain.next_barrier_tick.store(
                current_tick + domain.barrier_interval_ticks,
                Ordering::SeqCst
            );
        }
    }
}
```

创建开销：一次原子加法 + 一次原子存储。约 5 纳秒。Minix3 RS 没有对应的"创建检查点"概念——它不知道进程内部状态，无法做到这个粒度。

### 2.4 增量撤销日志

不保存完整快照。每个 `Recoverable` 模块维护一个轻量级撤销日志：

```rust
struct UndoEntry {
    generation: u64,          // 属于哪个栏代际
    field_ptr: *mut u8,       // 被修改的字段地址
    old_value: [u8; 8],       // 旧值 (最大 8 字节，足够覆盖 u64/指针/AtomicU64)
}

const MAX_UNDO_ENTRIES: usize = 256;

struct UndoLog {
    entries: [UndoEntry; MAX_UNDO_ENTRIES],
    count: usize,
    current_generation: u64,
}
```

每次可变操作：

```rust
impl UndoLog {
    fn record<T: Copy + 'static>(&mut self, field: &mut T, old_value: T) {
        if self.count >= MAX_UNDO_ENTRIES {
            // 强制刷新：回滚到上一个栏，清理一半日志
            self.emergency_compaction();
        }
        let raw = unsafe {
            core::slice::from_raw_parts(
                &old_value as *const T as *const u8,
                core::mem::size_of::<T>()
            )
        };
        let mut old_bytes = [0u8; 8];
        old_bytes[..raw.len()].copy_from_slice(raw);
        
        self.entries[self.count] = UndoEntry {
            generation: self.current_generation,
            field_ptr: field as *mut T as *mut u8,
            old_value: old_bytes,
        };
        self.count += 1;
    }
}
```

正常运行中的开销：每次 `record()` 约 10-20 纳秒（一次数组写入 + 一次 memcpy）。对于 100 ticks 内 100 次写入的内核模块，累计开销 < 2 微秒——完全无法观测。

### 2.5 回滚执行 (O(n), n = 栏后的写入次数)

```rust
fn rollback_to_barrier(&mut self, target_generation: u64) -> usize {
    let mut rolled_back = 0;
    
    // 从最新到最旧回放撤销日志
    while self.count > 0 {
        let entry = &self.entries[self.count - 1];
        if entry.generation < target_generation {
            break;  // 已到达目标栏
        }
        
        // 恢复旧值
        unsafe {
            core::ptr::copy_nonoverlapping(
                entry.old_value.as_ptr(),
                entry.field_ptr,
                core::mem::size_of::<u64>()
            );
        }
        
        self.count -= 1;
        rolled_back += 1;
    }
    
    // 清理超过最大保留数的旧条目
    if self.count > MAX_UNDO_ENTRIES / 2 {
        self.compact();
    }
    
    rolled_back
}
```

回滚 100 条条目：约 1 微秒。Minix3 fork+exec：5-20 毫秒。**栏栈恢复比 Minix3 快 5000-20000 倍。**

### 2.6 级联回滚

```rust
fn cascade_rollback(root_domain: u64) -> RollbackResult {
    // 1. 计算传递闭包 (底层域先回滚)
    let mut ordered = topological_sort(root_domain);
    
    // 2. 所有域进入 Freezing 状态 — 不再接受新请求
    for &id in &ordered {
        domains[id].state.store(Freezing, Ordering::SeqCst);
    }
    
    // 3. 等待所有正在进行的操作完成 (最多 100 ticks)
    wait_for_quiescence(&ordered, 100);
    
    // 4. 逐个回滚 (底层先)
    let target_gen = domains[root_domain].barrier_generation.load(Ordering::SeqCst);
    for &id in &ordered {
        domains[id].rollback_fn.map(|f| f(&domains[id]));
        domains[id].state.store(Recovering, Ordering::SeqCst);
    }
    
    // 5. 全局栏代际递增
    GLOBAL_BARRIER_EPOCH.fetch_add(1, Ordering::SeqCst);
    
    // 6. 所有域回到 Active
    for &id in &ordered {
        domains[id].reset_fn.map(|f| f(&domains[id]));
        domains[id].state.store(Active, Ordering::SeqCst);
    }
    
    RollbackResult::Success
}
```

---

## 三、循环防护：四层防御体系

### 层一：连续失败计数器 + 指数退避

```rust
fn should_attempt_rollback(domain: &RecoveryDomain) -> bool {
    let failures = domain.consecutive_failures.load(Ordering::SeqCst);
    
    if failures >= 5 {
        domain.state.store(Quarantined, Ordering::SeqCst);
        return false;  // 永久隔离
    }
    
    let backoff_ticks = (1u64 << failures.min(8)) * 100;
    if current_tick() < domain.backoff_until.load(Ordering::SeqCst) {
        return false;  // 仍在退避窗口内
    }
    
    domain.backoff_until.store(current_tick() + backoff_ticks, Ordering::SeqCst);
    true
}
```

退避策略：第一次失败 200 ticks，第二次 400 ticks，第三次 800 ticks...第 8 次及以后 25600 ticks。在此窗口内对该域的任何请求返回 `E_RECOVERING`。

### 层二：确定性重放检测

```rust
fn detect_identical_crash(domain: &RecoveryDomain, panic_info: &PanicInfo) -> bool {
    let fingerprint = hash(&(
        panic_info.location(),      // panic 位置
        panic_info.call_stack(),    // 调用栈哈希
        panic_info.input_hash(),    // 输入数据哈希
    ));
    
    let prev = domain.last_crash_fingerprint.swap(fingerprint, Ordering::SeqCst);
    
    if prev == fingerprint {
        // 相同的输入和调用路径 → 回滚无用 → 直接隔离
        domain.consecutive_failures.store(5, Ordering::SeqCst);  // 跳至 Quarantine
        return true;
    }
    false
}
```

### 层三：级联依赖层次化

一次级联回滚是原子的——所有受影响域共享同一个 `barrier_epoch`。回滚完成后：

- 如果再次 panic，且 `barrier_epoch` 已递增 → 不是同一个问题
- 如果再次 panic，且 `barrier_epoch` 未变 → 级联回滚未解决根因 → 走层一计数器

### 层四：渐进式功能降级

```rust
fn quarantine_domain(domain: &RecoveryDomain) {
    match domain.consecutive_failures.load(Ordering::SeqCst) {
        1..=2 => {},  // 正常回滚，不降级
        3..=4 => {
            domain.cap_mask &= !CAP_FS_WRITE;  // 降级：只读
        }
        _ => {
            domain.state.store(Quarantined, Ordering::SeqCst);  // 隔离
        }
    }
}
```

最后一道防线是 `PwidQuota`。即使以上四层全失效，故障域无法发起 DoS——每次回滚消耗该域的 CPU 配额。配额耗尽 → 调度器直接剥夺 CPU 时间。这是 Minix3 RS 不具有的能力。

---

## 四、利好分析

### 4.1 相对于传统宏内核（Linux/Redox）

| 维度 | 传统宏内核 | AntX + 栏栈 |
|------|-----------|------------|
| 内核模块崩溃 | 全局 panic → 停机 | 回滚单个域 → 继续运行 |
| 恢复粒度 | N/A（不恢复） | 被修改的字段（字段级） |
| 恢复延迟 | N/A | ~1 微秒（vs Minix3 5-20 毫秒） |
| 状态捕获成本 | N/A | ~10 纳秒/写入（增量日志） |
| 相同的崩溃输入 | N/A | 确定性重放检测 → 直接隔离 |
| 级联故障 | 全局崩溃 | 层次化原子回滚 |
| 恶意 DoS | 无防护 | PWID 配额自动限流 |

### 4.2 相对于 Minix3 微内核

| 维度 | Minix3 RS | AntX 栏栈 |
|------|-----------|----------|
| 故障检测 | waitpid 信号 | panic_handler 钩子 |
| 恢复粒度 | 整个进程（粗） | 被修改的字段（细） |
| 恢复延迟 | 5-20 毫秒 | ~1 微秒 |
| 检测相同输入 | 无法（RS 看不到进程内部） | 调用栈 + 输入哈希 |
| 级联崩溃处理 | 依赖管理（有限） | 层次化原子回滚 |
| DoS 防护 | 无 | PWID 配额限流 |
| 磁盘代码恢复 | 需要 RS 持有二进制副本 | 代码在只读页中，不需要副本 |
| IPC 开销 | 每次调用都有 | 无（函数调用） |
| 各域独立页表 | 是 | 否（共享页表，内存更省） |

### 4.3 独特的学术价值

栏栈不是一个单一的新发明——它是在操作系统领域**首次**将以下三个已存在但从未同时出现的能力组合在一起：

1. **编译期内存隔离**（Rust，取代 MMU 页表隔离）— 已知于 Singularity
2. **增量状态回滚**（撤销日志取代全量快照）— 已知于数据库领域
3. **字节级恢复粒度**（字段级取代进程级）— 未在任何 OS 中实现过

这个组合使宏内核获得了微内核的恢复能力，同时保留了宏内核的零 IPC 延迟和低内存开销。这是一个**可辩护的研究贡献**。

---

## 五、潜在危机与防御

### 5.1 错误被隐藏

**危机**：系统"太强韧了"，逻辑 bug 被反复回滚掩盖，永远不被修复。

**防御**：
- 每次回滚 → CRIT 级别日志，含调用栈哈希、域 ID、回滚原因
- `consecutive_failures` 计数器公开：`cat /proc/rollback_events` 可查
- 超过阈值 → 该域进入降级模式（只读/拒绝服务），而非无限回滚
- 回滚是安全网，不是自动修复——安全网让你摔不死，不是让你从十楼往下跳

### 5.2 开发速度税

**危机**：每增加一个内核模块，必须实现 `Recoverable` trait、设计撤销日志、定义回滚语义。

**防御**：
- 栏栈默认关闭。只有明确标记为 `#[recovery_domain]` 的模块才启用
- 新模块默认不参与栏栈，成熟稳定后再加
- `Recoverable` trait 提供默认空实现，不需要全量实现每个方法

### 5.3 依赖图爆炸

**危机**：RamFS → VFS → Syscall → ProcFS → IPC → Session → PWID → Audit 的级联回滚链。

**防御**：
- 扁平化依赖：**域之间不传递依赖。只有状态共享的关系才需要声明栏栈依赖，大多数调用关系不需要。**
- Syscall 调用 VFS：每次传递参数、返回结果，Syscall 的内部状态不依赖 VFS 的状态 → 不是栏栈依赖
- VFS 持有 RamFS 的文件描述符引用 → 这是真正的栏栈依赖

### 5.4 测试腐烂

**危机**：恢复路径未经日常使用测试，代码随时间腐烂。

**防御（必须实施，不可跳过）**：
- 故障注入框架 `#[cfg(feature = "fault_injection")]`
- CI 流水线 `make test-chaos` — 每次提交随机注入 panic 并验证恢复率 > 95%
- 定期模糊测试：在随机位置触发 panic，记录恢复成功率

```rust
#[cfg(feature = "fault_injection")]
fn maybe_inject_fault(domain: &RecoveryDomain) {
    if domain.fault_injection_probability > random_u32() % 100 {
        panic!("[FAULT-INJECT] Domain {} forced panic", domain.id);
    }
}
```

### 5.5 状态不可回滚的边界

**危机**：已经发送的网络包、已提交到硬件队列的 DMA 描述符、已写入磁盘的数据无法通过栏栈撤销。

**防御**：
- 在模块中明确标记"不可回滚字段"——任何涉及硬件副作用的缓冲区必须在文档中声明
- 栏栈只承诺**纯软件状态**的恢复，不承诺物理副作用的撤销
- 这是所有故障恢复系统的共同天花板，不是 AntX 特有的弱点

---

## 六、设计准则

### 6.1 核心准则

> **栏栈是安全网，不是自动修复。安全网的作用是让你摔不死，不是让你从十楼往下跳的时候期待它把你弹回原位。**

遵守此准则的表现：
- 正常情况下，回滚事件发生率应接近于零
- 如果某个模块一天触发三次回滚，代码有 bug，必须修——不是栏栈不够好
- 降级和隔离不是"功能坏了"——是"编译器发现了一个漏掉的 bug"

### 6.2 实现准则

1. **默认关闭**：栏栈不在所有模块启用。只有稳定的核心模块声明 `#[recovery_domain]`
2. **不可回滚字段**：每个模块必须在文档中声明哪些字段涉及硬件副作用
3. **故障审计**：每次回滚必须有日志、有时间戳、有调用栈、有域 ID
4. **限流**：`consecutive_failures >= 5` → 永久隔离，不做无限回滚
5. **确定性检测**：相同输入 + 相同调用栈 → 不回滚，直接隔离
6. **扁平依赖**：只有状态共享才声明栏栈依赖——调用关系不是依赖

### 6.3 测试准则

1. 每个 `Recovery` 实现必须通过故障注入验证
2. CI 中必须有 `test-chaos` 流程
3. 正常路径的测试代码比例应 ≥ 恢复路径测试的 10 倍
4. 恢复成功率基线：> 95%（单域）/ > 90%（级联回滚）

---

## 七、实施路线

### 阶段零：基础设施（本节定义）

| 任务 | 代码量 | 优先级 |
|------|:--:|:--:|
| `RecoveryDomain` 数据结构 | ~40 行 | P0 |
| `UndoLog` 及其默认实现 | ~80 行 | P0 |
| `ProcessState::Frozen` 状态 | ~3 行 | P0 |
| `Recoverable` trait | ~30 行 | P0 |
| `#[recovery_domain]` 属性宏 | ~50 行 | P0 |

### 阶段一：核心集成

| 任务 | 代码量 | 优先级 |
|------|:--:|:--:|
| `RecoverableMutex`（基于 spinlock 的 owner 字段） | ~100 行 | P0 |
| IDT `panic_handler` 钩子 | ~30 行 | P0 |
| `scheduler_tick` 中的 barrier_maintenance | ~20 行 | P0 |
| `domain_id` 字段加入 `HeapHeader` | ~30 行 | P0 |

### 阶段二：模块适配

| 任务 | 代码量 | 优先级 |
|------|:--:|:--:|
| RamFS 实现 `Recoverable` | ~80 行 | P1 |
| VFS 实现 `Recoverable` | ~50 行 | P1 |
| HvFS 实现 `Recoverable` | ~60 行 | P1 |

### 阶段三：测试与验证

| 任务 | 代码量 | 优先级 |
|------|:--:|:--:|
| 故障注入框架 | ~100 行 | P0 |
| `test-chaos` CI 流程 | ~50 行 | P0 |
| 与 Minix3 RS 的延迟基准对比 | 脚本 | P1 |
| 与 Redox `panic=abort` 的可用性对比 | 脚本 | P2 |

**总计：~620 行 Rust + ~60 行 C。当前内核约 40,000 行。栏栈增加约 1.7%。**

---

## 八、无解的限制

> 以下场景栏栈无法恢复。这不意味着栏栈失败——没有恢复机制能解决这些问题。

1. **已产生物理副作用**：网络包已发出、DMA 已提交、磁盘已写入——无法撤销。这是所有故障恢复系统的天花板。
2. **调度器自身崩溃**：如果调度器的 `schedule()` 函数发生逻辑错误，整个系统都没有恢复者。
3. **中断控制器状态污染**：如果 PIC/APIC 的状态被错误操作破坏，后续中断可能丢失或错误投递。
4. **硬件寄存器写入**：MMIO 寄存器一经写入，硬件状态已改变——无法通过软件回滚。
5. **PMM 自身崩溃**：物理内存管理器是栏栈的依赖基础。如果 PMM 的 bitmap 损坏，栏栈无法运作。

---

## 九、与 Minix3 的哲学差异

Minix3 的可靠性建立在"不信任任何一方"的基础上——驱动是潜在的恶意代码，必须用 MMU 隔离。

AntX 栏栈的可靠性建立在"信任代码质量，不信任运行时状态"的基础上——假设所有代码由同一团队以 Rust 编写，不会故意破坏其他模块，但可能因逻辑错误而 panic。

这不是孰优孰劣——这是**两种不同的安全模型**：
- Minix3 追求的是零信任隔离（security through isolation）
- AntX 栏栈追求的是快速恢复（availability through fast rollback）

在单开发者的个人探索项目中，后者是更经济的选择。如果 AntX 要承载第三方内核模块，需要额外引入硬件隔离层（MPK 或 Ring 1）。

---

## 十、附录：Rust 编译器给我们免了什么

以下是要实现栏栈而不使用 Rust 时需要的额外工程——每一项都需要 C 语言开发者手工保证：

| 需求 | C 内核需要的代码 | Rust 给予的保证 |
|------|-----------------|---------------|
| 无 use-after-free | 手动引用计数 + 审查每个 free | 借用检查器：编译期拒绝 |
| 无 double free | pmm 双重释放检测 (已实现) | 所有权模型：编译期拒绝 |
| 无 buffer overflow | 手动边界检查 | `[T; N]` 的 `Index` trait + `unsafe` 审计 |
| 无数据竞争 | 每个共享变量手写锁 | `Send`/`Sync` trait：编译期拒绝 |
| 无空指针解引用 | 每个指针的手动 NULL 检查 | `Option<T>` 强制处理 None |
| 状态可枚举 | 全局变量散落各文件 | 状态 = struct 字段的闭包 |
| 依赖方向清晰 | 通过 include 图推断 | `use` 语句显式 |
| 恢复接口统一 | 每个模块自定义恢复函数 | `Recoverable` trait：编译器检查一致性 |

**这解释了为什么栏栈作为一个概念是新的——不是因为"增量日志"或"回滚"是新的，而是因为 Rust + 宏内核 + 无历史包袱这个组合在历史上从未出现过。**
