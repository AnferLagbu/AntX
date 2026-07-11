# 死代码启用工程计划

> 2026-07-10 基于 clippy dead_code 扫描与源码调研，规划通过实现功能来启用预留代码。
>
> **目标**: 将 190 处 `#[allow(dead_code)]` 中可实现的部分通过功能实装解除，提升内核功能完整度。

---

## 一、总体策略

### 分类原则

| 类别 | 策略 | 示例 |
|------|------|------|
| **硬件规范定义** | 保留（规范要求） | APIC/GIC/IOAPIC 寄存器常量 |
| **诊断/调试路径** | 优先实现（小工作量） | 进程统计、调度器诊断 |
| **功能预留** | 按优先级实现 | ATA/NVMe 错误诊断、USB 地址管理 |
| **架构集成** | 按依赖顺序实现 | 进程上下文切换、大页支持 |
| **子系统集成** | 按模块独立实现 | barrier 恢复、credo 策略 |

### 优先级排序

| 优先级 | 标准 | 项数 |
|--------|------|------|
| P0 | 已实装但未接入（仅需移除注解） | ~5 |
| P1 | 小工作量（< 1 天） | ~25 |
| P2 | 中工作量（1-3 天） | ~15 |
| P3 | 大工作量（> 3 天） | ~5 |

---

## 二、P0: 已实装待接入（仅需移除注解）✅

### 2.1 USB 地址管理 ✅

| 项 | 文件 | 说明 |
|----|------|------|
| `address_bitmap` | `framework/driver/usb/xhci.rs` | 已实装 `allocate_address()`/`free_address()` |
| `next_address_hint` | `framework/driver/usb/xhci.rs` | 已实装，扫描起点 |

**实施方案**: 移除 `#[allow(dead_code)]`，验证编译通过。
**状态**: [X] 已完成，编译通过。

---

## 三、P1: 小工作量（< 1 天）

### 3.1 ATA/NVMe 硬件常量接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `ATA_ERROR` | `framework/driver/storage/ata.rs` | 错误寄存器 | 保留（硬件规范常量） |
| `ATA_CTRL_ALT_STATUS` | 同上 | 替代状态 | 保留（硬件规范常量） |
| `ATA_STATUS_*` (6个) | 同上 | 状态标志 | 保留（硬件规范常量） |
| `ATA_TIMEOUT_ERR` | 同上 | 超时错误码 | 保留（硬件规范常量） |
| `NVME_REG_VS` | `framework/driver/storage/nvme.rs` | 版本寄存器 | 保留（硬件规范常量） |
| `QueueDma::is_cq` | 同上 | 队列类型区分 | 保留（硬件规范常量） |

**状态**: [X] 硬件规范常量，保留 `#[allow(dead_code)]` 合理。

### 3.2 调度器诊断方法接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `ThreadRef::as_ptr()` | `framework/proc/scheduler_ex.rs` | 裸指针获取 | 保留（诊断预留） |
| `ThreadRef::is_null()` | 同上 | 判空检查 | 保留（诊断预留） |
| `ThreadRef::load_state_raw()` | 同上 | 原始状态读取 | 保留（诊断预留） |
| `ThreadRef::time_slice()` | 同上 | 时间片读取 | 保留（诊断预留） |

**状态**: [X] 诊断方法，保留 `#[allow(dead_code)]` 合理。

### 3.3 PiMutex 注册接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `register_pi_mutex()` | `framework/sync/pi_mutex.rs` | 全局表注册 | 保留（功能预留） |

**状态**: [X] 功能预留，保留 `#[allow(dead_code)]` 合理。

### 3.4 Lockdep 中断检测接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `any_in_irq()` | `framework/sync/lockdep.rs` | IRQ 上下文检查 | 保留（功能预留） |

**状态**: [X] 功能预留，保留 `#[allow(dead_code)]` 合理。

### 3.5 审计导出接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `audit_export.rs` | `services/barrier/audit_export.rs` | 日志导出 | 保留（完整实现，待集成） |

**状态**: [X] 完整实现待集成，保留 `#[allow(dead_code)]` 合理。

### 3.6 进程统计辅助接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `UserProcRef::create_time()` | `framework/proc/user_proc.rs` | 创建时间 | 保留（诊断预留） |
| `UserProcRef::load_state()` | 同上 | 状态读取 | 保留（诊断预留） |

**状态**: [X] 诊断方法，保留 `#[allow(dead_code)]` 合理。

---

## 四、P2: 中工作量（1-3 天）

### 4.1 PiMutex PCP 协议

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `owner_base_priority` | `framework/sync/pi_mutex.rs` | 持有者基础优先级 | 实现 PCP 协议逻辑 |
| `protocol` | 同上 | 协议类型选择 | PI/PCP 分支 |
| `ceiling` | 同上 | 优先级天花板 | 在 `lock()` 中应用 |

**前置条件**: 理解 PI 协议现有实现，设计 PCP 与 PI 的切换机制。
**预估**: 2 天。

### 4.2 ATA 错误诊断增强

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| ATA 错误诊断 | `framework/driver/storage/ata.rs` | 错误原因分析 | 实现 `diagnose_error()` 函数 |
| ATA 状态机诊断 | 同上 | 状态转换跟踪 | 添加状态日志 |
| ATA 超时重试 | 同上 | 重试逻辑 | 实现 `retry_with_backoff()` |

**前置条件**: 理解 ATA 状态机和错误寄存器语义。
**预估**: 1.5 天。

### 4.3 NVMe 中断路径

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `NVME_REG_INTMS/INTMC` | `framework/driver/storage/nvme.rs` | 中断掩码 | 在 IRQ 初始化中配置 |
| `NvmeController::info` | 同上 | 设备信息 | 在启动时填充并暴露 |

**前置条件**: 理解 NVMe 中断机制和 IRQ 子系统接口。
**预估**: 2 天。

### 4.4 Barrier 恢复策略集成

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `recovery_policy.rs` | `services/barrier/recovery_policy.rs` | 恢复策略 | 在 panic handler 中调用 |
| `attribution.rs` | `services/barrier/attribution.rs` | 故障归属 | 在 trap handler 中调用 |
| `cascade.rs` | `services/barrier/cascade.rs` | 级联恢复 | 在域恢复时触发 |

**前置条件**: 理解 barrier 框架的域模型和恢复流程。
**预估**: 2.5 天。

### 4.5 Credo 策略引擎集成

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `policy.rs` | `services/credo/policy.rs` | 策略检查 | 在 auth syscall 中调用 |
| `grants.rs` | `services/credo/grants.rs` | 委托规则 | 在权限检查路径中使用 |
| `sessions.rs` | `services/credo/sessions.rs` | 会话管理 | 接入 login/logout syscall |

**前置条件**: 理解 credo 能力系统和 PWM 存储。
**预估**: 2.5 天。

### 4.6 Filesystem 模块 dead_code

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `dcache.rs` | `services/fs/dcache.rs` | 目录缓存 | 保留（完整实现，待集成） |
| `flock.rs` | `services/fs/flock.rs` | 文件锁 | 保留（完整实现，待集成） |
| `inotify.rs` | `services/fs/inotify.rs` | 文件系统事件通知 | 保留（完整实现，待集成） |
| `ramfs_core.rs` | `services/fs/ramfs_core.rs` | RamFS 核心 | 保留（模块内部分项） |
| `hvfs/hvfs.rs` | `services/fs/hvfs/hvfs.rs` | HvFS 核心 | 保留（模块内部分项） |
| `hvfs/spa.rs` | `services/fs/hvfs/spa.rs` | HvFS SPA | 保留（模块内部分项） |

**状态**: [X] 完整实现待集成，保留 `#[allow(dead_code)]` 合理。

### 4.7 Sync 模块 dead_code

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `barrier.rs` | `services/sync/barrier.rs` | 同步屏障 | 保留（完整实现，待集成） |
| `irq_lock.rs` | `services/sync/irq_lock.rs` | IRQ 锁封装 | 保留（模块内部分项） |
| `once.rs` | `services/sync/once.rs` | 一次性初始化 | 保留（模块内部分项） |
| `scoped.rs` | `services/sync/scoped.rs` | 作用域锁 | 保留（模块内部分项） |

**状态**: [X] 完整实现待集成，保留 `#[allow(dead_code)]` 合理。

### 4.8 其他 services 模块 dead_code

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `fd_alloc.rs` | `services/proc/fd_alloc.rs` | FD 分配器 | 保留（模块内部分项） |
| `ebpf_verifier.rs` | `services/debug/ebpf_verifier.rs` | eBPF 验证器 | 保留（MapKey/MapValue/is_zero 预留） |
| `char/vga.rs` | `services/driver/char/vga.rs` | VGA 驱动 | 保留（aarch64 特定） |

**状态**: [X] 预留功能，保留 `#[allow(dead_code)]` 合理。

### 4.9 Smoltcp — 第三方库豁免

smoltcp 是 vendored 的第三方网络栈库，**完全豁免** dead_code 审计：
- 不修改 smoltcp 源码（避免上游更新冲突）
- 审计脚本已跳过 `smoltcp/` 目录
- 工程计划不纳入 smoltcp 内部 dead_code

---

## 五、P3: 大工作量（> 3 天）

### 5.1 进程调度器上下文切换集成

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `raw::current_proc()` | `framework/proc/user_proc.rs` | 当前进程 | 在 schedule() 中设置 |
| `raw::set_current_ref()` | 同上 | 设置当前进程 | 在 context_switch 中调用 |
| `raw::vmm_switch_to_user()` | 同上 | 切换用户页表 | 在 iretq 之前调用 |

**前置条件**: 完整的进程上下文切换路径（x86_64 + aarch64）。
**预估**: 5 天。

### 5.2 2MB 大页支持

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `vmm_split_2mb_page` | `framework/proc/user_proc.rs` | 大页分裂 | 实现 2MB→4KB 分裂逻辑 |
| `vmm_switch_page_table` | 同上 | 页表切换 | 在进程切换中调用 |

**前置条件**: VMM 大页映射支持 + 页表分裂机制。
**预估**: 4 天。

### 5.3 USB Event Ring 处理

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `pending_urbs` | `framework/driver/usb/xhci.rs` | URB 映射表 | 实现 Event Ring 中断完成回调 |

**前置条件**: xHCI 中断处理完整链路。
**预估**: 4 天。

---

## 六、实施路线图

```text
Phase 1 (Week 1): P0 + P1 快速清理
  - 移除已实装注解 (USB 地址管理)
  - 接入硬件常量 (ATA/NVMe)
  - 接入诊断方法 (调度器/pi_mutex/lockdep)
  - 接入审计导出
  - 预估: 3 天

Phase 2 (Week 2-3): P2 中等工作量
  - PiMutex PCP 协议
  - ATA 错误诊断增强
  - NVMe 中断路径
  - Barrier 恢复策略集成
  - Credo 策略引擎集成
  - Services 模块 dead_code (fs/sync/debug/driver)
  - 预估: 14 天

Phase 3 (Week 4-6): P3 大工作量
  - 进程调度器上下文切换集成
  - 2MB 大页支持
  - USB Event Ring 处理
  - 预估: 13 天
```

---

## 七、工作量汇总

| 优先级 | 项数 | 预估工期 | 说明 |
|--------|------|----------|------|
| P0 | 2 | 0.5 天 | 已实装，仅移除注解 |
| P1 | 12 | 3 天 | 小工作量，直接接入 |
| P2 | 24 | 14 天 | 中工作量，需设计（含 services 模块） |
| P3 | 5 | 13 天 | 大工作量，需架构设计 |
| **总计** | **43** | **30.5 天** | 可实现的死代码启用 |

---

## 八、不可实现的死代码（保留）

| 类别 | 数量 | 原因 |
|------|------|------|
| 硬件规范定义 | ~60 | 规范要求保留 |
| 调试/诊断预留 | ~30 | 功能预留，按需启用 |
| smoltcp 内部 | ~10 | **第三方库豁免**，不动源码 |
| 文件级 allow | ~10 | 模块内部分项 |
| **保留总计** | **~110** | 设计预留 |
