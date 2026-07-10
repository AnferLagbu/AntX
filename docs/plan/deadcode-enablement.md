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

## 二、P0: 已实装待接入（仅需移除注解）

### 2.1 USB 地址管理

| 项 | 文件 | 说明 |
|----|------|------|
| `address_bitmap` | `framework/driver/usb/xhci.rs` | 已实装 `allocate_address()`/`free_address()` |
| `next_address_hint` | `framework/driver/usb/xhci.rs` | 已实装，扫描起点 |

**实施方案**: 移除 `#[allow(dead_code)]`，验证编译通过。

---

## 三、P1: 小工作量（< 1 天）

### 3.1 ATA/NVMe 硬件常量接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `ATA_ERROR` | `framework/driver/storage/ata.rs` | 错误寄存器 | 在 `read_sector` 错误路径读取 error reg |
| `ATA_CTRL_ALT_STATUS` | 同上 | 替代状态 | 在软复位路径使用 |
| `ATA_STATUS_*` (6个) | 同上 | 状态标志 | 在状态机诊断中使用 |
| `ATA_TIMEOUT_ERR` | 同上 | 超时错误码 | 在重试路径返回 |
| `NVME_REG_VS` | `framework/driver/storage/nvme.rs` | 版本寄存器 | 启动时读取控制器版本并日志输出 |
| `QueueDma::is_cq` | 同上 | 队列类型区分 | 在队列创建断言中使用 |

**预估**: 0.5 天。仅需在现有诊断/日志路径中添加常量引用。

### 3.2 调度器诊断方法接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `ThreadRef::as_ptr()` | `framework/proc/scheduler_ex.rs` | 裸指针获取 | 在 debug 日志中使用 |
| `ThreadRef::is_null()` | 同上 | 判空检查 | 同上 |
| `ThreadRef::load_state_raw()` | 同上 | 原始状态读取 | 在状态 dump 中使用 |
| `ThreadRef::time_slice()` | 同上 | 时间片读取 | 在调试输出中使用 |

**预估**: 0.5 天。在 `dump_state()` 或 `debug_info()` 中调用。

### 3.3 PiMutex 注册接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `register_pi_mutex()` | `framework/sync/pi_mutex.rs` | 全局表注册 | 在 `PiMutex::new()` 中调用 |

**预估**: 0.5 天。添加一行调用。

### 3.4 Lockdep 中断检测接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `any_in_irq()` | `framework/sync/lockdep.rs` | IRQ 上下文检查 | 在 `acquire()` 中调用检测 |

**预估**: 0.5 天。在锁获取路径添加检测。

### 3.5 审计导出接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `audit_export.rs` | `services/barrier/audit_export.rs` | 日志导出 | 连接到 klog 输出 |

**预估**: 0.5 天。实现 `export_to_klog()` 函数。

### 3.6 进程统计辅助接入

| 项 | 文件 | 功能 | 实施方案 |
|----|------|------|----------|
| `UserProcRef::create_time()` | `framework/proc/user_proc.rs` | 创建时间 | 在 `/proc/[pid]/stat` 中使用 |
| `UserProcRef::load_state()` | 同上 | 状态读取 | 在进程列表查询中使用 |

**预估**: 0.5 天。在 procfs 读取路径中调用。

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
  - 预估: 10 天

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
| P2 | 15 | 10 天 | 中工作量，需设计 |
| P3 | 5 | 13 天 | 大工作量，需架构设计 |
| **总计** | **34** | **26.5 天** | 可实现的死代码启用 |

---

## 八、不可实现的死代码（保留）

| 类别 | 数量 | 原因 |
|------|------|------|
| 硬件规范定义 | ~60 | 规范要求保留 |
| 调试/诊断预留 | ~30 | 功能预留，按需启用 |
| smoltcp 相关 | ~20 | 固定使用 smoltcp |
| 文件级 allow | ~20 | 模块内部分项 |
| **保留总计** | **~130** | 设计预留 |
