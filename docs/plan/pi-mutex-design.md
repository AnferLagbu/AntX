# Priority Inheritance Mutex 设计决策

> 状态: 已完成 (2026-06-08)
> 日期: 2026-06-08
> 关联: DECISION-009/010/011

## 目标

实现 POSIX `pthread_mutex` 的可选 PTHREAD_PRIO_INHERIT 协议: 当高优先级线程因等待低优先级线程持有的 mutex 而阻塞时, 临时把低优先级线程的有效优先级提升到与高优先级线程相同, 防止**优先级反转** (priority inversion)。

## 范围

| 项 | v1 范围 | 后续 |
|----|---------|------|
| 直接捐赠 (A→B) | ✅ 单级捐赠, A 等待 B 的 mutex 时 B 优先级提升 | v2 链式 (A→B→C) |
| 等待者优先级 | ✅ 每次注册时按 `base_priority` 计算 | v2 动态重算 |
| 多等待者取 max | ✅ 所有等待者中 max 优先级 | — |
| 释放时优先级恢复 | ✅ 释放后调用方负责恢复 | — |
| 调度器集成 | ❌ PI Mutex 暴露 `effective_priority()` 钩子, 调度器可选调用 | v2 强制集成 |
| 优先级天花板协议 (PCP) | ❌ | v2 |
| 鲁棒 mutex (PTHREAD_MUTEX_ROBUST) | ❌ | v2 |
| Futex-style 阻塞 | ❌ (v1 自旋 + yield) | v2 |

## 关键设计

### 1. 状态机

```text
Unlocked → lock(prio=A) → Locked(holder=A, effective=max(A_prio, waiters_prio))
Locked → wait(B with prio=B_prio) → Locked(holder=A, effective=max(A_prio, B_prio))
Locked → unlock → Unlocked + wake highest-prio waiter
```

### 2. 数据结构

```rust
pub struct PiMutex<T: ?Sized> {
    inner: PiMutexInner,
    data: UnsafeCell<T>,
}

struct PiMutexInner {
    /// 是否被持有 (true/false)
    locked: AtomicBool,
    /// 当前持有者 PID (None = 未持有)
    holder: AtomicU32,
    /// 等待队列 (FIFO 顺序, 同优先级按 FIFO)
    waiters: Mutex<VecDeque<WaiterEntry>>,
    /// 内层中断安全自旋锁 (保护 waiters)
    inner_lock: IrqSpinLock<()>,
}

struct WaiterEntry {
    pid: u32,
    /// 该等待者入队时的 base_priority (用于取消时正确撤销捐赠)
    base_priority: u32,
}
```

### 3. 捐赠算法 (v1 直接捐赠)

```text
lock(self, my_pid, my_base_prio):
  loop:
    if self.try_acquire(my_pid):
      return  # 成功获取

    # 注册为等待者
    inner_lock.lock()
    self.waiters.push_back(WaiterEntry { pid: my_pid, base_priority: my_base_prio })
    holder = self.holder
    if holder != 0:
      notify_donation(holder, my_base_prio)  # 调用方钩子
    inner_lock.unlock()

    # 自旋等待直到被唤醒 (v1: 简单自旋 + yield)
    while self.holder.load() != my_pid:
      scheduler_yield()

unlock(self):
  my_pid = current
  inner_lock.lock()
  self.locked = false
  self.holder = 0
  # 找到下一个最高优先级等待者
  next = self.waiters.iter().max_by_key(|w| w.base_priority)
  if let Some(next) = next:
    self.holder = next.pid
    self.locked = true
    self.waiters.remove(next)  # O(n), v2 可优化为堆
  # 通知调度器优先级变化
  notify_revoke(my_pid)
  inner_lock.unlock()
```

### 4. 与 Process 解耦

PI Mutex **不直接修改 Process.priority**。原因:
- 避免热路径 Process struct 修改
- 测试可独立运行, 不依赖完整 Process 子系统
- 调度器集成是独立关注点

钩子函数:
```rust
/// 由调用方注入的 "donation 通知" 回调
pub type DonationCallback = fn(holder_pid: u32, donated_prio: u32);
static NOTIFY_DONATION: AtomicPtr<()> = AtomicPtr::new(null());
static NOTIFY_REVOKE: AtomicPtr<()> = AtomicPtr::new(null());
```

### 5. 错误处理

- 重复 lock: 失败返回错误 (v1 不支持递归 lock, 与 Linux PTHREAD_PRIO_INHERIT 默认行为一致)
- 持有者死亡 (TID 无效): 鲁棒性由 v2 实现

### 6. 中断上下文约束

- 持锁期间**禁止**进入中断上下文
- 自旋 + yield 是非阻塞路径
- v1 调度器集成: 等待期间 `scheduler_yield()` 让出 CPU, 不占用调度队列

## 文件结构

| 路径 | 类型 | 职责 |
|------|------|------|
| `src/kernel/framework/sync/pi_mutex.rs` | 新增 (TCB) | `PiMutex<T>`, `PiMutexGuard<T>`, 12 个 `pi_*` 函数 |
| `src/kernel/framework/sync/mod.rs` | 修改 | `pub mod pi_mutex;` + re-exports |
| `src/kernel/services/sync/pi_mutex.rs` | 新增 (safe) | 强类型包装, 0 unsafe |
| `src/kernel/services/sync/mod.rs` | 修改 | re-export |
| `src/kernel/framework/tests/test_pi_mutex.rs` | 新增 | 6 个 no_std 单元测试 |
| `src/kernel/framework/tests/mod.rs` | 修改 | 注册测试 |

## 验证

1. `cargo check -p queenx --target x86_64-unknown-none` 0 warning 0 error
2. `cargo check -p queenx --target aarch64-unknown-none` 0 warning 0 error
3. `cargo test -p antx-host-tests` 全通过
4. `scripts/audit_safety_coverage.py` 100% SAFETY 覆盖
5. `scripts/audit_services_boundary.py` services 不越界
6. `scripts/ci_check_services_unsafe.py` services 0 unsafe
7. `scripts/audit_deadlock_matrix.py` PI mutex 锁链登记

## 决策记录

- DECISION-009: PI Mutex v1 只支持直接捐赠, 不处理链式 A→B→C。理由: 链式捐赠实现复杂且需遍历 Process 树, v1 满足 "防中等优先级线程饿死" 主目标即可。
- DECISION-010: PI Mutex 不直接修改 Process 状态, 通过回调通知。理由: 解耦 + 可独立测试 + 调度器集成是 v2 任务。
- DECISION-011: 等待策略 v1 退化为自旋 + yield, 不入调度等待队列。理由: 调度器集成工作量是独立的 C6 Lockdep 任务, 提前做会扩散 scope。
