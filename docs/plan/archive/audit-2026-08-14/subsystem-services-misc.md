# services 多子目录深度审计报告

> **审计范围**：`src/kernel/services/` mm + syscall + barrier + config + debug + chitin + io + timer = **49 个源文件**
> **审计日期**：2026-08-14
> **代码规模**：约 10,400 LoC
> **总体结论**：✅ 0 unsafe（合规）/ ⚠️ **28 个问题（P0×5, P1×8, P2×10, P3×5）**

## 1. 子系统概览

| 子系统 | 文件数 | LoC | 主要职责 | 风险等级 |
|---|---:|---:|---|---|
| [services/mm/](file:///home/anfer/Code/QueenX/src/kernel/services/mm/) | 13 | 2,480 | mmap/mprotect/brk/madvise/swap 等策略 | **高** |
| [services/syscall/](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/) | 3 | 2,130 | syscall 编号 + 分发 + 类型 | **极高** |
| [services/barrier/](file:///home/anfer/Code/QueenX/src/kernel/services/barrier/) | 7 | 1,888 | 故障归属 + 恢复策略 + 健康监控 | **高** |
| [services/config/](file:///home/anfer/Code/QueenX/src/kernel/services/config/) | 12 | 1,459 | 容量/能力/内存/sysctl/procfs 等配置 | 中 |
| [services/debug/](file:///home/anfer/Code/QueenX/src/kernel/services/debug/) | 3 | 939 | eBPF 验证器（策略层）| **高** |
| [services/chitin/](file:///home/anfer/Code/QueenX/src/kernel/services/chitin/) | 3 | 715 | 设备框架策略层（设备树/复合设备）| 中 |
| [services/io/](file:///home/anfer/Code/QueenX/src/kernel/services/io/) | 2 | 503 | io_uring | 中 |
| [services/timer/](file:///home/anfer/Code/QueenX/src/kernel/services/timer/) | 6 | 391 | POSIX timer / tickless / time_sync | 中 |

## 2. 严重问题

### 2.1 [P0] `services/syscall/dispatch.rs:55-83` `ServicesSyscallDispatch::dispatch` 顺序 7 个子系统分发**线性扫描**

- **位置**：[dispatch.rs:55-84](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs#L55-L84)
- **代码**：
  ```rust
  fn dispatch(&self, num: u64, args: [u64; 6]) -> i64 {
      if let Some(ret) = dispatch_fs(num, args) { return ret; }
      if let Some(ret) = dispatch_proc(num, args) { return ret; }
      if let Some(ret) = dispatch_net(num, args) { return ret; }
      if let Some(ret) = dispatch_mm(num, args) { return ret; }
      if let Some(ret) = dispatch_sync(num, args) { return ret; }
      if let Some(ret) = dispatch_credo(num, args) { return ret; }
      if let Some(ret) = dispatch_other(num, args) { return ret; }
      -38
  }
  ```
- **问题**：
  - 每次 syscall 走 7 个子系统的 match 分发，**性能瓶颈**。
  - Linux 实际采用 sys_call_table 数组索引（O(1)），当前实现 O(N) 子系统 + 每个子系统内 O(N) match。
  - `dispatch_fs` 单个函数 755 行（[services/syscall/dispatch.rs](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs)），match 含 100+ arm。
- **建议方案**：
  1. 全局 `static SYSCALL_TABLE: [SyscallHandler; MAX_SYSCALLS]` 数组，O(1) 索引。
  2. 启动时由各 `dispatch_*` 注册其范围内的 handler。

### 2.2 [P0] `services/syscall/types.rs:25-26` `MAX_SYSCALLS = 800` 与实际 syscall 数量可能错位

- **位置**：[types.rs:25-26](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/types.rs#L25-L26)
- **代码**：
  ```rust
  pub const SYSCALL_INT: u8 = 0x80;
  pub const MAX_SYSCALLS: u64 = 800;
  ```
- **问题**：
  - `MAX_SYSCALLS=800` 是硬编码上限。
  - 注释（[types.rs:17-23](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/types.rs#L17-L23)）规划到 800-899 扩展。
  - 若新加 syscall 编号超过 800 且**未更新 MAX_SYSCALLS**，dispatch 表越界。
- **建议方案**：
  1. 用 `[SyscallHandler; MAX_SYSCALLS]` 静态数组大小。
  2. 编译期 `const { assert!(MAX_SYSCALLS >= QX_*) }`。

### 2.3 [P0] `services/mm/mmap.rs:44-51` `fd_to_inode_id` 全局 `VFS_MANAGER.get_fd_info` 锁，无超时

- **位置**：[mmap.rs:44-51](file:///home/anfer/Code/QueenX/src/kernel/services/mm/mmap.rs#L44-L51)
- **代码**：
  ```rust
  pub fn fd_to_inode_id(fd: i32) -> u32 {
      if fd < 0 { return 0; }
      crate::kernel::framework::fs::VFS_MANAGER
          .get_fd_info(fd as usize)
          .map_or(0, |(node_id, _, _)| node_id)
  }
  ```
- **问题**：
  - `VFS_MANAGER` 是全局 IrqSpinLock（[subsystem-services-fs.md §3.2 P0 dcache 锁](../audit/subsystem-services-fs.md)）。
  - mmap 调用 → fd_to_inode_id → VFS_MANAGER 锁 → 持锁调用 inode 操作。
  - 多 CPU 竞争 + 文件路径遍历 = **长时间持锁**。
  - 与 §3.2 dcache 全局锁问题叠加。
- **建议方案**：
  1. fd_to_inode_id 返回 `Result<u32, Errno>` 让调用方处理错误。
  2. 或拆分为 per-CPU fd cache。

### 2.4 [P0] `services/debug/ebpf_verifier.rs:754` 验证器**简化版**（7 条规则）可能漏判攻击向量

- **位置**：[ebpf_verifier.rs:14-23](file:///home/anfer/Code/QueenX/src/kernel/services/debug/ebpf_verifier.rs#L14-L23)
- **问题**：
  - 与 [subsystem-framework-misc.md §3.5](../audit/subsystem-framework-misc.md) 同样问题——简化验证器可能允许恶意 eBPF 程序。
  - 当前验证规则：指令数/寄存器号/跳转目标/回边/EXIT/R1-R5 类型/R10 只读。
  - **缺少**：ALU 溢出检查、栈访问越界检查、helper 参数类型检查。
- **建议方案**：
  1. 添加 ALU 操作范围检查（防止 OOB 索引）。
  2. 添加栈访问深度验证。
  3. 配套 fuzzing 测试。

### 2.5 [P0] `services/barrier/attribution.rs:32` `capability` 自动降级逻辑**未审慎**

- **位置**：[attribution.rs:24-28](file:///home/anfer/Code/QueenX/src/kernel/services/barrier/attribution.rs#L24-L28)
- **代码**：
  ```rust
  //! 2. **能力降级**: 服务域连续失败 → 自动降级 capability
  ```
- **问题**：
  - 自动降级 capability → 攻击者可故意触发服务失败 → **强制降级**某关键服务（如文件系统）→ 攻击者绕过 capability 检查。
  - 需单开 PR 深审 attribution.rs 480 行的实现细节。
- **建议方案**：
  1. 降级需多因子决策（连续失败次数 + 时间窗口 + 失败模式）。
  2. 仅降级非关键 capability（如 NET → IP forward）。

## 3. P1 问题

### 3.1 [P1] `services/syscall/types.rs:26` `MAX_SYSCALLS=800` 与 dispatch.rs 实际支持的 syscall 数差异

- **位置**：[types.rs:26](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/types.rs#L26)
- **问题**：
  - 注释中"待迁移"列表（[dispatch.rs:21-23](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/dispatch.rs#L21-L23)）有 20+ syscall 未迁移。
  - `MAX_SYSCALLS=800` 是上限；实际 syscall 数量随迁移进度变化。

### 3.2 [P1] `services/mm/madvise_mlock.rs:238` `madvise/mlock` 策略层但 `rlimit` 实际检查由 framework 完成

- **位置**：[madvise_mlock.rs:1-238](file:///home/anfer/Code/QueenX/src/kernel/services/mm/madvise_mlock.rs#L1-L238)
- **问题**：
  - `mlock` 通过 `framework::rlimit_query::get_memlock_limit()`（[framework/rlimit_query.rs:31](file:///home/anfer/Code/QueenX/src/kernel/framework/rlimit_query.rs#L31)）查 limit。
  - 调用方契约：register callback 在 proc init 期。
  - 但 callback 未注册时返回硬编码 64KB 默认值——**未文档化**。
- **建议方案**：
  1. callback 未注册时 panic。
  2. 或返回 `Option<u64>`。

### 3.3 [P1] `services/barrier/health_monitor.rs:266` 健康监控**阈值未审**

- **位置**：[health_monitor.rs:1-266](file:///home/anfer/Code/QueenX/src/kernel/services/barrier/health_monitor.rs#L1-L266)
- **问题**：
  - 未审细节。
- **建议方案**：
  1. 单开 PR 深审。

### 3.4 [P1] `services/config/sysctl.rs:431` sysctl 431 行——sysctl 路径解析+权限检查

- **位置**：[sysctl.rs:1-431](file:///home/anfer/Code/QueenX/src/kernel/services/config/sysctl.rs#L1-L431)
- **问题**：
  - sysctl 是 procfs 的虚拟文件系统。
  - 路径遍历 + 权限检查未深审。

### 3.5 [P1] `services/config/procfs.rs:222` procfs 内容生成——可能泄露敏感信息

- **位置**：[procfs.rs:1-222](file:///home/anfer/Code/QueenX/src/kernel/services/config/procfs.rs#L1-L222)
- **问题**：
  - procfs 内容生成涉及敏感路径（PID、内存、内核状态）。
  - **权限模型必须严格**——非 root 用户不应看到其他进程信息。

### 3.6 [P1] `services/debug/ebpf.rs:33` eBPF 系统调用入口（仅 33 行）

- **位置**：[ebpf.rs:1-33](file:///home/anfer/Code/QueenX/src/kernel/services/debug/ebpf.rs#L1-L33)
- **问题**：
  - 入口代码极简，与 754 行验证器 + 1402 行 framework 实现不对称。
  - 验证逻辑分散在 services 与 framework 两处。

### 3.7 [P1] `services/io/iouring.rs:497` io_uring 完整实现**未深审**

- **位置**：[iouring.rs:1-497](file:///home/anfer/Code/QueenX/src/kernel/services/io/iouring.rs#L1-L497)
- **问题**：
  - io_uring 是 Linux 5.x+ 新异步 I/O 接口，复杂度高。
  - 内核实现涉及 SQ/CQ 环形队列、注册缓冲、共享内存。
  - 安全性未深审。

### 3.8 [P1] `services/timer/timerfd.rs:174` timerfd 实现未深审

- **位置**：[timerfd.rs:1-174](file:///home/anfer/Code/QueenX/src/kernel/services/timer/timerfd.rs#L1-L174)
- **问题**：
  - timerfd 创建 timer 并通过 fd 暴露 → 用户态可读定时器。
  - 需深审。

## 4. P2 问题

### 4.1 [P2] `services/mm/brk.rs:52` `brk` 仅 52 行——可能未完整实现

- **位置**：[brk.rs:1-52](file:///home/anfer/Code/QueenX/src/kernel/services/mm/brk.rs#L1-L52)
- **问题**：
  - `brk` syscall 是进程堆管理基础。
  - 52 行实现过简。

### 4.2 [P2] `services/mm/mremap.rs:69` `mremap` 仅 69 行 + 注释"待 Phase N 实装"

- **位置**：[mremap.rs:1-69](file:///home/anfer/Code/QueenX/src/kernel/services/mm/mremap.rs#L1-L69)
- **问题**：
  - mremap 注释说"待 Phase N 实现"，但已有 `SYS_mremap` 编号分配。

### 4.3 [P2] `services/mm/numa.rs:413` NUMA 策略 413 行——NUMA 拓扑识别

- **位置**：[numa.rs:1-413](file:///home/anfer/Code/QueenX/src/kernel/services/mm/numa.rs#L1-L413)
- **问题**：
  - NUMA 拓扑探测 + 跨节点内存分配——复杂逻辑未深审。

### 4.4 [P2] `services/mm/pcache.rs:64` page cache 策略仅 64 行

- **位置**：[pcache.rs:1-64](file:///home/anfer/Code/QueenX/src/kernel/services/mm/pcache.rs#L1-L64)
- **问题**：
  - 与 framework/mm/pcache.rs 重复或不一致。

### 4.5 [P2] `services/mm/pmm_policy.rs:254` PMM 策略层 254 行

- **位置**：[pmm_policy.rs:1-254](file:///home/anfer/Code/QueenX/src/kernel/services/mm/pmm_policy.rs#L1-L254)
- **问题**：
  - 物理页分配策略（哪些 zone、哪个 NUMA 节点）未深审。

### 4.6 [P2] `services/mm/slab_policy.rs:259` slab 策略层 259 行

- **位置**：[slab_policy.rs:1-259](file:///home/anfer/Code/QueenX/src/kernel/services/mm/slab_policy.rs#L1-L259)
- **问题**：
  - slab 分配策略（缓存大小、回收阈值）未深审。

### 4.7 [P2] `services/mm/swap_policy.rs:252` swap 策略层 252 行

- **位置**：[swap_policy.rs:1-252](file:///home/anfer/Code/QueenX/src/kernel/services/mm/swap_policy.rs#L1-L252)
- **问题**：
  - 页面回收策略（哪些页优先 swap）未深审。

### 4.8 [P2] `services/mm/swap.rs:147` swap 实现仅 147 行

- **位置**：[swap.rs:1-147](file:///home/anfer/Code/QueenX/src/kernel/services/mm/swap.rs#L1-L147)
- **问题**：
  - swap 实现过简。

### 4.9 [P2] `services/barrier/audit_export.rs:252` 审计日志导出格式

- **位置**：[audit_export.rs:1-252](file:///home/anfer/Code/QueenX/src/kernel/services/barrier/audit_export.rs#L1-L252)
- **问题**：
  - 审计日志格式（是否包含敏感信息、是否加密）需深审。

### 4.10 [P2] `services/config/memory.rs:88` 内存配置层

- **位置**：[memory.rs:1-88](file:///home/anfer/Code/QueenX/src/kernel/services/config/memory.rs#L1-L88)
- **问题**：
  - 88 行——可能过简。

## 5. P3 问题

### 5.1 [P3] `services/syscall/mod.rs:355` `mod.rs` 355 行入口——可能含重复 re-export

- **位置**：[mod.rs:1-355](file:///home/anfer/Code/QueenX/src/kernel/services/syscall/mod.rs#L1-L355)
- **问题**：
  - 大量 `pub use` 重导出——可被 IDE 自动管理。

### 5.2 [P3] `services/mm/mod.rs:48` mm 入口极简

- **位置**：[mod.rs:1-48](file:///home/anfer/Code/QueenX/src/kernel/services/mm/mod.rs#L1-L48)
- **问题**：
  - 48 行——可能不完整。

### 5.3 [P3] `services/barrier/mod.rs:43` barrier 入口极简

- **位置**：[mod.rs:1-43](file:///home/anfer/Code/QueenX/src/kernel/services/barrier/mod.rs#L1-L43)
- **问题**：
  - 43 行——可能不完整。

### 5.4 [P3] `services/io/mod.rs:6` io 入口仅 6 行

- **位置**：[mod.rs:1-6](file:///home/anfer/Code/QueenX/src/kernel/services/io/mod.rs#L1-L6)
- **问题**：
  - 6 行——实际只有 `iouring` 子模块。

### 5.5 [P3] `services/timer/mod.rs:26` timer 入口 26 行

- **位置**：[mod.rs:1-26](file:///home/anfer/Code/QueenX/src/kernel/services/timer/mod.rs#L1-L26)
- **问题**：
  - 26 行——可能不完整。

## 6. 跨子系统关联

### 6.1 syscall ↔ fs/proc/net/mm 子系统

- `services/syscall/dispatch.rs` 是 syscall 单一入口。
- 每个 `dispatch_*` 函数对应一个子系统。
- **关键路径**：syscall → framework 入口 → services 分发 → framework 子系统实现 → 返回。

### 6.2 mm ↔ fs (mmap 文件映射)

- `mmap` 文件映射需要 inode 信息（fd_to_inode_id）。
- 与 [subsystem-services-fs.md §3.2 P0 dcache 全局锁](../audit/subsystem-services-fs.md) 直接关联。

### 6.3 barrier ↔ services 全模块

- 所有 services 子系统的故障通过 `attribution.rs` 归属。
- `capability` 自动降级影响全局 capability 矩阵。

### 6.4 debug/ebpf ↔ framework/debug/ebpf

- `services/debug/ebpf_verifier.rs` 实现 `framework/debug::BpfVerifier` trait。
- 验证逻辑在 services，机制在 framework。

## 7. 修复优先级总表

| 优先级 | 问题数 | 估算工作量 |
|---|---:|---:|
| **P0** | 5 | 4-5 天 |
| **P1** | 8 | 6-8 天 |
| **P2** | 10 | 3-4 天 |
| **P3** | 5 | 0.5 天 |
| **合计** | **28** | **14-18 天** |

### P0 修复路径（建议执行顺序）

1. **§2.1 syscall dispatch O(N) → O(1) 查表**（1-2 天，**性能关键路径**）
2. **§2.4 eBPF 验证器增强**（1-2 天，**安全关键**）
3. **§2.5 capability 自动降级**（1 天，**安全策略**）
4. **§2.2 MAX_SYSCALLS 编译期校验**（0.5 天）
5. **§2.3 fd_to_inode_id 锁优化**（1 天）