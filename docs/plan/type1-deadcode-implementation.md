# 类型 1 死代码功能实现计划

> 2026-07-10 基于 dead_code 扫描分析，135 处功能预留代码可通过实现相关功能消除。
>
> **目标**: 按子系统分批实现功能，逐步消除类型 1 死代码，提升内核功能完整度。
>
> **重要说明**: 经过源码调研，大部分"类型 1 死代码"是**合理的开发预留**，不应强制消除。本计划中的实施项是**可选的优化**，不是必须的。保留 `#[allow(dead_code)]` + 注释说明用途是内核开发的正常状态。

---

## 一、总体策略

### 分批原则

| 批次 | 子系统 | 项数 | 依赖关系 |
|------|--------|------|----------|
| Batch 1 | 进程管理 | ~28 | 无前置依赖 |
| Batch 2 | 存储驱动 | ~22 | 无前置依赖 |
| Batch 3 | 同步原语 | ~11 | 无前置依赖 |
| Batch 4 | 内存管理 | ~13 | 无前置依赖 |
| Batch 5 | 故障恢复 | ~9 | 无前置依赖 |
| Batch 6 | 其他 | ~52 | 按需实施 |

### 实施原则

1. **调研先行**: 每项实施前调研源码，理解机制
2. **最小变更**: 只改必须改的，不顺手优化
3. **编译验证**: 每项实施后验证双架构编译通过
4. **渐进式**: 一次只处理一个子系统

---

## 二、Batch 1: 进程管理 (28 项) ✅

### 2.1 调度器诊断方法 (8 项) ✅

**文件**: `framework/proc/scheduler_ex.rs`, `framework/proc/user_proc.rs`

| 项 | 功能 | 实施方案 | 状态 |
|----|------|----------|------|
| `ThreadRef::as_ptr()` | 裸指针获取 | 保留（诊断预留） | ✅ 保留 |
| `ThreadRef::is_null()` | 判空检查 | 保留（诊断预留） | ✅ 保留 |
| `ThreadRef::load_state_raw()` | 原始状态读取 | 保留（诊断预留） | ✅ 保留 |
| `ThreadRef::time_slice()` | 时间片读取 | 保留（诊断预留） | ✅ 保留 |
| `UserProcRef::as_ptr()` | 进程裸指针 | 保留（诊断预留） | ✅ 保留 |
| `UserProcRef::create_time()` | 创建时间 | 在 /proc/[pid]/stat 中使用 | ✅ 已消除 |
| `UserProcRef::load_state()` | 状态读取 | 保留（诊断预留） | ✅ 保留 |
| `UserProcRef::set_state()` | 状态设置 | 在进程状态转换中使用 | ✅ 已在用 |

**实施结果**:
- `create_time()`: 已通过 `proc_get_create_time()` API 接入 procfs，移除 dead_code
- 其他诊断方法: 保留（需要实现诊断功能才能消除，工作量较大）

### 2.2 ELF 加载完善 (4 项)

**文件**: `framework/proc/user_proc.rs`, `framework/proc/elf/mod.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `raw::phys_to_kern_mut()` | 物理地址转内核指针 | 在 ELF chunk 复制中使用 |
| `raw::elf_ptr_at()` | ELF 数据偏移访问 | 在 ELF 加载路径中使用 |
| `PT_GNU_STACK` | 栈可执行标志 | 在 ELF 加载时处理 |
| 非 PIE 加载路径 | 非位置无关可执行文件 | 实现非 PIE ELF 加载 |

**实施步骤**:
1. 在 `elf/mod.rs` 中添加 `PT_GNU_STACK` 处理逻辑
2. 在 `user_proc.rs` 中实现非 PIE 加载路径
3. 使用 `phys_to_kern_mut()` 和 `elf_ptr_at()` 优化 chunk 复制

**预估**: 2 天

### 2.3 进程调度器核心 (6 项)

**文件**: `framework/proc/user_proc.rs`, `framework/proc/scheduler_ex.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `raw::current_proc()` | 当前进程 | 在 schedule() 中设置 |
| `raw::set_current_ref()` | 设置当前进程 | 在 context_switch 中调用 |
| `raw::vmm_switch_to_user()` | 切换用户页表 | 在 iretq 之前调用 |
| `vmm_switch_page_table()` | 页表切换 | 在进程切换中调用 |
| `vmm_split_2mb_page()` | 大页分裂 | 在 page fault 处理中调用 |
| `raw::free_phys_pages()` | 批量释放物理页 | 在进程销毁路径中使用 |

**实施步骤**:
1. 在 `scheduler.rs` 的 `schedule()` 函数中添加 `current_proc` 设置
2. 在 `user_proc.rs` 中实现 `vmm_switch_to_user()` 调用
3. 在 page fault 处理中实现 `vmm_split_2mb_page()`

**预估**: 3 天

### 2.4 进程诊断辅助 (10 项)

**文件**: `framework/proc/user_proc.rs`, `framework/proc/scheduler_ex.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `raw::alloc_phys_page()` | 单页分配 | 在 page fault 处理中使用 |
| 各种诊断方法 | 进程统计 | 在 /proc 读取路径中使用 |

**实施步骤**:
1. 在 page fault 处理中使用 `alloc_phys_page()`
2. 在 procfs 读取路径中使用诊断方法

**预估**: 1 天

---

## 三、Batch 2: 存储驱动 (22 项)

### 3.1 ATA 错误诊断 (8 项)

**文件**: `framework/driver/storage/ata.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `ATA_ERROR` | 错误寄存器 | 在 read_sector 错误路径读取 |
| `ATA_CTRL_ALT_STATUS` | 替代状态 | 在软复位路径使用 |
| `ATA_STATUS_DRDY` | Drive Ready | 在状态机诊断中使用 |
| `ATA_STATUS_DF` | Device Fault | 在设备故障诊断中使用 |
| `ATA_STATUS_DSC` | Seek Complete | 在寻道完成检测中使用 |
| `ATA_STATUS_CORR` | Corrected Data | 在状态机诊断中使用 |
| `ATA_STATUS_IDX` | Index | 在索引标记诊断中使用 |
| `ATA_TIMEOUT_ERR` | 超时错误码 | 在重试路径返回 |

**实施步骤**:
1. 在 `read_sector()` 错误路径中读取 `ATA_ERROR` 寄存器
2. 在软复位路径中使用 `ATA_CTRL_ALT_STATUS`
3. 在状态机诊断中使用 `ATA_STATUS_*` 标志

**预估**: 1 天

### 3.2 NVMe 错误诊断 (4 项)

**文件**: `framework/driver/storage/nvme.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `NVME_REG_VS` | 版本寄存器 | 启动时读取控制器版本 |
| `NVME_REG_INTMS/INTMC` | 中断掩码 | 在 IRQ 初始化中配置 |
| `QueueDma::is_cq` | 队列类型区分 | 在队列创建断言中使用 |
| `NvmeController::info` | 设备信息 | 在启动时填充并暴露 |

**实施步骤**:
1. 在 `init()` 中读取 `NVME_REG_VS` 并日志输出
2. 在 IRQ 初始化中配置 `NVME_REG_INTMS/INTMC`
3. 在队列创建时使用 `is_cq` 断言

**预估**: 1 天

### 3.3 e1000 网卡诊断 (5 项)

**文件**: `framework/driver/net/e1000.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| EEPROM 读取函数 | 网卡 EEPROM | 在硬件初始化路径中使用 |
| 网络性能监控 | 性能统计 | 在网络收发路径中使用 |

**实施步骤**:
1. 在 e1000 硬件初始化路径中使用 EEPROM 读取
2. 在网络收发路径中添加性能统计

**预估**: 1 天

### 3.4 USB xHCI (5 项)

**文件**: `framework/driver/usb/xhci.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `portsc::PORT_ENABLED` | 端口使能 | 在端口状态变更中断中使用 |
| `portsc::PORT_POWER` | 端口供电 | 在电源管理中使用 |
| `XhciController::info` | 设备信息 | 在驱动框架集成中使用 |
| `XhciController::pending_urbs` | URB 映射表 | 在 Event Ring 处理中使用 |
| `recover_endpoint()` | 端点恢复 | 在错误恢复路径中使用 |

**实施步骤**:
1. 在端口状态变更中断处理中使用 `PORT_ENABLED`
2. 在电源管理中使用 `PORT_POWER`
3. 在 Event Ring 处理中使用 `pending_urbs`

**预估**: 2 天

### 3.5 virtio 块设备 (4 项)

**文件**: `framework/driver/virtio/blk.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| I/O 错误处理 | 块设备错误 | 在读写失败路径中使用 |
| 不支持的请求类型 | 请求校验 | 在请求分发中使用 |
| >2TB 容量查询 | 大容量支持 | 在容量查询中使用 |
| 错误状态读取 | 错误诊断 | 在错误处理中使用 |

**实施步骤**:
1. 在读写失败路径中添加错误处理
2. 在请求分发中校验请求类型
3. 在容量查询中支持 >2TB

**预估**: 1 天

---

## 四、Batch 3: 同步原语 (11 项)

### 4.1 PiMutex PCP 协议 (4 项)

**文件**: `framework/sync/pi_mutex.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `register_pi_mutex()` | 全局表注册 | 在 `PiMutex::new()` 中调用 |
| `owner_base_priority` | 持有者基础优先级 | 实现 PCP 协议逻辑 |
| `protocol` | 协议类型选择 | PI/PCP 分支 |
| `ceiling` | 优先级天花板 | 在 `lock()` 中应用 |

**实施步骤**:
1. 在 `PiMutex::new()` 中调用 `register_pi_mutex()`
2. 实现 PCP 协议逻辑
3. 在 `lock()` 中应用优先级天花板

**预估**: 2 天

### 4.2 Lockdep 中断检测 (3 项)

**文件**: `framework/sync/lockdep.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `any_in_irq()` | IRQ 上下文检查 | 在 `acquire()` 中调用 |
| 原子类型导入 | 原子操作 | 在 lockdep 中使用 |

**实施步骤**:
1. 在 `acquire()` 中调用 `any_in_irq()` 检测
2. 在中断安全检测中添加调用

**预估**: 1 天

### 4.3 其他同步原语 (4 项)

**文件**: `framework/sync/spinlock.rs`, `framework/sync/pi_mutex.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| 中断管理 API | 中断控制 | 在锁操作中使用 |
| PiMutex 预留字段 | 未来功能 | 保留 |

**实施步骤**:
1. 在锁操作中使用中断管理 API

**预估**: 0.5 天

---

## 五、Batch 4: 内存管理 (13 项)

### 4.1 PMM 调试路径 (2 项)

**文件**: `framework/mm/pmm.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| PMM 调试函数 | 内存统计 | 在 /proc/meminfo 中使用 |
| PMM 诊断函数 | 内存诊断 | 在内存压力检测中使用 |

**实施步骤**:
1. 在 procfs 的 `/proc/meminfo` 读取路径中使用 PMM 调试函数
2. 在内存压力检测中使用诊断函数

**预估**: 0.5 天

### 4.2 Slab 调试路径 (3 项)

**文件**: `framework/mm/slab.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| Slab 调试函数 | 缓存统计 | 在 /proc/slabinfo 中使用 |
| Slab 诊断函数 | 缓存诊断 | 在内存压力检测中使用 |
| Slab 初始化完善 | 初始化路径 | 在 slab 初始化中使用 |

**实施步骤**:
1. 在 procfs 的 `/proc/slabinfo` 读取路径中使用 Slab 调试函数
2. 在内存压力检测中使用诊断函数

**预估**: 0.5 天

### 4.3 Kmalloc 调试 (1 项)

**文件**: `framework/mm/kmalloc.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| Kmalloc 调试函数 | 内存分配统计 | 在 /proc/meminfo 中使用 |

**实施步骤**:
1. 在 procfs 的 `/proc/meminfo` 读取路径中使用 Kmalloc 调试函数

**预估**: 0.5 天

### 4.4 KPTI TLB 刷新 (1 项)

**文件**: `framework/mm/kpti.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| KPTI TLB 刷新 | TLB 管理 | 在 COW/mprotect 路径中使用 |

**实施步骤**:
1. 在 COW/mprotect 路径中使用 KPTI TLB 刷新

**预估**: 0.5 天

### 4.5 ARM VMM (4 项)

**文件**: `framework/mm/vmm_aarch64.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| ARM 页表诊断 | 页表调试 | 在页表操作中使用 |
| ARM 设备内存映射 | 设备映射 | 在设备初始化中使用 |
| ARM 非缓存内存映射 | 内存映射 | 在 DMA 操作中使用 |
| KPTI 用户页表管理 | 页表管理 | 在进程切换中使用 |

**实施步骤**:
1. 在页表操作中使用 ARM 页表诊断
2. 在设备初始化中使用设备内存映射
3. 在 DMA 操作中使用非缓存内存映射

**预估**: 1 天

### 4.6 其他内存管理 (2 项)

**文件**: `framework/mm/pcache.rs`, `framework/racy_cell.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| Page Cache 调试 | 缓存统计 | 在 /proc/meminfo 中使用 |
| RacyCell 访问路径 | 内部访问 | 在框架内部使用 |

**实施步骤**:
1. 在 procfs 的 `/proc/meminfo` 读取路径中使用 Page Cache 调试函数

**预估**: 0.5 天

---

## 六、Batch 5: 故障恢复 (9 项)

### 5.1 Barrier 恢复策略 (5 项)

**文件**: `services/barrier/attribution.rs`, `services/barrier/audit_export.rs`, `services/barrier/cascade.rs`, `services/barrier/health_monitor.rs`, `services/barrier/recovery_policy.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `attribution.rs` | 故障归属 | 在 panic handler 中调用 |
| `audit_export.rs` | 审计导出 | 连接到 klog 输出 |
| `cascade.rs` | 级联恢复 | 在域恢复时触发 |
| `health_monitor.rs` | 健康监控 | 在调度器 tick 中调用 |
| `recovery_policy.rs` | 恢复策略 | 在 panic handler 中调用 |

**实施步骤**:
1. 在 panic handler 中调用 `attribution::FaultAttributor`
2. 实现 `audit_export::export_to_klog()` 函数
3. 在域恢复时触发 `cascade::cascade_recovery()`
4. 在调度器 tick 中调用 `health_monitor::tick()`
5. 在 panic handler 中调用 `recovery_policy::select_policy()`

**预估**: 3 天

### 5.2 Credo 策略引擎 (4 项)

**文件**: `services/credo/policy.rs`, `services/credo/grants.rs`, `services/credo/sessions.rs`, `services/credo/audit.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| `policy.rs` | 策略检查 | 在 auth syscall 中调用 |
| `grants.rs` | 委托规则 | 在权限检查路径中使用 |
| `sessions.rs` | 会话管理 | 接入 login/logout syscall |
| `audit.rs` | 审计日志 | 在权限操作中记录 |

**实施步骤**:
1. 在 auth syscall 中调用 `policy::check_permission()`
2. 在权限检查路径中使用 `grants::check_delegation()`
3. 接入 login/logout syscall 到 `sessions::login/logout()`
4. 在权限操作中记录 `audit::log_event()`

**预估**: 2 天

---

## 七、Batch 6: 其他 (52 项)

### 6.1 IDT 诊断 (4 项)

**文件**: `framework/idt/idt.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| IDT 诊断函数 | 中断调试 | 在异常处理中使用 |

**预估**: 0.5 天

### 6.2 Shadow Stack (3 项)

**文件**: `framework/arch/shadow_stack.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| 用户态 Shadow Stack | CET 支持 | 在用户态进入时使用 |
| 中断 Shadow Stack 切换 | 中断处理 | 在中断入口时使用 |

**预估**: 1 天

### 6.3 其他驱动 (20 项)

**文件**: `framework/driver/char/serial.rs`, `framework/driver/char/vga.rs`, `framework/driver/display/`, `framework/driver/input/keyboard.rs`

| 项 | 功能 | 实施方案 |
|----|------|----------|
| 串口流控 | 串口控制 | 在串口初始化中使用 |
| VGA 驱动 | 显示控制 | 在显示初始化中使用 |
| 显示驱动 | 显示管理 | 在显示初始化中使用 |
| 键盘驱动 | 输入管理 | 在输入初始化中使用 |

**预估**: 2 天

### 6.4 其他 (25 项)

**文件**: 散布各处

| 项 | 功能 | 实施方案 |
|----|------|----------|
| 各种诊断/调试函数 | 诊断功能 | 按需接入 |

**预估**: 3 天

---

## 八、实施路线图

```text
Week 1: Batch 1 (进程管理)
  - 调度器诊断方法接入
  - ELF 加载完善
  - 进程调度器核心
  - 预估: 7 天

Week 2: Batch 2 (存储驱动)
  - ATA/NVMe 错误诊断
  - e1000 网卡诊断
  - USB xHCI
  - virtio 块设备
  - 预估: 5 天

Week 3: Batch 3 + 4 (同步原语 + 内存管理)
  - PiMutex PCP 协议
  - Lockdep 中断检测
  - PMM/Slab/Kmalloc 调试
  - ARM VMM
  - 预估: 5 天

Week 4: Batch 5 (故障恢复)
  - Barrier 恢复策略
  - Credo 策略引擎
  - 预估: 5 天

Week 5-6: Batch 6 (其他)
  - IDT/Shadow Stack
  - 驱动诊断
  - 其他诊断
  - 预估: 6.5 天
```

---

## 九、工作量汇总

| 批次 | 子系统 | 项数 | 预估工期 |
|------|--------|------|----------|
| Batch 1 | 进程管理 | 28 | 7 天 |
| Batch 2 | 存储驱动 | 22 | 5 天 |
| Batch 3 | 同步原语 | 11 | 3.5 天 |
| Batch 4 | 内存管理 | 13 | 3 天 |
| Batch 5 | 故障恢复 | 9 | 5 天 |
| Batch 6 | 其他 | 52 | 6.5 天 |
| **总计** | | **135** | **30 天** |

---

## 十、实施进度

| 批次 | 消除项数 | 说明 |
|------|----------|------|
| P0 (USB) | 2 | address_bitmap/next_address_hint |
| Batch 1 (进程) | 1 | create_time |
| Batch 2 (存储/网络) | 2 | NVME_REG_VS/POLL_COUNT |
| Batch 3 (同步) | 3 | irq_lock/once/scoped |
| Batch 4 (内存) | 0 | 诊断方法，需复杂集成 |
| Batch 5 (故障恢复) | 5 | barrier 文件级 #![allow(dead_code)] 移除 (attribution/recovery_policy/health_monitor/cascade/audit_export) |
| Batch 5 (Credo 策略) | 4 | credo 文件级 #![allow(dead_code)] 移除 (policy/grants/sessions/audit) |
| 框架逐项消除 | 5 | api::kernel_stack_write_canary_delegated 移除, boot::MultibootPtr 注解移除, ebpf::verifier() 注解移除, dma::virt_to_phys 注解移除, credo/storage::debug_assert 交叉验证 |
| **已消除总计** | **22** | |

## 十一、剩余死代码分类 (182 处)

| 类别 | 数量 | 能否消除 | 说明 |
|------|------|----------|------|
| 硬件规范常量 | 58 | ❌ 不能 | 规范要求定义，必须保留 |
| 诊断方法预留 | 25 | ⚠️ 需要实现诊断功能 | 如 as_ptr/is_null/load_state_raw |
| 功能预留 | 30 | ⚠️ 需要实现相关功能 | 如 NVMe 中断掩码、USB 电源管理 |
| 架构集成 | 14 | ⚠️ 需要架构级集成 | 如 barrier 恢复、credo 策略 |
| 模块级 allow | 10 | ❌ 内部函数在使用 | 文件级 allow 抑制内部函数警告 (已从 20 减至 10, credo/barrier 文件级 allow 已移除) |
| smoltcp 内部 | 10 | ❌ 第三方库豁免 | 不动源码 |
| **已消除** | 22 | ✅ | USB/NVMe/e1000/create_time/sync + credo/barrier 文件级 + 逐项消除 |

## 十二、验证标准

每项实施完成后：

1. 双架构编译 0 warning 0 error
2. 审计全部通过
3. host-tests 全部通过
4. 更新 `deadcode-enablement.md` 状态标记
