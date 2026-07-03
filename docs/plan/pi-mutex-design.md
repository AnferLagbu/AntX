# Priority Inheritance Mutex 设计决策

> PI Mutex 设计 (DECISION-009/010/011/012/013/014/015/016/017), 2026-06-08 启动 v1, 2026-06-29 推进 v2.

## 目标
- **目标条目**
  - 描述: 实现 POSIX `pthread_mutex` 的可选 PTHREAD_PRIO_INHERIT 协议, 防优先级反转
  - 方案: 当高优先级线程因等待低优先级线程持有的 mutex 而阻塞时, 临时把低优先级线程的有效优先级提升到与高优先级线程相同
  - 状态: [X]

## 范围
- **v1 范围**
  - 描述: 直接捐赠 (单级 A→B) / 等待者优先级 (注册时按 base_priority 计算) / 多等待者取 max / 释放时优先级恢复
  - 方案: 最小化实现, 满足"防中等优先级线程饿死"主目标
  - 状态: [X]
  - 详情:

    | 项 | v1 范围 | 后续 |
    |----|---------|------|
    | 直接捐赠 (A→B) | ✅ 单级捐赠 | v2 链式 (A→B→C) |
    | 等待者优先级 | ✅ 注册时按 base_priority 计算 | ✅ v2.1 动态重算 |
    | 多等待者取 max | ✅ 所有等待者中 max | — |
    | 释放时优先级恢复 | ✅ 释放后调用方负责 | — |
    | 调度器集成 | ❌ PI Mutex 暴露 effective_priority() 钩子 | v2.3 强制集成 |
    | 优先级天花板协议 (PCP) | ❌ | v2.4 |
    | 鲁棒 mutex | ❌ | v2.5 |
    | Futex-style 阻塞 | ❌ (v1 自旋 + yield) | v2.6 |

- **v2.1 子特性 (2026-06-29 实施)**
  - 描述: 等待者优先级动态重算
  - 方案: 新增 `PiMutex::update_waiter_priority(pid, new_base_priority)` 方法 + 提取私有助手 `recompute_and_notify()` 统一 register/unlock/update 3 路径; 调用方在 nice/setpriority 变化时触发
  - 状态: [X]
  - 详情: 算法 O(n) 扫描 waiters 队列; 包含 5 个 host 端单元测试 + 5 个 no_std 单元测试; register/unlock 路径重构为调用 `recompute_and_notify` 助手, 行为不变

- **v2.2 子特性 (2026-07-02 实施)**
  - 描述: 链式捐赠 A→B→C
  - 方案: 在捐赠追踪中维护 donation_chain 链表 (holder→donor→donor...), 通过遍历链路递归提升; DECISION-013
  - 状态: [X]

- **v2.3 子特性 (2026-07-02 实施)**
  - 描述: 调度器集成 (强制)
  - 方案: notify_donation 钩子实装为真正修改 Process.priority + 触发 CFS 重排; 跨 sync/proc/sched 三大子系统; DECISION-014
  - 状态: [X]

- **v2.4 子特性 (计划中)**
  - 描述: 优先级天花板协议 (PCP)
  - 方案: 新增独立协议枚举 PiMutexProtocol { Pi, Pcp }; 互斥时二选一; Pcp 模式下取 max(holder.base, ceiling) 一次性提升; DECISION-015
  - 状态: []

- **v2.5 子特性 (计划中)**
  - 描述: 鲁棒 mutex (持有者死亡)
  - 方案: 在 Process.exit() 处遍历 PI mutex 注册表, 释放持有者已死锁的 mutex, 唤醒等待者; DECISION-016
  - 状态: []

- **v2.6 子特性 (计划中)**
  - 描述: Futex-style 阻塞
  - 方案: 用 wait_queue 替换自旋 + yield; 等待者放入调度等待队列, 解锁时直接唤醒; DECISION-017
  - 状态: []

- **后续范围**
  - 描述: v2 实现的子特性
  - 方案: 走独立路线扩展, 不影响 v1
  - 状态: [X] (v2.1 已完成)

## 关键设计
- **状态机**
  - 描述: 3 状态 (Unlocked / Locked / Waiters)
  - 方案: Unlocked→lock→Locked(holder=A); Locked→wait(B)→Locked(holder=A, effective=max(A,B)); Locked→unlock→Unlocked+wake highest-prio waiter
  - 状态: [X]

- **数据结构**
  - 描述: PiMutex<T> = inner + data(AtomicBool locked, AtomicU32 holder, Mutex<VecDeque<WaiterEntry>>, IrqSpinLock<()> inner_lock)
  - 方案: WaiterEntry { pid, base_priority } 用于 FIFO 同优先级 + 撤销捐赠正确性
  - 状态: [X]

- **捐赠算法 (v1 直接捐赠)**
  - 描述: lock 循环 + 自旋等待; unlock 找最高优先级等待者
  - 方案: lock: try_acquire 失败则注册为 waiter + notify_donation(holder) + 自旋等待被唤醒; unlock: 释放 + 找 max priority 等待者 + 转移持有 + notify_revoke
  - 状态: [X]
  - 详情: 找到 next 是 O(n), v2 可优化为堆

- **v2.1 动态重算**
  - 描述: 等待者 base_priority 变化时 (nice/setpriority) 重算 effective
  - 方案: update_waiter_priority(pid, new_prio) 替换 waiter 条目, 调用 recompute_and_notify() 助手; 通知策略: prev<new→donate, prev>new→revoke
  - 状态: [X]

- **与 Process 解耦**
  - 描述: PI Mutex 不直接修改 Process.priority (v1+v2.1)
  - 方案: 通过 DonationCallback 钩子函数 (NOTIFY_DONATION + NOTIFY_REVOKE, AtomicPtr<()>); 避免热路径 Process struct 修改 + 测试独立运行
  - 状态: [X]
  - 详情: v2.3 将扩展为真正修改 Process.priority + 触发 CFS 重排

- **错误处理**
  - 描述: 重复 lock 失败 + 持有者死亡鲁棒性延后
  - 方案: 重复 lock 失败返回错误 (v1 不支持递归, 与 Linux PTHREAD_PRIO_INHERIT 默认行为一致); 持有者死亡鲁棒性由 v2.5 实现
  - 状态: [X]

- **中断上下文约束**
  - 描述: 持锁期间禁止进入中断, 等待非阻塞
  - 方案: 自旋 + yield 是非阻塞路径; v1 调度器集成: 等待期间 scheduler_yield() 让出 CPU, 不占用调度队列
  - 状态: [X]
  - 详情: v2.6 将替换为 wait_queue 真正睡眠

## 文件结构
- **TCB 侧**
  - 描述: framework/sync 新增 + mod.rs 修改
  - 方案: `src/kernel/framework/sync/pi_mutex.rs` (新增 TCB, 含 PiMutex<T>, PiMutexGuard<T>, 12 个 pi_* 函数) + `src/kernel/framework/sync/mod.rs` (添加 pub mod + re-exports)
  - 状态: [X]
  - 详情: v2.1 增加 update_waiter_priority / recompute_and_notify

- **Services 侧**
  - 描述: services/sync 新增 safe 包装
  - 方案: `src/kernel/services/sync/pi_mutex.rs` (新增, 强类型包装, 0 unsafe) + `src/kernel/services/sync/mod.rs` (re-export)
  - 状态: [X]
- **测试**
  - 描述: 8 个 no_std 单元测试 (v1) + 5 个 v2.1 单元测试
  - 方案: `src/kernel/framework/tests/test_pi_mutex.rs` (新增) + `src/kernel/framework/tests/mod.rs` (注册)
  - 状态: [X]

## 验证
- **编译验证**
  - 描述: 双架构编译
  - 方案: `cargo check -p queenx --target x86_64-unknown-none` 0 warning 0 error; `cargo check -p queenx --target aarch64-unknown-none` 0 warning 0 error
  - 状态: [X]
- **测试与审计**
  - 描述: host-tests + 4 项审计脚本
  - 方案: `cargo test -p queenx-host-tests` 全通过; `audit_safety_coverage.py` 100% SAFETY 覆盖; `audit_services_boundary.py` services 不越界; `ci_check_services_unsafe.py` services 0 unsafe; `audit_deadlock_matrix.py` PI mutex 锁链登记
  - 状态: [X]
- **v2.1 增量验证 (2026-06-29)**
  - 描述: v2.1 引入 5 个 host 端 + 5 个 no_std 单元测试
  - 方案: host `cargo test --release` v2_1_* 5 测试通过; no_std `make test-unit` (QEMU) 全 PI_MUTEX 13 测试通过; 双架构 cargo check 0w0e; clippy 0 warning
  - 状态: [X]

## 决策记录
- **DECISION-009**
  - 描述: PI Mutex v1 只支持直接捐赠, 不处理链式 A→B→C
  - 方案: 理由: 链式捐赠实现复杂且需遍历 Process 树, v1 满足"防中等优先级线程饿死"主目标即可
  - 状态: [X] (2026-06-08)
- **DECISION-010**
  - 描述: PI Mutex 不直接修改 Process 状态, 通过回调通知
  - 方案: 理由: 解耦 + 可独立测试 + 调度器集成是 v2 任务
  - 状态: [X] (2026-06-08)
  - 详情: v2.3 将扩展为强制集成
- **DECISION-011**
  - 描述: 等待策略 v1 退化为自旋 + yield, 不入调度等待队列
  - 方案: 理由: 调度器集成工作量是独立的 C6 Lockdep 任务, 提前做会扩散 scope
  - 状态: [X] (2026-06-08)
  - 详情: v2.6 将替换为 wait_queue
- **DECISION-012 (新增 2026-06-29)**
  - 描述: PI Mutex v2.1 新增 `update_waiter_priority` 公开 API
  - 方案: 理由: nice/setpriority 变化时等待者优先级随之变化, 必须能动态更新 mutex 状态机的 max 计算; 不实现会导致 nice 修改无效, mutex 状态与实际优先级不一致
  - 状态: [X] (2026-06-29)
- **DECISION-013 (2026-07-02 实施)**
  - 描述: v2.2 链式捐赠
  - 方案: 理由: 满足 POSIX PTHREAD_PRIO_INHERIT 完整规范
  - 状态: [X]
- **DECISION-014 (2026-07-02 实施)**
  - 描述: v2.3 调度器强制集成
  - 方案: 理由: 仅有钩子不够, 必须实装为真正修改 Process.priority + CFS 重排, 否则捐赠只是"账面"行为
  - 状态: [X]
- **DECISION-015 (计划 2026-Q3)**
  - 描述: v2.4 PCP 协议
  - 方案: 理由: POSIX PTHREAD_PRIO_PROTECT 协议对应, 适用于有界临界区
  - 状态: []
- **DECISION-016 (计划 2026-Q3)**
  - 描述: v2.5 鲁棒 mutex
  - 方案: 理由: 持有者进程死亡时, 锁不释放, 等待者永久阻塞
  - 状态: []
- **DECISION-017 (计划 2026-Q3)**
  - 描述: v2.6 Futex-style 阻塞
  - 方案: 理由: 自旋+yield 浪费 CPU, 应改用调度等待队列真正睡眠
  - 状态: []
