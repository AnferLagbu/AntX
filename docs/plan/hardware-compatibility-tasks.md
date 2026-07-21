# 硬件兼容性开发任务

> IOAPIC/Local APIC/PS/2 子系统的硬件兼容性缺口，需独立开发补齐。

## 背景

QueenX 中断与输入子系统在基础功能验证通过后，缺少若干高级硬件特性支持。本文件记录已识别的缺口及对应开发任务。

---

## P0: IOAPIC 高级投递模式 — ✅ 已完成 (2026-07-21)

**风险等级:** 高
**涉及已删除常量:** `DELIVERY_SMI` (0x200), `DELIVERY_NMI` (0x400), `DELIVERY_EXTINT` (0x700)

### 问题描述

当前 IOAPIC 驱动 (`framework/arch/x86_64/ioapic.rs`) 仅支持 `DELIVERY_FIXED` 投递模式。删除的 3 个常量对应 IOAPIC 规范中 3 种高级投递模式:

| 模式 | 值 | 用途 | 影响 |
|------|-----|------|------|
| SMI | 0x200 | 系统管理中断 — 固件 (BIOS/UEFI) 与 OS 通信机制 | 服务器固件事件无法正确路由 |
| NMI | 0x400 | 不可屏蔽中断 — 硬件错误报告 (ECC 内存错误、看门狗超时) | 多核系统上硬件错误无法到达目标 CPU |
| ExtINT | 0x700 | 8259A 兼容中断 — 遗留 ISA 设备 | ISA 设备中断无法正确投递 |

### 实现内容

1. 添加 IOAPIC 投递模式常量: `DELIVERY_FIXED`/`DELIVERY_LOWEST`/`DELIVERY_SMI`/`DELIVERY_NMI`/`DELIVERY_INIT`/`DELIVERY_EXTINT`
2. 添加 `set_irq_with_mode` 函数，支持指定投递模式
3. 添加公共 accessor 函数: `delivery_fixed()`/`delivery_smi()`/`delivery_nmi()`/`delivery_extint()` 等
4. 添加 FFI 包装 `ioapic_set_irq_with_mode`

### 涉及文件

- `src/kernel/framework/arch/x86_64/ioapic.rs` — 添加投递模式常量 + set_irq_with_mode + accessor 函数

### 验证标准

- [x] IOAPIC 路由配置支持 SMI/NMI/ExtINT 投递模式
- [x] 双架构编译 0 warning 0 error
- [x] 审计全部通过

---

## P1: Local APIC 寄存器内省

**风险等级:** 中
**涉及已删除常量:** `APIC_ISR_BASE` (0x100), `APIC_TMR_BASE` (0x180), `APIC_IRR_BASE` (0x200)

### 问题描述

Local APIC 有 3 组只读寄存器用于中断状态内省:

| 寄存器组 | 偏移 | 用途 |
|---------|------|------|
| ISR (In-Service Register) | 0x100-0x17F | 记录正在处理的中断 |
| TMR (Trigger Mode Register) | 0x180-0x1FF | 记录中断触发模式 (边沿/电平) |
| IRR (Interrupt Request Register) | 0x200-0x27F | 记录待处理的中断请求 |

**诊断价值:** 当出现"中断挂起但 CPU 未响应"时，通过读取 IRR 确认中断已到达 APIC，通过 ISR 确认是否在处理中，通过 TMR 确认触发模式是否正确。没有此能力，中断调试只能靠猜测。

### 涉及文件

- `src/kernel/framework/arch/x86_64/apic.rs` — 添加寄存器读取函数
- `src/kernel/framework/idt/` — 中断诊断路径接入

### 验证标准

- [ ] 提供 `apic_read_isr()`, `apic_read_tmr()`, `apic_read_irr()` 函数
- [ ] 每个 32-bit 寄存器组的 8 个寄存器均可读取
- [ ] 双架构编译 0 warning 0 error
- [ ] 审计全部通过

---

## P1: PS/2 键盘扫描码集协商

**风险等级:** 中
**涉及已删除常量:** `KB_CMD_SCANCODE` (0xF0), `KB_CMD_IDENTIFY` (0xF2)

### 问题描述

键盘驱动 (`framework/driver/input/keyboard.rs`) 硬编码了 scancode set 1 映射表。现代 PS/2 键盘默认使用 scancode set 2:

- **QEMU/Bochs:** PS/2 控制器自动做 set 2 → set 1 转换，驱动正常工作
- **真实硬件:** 某些 PS/2 控制器**不做此转换**，键盘直接发送 set 2 扫描码，驱动解析出错误字符

**需要的能力:**

1. `KB_CMD_SCANCODE` (0xF0): 查询当前扫描码集 + 切换到 set 1
2. `KB_CMD_IDENTIFY` (0xF2): 识别键盘类型 (AT/XT/Enhanced)，处理不同键盘的初始化差异

### 涉及文件

- `src/kernel/framework/driver/input/keyboard.rs` — 键盘初始化路径添加扫描码集协商

### 验证标准

- [ ] 键盘初始化时发送 `KB_CMD_SCANCODE` 查询当前扫描码集
- [ ] 若非 set 1，发送 `KB_CMD_SCANCODE` 切换到 set 1
- [ ] 若 set 2, 切换失败，使用 set 2 映射表 (后备方案)
- [ ] 双架构编译 0 warning 0 error
- [ ] QEMU 键盘输入测试通过

---

## P2: 多 IOAPIC 支持

**风险等级:** 低
**涉及已删除常量:** `IOAPIC_ID` (0x00), `IOAPIC_ARB` (0x02)

### 问题描述

当前 IOAPIC 驱动假设系统只有 1 个 IOAPIC 控制器。多路 CPU 服务器可能有多个 IOAPIC，需要通过 `IOAPIC_ID` 寄存器区分:

- `IOAPIC_ID` (offset 0x00): 读取/设置 IOAPIC 标识符
- `IOAPIC_ARB` (offset 0x02): 仲裁 ID (多 IOAPIC 中断分配)

**当前影响:** 单 IOAPIC 场景 (QEMU/桌面) 不受影响。多路服务器才需要。

### 涉及文件

- `src/kernel/framework/arch/x86_64/ioapic.rs` — 多 IOAPIC 枚举与管理

### 验证标准

- [ ] 支持通过 MADT 表枚举多个 IOAPIC
- [ ] 每个 IOAPIC 可独立配置中断路由
- [ ] 双架构编译 0 warning 0 error

---

## P0: PCI MSI/MSI-X 中断基础设施

**风险等级:** 高
**涉及已删除常量:** `pci/msi.rs` 全模块 (`#![allow(dead_code)]` 已移除, 4 个规范常量已删除)

### 问题描述

`pci/msi.rs` 包含完整的 MSI/MSI-X 实现 (向量分配、PCI 配置空间写入、MMIO 表操作)，但从未被任何驱动调用。经深度调研发现，`msi_enable()` 写入 PCI config 后返回 `MsiConfig`，但**未注册 IDT handler** — CPU 跳转到 MSI vector 后无处理代码。

**缺失的基础设施:**

| 缺口 | 说明 |
|------|------|
| 无 ISR stub | vector 0x40-0x7F (MSI 范围) 无中断入口代码 |
| IDT dispatch 仅支持 IRQ 0-15 | `IdtManager::register_irq` 硬限制 `irq < 16` |
| `msi_enable()` 未注册 handler | 写入 PCI config 后返回 MsiConfig，但未调用 IDT 注册 |
| `IrqLine` 零调用者 | 类型存在但从未被任何驱动使用 |
| NVMe 未接 MSI | 有 PciDevice 引用但未调用 `msi_enable()` |

**影响:** NVMe、virtio-net (PCI 模式) 等现代 PCI 设备无法使用 MSI/MSI-X 中断，只能使用 legacy INTx 中断。

**已删除的规范常量 (需在完整实现时恢复):**

| 常量 | 规范含义 | 用途 |
|------|----------|------|
| `MSI_CTRL_QMASK` | Multi-Message Capable (bits 1-3) | 查询设备支持多少中断向量 |
| `MSI_CTRL_QSIZE` | Multi-Message Enable (bits 4-6) | 配置实际分配的向量数 |
| `MSI_CTRL_PERVEC` | Per-Vector Masking (bit 8) | 每向量独立屏蔽能力 |
| `MSIX_CTRL_FMASK` | Function Mask (bit 14) | 全局屏蔽 MSI-X |

### 涉及文件

- `src/kernel/framework/idt/mod.rs` — 扩展 ISR stub (0x40-0x7F) + dispatch 路径
- `src/kernel/framework/idt/irqline.rs` — IrqLine 用于 MSI vector 注册
- `src/kernel/framework/pci/msi.rs` — `msi_enable()` 接受 InterruptHandler 参数
- `src/kernel/framework/driver/storage/nvme.rs` — 存储 PciDevice + 调用 msi_enable

### 验证标准

- [ ] vector 0x40-0x7F 有 ISR stub 入口
- [ ] `msi_enable()` 返回的 MsiConfig 可注册 IDT handler
- [ ] NVMe 驱动通过 MSI 接收中断
- [ ] 双架构编译 0 warning 0 error
- [ ] host-tests 全部通过

---

## P1: Syscall sm_getsockname/getpeername 迁移 — ✅ 已完成 (2026-07-21)

**风险等级:** 中
**涉及死代码:** `sm_getsockname_call`, `sm_getpeername_call` + 对应 extern 声明

### 问题描述

`sys_getsockname` 和 `sys_getpeername` 是唯一仍通过 `raw` 模块 FFI 包装函数调用的网络系统调用。其他网络系统调用 (socket/bind/listen/connect/send/recv/close 等) 已迁移到 `net_socket.rs` 路径。这两个未迁移，导致 4 项死代码无法消除。

### 迁移内容

1. `framework/net_socket.rs` — 添加 `sm_getsockname`/`sm_getpeername` FFI 包装 + kernel_test 桩
2. `framework/net/syscall.rs` — 添加 `getsockname_syscall`/`getpeername_syscall`
3. `services/net/syscall.rs` — 添加安全代理
4. `framework/syscall/mod.rs` — `sys_getsockname`/`sys_getpeername` 改用新路径，删除旧 raw 模块代码

### 涉及文件

- `src/kernel/framework/net_socket.rs` — 添加 `sm_getsockname`/`sm_getpeername` FFI 包装
- `src/kernel/framework/net/syscall.rs` — 添加 `getsockname_syscall`/`getpeername_syscall`
- `src/kernel/services/net/syscall.rs` — 添加安全代理
- `src/kernel/framework/syscall/mod.rs` — 修改 `sys_getsockname`/`sys_getpeername` 使用新路径

### 验证标准

- [x] `sys_getsockname`/`sys_getpeername` 通过 `net_socket.rs` 路径调用
- [x] raw 模块 `sm_getsockname_call`/`sm_getpeername_call` 及其 extern 声明已删除
- [x] 双架构编译 0 warning 0 error
- [x] host-tests 全部通过

---

## 关联文档

- `docs/explain/explain-framekernel.md` — 框内核架构说明
- `AGENTS.md` §4 — 架构责任分离
