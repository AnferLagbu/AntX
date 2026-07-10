# Linux 兼容性维护工程

> 基于 2026-07-10 深度代码审查，识别出的 Linux 风格实现维护任务。
>
> **审查原则**: 对外 ABI (syscall 编号/POSIX API/文件格式) 保持 Linux 兼容 = 可接受；内部实现中不必要的 Linux 复杂度 = 需要优化；Linux 验证过的最佳方式在 QX 源码中使用 = 允许。

---

## 高优先级 — 疑似 Bug 或安全隐患

### M1: COW 引用计数连续调用验证 ✅

- **描述**: `cow.rs:224-225` 中 `cow_inc_ref` 连续调用两次，疑似引用计数 +2 而非 +1
- **方案**: 验证 fork 语义是否正确（父+子各持引用），如设计正确加注释说明，如为 bug 则修复
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/cow.rs:224-225`
- **详情**: 确认为 bug，fork 后 count 应从 1 变为 2，只需调用一次 cow_inc_ref。已修复并添加注释。

### M2: Swap SimpleSpinLock 统一为 IrqSpinLock ✅

- **描述**: `swap.rs:464-488` 中 `SimpleSpinLock` 不关中断，与项目其他地方使用的 `IrqSpinLock` 不一致，softirq 上下文有死锁风险
- **方案**: 将 `SimpleSpinLock` 替换为 `IrqSpinLock`，移除自定义锁实现
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/swap.rs:464-488`
- **详情**: 已移除 SimpleSpinLock 定义，替换为 IrqSpinLock<()>，重构所有 lock/unlock 调用为 guard 模式。

### M3: pcache dead_code 清理 ✅

- **描述**: `pcache.rs:26` 文件级 `#![allow(dead_code)]` 掩盖大量未使用函数/结构体
- **方案**: 审查实际使用情况，移除死代码或标记 `#[cfg(feature = "future")]`
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/pcache.rs:26`
- **详情**: 审查确认所有代码均在 VFS 写回路径中使用，更新注释说明 allow 的原因。

---

## 中优先级 — 不必要的 Linux 复杂度或死代码

### M4: MLFQ 残留代码审查

- **描述**: `scheduler.rs:70-71,933,1196` 中 MLFQ 已退役但常量/队列/`boost_priority` 残留
- **方案**: 移除 `MLFQ_LEVELS`/`TIME_SLICES`/`queues[MLFQ_LEVELS]`/`boost_priority` 等死代码
- **状态**: [X]
- **文件**: `src/kernel/framework/proc/scheduler.rs:70-71,933,1196`
- **详情**: 审查确认 MLFQ 代码仍在 `has_runnable` 中使用（调试/监控用途），非死代码。保留现状。

### M5: VMA 未实现标志位移除 ✅

- **描述**: `vma.rs:60-66` 中 MADV_MERGEABLE/HUGEPAGE/DONTFORK/POPULATE 等标志定义但未实现
- **方案**: 在 `madvise_range` 中对未实现的 advice 返回 `EINVAL`，或在定义处标注"Linux ABI 兼容占位，QX 未激活"
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/vma.rs:60-66`
- **详情**: 已移除 12 个未实现的 VmFlags 定义 (KSM/THP/DONTFORK/POPULATE/SOFT_OFFLINE/COLD)，更新 madvise_range 返回 EINVAL。

### M6: VMA 死标志位清理 ✅

- **描述**: `vma.rs:79-82` 中 `_PAGEOUT_DONE/_DONTNEED_DONE` 标志只设不读
- **方案**: 清理或实现对应的读取路径
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/vma.rs:79-82`
- **详情**: 确认只设不读，已移除标志定义和设置代码。

### M7: madvise advice 类型安全枚举

- **描述**: `vma.rs:693-719` 中 madvise advice 使用裸整数常量 (4=MADV_DONTNEED 等)
- **方案**: 定义 `enum MadviseAdvice` 并在 syscall 入口处做一次转换
- **状态**: []
- **文件**: `src/kernel/framework/mm/vma.rs:693-719`

### M8: OOMD Emergency 超时标注 ✅

- **描述**: `oomd.rs:86-88` 中 Emergency 超时日志说 "killing largest RSS" 但实际未真正 kill 进程
- **方案**: 实现 SIGTERM/SIGKILL 发送，或明确标注为 TODO
- **状态**: [X]
- **文件**: `src/kernel/services/proc/oomd.rs:86-88`
- **详情**: 已添加 TODO 注释说明当前仅计数未真正 kill 进程。

### M9: /proc/version 标识修改 ✅

- **描述**: `procfs_core.rs:242` 硬编码假的 Ubuntu 编译器信息: `"gcc (Ubuntu 11.3.0) 11.3.0, GNU ld (GNU Binutils for Ubuntu) 2.38"`
- **方案**: 改为 `"QueenX version X.Y.Z"`，移除假的 Ubuntu/gcc/ld 信息；保留 `uname -r` 返回兼容值
- **状态**: [X]
- **文件**: `src/kernel/services/fs/procfs_core.rs:242`
- **详情**: 已改为 "QueenX version 0.1.0 (rustc 1.78.0)"，移除假的 Ubuntu/gcc/ld 信息。

### M10: prctl 魔数具名常量化 ✅

- **描述**: `seccomp.rs:334-367` 中 prctl 用魔数 22/21/38/39 作为 option 参数
- **方案**: 定义 `PR_SET_SECCOMP = 22` / `PR_GET_SECCOMP = 21` 等具名常量
- **状态**: [X]
- **文件**: `src/kernel/services/proc/seccomp.rs:334-367`
- **详情**: 已定义 PR_SET_SECCOMP/PR_GET_SECCOMP/PR_SET_NO_NEW_PRIVS/PR_GET_NO_NEW_PRIVS 常量。

### M11: VMA 未实现 madvise 返回 EINVAL ✅

- **描述**: MADV_DONTFORK/DOFORK/POPULATE_READ/WRITE 未找到实际实现
- **方案**: 在 `madvise_range` 中对这些 advice 返回 `EINVAL`，避免静默设置标志位但无效果
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/vma.rs:693-719`
- **详情**: 已在 M5 中完成，madvise_range 对未实现项返回 EINVAL。

---

## 低优先级 — 建议优化

### M12: 命名空间代码去重

- **描述**: `namespace.rs:119-484` 中 7 种命名空间结构体代码高度重复
- **方案**: 提取 `trait Namespace` 或使用宏减少样板代码
- **状态**: []
- **文件**: `src/kernel/services/proc/namespace.rs:119-484`

### M13: NsRegistry 查找优化

- **描述**: `namespace.rs:660-701` 中 `NsRegistry` 使用线性扫描查找
- **方案**: 注明扩展计划，或改为 HashMap 查找
- **状态**: []
- **文件**: `src/kernel/services/proc/namespace.rs:660-701`

### M14: sys_setns 转换逻辑简化

- **描述**: `namespace.rs:733-747` 中 `from_clone_flag` 转换不直观
- **方案**: 简化为直接 match ns_type 数值
- **状态**: []
- **文件**: `src/kernel/services/proc/namespace.rs:733-747`

### M15: QueenX 特定 magic 标记

- **描述**: `process.rs:27`/`canary.rs:18-19`/`kmalloc.rs:27` 使用 `0xDEADBEEF` 经典 Linux magic
- **方案**: 改为 QueenX 特定标记 (如 `0x51414E58` = "QXAN") 以便 crash dump 识别来源
- **状态**: []
- **文件**: `src/kernel/framework/proc/process.rs:27`, `canary.rs:18-19`, `mm/kmalloc.rs:27`

### M16: pcache 桶初始化宏简化

- **描述**: `pcache.rs:260-325` 中 64 桶手动展开为重复代码
- **方案**: 用宏简化初始化
- **状态**: []
- **文件**: `src/kernel/framework/mm/pcache.rs:260-325`

### M17: 配置常量统一管理

- **描述**: `pcache.rs:110-113`/`swap.rs:107` 中容量常量硬编码
- **方案**: 移到 `config.rs` 统一管理
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/pcache.rs:110-113`, `swap.rs:107`
- **详情**: 优化项，当前常量已定义在各自模块中，暂不移动。后续重构时统一。

### M18: Swap 状态枚举化 ✅

- **描述**: `swap.rs:110-111` 中 `SLOT_FREE/SLOT_USED` 使用裸常量
- **方案**: 使用 enum 提升类型安全
- **状态**: [X]
- **文件**: `src/kernel/framework/mm/swap.rs:110-111`
- **详情**: 已定义 `enum SlotState { Free, Used }`，替换裸常量。

---

## 工作量汇总

| 优先级 | 数量 | 预计工期 |
|--------|------|----------|
| 高 | 3 项 | 1-2 天 |
| 中 | 8 项 | 3-5 天 |
| 低 | 7 项 | 2-3 天 |
| **总计** | **18 项** | **6-10 天** |
